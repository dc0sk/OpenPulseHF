---
project: openpulsehf
doc: docs/dev/requirements.md
status: living
last_updated: 2026-07-14
---

# Requirements

> **Numbered REQ-IDs and end-to-end traceability** (requirement → capability → design
> decision → implementation → tests → result → assets → PRs) live in
> [traceability-matrix.md](project/traceability-matrix.md). Each requirement below maps to a
> `REQ-<CAT>-NN` ID in that matrix's Requirements table.

## Functional requirements

- Provide a CLI capable of transmit, receive, device listing, and mode listing.
- Support at least one production modulation plugin (BPSK family).
- Preserve a loopback backend for hardware-free development and testing.
- Support cross-platform audio through CPAL-backed implementations.
- Validate frame integrity with versioning, sequence handling, and CRC checks.
- Define and implement a high-performance plugin mode (HPX) with adaptive modulation and coding.
- Support occupied bandwidth classes centered on 500 Hz and 2300-2400 Hz operation.
- Provide deterministic session state handling: discovery, training, active transfer, recovery, teardown.
- Support selective retransmission for ARQ-capable sessions.
- Support signed transfer handshake and signed transfer manifests.
- Support trust-store-based verification for station identities.
- Support peer caching of identity, capability, and link-quality metadata.
- Support local and network query interfaces for peer discovery and filtering.
- Support relayed transfers across multiple hops with configurable hop limits.
- Support route selection policies based on trust, reliability, and latency estimates.
- Define versioned wire-level envelopes for peer query, route discovery, and relay transfer control messages.

## Physical layer and radio interface requirements

- Audio backend must support a minimum sample rate of 48 kHz at 16-bit integer or 32-bit float resolution.
- The receive pipeline must apply a high-pass filter (cutoff ≤ 10 Hz) before demodulation to remove DC bias introduced by SSB radio audio paths.
- The demodulator must track station-to-station frequency offsets of up to ±50 Hz without operator intervention (automatic frequency control, AFC).
- The AFC subsystem must handle transmitter drift up to 1 Hz per second for normal SSB radio operation.
- Transmitter release (PTT drop) must occur within 50 ms of the last transmitted sample to preserve turnaround timing budgets.
- The receive path must begin acquiring signal within 150 ms of remote key-down to honour the turnaround timing contract.
- PTT keying must support at minimum: serial port RTS/DTR assertion and software-controlled VOX.
- CAT-based PTT control via Hamlib/rigctld is a recommended integration path that provides access to the majority of amateur transceivers without per-rig code.
- Audio input gain must remain within a range that preserves symbol amplitude stability; the system must document the expected input level range and provide a level indicator.

## Platform and dependency requirements

- Linux support is the primary target and requires ALSA development headers for CPAL builds.
- macOS support uses CoreAudio through CPAL.
- Windows support uses WASAPI through CPAL.
- Raspberry Pi 4 and Raspberry Pi 5 must be supported as first-class Linux deployment targets.
- ARM64 builds for Raspberry Pi 4/5 must be part of regular compatibility testing.
- Any development environment must support loopback mode for hardware-free testing.
- Rust toolchain must build the full workspace and no-default-features variant.

## Non-functional requirements

- Maintain workspace-level buildability on Linux and macOS CI runners.
- Keep tests runnable without physical audio hardware in default CI workflows.
- Ensure crate boundaries are clear enough for independent testing.
- Keep plugin additions from requiring broad refactors across unrelated crates.
- Define objective benchmark suites and publish method and result artifacts.
- Track goodput, completion rate, retry efficiency, and completion latency across channel profiles.
- Require HPX performance claims to be tied to reproducible benchmark runs.
- Benchmark channel profiles must include parameterized Watterson model scenarios (Good/Moderate/Poor path conditions) and Gilbert-Elliott burst error scenarios; AWGN-only benchmarks are insufficient for HF performance claims.
- Maintain deterministic timeout and retry behavior for session-state transitions.
- Use multithreaded execution for modem pipelines where it improves deterministic real-time behavior.
- Support optional GPU acceleration for compute-heavy signal-processing stages when it produces measurable benefit.
- GPU acceleration paths must use open frameworks (for example Vulkan via wgpu, or OpenCL) and provide a CPU fallback.
- Raspberry Pi 4/5 performance targets must be measured and published in benchmark artifacts.
- Peer cache lookup and query operations should remain bounded under large peer tables.
- Multi-hop relay control-plane traffic should include duplicate suppression and loop prevention.

## FEC and interleaving requirements

- All FEC-enabled transfer modes must pair the FEC codec with a block interleaver.
- The interleaver must shuffle symbols across multiple FEC blocks before transmission such that burst errors are dispersed into correctable random-error patterns.
- Default interleaver depth must be at least 5× the expected maximum burst error duration expressed in symbols at the target baud rate.
- Interleaver depth must be a documented parameter in each mode profile definition; it must not be a hidden constant.
- FEC and interleaver parameters must be agreed upon during session handshake and must not be assumed by either party.
- Benchmark scenarios must test FEC+interleaver effectiveness under burst-error conditions (Gilbert-Elliott model) and not only under AWGN.

## Channel access requirements

- Sessions operating in point-to-point mode may assume a dedicated channel and are not required to implement channel sensing.
- Sessions operating in broadcast or relay mode on a shared channel must implement a channel-clear detection (CCD) mechanism before transmitting.
- The reference channel access algorithm for shared-channel operation is 0.3-persistence CSMA: sense the channel, transmit immediately with 30% probability if clear, back off and retry otherwise.
- Data Carrier Detect (DCD) is the mechanism for CCD and must be derived from the demodulated signal energy, not from audio amplitude alone.
- Channel access policy must be documented per mode profile.

## Compression requirements

- Optional lossless payload compression at the session layer is in scope.
- Compression algorithm must be deterministic and produce identical output for identical input across platforms.
- Compression capability must be negotiated during session handshake and must not be assumed.
- If compression is active, compressed size must be compared to uncompressed size before transmission; a compressed frame larger than the uncompressed original must be sent uncompressed.
- Decompression failure must be treated as a frame integrity error.

## Security and trust requirements

- Signed transfers are mandatory for HPX file or object transfer mode.
- Station identities use asymmetric key pairs with operator-managed trust anchors.
- Session handshake messages must be signed and verified.
- Transfer manifests must be signed and verified before final acceptance.
- Trust status must include trusted, untrusted, revoked, and unknown states.
- Key lifecycle must include validity windows and revocation handling.
- Cryptographic defaults should use Ed25519 signatures and SHA-256 or stronger hashing.
- A post-quantum-safe signature method must be supported for identity and transfer-signing workflows.
- The implementation should support a hybrid signature mode (classical + post-quantum) during migration.
- Initial post-quantum-safe default should target ML-DSA (FIPS 204) where available.
- If session key establishment is used, a post-quantum-safe KEM option should be supported, with ML-KEM (FIPS 203) preferred.
- Trust-store metadata must record algorithm type and hybrid-policy requirements per identity.
- Relay path admission must enforce trust policy on each intermediate hop.
- Multi-hop transfers must preserve end-to-end signed integrity and fail closed on trust violations.
- Route metadata should support post-quantum-capable signing under configured policy.
- Relay and query messages must include anti-replay fields and enforce loop-prevention semantics.

### Post-quantum and frame size dependency

- ML-DSA-44 signatures are 2420 bytes. ML-KEM-768 public keys are 1184 bytes. Both exceed the current 255-byte frame payload limit.
- In-band post-quantum handshake messages cannot be carried in the current wire format without a segmentation and reassembly (SAR) sub-layer.

## Control-channel security requirements

The daemon control channel (TCP + WebSocket, ports 9000/9001) carries operator commands — PTT,
frequency/mode, transmit, messaging — between the daemon (server) and the operator panel and other
clients. It is distinct from the on-air/RF peer link (secured separately, above); today it is
plaintext with no authentication, bound to loopback by default. The reference is K4remote's TLS-PSK
client. See `docs/dev/design/control-channel-security.md` for the design and threat model.

- The control channel must support mutual authentication and on-the-wire encryption using a
  pre-shared key (PSK). (REQ-SEC-CTL-01)
- When the daemon binds to any non-loopback address, an authenticated + encrypted channel must be
  required; unauthenticated plaintext is permitted only on a loopback (`127.0.0.1`/`::1`) bind.
  Transmitter-keying commands (PTT, transmit) must never be accepted from an unauthenticated client
  on a non-loopback bind — fail closed. (REQ-SEC-CTL-02)
- Secrets (the control-channel PSK, station identity keys) should be storable in the operating
  system's secret store — Secret Service / GNOME Keyring / KWallet on Linux, Keychain on macOS,
  Credential Manager on Windows — as the preferred backend when available, for both the daemon
  (server) and the clients. (REQ-SEC-CTL-03)
- A file-based keystore must be available as a fallback for hosts without a usable system secret
  store, encrypting secrets at rest under an operator master password (memory-hard KDF, e.g.
  Argon2id, plus authenticated encryption). The master password must never be written to disk in
  plaintext. (REQ-SEC-CTL-04)
- Any file holding key or secret material (identity key, trust store, keystore, PSK file) must be
  owner-only: `0600` for files, `0700` for the containing directory. Both the daemon (server) and the
  panel and other clients (client) must validate permissions when loading such a file and refuse to
  read one that is group- or world-accessible, and must set owner-only permissions on write. This
  generalises the existing `validate_trust_store_permissions` / `enforce_trust_store_permissions` in
  `openpulse-cli` to every secret file on both sides. (REQ-SEC-CTL-05)
- SAR must be designed and implemented before in-band PQ handshake requirements can be satisfied.
- PQ signature transport requirements are therefore sequentially dependent on SAR delivery; planning must reflect this ordering.
- Out-of-band or application-layer PQ key distribution (for example via the PKI tooling) may proceed independently of SAR.

## Regulatory compliance requirements

Regulatory compliance is a hard requirement for any transmission on amateur radio frequencies. The following rules apply in the primary jurisdictions of interest. See docs/regulatory.md for full analysis and derivations.

### United States — FCC Part 97

- §97.307(f): The maximum symbol rate on any single carrier must not exceed 300 baud below 28 MHz in phone subbands. OpenPulseHF includes sub-300-baud modes (for example BPSK31/BPSK63/BPSK100/BPSK250) and higher-rate single-carrier modes (for example QPSK500+ and 8PSK500+). Operators must select frequencies, modes, and regional band segments consistent with local rules before transmission.
- §97.309(a)(4): Digital codes whose use is not specifically prohibited elsewhere and whose technical characteristics are publicly documented are permitted. OpenPulseHF must maintain a published technical specification sufficient for any amateur to decode the transmitted signal.
- §97.119(a): Station identification is required every 10 minutes during a transmission and at the end of each transmission series. In digital modes, identification must be in a format decodable by a receiving station.
- §97.221: Automatically controlled digital stations (unattended nodes, relay nodes) require an automatic control point. HPX relay nodes operating without a control operator present are automatically controlled stations and must comply with §97.221 including power limits and frequency restrictions.

### European Union and CEPT

- ECC/REC(05)06 and national implementations: CEPT harmonises amateur radio digital mode permissions across member administrations. Most EU member states permit amateur digital modes across all authorised bands subject to the general licence conditions (power, bandwidth, identification).
- CEPT T/R 61-01: harmonised licensing for portable cross-border operation within CEPT countries. OpenPulseHF documentation must state which modes and bandwidths are intended so that visiting operators can assess compliance with their visiting licence conditions.
- Bandwidth constraint: many EU administrations limit occupied bandwidth by band and mode class. For HF digital modes the typical permitted bandwidth is ≤ 2.7 kHz (matching SSB channel spacing). HPX2300/2400 Hz profiles must be validated against the occupied bandwidth definition used by the relevant national administration.
- Station identification: EU member states typically require identification at least every 10 minutes (consistent with FCC), though interval requirements vary (e.g. UK: every 15 minutes). The identification requirement is the same: it must be decodable by the receiving station in the digital mode in use, or transmitted in supplementary CW or voice.
- Germany (BNetzA): §12 Amateurfunkverordnung (AFuV) requires that technical characteristics of amateur emissions be determinable. Digital modes without a published open specification may be questioned by authorities; OpenPulseHF's open specification satisfies this requirement by design.
- United Kingdom (Ofcom): The UK Full licence permits digital modes on all amateur bands. Station identification every 15 minutes and at end of transmission. The UK left CEPT licensing arrangements post-Brexit; UK operators verify compliance with the current Ofcom amateur licence conditions document directly.

### IARU Region 1 and Region 2 band plans

- IARU band plans are non-binding recommendations but are widely observed to avoid mutual interference.
- Region 2 (Americas) and Region 1 (Europe/Africa/Middle East) both designate sub-bands for HF narrowband digital modes (e.g. 14.070–14.099 MHz on 20 m).
- OpenPulseHF documentation should recommend operating frequencies aligned with IARU band plan digital sub-bands for each supported band.
- Wide-band HPX2300 profiles should operate in segments where wide-band digital modes are plan-consistent (e.g. 14.099–14.112 MHz on 20 m where permitted by national administration).

## Competitive performance requirements

- Primary strategic goal: develop an independent, first-principles OpenPulse protocol stack that competes on reliability, throughput, and usability.
- HPX must target outcome parity or better versus incumbent modems in benchmarked scenarios.
- Comparisons must use equal occupied bandwidth classes and published test conditions.
- Performance evaluation must include at least HF narrow, HF wide, and VHF FM profile families.
- No claim of proprietary protocol compatibility may be made without defensible public evidence.
- Any compatibility mode targeting proprietary systems (including VARA or PACTOR-4) requires explicit legal review and approval before implementation work starts.

## Compatibility and UX requirements

- CLI usage and docs must stay aligned across releases.
- New user-facing options must be documented in docs/cli-guide.md.
- README usage examples should stay current with implemented behavior.
- The operator panel application (`apps/openpulse-panel`) shall be re-implemented on the `iced` GUI
  toolkit (replacing egui/eframe), presenting the operating surface as a scrollable stack: a controls
  band, spectrum, waterfall, ladder (adaptive rate/mode), and a tabbed lower panel (additional info /
  daemon config / messages / event log). (REQ-UX-04)

## Observability and diagnostics requirements

- OpenPulse shall provide an opt-in observability/audit mode that persists logs and structured
  events to disk, so a run can be analysed after the fact without a live client attached. Audit
  mode is off by default and enabled via configuration. (REQ-OBS-01)
- Long-running binaries (at minimum `openpulse-daemon`) shall support persistent, rotating
  file logging in addition to stdout, enabled via a `[logging]` config path, with the resolved
  log path visible at startup. Log level continues to honour `RUST_LOG` over config over default.
  (REQ-OBS-02)
- OpenPulse shall provide a single command to collect a diagnostic bundle — recent logs, the
  latest session diagnostics/metrics, a config snapshot with secrets redacted, and
  version/git/system metadata — packaged for handoff to a developer, generalising the existing
  on-air `bundle-evidence` script to everyday runs. (REQ-OBS-03)

## Wide-channel (VHF/UHF) requirements — release 1.x

Extending the modem from its ~2.7 kHz HF SSB channel to 12.5 kHz and 25 kHz VHF/UHF-class channels.
Targeted at a future **1.x** release; not part of the current line. Design and phased action list in
`docs/dev/design/wide-channel-extension.md`. These requirements are gated on the RF-architecture
decision (REQ-BW-01).

- The audio sample rate shall be configurable rather than fixed at 8 kHz: the modem engine and all
  rate-parameterized DSP must run at a `[audio] sample_rate` selected from at least {8000, 48000,
  96000} Hz, defaulting to 8000. (REQ-BW-02)
- The system shall support wide modes occupying up to ~12.5 kHz at a 48 kHz audio path (e.g.
  clock-scaled OFDM/SC-FDMA and the existing 9600-baud RRC modes), reachable via the adaptive ladder.
  (REQ-BW-03)
- The system shall support wide modes occupying up to ~25 kHz, via a 96 kHz real-audio path or a
  48 kHz complex-IQ path. (REQ-BW-04)
- The system shall provide a direct-IQ receive path (complementing the existing IQ transmit seam) so
  wide operation can use an SDR front-end rather than a bandwidth-limited soundcard/SSB path.
  (REQ-BW-05)
- Bandplan awareness shall be extended to VHF/UHF bands (6 m/2 m/1.25 m/70 cm) with per-segment
  occupied-bandwidth limits (12.5/25 kHz where regionally permitted) and channel-raster-aligned QSY.
  (REQ-BW-06)
- Wide-mode SNR floors shall be calibrated against a VHF/UHF mobile-fading channel model (flat
  Rayleigh/Rician + vehicle Doppler) with an explicitly documented SNR reference bandwidth. (REQ-BW-07)
- **RF-architecture decision (blocking):** the wide path shall use a direct-IQ SDR path and/or a
  linear wide exciter; a constant-envelope (e.g. 4FSK) wide mode family is an optional fallback for
  class-C FM transmitters. This decision governs all of REQ-BW-02..07 and must be recorded before
  implementation. (REQ-BW-01)

## JS8-based station discovery and rendezvous requirements (FF-15)

Idle-time discovery of other OpenPulse stations on the shared JS8 calling frequency, and negotiated
handoff from JS8 to a native HPX session. Shipped (Phases A–G); only on-air validation (Phase H) is
deferred. Design and locked decisions D1–D7 in `docs/dev/design/js8-discovery-rendezvous-plan.md`;
capability rows CAP-70 in the traceability matrix.

- The station shall implement a native JS8-compatible weak-signal waveform (8-GFSK, 79 symbols,
  Costas 3×7 sync, LDPC(174,87), CRC-12) that interoperates with stock JS8Call, without depending on an
  external JS8Call process at runtime. (REQ-DISC-01)
- When discovery is enabled and the station is idle, it shall QSY to the current band's JS8 calling
  frequency, participate as a well-behaved JS8 station (heartbeats at community-norm cadence), and
  restore its home frequency when discovery stands down or is preempted. (REQ-DISC-02)
- The station shall mark itself with an in-band `@OPULSE` capability hint, recognize other OpenPulse
  stations from that hint, and cache them (identity, capability, link-quality) in the shared peer
  cache. (REQ-DISC-03)
- All discovery transmission shall be **off by default**; when enabled the default mode shall be
  receive-only. Beacon and rendezvous transmission shall each require an explicit opt-in plus a
  configured callsign. (REQ-DISC-04)
- The station shall not transmit unless its clock is NTP-disciplined to within ±2 s of UTC (residual
  bias estimated from decode timing); beyond that bound it shall hard-refuse transmission and degrade
  to receive-only. (REQ-DISC-05)
- Unattended beacon transmission shall satisfy §97.221 automatic control — a reachable control point
  able to terminate transmission, off-by-default gating, periodic identification, and operator-set
  power — as documented in `docs/regulatory.md` (see REQ-REG-04). (REQ-DISC-06)
- The station shall negotiate a working frequency with a discovered peer via a compact 2-message
  rendezvous exchange over JS8, then QSY and hand off to the signed HPX handshake (CONREQ/CONACK),
  which provides authentication; the rendezvous exchange itself carries no signature. (REQ-DISC-07)

## Direct peer-to-peer file transfer requirements (FF-16)

Sending a file to a connected peer over an RF session, with cryptographic end-to-end verification.
Shipped (Phases A–E); on-air validation (Phase F) is deferred. Design and locked decisions D1–D5 in
`docs/dev/design/file-transfer-plan.md`; capability row CAP-71 in the traceability matrix.

- The station shall send a file to a connected peer over an RF session using a dedicated framed
  protocol (offer / accept / reject / data / block-ack / complete / cancel), carried over the shared
  SAR segmentation layer. (REQ-FX-01)
- File objects shall be split into fixed-size blocks (default 16 KiB, ≤48 KiB) each carried as one SAR
  segment, lifting the single-object SAR size limit so multi-megabyte transfers are supported, with a
  configurable hard cap (default 1 MiB). (REQ-FX-02)
- Each transfer shall carry an inline signed `TransferManifest` with a SHA-256 payload hash; the
  receiver shall verify it against the peer's handshake key before final acceptance and shall
  quarantine (mark UNVERIFIED) any file that fails verification. (REQ-FX-03)
- File acceptance shall be operator-controlled: a verified-peer requirement, size-gated auto-accept
  (default off), an optional per-peer retained-bytes quota, and prompt-on-offer by default. (REQ-FX-04)
- Reliable delivery shall use a hybrid scheme — over-the-air per-burst rate feedback plus a
  block-level acknowledgement bitmap for selective retransmission — and shall support resuming an
  interrupted transfer from the last completed block. (REQ-FX-05)
- Transmission shall be airtime-bounded into bursts so PTT keying stays within the radio's watchdog
  limit and the channel is yielded between bursts. (REQ-FX-06)

## Reference-derived requirements (software-defined modem study, 2026-07-14)

Derived from studying modern open-source modems (`docs/dev/research/references.md`:
RFnexus/modem73, chrissnell/omnimodem, chrissnell/graywolf). **We re-implement independently — no code
is copied**; these capture techniques worth building from first principles. Scheduling and priority live
in the roadmap; each is a candidate, not a committed deliverable.

- The receive **AGC / input-level normalization** front-end (`openpulse_dsp::agc::Agc`) — wired at the
  single `route_audio_stage(InputCapture)` seam (DC-block → notch → DCD → AGC, with DCD read *pre-AGC*),
  gain-locked per burst so a mid-frame gain change can't corrupt soft-decision scaling, off by default,
  with an `agc_blocks_processed` tripwire and runtime `SetAgc` control (daemon/CLI/panel) — is **already
  shipped** (PRs #583/#699/#700/#826; verified by the 2026-07-14 Fable design review). The earlier "we
  have no AGC" claim was stale (predated PR #583). Remaining delta: (a) a TOML config gate
  `[modem] agc_enabled` (+ optional target-RMS / bandwidth / max-gain-dB) applied at daemon startup like
  the notch — currently AGC is runtime-toggleable only; and (b) a systematic input-amplitude-sweep
  acceptance test documenting that decode is level-invariant above the squelch (AGC on vs. off, because
  the LLR/SNR/acquisition estimators are amplitude-ratio-based) and that the AGC's value is QSB
  level-tracking + metering, not sub-squelch rescue. The hard-limiter-correlator option is **rejected**:
  a hard limiter is constant-envelope and destroys the amplitude information the calibrated soft-LLR path
  needs (QAM/APSK), and acquisition is already amplitude-invariant (`search_normalized` / relative
  `refine_onset`), so nothing motivates it. (REQ-AGC-01)
- Every PTT-keyed transmit scope shall release the transmitter **deterministically on scope exit** —
  including on an early return or a panic/unwind — via an RAII guard, rather than relying solely on the
  max-duration watchdog (REQ-REG-10 / #863). This bounds an unexpected key-down to the current stack
  scope instead of up to `ptt_max_duration`. Acceptance: a test that panics inside a keyed transmit scope
  and asserts the transmitter was released without waiting for the watchdog timer. (REQ-PTT-01)
- `openpulse-radio` shall support keying via the **CM108/CM119 sound-chip GPIO over USB-HID** (the common
  cheap-interface PTT path), selectable from config like the existing backends. Acceptance: unit tests
  for the HID output-report encoding; documented in the PTT backend list. (REQ-PTT-02)
- `openpulse-radio` shall support keying via a **Linux GPIO line** (Raspberry Pi header), selectable from
  config, behind a target/feature gate with a mockable line interface. Acceptance: unit-tested report
  path; documented. (REQ-PTT-03)
- A purpose-built **robust narrowband weak-signal waveform** (~500–600 Hz, fading-tolerant) shall be
  evaluated as the sub-floor rung below the current SL floor — the direction chosen over
  frequency-diversity repetition (measured net-negative in #864). Acceptance: a coded frame-success
  bake-off on Watterson good/moderate/poor showing a margin gain over the current floor at matched
  occupied bandwidth, or an honest no-ship finding. (REQ-WSIG-01)
- The receiver shall optionally **decode multiple registered waveforms concurrently** from a single
  capture stream (off the shared `InputCapture` tap) rather than committing to one mode, for a
  discovery/monitor role. Acceptance: a loopback test injecting two different modes into one capture
  buffer and decoding both in one tick. (REQ-RX-01)
- Audio device selection shall be **hotplug-safe**, surviving OS renaming/reorder by keying on a stable
  device identity rather than an ordinal index or path. Acceptance: a test that a configured device
  resolves after a simulated reorder. (REQ-DEV-01)

## Documentation requirements

- Version bumps require updates to docs/dev/project/changelog.md and docs/releasenotes.md.
- Docs files under docs/ must pass frontmatter validation in CI.
- HPX benchmark assumptions and result summaries must be captured in docs/high-performance-mode.md.
