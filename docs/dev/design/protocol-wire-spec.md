---
project: openpulsehf
doc: docs/dev/design/protocol-wire-spec.md
status: living
last_updated: 2026-06-29
---

# Protocol & Handshake Wire Specification

Normative byte-level specification of the OpenPulseHF **data-plane** frames and the **signed
session handshake**. It complements the two companion specs:

- [HPX Session State Machine Specification](../hpx-session-state-machine.md) — the states,
  events, transitions, timing, and security gates that *drive* these frames.
- [Peer Query and Relay Wire Schema](../peer-query-relay-wire.md) — the `OPHF` control-plane
  envelope (peer query, route discovery, relay control). Not repeated here.

The authoritative source is always the code; this document pins the layouts and links each to its
module. Capability IDs (CAP-NN) refer to [traceability-matrix.md](../steering/traceability-matrix.md).

## Conventions

- **Endianness**: multi-byte integers are **big-endian** on the wire unless stated otherwise.
- **Audio context**: 8 kHz mono, centre frequency 1500 Hz (engine defaults); not part of the byte
  layout but assumed by the modulators.
- **Single-frame payload cap**: the base [frame](#1-base-frame-opls) payload is **≤ 255 bytes**.
  Anything larger (PQ handshake, signed classical handshake, multi-block transfers) is carried by
  the [SAR layer](#2-segmentation-and-reassembly-sar).
- A "frame" below is the application/protocol PDU handed to the modem; the plugin then adds its own
  preamble + pulse shaping (out of scope here — see the plugin demods).

---

## 1. Base frame (`OPLS`)

Source: `crates/openpulse-core/src/frame.rs` · CAP-65, CAP-10.

```
┌────────┬─────────┬───────────────────┬─────────────┬──────────┬───────────┐
│ magic  │ version │ sequence (u16 BE) │ length (u8) │ payload  │ CRC-16    │
│ "OPLS" │  0x01   │     2 bytes       │   1 byte    │ 0–255 B  │  2 bytes  │
│  4 B   │  1 B    │                   │             │          │  (BE)     │
└────────┴─────────┴───────────────────┴─────────────┴──────────┴───────────┘
```

| Field | Bytes | Notes |
|---|---|---|
| `magic` | 4 | ASCII `OPLS`; decode rejects any other value |
| `version` | 1 | `0x01`; decode rejects unknown versions |
| `sequence` | 2 | monotonic, wraps at 65535 |
| `length` | 1 | payload length, 0–255 |
| `payload` | 0–255 | application bytes |
| `CRC-16` | 2 | **CRC-16/CCITT**, big-endian, over `magic … payload` (everything before the CRC) |

Header is 8 bytes; min frame (empty payload) is 10 bytes; max is 265 bytes. A payload > 255 returns
`FrameError::PayloadTooLarge` — callers must SAR-fragment first.

---

## 2. Segmentation and reassembly (SAR)

Source: `crates/openpulse-core/src/sar.rs` · CAP-07. Used to carry any PDU larger than one base
frame (the signed handshakes below, multi-block objects).

```
┌───────────────────┬─────────────────────┬─────────────────────┬────────┐
│ segment_id (u16)  │ fragment_index (u8) │ fragment_total (u8) │ data   │
│      2 B (BE)     │       1 B           │       1 B           │ ≤251 B │
└───────────────────┴─────────────────────┴─────────────────────┴────────┘
```

- `SAR_HEADER_SIZE = 4`; `SAR_MAX_FRAGMENT_DATA = 251` (255 − header); `SAR_MAX_SEGMENT_DATA =
  255 × 251 = 64 005` bytes.
- `fragment_index` is 0-based; `fragment_total` is 1–255.
- Reassembly is keyed on `(session_id, segment_id)` with a timeout; duplicate fragments are
  idempotent. Each SAR fragment is itself carried in one base frame's payload.

---

## 3. Signed classical handshake

Source: `crates/openpulse-core/src/handshake.rs` · CAP-01 (+ CAP-05 signing, CAP-04 trust). Driven
by the `discovery` state of the [HPX state machine](../hpx-session-state-machine.md).

Both frames share an outer container, then a JSON body:

```
┌────────┬─────────┬──────────────────┬──────────────────────────┐
│ magic  │ version │ length (u32 BE)  │ JSON body (UTF-8)         │
│ 4 B    │  0x01   │    4 bytes       │ `length` bytes            │
└────────┴─────────┴──────────────────┴──────────────────────────┘
   CONREQ magic = "HSCQ"      CONACK magic = "HSAK"
```

These frames exceed 255 bytes (~530 B with Ed25519 key + signature), so on the modem they are
**SAR-fragmented** ([§2](#2-segmentation-and-reassembly-sar)) and reassembled before decode.

### 3.1 CONREQ body (`HSCQ`)

JSON object — initiator → responder:

| Field | Type | Meaning |
|---|---|---|
| `station_id` | string | initiator callsign |
| `pubkey` | bytes (32) | Ed25519 verifying key |
| `signing_modes` | [SigningMode] | modes the initiator offers |
| `session_id` | string | session identifier (responder must echo) |
| `supported_compression` | [CompressionAlgorithm] | offered compression (empty = none) |
| `supported_fec_modes` | [FecMode] | offered FEC (omitted/empty = none) |
| `station_grid` | string | Maidenhead grid; **omitted when empty** (legacy frames stay byte-identical) |
| `signature` | bytes (64) | Ed25519 signature over the canonical body |

### 3.2 CONACK body (`HSAK`)

JSON object — responder → initiator:

| Field | Type | Meaning |
|---|---|---|
| `station_id` | string | responder callsign |
| `pubkey` | bytes (32) | Ed25519 verifying key |
| `selected_mode` | SigningMode | chosen from the initiator's offered modes |
| `session_id` | string | **must equal** the CONREQ `session_id` |
| `selected_compression` | CompressionAlgorithm | chosen algorithm |
| `selected_fec_mode` | FecMode | chosen FEC (omitted when `None`) |
| `station_grid` | string | responder grid; omitted when empty |
| `signature` | bytes (64) | Ed25519 signature over the canonical body |

### 3.3 Signing & canonical form

- The signature covers the **canonical JSON** of the body fields (excluding `signature`), with keys
  sorted recursively, so any post-signing field injection invalidates it.
- Verification (`verify_conreq` / `verify_conack`): reconstruct canonical JSON → check Ed25519 →
  evaluate trust via `evaluate_handshake`. CONACK additionally requires the echoed `session_id` to
  match and the selected compression/FEC to be one the CONREQ offered.
- Empty `station_grid` is `skip_serializing_if`-omitted, so a zero-grid frame and its signature are
  byte-identical to the pre-grid format.

### 3.4 Daemon RF exchange

The daemon (CAP-63) drives this over RF on `ConnectPeer`: initiator sends CONREQ (SAR), responder
verifies and replies CONACK (SAR), initiator verifies. The verified peer (callsign + grid) is stored
and emitted as `ControlEvent::PeerVerified`; an unanswered handshake times out after 30 s. The
station key is the Ed25519 seed at `[station] identity_key_path`.

---

## 4. Post-quantum handshake

Source: `crates/openpulse-core/src/pq_handshake.rs` · CAP-02. `PqConReq` / `PqConAck` carry classical
+ PQ public keys, the KEM material, and dual signatures, serialized as JSON and SAR-transported
(`encode_pq_conreq`/`decode_pq_conreq`, etc.). Component sizes:

| Component | Bytes | Used in |
|---|---|---|
| ML-DSA-44 public key | 1312 | both frames |
| ML-DSA-44 signature | 2420 | both frames (PQ signature) |
| ML-KEM-768 encapsulation key | 1184 | `PqConReq` |
| ML-KEM-768 decapsulation seed (`d‖z`) | 64 | local only (not on wire) |
| ML-KEM-768 ciphertext | 1088 | `PqConAck` |
| ML-KEM-768 shared secret | 32 | derived (`kem_decapsulate`) |

- `SigningMode::Hybrid` signs with **both** Ed25519 and ML-DSA-44 (defense in depth); `SigningMode::Pq`
  leaves the classical signature empty.
- `PqConAck.kem_ciphertext` lets the initiator recover the 32-byte shared secret.

---

## 5. ACK frame (FSK4)

Source: `crates/openpulse-core/src/ack.rs` · driven by the rate/ACK taxonomy (CAP-32). A fixed
**5-byte** frame, sent on the `FSK4-ACK` waveform (20 symbols @ 100 baud ≈ 200 ms on air).

```
byte 0:  bits[2:0] ACK type │ bit[3] has_reverse_ack │ bit[4] has_recommended_level │ bits[7:5] reserved
bytes 1–2: session_hash (u16 BE)  — 16-bit FNV-1a of session_id (anti-collision)
byte 3:  reverse_ack / recommended_level low bits (backward-compatible; old RX ignores)
byte 4:  CRC-8/SMBUS over bytes 0–3
```

ACK types (byte 0 bits[2:0]):

| Code | Type | Meaning |
|---|---|---|
| `0b000` | AckOk | decoded OK — hold speed |
| `0b001` | AckUp | decoded OK, high margin — step up |
| `0b010` | AckDown | marginal — step down |
| `0b011` | Nack | uncorrectable — retransmit |
| `0b100` | Break | request direction changeover |
| `0b101` | Req | repeat last frame |
| `0b110` | Qrt | graceful end |
| `0b111` | Abort | abnormal teardown |

`session_hash` lets a receiver filter ACKs not addressed to its session. Byte 3 carries the optional
`reverse_ack` (peer's RX-direction quality, for bidirectional sessions) and `recommended_level`
(receiver-led OTA rate control, CAP-33) — both gated by the byte-0 flag bits and ignored by older
receivers while the CRC still validates.

---

## 6. Transfer manifest

Source: `crates/openpulse-core/src/manifest.rs` · CAP-03. A `TransferManifest` carries a SHA-256
payload hash, sender id, and an Ed25519 signature, verified before final acceptance of an object
transfer (the `active_transfer` → completion gate).

---

## 7. Negotiated parameters

### 7.1 Compression (`CompressionAlgorithm`, CAP-08)

| Variant | On-wire framing |
|---|---|
| `None` | payload as-is |
| `Lz4` | LZ4 block + 4-byte **little-endian** decompressed-size prefix |
| `Zstd(dict_id: u32)` | Zstd with the shared HPX dictionary; `dict_id` catches version skew |

Negotiated in the handshake (`supported_compression` → `selected_compression`). A compressed frame
larger than the original is sent uncompressed (`compress_if_smaller`).

### 7.2 FEC modes (`FecMode`, CAP-26 and the soft-FEC caps)

`None`, `Rs`, `RsInterleaved`, `Concatenated`, `ShortRs` (ACK-sized), `RsStrong`, `SoftConcatenated`,
`Ldpc`, `LdpcHighRate`, `Turbo`. Negotiated as `supported_fec_modes` → `selected_fec_mode`. RS modes
bundle a block interleaver into the codec path so every FEC-protected frame is de-bursted by
construction. (Padded OFDM/SC-FDMA modes don't round-trip the hard 255-byte-block RS framing — see
the testmatrix note in the traceability matrix.)

### 7.3 Trust & signing modes (`trust.rs`, CAP-04)

Signing modes `Normal` / `Psk` / `Pq` / `Hybrid` (increasing strength; PQ=4, Hybrid=5). Trust levels
`Verified` / `PskVerified` / `Unknown` / `Reduced` / `Revoked`; policy profiles `Strict` / `Balanced`
/ `Permissive` set the minimum acceptable trust. See the HPX spec's *Security Gates* section.

---

## Cross-references

| Layer | Spec / source |
|---|---|
| Session lifecycle (states, transitions, timing) | [hpx-session-state-machine.md](../hpx-session-state-machine.md) |
| Peer query / route discovery / relay control (`OPHF`) | [peer-query-relay-wire.md](../peer-query-relay-wire.md) |
| Base frame / SAR / handshake / ACK / manifest byte layouts | this document + the cited `crates/openpulse-core/src/*.rs` |
| Capability → implementation → tests | [traceability-matrix.md](../steering/traceability-matrix.md) |
