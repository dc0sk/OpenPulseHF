---
project: openpulsehf
doc: docs/architecture.md
status: living
last_updated: 2026-05-01
---

# Architecture

## System goals

- Provide a Rust-native, plugin-based software modem for amateur radio data links.
- Keep a reusable workspace split into core, audio, modem engine, and frontend crates.
- Maintain reliable loopback testing that works without external audio hardware.
- Keep frontend behavior consistent by making CLI the reference execution path.
- Support incremental protocol growth through plugin-based modulation modes.

## Core architecture

1. Input payload is framed into OpenPulseHF packets with sequence and CRC.
2. A modulation plugin transforms frames into baseband symbols and samples.
3. An audio backend transports samples to and from loopback or hardware I/O.
4. A receive pipeline demodulates, validates frames, and reassembles payload data.
5. Frontend surfaces status and decoded payloads to users and automation.

For HPX, the pipeline also includes signed session handshake validation and signed transfer manifest verification before delivery completion is acknowledged.
For relayed operation, the control plane includes peer discovery cache, query handling, and route selection across one or more relay hops.

## Workspace architecture

| Crate/Path | Role |
|-----------|------|
| crates/openpulse-core | Core traits (ModulationPlugin and AudioBackend), frame format, CRC-16, and plugin registry |
| crates/openpulse-audio | Audio backend implementations: in-process loopback (testing) and CPAL-based backends |
| crates/openpulse-modem | Modem engine wiring plugins and audio together |
| crates/openpulse-cli | openpulse binary and user-facing CLI options |
| plugins/bpsk | BPSK modulation plugin with NRZI and raised-cosine pulse shaping |

Future plugins (for example QPSK or ARDOP-compatible modes) should implement ModulationPlugin and register at startup.

## Modulation design decisions

### Single-carrier versus OFDM

OpenPulseHF uses single-carrier phase-shift keying for all current modes. VARA HF and PACTOR-3/4 both use OFDM. This is a deliberate choice with the following rationale:

**Advantages of single-carrier for this project:**
- Peak-to-Average Power Ratio (PAPR) is near 0 dB for BPSK and approximately 3–4 dB for QPSK. OFDM with many subcarriers produces PAPR of 9–12 dB. Low PAPR means OpenPulseHF can transmit at rated power from any linear amplifier without back-off, which is critical for portable and QRP operation.
- No IQ balance or phase noise requirements. Any SSB-capable transceiver works without special calibration.
- No cyclic prefix overhead. OFDM requires a guard interval (VARA uses 5.33 ms per symbol) that consumes channel time with no data.
- Simpler receiver. A single correlator or matched filter per symbol versus an N-point FFT plus per-subcarrier equalization.
- Automatic frequency control (AFC) is simpler: a single carrier frequency estimate versus per-subcarrier frequency drift tracking.

**Accepted trade-offs:**
- Single-carrier modes are more susceptible to inter-symbol interference (ISI) from multipath delay spread. At the symbol periods used by OpenPulseHF (32 ms for BPSK31, 4 ms for BPSK250), ISI from HF multipath delays of 0.5–2 ms is tolerable for lower baud rates without equalization. Higher-rate single-carrier modes will require adaptive equalization as they are developed.
- Peak throughput for a given occupied bandwidth is lower than OFDM with higher-order QAM, because OFDM can overlap subcarriers spectrally. HPX500 and HPX2300 address this through adaptive modulation across the rate ladder (BPSK → QPSK → 8PSK) within a single occupied bandwidth class.

### Differential BPSK encoding

Current BPSK modes use differential encoding (DBPSK): the data bit is encoded as a phase *change* relative to the previous symbol, not as an absolute phase. This eliminates the need for carrier phase recovery at the receiver. The cost is approximately 3 dB SNR relative to coherent BPSK. For BPSK31 on HF where SNR often limits throughput, differential encoding is the pragmatic choice. Coherent detection may be added for higher-rate modes where the 3 dB gain is worth the receiver complexity.

### Raised-cosine pulse shaping

The BPSK plugin applies raised-cosine amplitude shaping (roll-off factor α = 1.0) to each symbol. This is the same shaping used by PSK31 (G3PLX, 1998). The result is near-zero out-of-band emissions despite the narrow 31–250 Hz symbol bandwidth: sidelobes fall at 1/f³ versus 1/f for rectangular pulse shaping. The trade-off is sensitivity to ISI, which is acceptable at HF symbol rates.

Note: this is an amplitude envelope shape applied per symbol, not a root-raised-cosine matched-filter pair. This is architecturally simpler but not optimal in the information-theoretic sense. Matched-filter design may be revisited for higher-rate modes.

## Supported modulation modes

| Mode | Baud rate | Encoding | Payload encoding | Notes |
|------|-----------|----------|-----------------|-------|
| BPSK31 | 31.25 | DBPSK | Byte (8-bit) | Narrow-band HF |
| BPSK63 | 62.5 | DBPSK | Byte (8-bit) | Higher throughput |
| BPSK100 | 100 | DBPSK | Byte (8-bit) | Loopback testing |
| BPSK250 | 250 | DBPSK | Byte (8-bit) | Wide-band, faster |
| QPSK125 | 125 | Gray-mapped QPSK | Byte (8-bit) | 2 bits/symbol |
| QPSK250 | 250 | Gray-mapped QPSK | Byte (8-bit) | 2 bits/symbol |
| QPSK500 | 500 | Gray-mapped QPSK | Byte (8-bit) | 2 bits/symbol |
| QPSK1000 | 1000 | Gray-mapped QPSK | Byte (8-bit) | 2 bits/symbol; HPX2300 SL9 |
| 8PSK500 | 500 | Gray-coded 8PSK | Byte (8-bit) | 3 bits/symbol |
| 8PSK1000 | 1000 | Gray-coded 8PSK | Byte (8-bit) | 3 bits/symbol; HPX2300 SL11 |
| FSK4-ACK | 100 (fixed) | 4-tone FSK | ACK frame (5 B) | ACK transport; independent of data mode |

## HPX adaptive profiles

HPX sessions select a modulation mode at runtime based on the current `SpeedLevel` reported by the `RateAdapter`. Two profiles are defined in `openpulse-core/src/profile.rs`:

### HPX500 (500 Hz class)

| SL  | Mode     | Notes |
|-----|----------|-------|
| SL1 | — | Chirp fallback (session teardown) |
| SL2 | BPSK31   | Initial level; most robust |
| SL3 | BPSK63   | |
| SL4 | BPSK250  | |
| SL5 | QPSK250  | |
| SL6 | QPSK500  | Highest HPX500 rate |
| SL7 | — | Reserved |

### HPX2300 (2300 Hz class)

Single-carrier modulation was chosen over OFDM for HPX2300. Rationale: lower PAPR (near 0 dB for 8PSK vs 9–12 dB for OFDM), no cyclic prefix overhead, simpler AFC, and architectural consistency with HPX500. OFDM is deferred to Phase 3 if multipath gain measurements justify the added receiver complexity.

| SL  | Mode      | Notes |
|-----|-----------|-------|
| SL1–SL7 | — | Chirp fallback / HPX500 territory |
| SL8 | QPSK500   | Initial level |
| SL9 | QPSK1000  | |
| SL10 | — | Reserved |
| SL11 | 8PSK1000 | Highest HPX2300 rate (~3 kbps raw) |

## Signed session handshake (Phase 2.3)

HPX sessions begin with a two-message signed handshake that authenticates both peers and negotiates the signing mode for the session.

### CONREQ (connection request)

Sent by the initiating station at the start of the Discovery phase.

```
MAGIC("HSCQ") | VERSION(0x01) | LENGTH(u32, BE) | JSON body
```

JSON body fields:

| Field | Type | Description |
|-------|------|-------------|
| `station_id` | string | Callsign of the initiating station |
| `pubkey` | `[u8]` | Ed25519 verifying-key bytes (32 bytes) |
| `signing_modes` | `[string]` | List of supported signing modes (subset of `normal`, `psk`, `relaxed`, `paranoid`) |
| `session_id` | string | Randomly-generated session identifier |
| `signature` | `[u8]` | Ed25519 signature (64 bytes) over canonical JSON of the above fields (excluding `signature`) |

### CONACK (connection acknowledgment)

Sent by the responder in reply to a valid CONREQ.

```
MAGIC("HSAK") | VERSION(0x01) | LENGTH(u32, BE) | JSON body
```

JSON body fields:

| Field | Type | Description |
|-------|------|-------------|
| `station_id` | string | Callsign of the responding station |
| `pubkey` | `[u8]` | Ed25519 verifying-key bytes (32 bytes) |
| `selected_mode` | string | Chosen signing mode from the CONREQ offer list |
| `session_id` | string | Must echo the `session_id` from the CONREQ |
| `signature` | `[u8]` | Ed25519 signature (64 bytes) over canonical JSON of the above fields (excluding `signature`) |

### Handshake trust evaluation

The receiver of each handshake frame:

1. Verifies the Ed25519 signature against the included `pubkey`.
2. Looks up the peer's trust level in the local trust store.
3. Calls `evaluate_handshake(policy, local_min_mode, peer_modes, key_trust, cert_source)` from `openpulse-core::trust`.
4. On `Rejected` trust level or no mutual signing mode → fires `HpxEvent::SignatureVerificationFailed` → session enters `Failed`.
5. On success → fires `HpxEvent::DiscoveryOk` → session advances to `Training`.

### Transfer manifest

At session teardown, the sender emits a `TransferManifest` with:

| Field | Type | Description |
|-------|------|-------------|
| `payload_hash` | `[u8]` | SHA-256 of the complete reassembled payload |
| `payload_size` | `u64` | Total payload bytes |
| `sender_id` | string | Callsign of the sender |
| `signature` | `[u8]` | Ed25519 signature (64 bytes) over canonical JSON of the above fields (excluding `signature`) |

The receiver verifies the signature with `verify_manifest()` and also checks that `payload_hash` matches the locally-computed hash of the received data. A mismatch fires `HpxEvent::TransferError` → `HpxReasonCode::ManifestVerificationFailed`.

## Frame format

OpenPulseHF frames follow this logical layout:

```text
magic("OPLS") | version(0x01) | sequence(u16, big-endian) | length(u8) | payload | crc16(ccitt)
```

The payload length range is 0–255 bytes.

### Frame size constraint and SAR dependency

The one-byte `length` field caps the payload at 255 bytes. This is a hard architectural constraint with the following implications:

- Any data object larger than 255 bytes requires multiple frames.
- Post-quantum signature sizes (ML-DSA-44: 2420 bytes; ML-KEM-768 public key: 1184 bytes) do not fit in a single frame. In-band PQ handshake transport requires a segmentation and reassembly (SAR) sub-layer.
- A future SAR sub-layer must define: segment ID (session-scoped), fragment number, total fragment count, and reassembly timeout.
- Until SAR is implemented, transfers above 255 bytes of application payload rely on application-layer framing (HPX session protocol) rather than the wire-layer frame format.
- Extending `length` to a `u16` field would require a frame format version bump and a migration strategy; this is a design option for evaluation during SAR planning.

## ACK frame taxonomy and turnaround timing

HPX sessions require a defined set of ACK frame types to drive ARQ and adaptive rate control. The following taxonomy is normative for HPX:

| ACK type | Meaning |
|----------|---------|
| ACK-OK | Data frame received correctly; maintain current rate |
| ACK-UP | Data frame received correctly; request rate increase |
| ACK-DOWN | Data frame received correctly; request rate decrease |
| NACK | Data frame received with uncorrectable errors; request retransmission |
| BREAK | Changeover: IRS requests to become ISS |
| REQ | ACK frame lost; request last data frame again |
| QRT | Session end; graceful teardown |
| ABORT | Session end; abnormal teardown |

ACK frames must be short enough to be demodulated reliably at lower SNR than data frames. The recommended implementation uses FSK modulation for ACK bursts independent of the data modulation in use, consistent with VARA's approach. This gives ACK frames approximately 6 dB PAPR headroom over data frames.

Turnaround timing contract:
- Transmitter must release PTT within 50 ms of last transmitted sample.
- Receiver must detect carrier and begin ACK within 150 ms of remote PTT drop.
- Total half-duplex cycle budget must be documented per mode profile.

## Frontend architecture

- CLI is production-first and defines expected behavior.
- Additional frontends may be added, but must call stable core APIs.
- Frontends must not duplicate modem logic that belongs in shared crates.

## Platform support

| Platform | Audio backend |
|----------|---------------|
| Linux | ALSA, including PipeWire through ALSA compatibility |
| macOS | CoreAudio |
| Windows | WASAPI |
| Any | In-process loopback for hardware-free testing |

## Performance architecture

- Real-time behavior depends on bounded buffering and deterministic frame timing.
- Loopback and no-default-features test paths remain fast and stable in CI.
- Optional optimization work should preserve functional parity with baseline paths.
- Modem execution should separate I/O, framing, and DSP stages so they can run on dedicated worker threads.
- Thread scheduling strategy should avoid unbounded queues and preserve deterministic latency under load.
- GPU offload should target compute-intensive DSP components only when benchmarks show net gain.
- GPU path should use open acceleration stacks (preferred: Vulkan via wgpu; optional: OpenCL) with an always-available CPU path.

## Edge platform support

- Raspberry Pi 4 and Raspberry Pi 5 are supported edge targets for HPX operation.
- ARM64 builds must preserve feature parity for signed-transfer and trust workflows.
- Resource-aware execution profiles should be available for Pi-class CPU and memory budgets.

## Extensibility architecture

- New modulation families are introduced as plugins implementing shared traits.
- Plugin APIs must remain stable enough for out-of-tree experimentation.
- Core crates should remain embeddable for future automation and integrations.

For HPX, keep signal path adaptation logic and trust/signature logic as separate internal components so they can be tested independently.

## Security architecture

- Identity management and trust evaluation are control-plane concerns.
- Transfer signing and verification are data-plane admission checks.
- Verification failures must surface clear failure reasons to frontends and logs.
- Session-state behavior for security and recovery is defined in docs/hpx-session-state-machine.md.
- Relay path trust and end-to-end signer trust are evaluated independently.

## Routing and relay architecture

- Peer cache stores signed identity and capability descriptors with aging policy.
- Query engine supports local filter queries and bounded network query propagation.
- Route planner selects direct or multi-hop path using trust and link-quality scoring.
- Relay layer enforces loop prevention, replay protection, and hop-limited forwarding.
- Wire-level relay and query envelopes are defined in docs/peer-query-relay-wire.md.

## Radio interface architecture

The radio interface layer sits between the audio backend and the physical transceiver.

- PTT control must be abstracted behind a `PttController` trait with implementations for: no-op (loopback), serial RTS/DTR, VOX (audio-triggered), and CAT/rigctld.
- The CAT/rigctld implementation communicates with a running `rigctld` daemon from the Hamlib project, which supports over 300 amateur transceivers.
- Audio level monitoring must be available as a diagnostic output so operators can set appropriate drive levels without external tools.
- AFC state (estimated frequency offset in Hz) must be exposed in session diagnostics.

## Documentation process constraints

- Documentation updates flow through pull requests only.
- Frontmatter validation and stamping automation are required quality gates.
