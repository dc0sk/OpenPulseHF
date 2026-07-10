---
project: openpulsehf
doc: docs/dev/design/file-transfer-plan.md
status: approved-plan (decisions D1–D5 locked 2026-07-10; not yet implemented)
last_updated: 2026-07-10
---

# Direct P2P file transfer — design & implementation plan

**Source requirement**: `docs/dev/research/varac-feature-gap-analysis.md` §4.1 (PR #729) ranked
direct P2P file transfer (VarAC V9) as the highest value-to-effort missing feature and recommended
it as the next build. This document is the engineering plan: the offer/accept/transfer/verify flow,
the wire protocol, the crate/daemon/panel wiring, and PR-sized milestones. **Planning only —
nothing here is implemented.**

Proposed requirement IDs (for the acceptance table and `docs/dev/project/traceability.md` ledger):
**REQ-FT-01** (offer/accept flow), **REQ-FT-02** (chunked reliable delivery), **REQ-FT-03**
(cryptographic verification), **REQ-FT-04** (size-gated auto-accept + operator prompt),
**REQ-FT-05** (progress reporting), **REQ-FT-06** (safe storage: sanitization/quota),
**REQ-FT-07** (resume).

---

## 1. Summary, goals, non-goals

An operator sends a file (document, image, arbitrary binary) to a connected peer over HF. The
receiver sees an offer (name, size, SHA-256, mime), auto-accepts under a configured size gate or
prompts the operator, receives the file chunked over the existing SAR + ARQ machinery with live
progress on both ends, then cryptographically verifies it — Ed25519 signature over the SHA-256
payload hash, bound to the peer identity proven by the signed CONREQ/CONACK handshake. The file
lands in a download directory with a **verify badge**. That badge is the differentiator: VarAC has
size-threshold auto-accept but *no* integrity/authenticity verification; we get it nearly for free
from `crates/openpulse-core/src/manifest.rs`.

The gap analysis is right that ~90 % of the substrate exists. This plan is mostly **assembly**:

| Layer | Existing substrate (verified in-tree) |
|---|---|
| Chunking | `sar_encode()` / `SarReassembler` — `crates/openpulse-core/src/sar.rs` |
| Integrity + provenance | `TransferManifest::sign()` / `verify_manifest_with_payload()` — `crates/openpulse-core/src/manifest.rs` |
| Compression | self-describing `pack()`/`unpack()` (`PACK_MAGIC = "OPZ1"`) — `crates/openpulse-core/src/compression.rs` |
| Reliable delivery + rate | OTA receiver-led ladder (`engine.rs` `ota_*`, `ota_send_with_ptt` in `crates/openpulse-daemon/src/server.rs`), HARQ (`crates/openpulse-modem/src/harq.rs`), soft combining (`receive_with_llr_combining`) |
| Identity | signed CONREQ/CONACK + `VerifiedPeer { callsign, grid, pubkey, .. }` — `crates/openpulse-core/src/handshake.rs`, `crates/openpulse-daemon/src/lib.rs` |
| Operator surface | daemon `ControlCommand`/`ControlEvent` NDJSON protocol (`protocol.rs`), panel tabs (`apps/openpulse-panel`) |

**Goals**

- G1: send/receive a file inside (or alongside) an established peer session, offer → accept →
  transfer → verify, with progress events at both ends (REQ-FT-01/02/05).
- G2: cryptographic verification of every received file: signature validity **and** payload-hash
  match, bound to the handshake-verified peer key (REQ-FT-03).
- G3: size-gated auto-accept with an operator prompt above the gate; reject/cancel/timeout paths
  (REQ-FT-04).
- G4: safe-by-construction storage: filename sanitization, size/quota limits, per-peer directories
  (REQ-FT-06).
- G5: everything testable under `cargo test --workspace --no-default-features` (no audio hardware),
  including a full modem-loopback file round-trip and a tamper test.
- G6 (post-MVP): resume after a dropped session; selective retransmission of missing chunks
  (REQ-FT-07).

**Non-goals**

- Not Winlink/CMS: no B2F/CMS interop for this path (`openpulse-b2f` / `openpulse-gateway` stay the
  mail path). §3.4 explains reuse vs. separation.
- Not a filesystem sync, BBS, or file-hosting service (VarAC V25 is explicitly deferred in the gap
  analysis — big attack surface).
- Not VarAC wire-format interop (closed, undocumented — "do not copy" list, gap analysis §5).
- No content encryption by default (prohibited over amateur RF in most jurisdictions; §9). Signing
  always, encryption never by default.
- No multi-transfer concurrency in v1: one active transfer per RF link (state machine enforces;
  concurrent offers are rejected with reason `busy`).

---

## 2. User story & UX flow

**Sender** (panel Files tab, or `SendFile` on the control port):

1. Operator enters/picks a file path and the peer callsign (pre-filled from the connected peer),
   presses **Send**. Daemon reads the file, enforces `max_file_bytes`, computes SHA-256, signs a
   `TransferManifest` with the station seed (`RuntimeControlState::station_seed`), and transmits a
   `FileOffer`.
2. Panel shows the transfer card: `offer sent — waiting for accept` with a timeout countdown
   (`offer_timeout_secs`).
3. On `FileAccept`: blocks stream out; the card shows a progress bar (`blocks 12/40 · 30 %`,
   effective B/s from the existing `Metrics.effective_bps`), plus the estimated time remaining.
4. On `FileComplete(status=verified_ok)`: card flips to `delivered ✓ verified by peer` (the
   completion carries the receiver's countersignature — a delivery receipt). Reject/cancel/timeout
   flip it to the corresponding terminal state.

**Receiver**:

1. Daemon receives the `FileOffer`, checks policy: feature enabled? size ≤ `max_file_bytes`? peer
   quota? offer signature valid against the handshake-verified pubkey (or policy for unverified
   peers)?
   - Policy fail → automatic `FileReject(reason)` on air, `FileFailed` event locally.
   - `size ≤ auto_accept_max_bytes` → automatic `FileAccept`; event `FileOffered { auto_accepted:
     true }`.
   - Otherwise → event `FileOffered { auto_accepted: false }`; the panel raises an accept/reject
     prompt (like the existing QSY accept/reject pair, `Message::AcceptQsy`/`RejectQsy` in
     `apps/openpulse-panel/src/app.rs`). No answer within `offer_timeout_secs` → automatic
     `FileReject(reason=timeout)`.
2. During transfer: `FileProgress` events; the panel card mirrors the sender's.
3. On completion: reassemble → `unpack()` per block → concatenate →
   `verify_manifest_with_payload(manifest, peer_pubkey, payload)`. Then:
   - **verified** → write to `download_dir/<PEERCALL>/<sanitized-name>`, emit
     `FileReceived { verified: true, path }`, send `FileComplete(verified_ok + countersignature)`.
   - **hash/signature mismatch** → do **not** delete silently: write to
     `download_dir/<PEERCALL>/quarantine/<name>.unverified`, emit `FileReceived { verified: false }`
     and send `FileComplete(hash_mismatch)`. The operator sees a red **UNVERIFIED** badge. (An
     RS/CRC-clean transfer that fails the manifest is either sender-side corruption or an imposter —
     both are worth surfacing, not hiding.)
4. Files tab lists received files with name/size/peer/date and the verify badge.

**Edge paths** (all first-class states in §3.3): reject (either policy or operator), cancel (either
side, any time, `FileCancel` on air), offer timeout, block-level retransmission on loss, transfer
stall timeout (no progress for `transfer_stall_secs` → abort + `FileFailed`), resume (phase E: a
re-offer of the same SHA-256 is answered by `FileAccept` carrying a bitmap of blocks already held).

---

## 3. Architecture

### 3.1 Where the logic lives

**New pure crate `crates/openpulse-filexfer`** — a no-I/O, no-tokio, no-modem-dependency protocol
library, exactly the `openpulse-b2f` / `openpulse-qsy` pattern (`B2fSession` and `QsySession` are
both sans-I/O state machines whose callers perform the transmission; the daemon already drives
`QsySession` from `process_received_bytes`, `crates/openpulse-daemon/src/lib.rs:1131`). Contents,
one concept per file:

```
crates/openpulse-filexfer/
├── src/lib.rs        # re-exports, FILEXFER_MAGIC, protocol version const
├── src/wire.rs       # FxFrame enum + binary encode/decode (§4)
├── src/offer.rs      # FileOfferInfo: metadata + embedded TransferManifest + policy evaluation
├── src/blocks.rs     # file ⇄ blocks: split, pack(), block/fragment bookkeeping, bitmaps
├── src/sender.rs     # SenderSession state machine (feed frames in, get FxAction out)
├── src/receiver.rs   # ReceiverSession state machine (incl. fragment-bitmap tracking)
├── src/sanitize.rs   # filename sanitization (§9)
└── src/error.rs      # FxError via thiserror
```

Dependencies: `openpulse-core` (for `sar`, `manifest`, `compression`), `serde`, `thiserror`. No
`unwrap()`/`expect()` in production paths (library-crate rule).

Both session types follow the action-queue idiom: `apply(frame, now_ms) -> Result<Vec<FxAction>,
FxError>` plus `poll_timeout(now_ms)`, where `FxAction` is e.g. `TransmitFrame(Vec<u8>)`,
`TransmitBlockFragments { block, missing: Vec<u8> }`, `EmitProgress {..}`, `WriteFile {..}`,
`Finished {..}`. Time is injected (`now_ms: u64`) so the state machines are fully deterministic in
tests — same reason `StationIdTimer` takes a ms clock.

**Daemon glue** — a new `crates/openpulse-daemon/src/filexfer.rs` module (the `logbook.rs` pattern):
owns file I/O, storage policy, quota accounting, and the mapping between `FxAction`s and engine
calls / `ControlEvent`s. `RuntimeControlState` gains the active session + policy fields (§6).

**Why a separate crate and not a module in `openpulse-core`**: core is already the grab-bag; the
protocol has enough surface (2 state machines, 8 frame types, block math) to deserve the
one-crate-per-protocol treatment the repo uses for b2f/qsy/mesh, and it keeps the file-transfer
tests (`tests/` with long-running loopback cases) out of core's test wall-clock.

### 3.2 Layering

```
 operator          ControlCommand::SendFile / AcceptFile / RejectFile / CancelFile
    │                                  │ (NDJSON control port)
    ▼                                  ▼
 openpulse-panel  ◄── ControlEvent ── openpulse-daemon (filexfer.rs glue: file I/O, policy, quota)
                                       │        ▲
                              FxAction │        │ FxFrame / raw SAR fragments
                                       ▼        │
                              openpulse-filexfer (pure state machines)
                                       │
             ┌─────────────────────────┼───────────────────────────┐
             ▼                         ▼                           ▼
   TransferManifest            pack()/unpack()               sar_encode()/
   (manifest.rs, Ed25519       per block (compression.rs)    SarReassembler (sar.rs)
    over SHA-256)                                            251 B fragments
             └─────────────────────────┬───────────────────────────┘
                                       ▼
                    ModemEngine transmit_with_fec_mode / accumulate_capture
                    + OTA rate ladder + FSK4 per-burst ACK + HARQ retry FEC
                    (engine.rs, harq.rs, ota_rate.rs — unchanged)
```

Nothing below the daemon changes. The modem engine, OTA ladder, HARQ policy, and FSK4 ACK loop are
consumed exactly as `SendMessage` consumes them today (`ota_send_with_ptt`,
`crates/openpulse-daemon/src/server.rs:878`).

### 3.3 State machines

**Sender** (`SenderSession`):

```
                 SendFile cmd
                     │  read file, sign manifest, split blocks
                     ▼
                ┌──────────┐  FileReject / offer timeout / FileCancel
   ┌────────────│ Offering │──────────────────────────────► Failed(reason)
   │            └──────────┘
   │ FileAccept(resume_bitmap)
   │  (bitmap ⇒ skip held blocks)
   ▼
┌────────────────┐  per block: TX fragments (missing-only on retry)
│ SendingBlock k │◄────────────────────────────┐
└────────────────┘                             │ BlockAck(missing ≠ ∅),
   │ BlockAck(missing = ∅)                     │ retry < max_block_retries
   │   k+1 ≤ n ──► SendingBlock k+1 ───────────┘
   │ k = n (all blocks acked)                  │ retry exhausted / stall timeout
   ▼                                           ▼
┌────────────┐  FileComplete(status)        Failed(delivery)
│ AwaitVerify│───────────────► Done { peer_verified: bool }
└────────────┘  verify-wait timeout ─► Done { peer_verified: unknown }
        any state: FileCancel (rx or local) ─► Cancelled
```

**Receiver** (`ReceiverSession`):

```
              FileOffer received
                     │ policy check (enabled? size? quota? peer? sig?)
        ┌────────────┼──────────────────────────────┐
        ▼            ▼                              ▼
  auto-reject   size ≤ auto_accept            operator prompt
  (TX FileReject)     │                        │ Accept  │ Reject / timeout
        │             ▼                        ▼         ▼
     Failed     ┌───────────┐◄─────────────────┘   TX FileReject ─► Rejected
                │ Receiving │  ingest fragments; per block:
                └───────────┘  track fragment bitmap; on block edge /
                     │         gap timer → TX BlockAck(missing)
                     │ all blocks complete
                     ▼
                ┌──────────┐ unpack per block, concat,
                │ Verifying│ verify_manifest_with_payload()
                └──────────┘
                  │ ok                       │ hash/sig mismatch
                  ▼                          ▼
        Done{verified:true}         Done{verified:false, quarantined}
        TX FileComplete(ok+countersig)  TX FileComplete(mismatch)
   any state: FileCancel / stall timeout ─► Cancelled/Failed (slots expired via SarReassembler::expire)
```

Every edge above (reject, offer timeout, cancel from either side, per-block NACK/retransmit, stall,
verify-fail, resume-accept) is a named transition with a pure unit test in phase A.

### 3.4 Why not reuse B2F proposal frames

`crates/openpulse-b2f` already has a propose/accept shape: `B2fFrame::Fc {proposal_type, mid, size,
date}` → `Fs {answers}` → data → `Ff` (`src/frame.rs`), driven by `B2fSession` (Iss/Irs). It is
tempting but wrong for this feature:

- FC/FS are **CR-terminated ASCII Winlink wire format** — no hash, no signature, no filename field,
  no mime, no block/fragment addressing, no resume. Extending them breaks the one thing they exist
  for (CMS interop, `gateway_round_trip` test).
- B2F's transfer unit is a whole compressed message over a stream (TCP or the ARDOP data port), not
  ARQ'd radio fragments.
- The session driver (`openpulse-b2f-driver`) assumes the ARDOP TCP framing.

What we **do** take from B2F is the pattern: pure sans-I/O session crate, proposal/answer state
machine, driver-side I/O. The wire format is new, binary, signed, and documented in
`docs/dev/design/protocol-wire-spec.md` (register it there in phase A — Germany's §12 AFuV
"determinable emissions" requirement is satisfied by exactly this openness, `docs/regulatory.md`).

---

## 4. Wire protocol

### 4.1 Magic and framing discipline

Existing on-air/wire magics (all 4-byte, checked): `OPLS` (modem frame, `core/src/frame.rs`),
`OPSE` (signed envelope, `signed_envelope.rs`), `OPHF` (query/relay envelope, `wire_query.rs`),
`OPZ1` (packed compression frame, `compression.rs`), `OPSP` (spectrum, daemon `protocol.rs`),
`HSCQ`/`HSAK` (handshake), ASCII `QSY ...` lines, and the `0xA5` FEC retransmit-map marker
(`fec.rs:714`). New magic: **`OPFX`** ("OpenPulse File Xfer") — no collision, and
`compression::unpack()` passes it through untouched (its magic check fails ⇒ `None` ⇒ caller keeps
bytes), exactly like the `OPHF`/`HSCQ`/`QSY` cases in its `unpack_passes_through_non_packed_frames`
test.

**Every `FxFrame` is SAR-encoded before transmission** — even single-fragment control frames —
mirroring `transmit_handshake_frame` (`daemon/src/lib.rs:1205`), so the receive path has exactly one
seam: raw modem payload → SAR reassembly → magic dispatch. Frame layout after reassembly:

```
OPFX (4) | ver (1) = 0x01 | type (1) | body…
```

Unknown `ver` → `FileReject(reason=unsupported_version)` if it is an offer, else drop + log.

### 4.2 Frame types

All integers big-endian (matches `frame.rs`/`sar.rs` convention). Strings are
`len(u8) | UTF-8 bytes`.

| type | name | body layout | size |
|---|---|---|---|
| 0x01 | `FileOffer` | `transfer_id u32 \| flags u8 \| file_size u64 \| sha256 [32] \| block_size u32 \| block_count u16 \| sender_id str≤16 \| name str≤48 \| mime str≤24 \| signature [64]` | ≤ 212 B → 1 fragment |
| 0x02 | `FileAccept` | `transfer_id u32 \| have_len u16 \| have_bitmap [have_len]` (bitmap of blocks already held; empty in v1, used for resume in phase E) | 7 B + bitmap |
| 0x03 | `FileReject` | `transfer_id u32 \| reason u8` | 5 B |
| 0x04 | `FileData` | `transfer_id u32 \| block_index u16 \| packed block bytes…` — this whole frame is one SAR segment (§4.4) | ≤ block_size + 12 |
| 0x05 | `BlockAck` | `transfer_id u32 \| block_index u16 \| complete u8 \| missing_len u8 \| missing_frag_bitmap [missing_len]` (≤ 32 B for 255 fragments) | ≤ 40 B |
| 0x06 | `FileComplete` | `transfer_id u32 \| status u8 \| countersignature [64]` (zeroed unless status = 0) | 69 B |
| 0x07 | `FileCancel` | `transfer_id u32 \| reason u8` | 5 B |
| 0x08 | *(reserved: `FileResume` — folded into `FileAccept.have_bitmap`; keep the code point free)* | | |

`reason` codes (shared by Reject/Cancel): `0` operator-declined, `1` feature-disabled, `2`
too-large, `3` quota-exceeded, `4` busy (transfer already active), `5` untrusted-peer, `6` timeout,
`7` unsupported-version, `8` operator-cancel, `9` stall. `FileComplete.status`: `0` verified-ok,
`1` hash-mismatch, `2` signature-invalid, `3` size-mismatch.

`transfer_id` is a random `u32` chosen by the sender; the receiver namespaces it by peer, and v1's
one-transfer-per-link rule makes collisions moot.

### 4.3 Manifest exchange and verification order

`FileOffer` **embeds the `TransferManifest` fields inline** (`sha256` = `payload_hash`, `file_size`
= `payload_size`, `sender_id`, `signature` — the four fields of
`crates/openpulse-core/src/manifest.rs::TransferManifest`), rather than shipping the serde-JSON
struct, to fit one fragment. The daemon reconstructs the struct and calls the existing functions —
no new crypto code:

1. **Before accepting** (offer time): `verify_manifest(&manifest, &peer_pubkey)` — signature over
   the canonical JSON body. `peer_pubkey` comes from `RuntimeControlState::verified_peer`
   (`VerifiedPeer::pubkey`, proven by the signed CONREQ/CONACK — `handle_inbound_conreq` /
   `verify_conreq` in `daemon/src/lib.rs`). This authenticates *who is offering what hash* before a
   single data byte is accepted, and pins the expected hash + size.
   - If no verified handshake exists: policy `require_verified_peer` (default **true**) → reject
     with `untrusted-peer`. If set to `false`, the transfer proceeds but can never earn the
     verified badge (badge shows "unsigned peer") — mirrors the trust-classification levels in
     `core/src/trust.rs`.
2. **After reassembly**: `verify_manifest_with_payload(&manifest, &peer_pubkey, &payload)` — the
   fail-closed both-checks entry point. This is the badge.
3. **Receipt**: on `verified_ok` the receiver countersigns the same canonical manifest body with its
   own `station_seed` and returns the signature in `FileComplete` — implementing the "both peers
   compute and sign a manifest" intent already documented on `TransferManifest`. The sender verifies
   it against the pubkey from the peer's CONACK and shows "delivery receipt ✓".

Manifest hash/size are computed over the **original file bytes** (pre-compression), so verification
is independent of per-block compression choices.

### 4.4 Chunk sizing, the 64 005-byte SAR cap, and multi-object framing

Constraints (all from code, all load-bearing):

- One modem `Frame` payload ≤ 255 B (`frame.rs`, u8 length) ⇒ one SAR fragment carries ≤ 251 B data
  (`SAR_MAX_FRAGMENT_DATA`).
- One SAR segment ≤ 255 fragments ⇒ ≤ 64 005 B (`SAR_MAX_SEGMENT_DATA`).
- `compression::unpack()` refuses claimed sizes > `MAX_DECOMPRESSED_SIZE` = 64 005 B.

A file larger than ~64 KB therefore **cannot** be one SAR object. Design: **the block is the
multi-object unit**.

- The file is split into **blocks** of `block_size` bytes (offer field; default **16 384**,
  bounded `1024..=49 152` so that `pack()` output + the 12-byte `FileData` header can never exceed
  either 64 005 cap, even for incompressible data where `pack()` adds 5 bytes).
- Each block is independently `pack()`ed (per-block compression keeps damage localized, keeps every
  unit under `MAX_DECOMPRESSED_SIZE`, and lets incompressible blocks pass through at +5 B — no
  negotiation needed, the `OPZ1` frame is self-describing).
- Each `FileData` frame (header + packed block) is one **SAR segment**:
  `sar_encode(segment_id, frame_bytes)` with `segment_id = block_index + 1`. **Segment-id 0 stays
  reserved for handshake frames** (`transmit_handshake_frame` uses `sar_encode(0, ..)`), so file
  fragments and handshake fragments can never land in the same reassembly slot even though they
  share the receive path (§6.2).
- `block_count` is `u16` ⇒ hard protocol ceiling 65 535 × 48 KiB ≈ 3 GiB — never the binding limit;
  the *config* cap (`max_file_bytes`, default 1 MiB) and HF airtime (§5.4) are.
- Fragment addressing inside a block is SAR's own `fragment_index/fragment_total` header; the
  receiver tracks a per-block fragment bitmap by peeking the public 4-byte SAR header **before**
  `ingest()` (no `SarReassembler` API change needed), which is what feeds `BlockAck.missing_frag_bitmap`.

### 4.5 How frames ride the modem

Identical to handshake frames today: each SAR fragment is passed to `engine.transmit(...)` /
`engine.transmit_with_fec_mode(...)` as an opaque payload; the engine wraps it in the `OPLS` frame
envelope with CRC-16 and modulates at the active/OTA mode. On receive, the daemon's rx tick
(`server::run`) yields decoded payload bytes, `compression::unpack()` passes OPFX/SAR bytes through
untouched, and `process_received_bytes` routes them (§6.2). No engine changes.

---

## 5. Reliable delivery & resume

### 5.1 Two ACK planes, cleanly layered

The existing OTA machinery already provides per-burst reliability + rate adaptation; the file layer
adds only *block-level* integrity. Concretely:

- **Rate/link plane (existing, untouched)**: when an OTA session is active, every data burst the
  receiver decodes triggers the FSK4 short-FEC ACK with the absolute `recommended_level`
  (`ota_decode_burst` + `transmit_ack_with_short_fec` in the daemon rx tick, `server.rs`), and the
  sender adopts it via `receive_ack_with_short_fec_within` + `apply_ota_ack` — the
  `ota_send_with_ptt` loop verbatim. File fragments simply *are* data bursts, so the transfer
  surfs the receiver-led ladder (`openpulse_core::ota_rate::OtaRateController`) and inherits HARQ
  retry-FEC escalation (`HarqPolicy::select`, `harq.rs`) with zero new code. Without OTA, fragments
  go out at the fixed `active_mode` with the session FEC — same as `SendMessage`'s fixed-mode branch
  in `apply_command_to_engine` (`lib.rs:1690`).
- **File plane (new)**: per-block `BlockAck` with a fragment bitmap. The receiver emits it when
  (a) the block's SAR segment completes (`missing = ∅` ⇒ sender advances), or (b) a gap timer fires
  after the last fragment of a burst with the block still incomplete (⇒ selective retransmission of
  exactly the missing fragments). Duplicate fragments are already idempotent
  (`SarReassembler::ingest`), so retransmit overshoot is harmless.

**MVP simplification (phase C ships this)**: stop-and-wait per block — the sender transmits all
fragments of block *k* (split into PTT bursts, §5.3), waits for `BlockAck`, retransmits missing
fragments up to `max_block_retries` (default 4, HARQ escalates FEC across retries when OTA is on),
then advances. Windowed multi-block pipelining is deliberately out of scope until the twin-harness
numbers say the turnaround is the bottleneck.

**HARQ soft combining**: retransmitted fragments are the same bytes at possibly stronger FEC, so the
receiver-side `receive_with_llr_combining` path (union of standalone-decode and MAP LLR sum —
CLAUDE.md "take the union" note, PR #694) applies unchanged whenever the engine's OTA receive is in
play. Nothing file-specific to build; one twin-harness test asserts a fragment lost at attempt 1
decodes at attempt 2 on a Watterson channel.

### 5.2 Timeouts and backpressure

- Offer: `offer_timeout_secs` (default 120) on both ends.
- Block stall: no `BlockAck` after the final retry window → `Failed(stall)` + `FileCancel` on air.
- Receiver slot hygiene: the file reassembler is a dedicated `SarReassembler::new(transfer_stall)`
  whose `expire()` runs on the daemon rx tick, like `handshake_sar` does today.
- Backpressure is structural: stop-and-wait means at most one block (≤ 255 fragments) of sender
  state and one reassembly slot of receiver state is in flight; memory is bounded by
  `max_file_bytes` (the sender holds the file; the receiver holds completed blocks on disk or in a
  `Vec` — v1 keeps it in memory, acceptable at a 1 MiB cap).

### 5.3 PTT, duty cycle, and burst sizing

`RuntimeControlState::ptt_max_duration` (180 s watchdog, `check_ptt_watchdog`) is a hard ceiling on
one keying. The sender splits a block's fragments into bursts of `fragments_per_burst`, computed so
that burst airtime ≈ `burst_max_secs` (config, default 20 s) at the current mode's bit rate:
`fragments_per_burst = clamp(burst_max_secs × bitrate / (267 × 8), 1, 64)` (267 = 251 data + SAR 4 +
OPLS overhead 12). Each burst is PTT-keyed and released like `ota_send_with_ptt` — release before
listening is what lets the FSK4 ACK and `BlockAck` come back on a half-duplex radio. This also keeps
individual transmissions courteous on a shared frequency and leaves airtime for the periodic
station ID (§10).

### 5.4 Throughput expectations and the practical ceiling

Raw mode bit rates → effective goodput after FEC (× ~0.5–0.87, `code_rate_for_fec` in
`crates/openpulse-modem/src/harq.rs`), framing (× ~0.9), and half-duplex turnaround at 20 s bursts + ~1.5 s
ACK/turnaround (× ~0.85–0.93). Honest planning numbers (validate with `apps/openpulse-linksim`,
which measures exactly this — effective two-way transfer rate with FSK4 ACKs, turnaround,
retransmission, and OTA rate adaptation):

| Ladder rung (example) | raw bps | est. goodput | 15 KB file | 100 KB file |
|---|---|---|---|---|
| BPSK31 (SL2 floor) | 31 | ~10 B/s | ~25 min | impractical |
| BPSK250 | 250 | ~90 B/s | ~3 min | ~19 min |
| QPSK500 | 1 000 | ~350 B/s | ~45 s | ~5 min |
| 8PSK1000 | 3 000 | ~1.0 kB/s | ~15 s | ~100 s |
| SCFDMA52-64QAM / 64QAM2000-RRC (top rungs) | ~8–12 k | ~2.5–4 kB/s | ~5 s | ~30–40 s |

Consequences baked into the design: default `max_file_bytes` = 1 MiB (hard), default
`auto_accept_max_bytes` = 0 (always prompt), panel shows the airtime estimate **before** the
operator confirms a send (size ÷ current `effective_bps` from `ControlEvent::Metrics`), and the docs
advise ≤ 15–30 KB on marginal channels — the same practical envelope VarAC states (~100 KB cap,
15 KB advised). A transfer that would exceed ~30 min of estimated airtime at the current rate gets a
panel warning, not a hard block.

### 5.5 Resume (phase E)

Keyed on content, not session: `FileAccept.have_bitmap`. After a dropped link, the sender re-offers
the *same* file (`transfer_id` fresh, `sha256` identical). The receiver, which persisted completed
blocks (`download_dir/<peer>/.partial/<sha256-hex-16>.blocks` + a tiny index file), recognizes the
hash and answers `FileAccept` with the bitmap of blocks already verified-complete; the sender skips
those. Block-level granularity (16 KiB default) is the deliberate compromise: fragment-level resume
would require persisting partial SAR slots for negligible gain (≤ 16 KiB per interrupted block).
Partial files are purged by age (`partial_ttl_hours`, default 72) and never counted verified until
the final manifest check passes over the concatenation.

---

## 6. Daemon integration

### 6.1 Control protocol (`crates/openpulse-daemon/src/protocol.rs`)

New variants, mirroring the existing serde shapes (`#[serde(tag = "cmd", rename_all =
"snake_case")]` for commands, `tag = "type"` for events) — and honoring the KEEP-IN-SYNC coupling
between `protocol.rs` and the `lib.rs` handlers:

```rust
// ControlCommand additions
/// Offer a file to the connected peer. `path` is daemon-host-local (§13 D3).
SendFile { to: String, path: String },
/// Accept a pending inbound file offer.
AcceptFile { transfer_id: u32 },
/// Reject a pending inbound file offer.
RejectFile { transfer_id: u32 },
/// Cancel the active transfer (either direction).
CancelFile { transfer_id: u32 },
/// List received files (server replies FileList + ok CommandResponse).
ListFiles,

// ControlEvent additions
/// An inbound offer needs (or received) a decision.
FileOffered { transfer_id: u32, from: String, name: String, size: u64,
              sha256_hex: String, mime: String, auto_accepted: bool,
              signature_valid: bool },
/// Periodic + on-block-edge progress, both directions.
FileProgress { transfer_id: u32, direction: String /* "tx"|"rx" */,
               name: String, blocks_done: u16, blocks_total: u16,
               bytes_done: u64, bytes_total: u64 },
/// Terminal: file landed on disk (rx side).
FileReceived { transfer_id: u32, from: String, name: String, size: u64,
               path: String, verified: bool },
/// Terminal: peer confirmed the transfer (tx side). `receipt_valid` = countersignature verified.
FileSent { transfer_id: u32, to: String, name: String, receipt_valid: Option<bool> },
/// Terminal failure/rejection/cancel, both directions.
FileFailed { transfer_id: u32, direction: String, reason: String },
/// Response to ListFiles (requesting client only, like MessageList).
FileList { files: Vec<FileSummary> },
```

plus `pub struct FileSummary { pub name: String, pub from: String, pub size: u64, pub verified:
bool, pub path: String, pub timestamp_secs: u64 }` next to `MessageSummary`. A serde round-trip test
mirrors `ota_commands_round_trip_via_json`.

Command routing copies the `SendMessage` split: `handle_command` (in `lib.rs`, the control-server
side) validates + replies `CommandResponse` + forwards via `ctx.cmd_tx` (exactly `lib.rs:879`'s
pattern); the side-effectful work happens where the engine and PTT live.

### 6.2 Receive-path routing (`process_received_bytes`)

Today's order (`lib.rs:1131`): relay-forward attempt → QSY ASCII parse → *everything else* feeds
`try_reassemble_handshake` → dispatch on `HSCQ`/`HSAK` magic. Change: generalize the tail into
`try_reassemble_sar(bytes, ..)` that ingests into **one shared reassembler pipeline** and dispatches
the *reassembled* segment on magic:

- starts with `HSCQ`/`HSAK` → existing `handle_inbound_conreq/conack` (unchanged);
- starts with `OPFX` → `filexfer::handle_frame(...)` (offer/accept/data/ack/complete/cancel);
- else → debug-log drop (today's behavior).

Fragment-level disambiguation is the segment-id reservation from §4.4 (handshake = 0, file blocks =
`block_index + 1`; OPFX *control* frames, which are single-fragment, use segment-id `0xFFFF` to
stay clear of both). Keep two `SarReassembler` instances (`handshake_sar` with
`HANDSHAKE_TIMEOUT`, new `filexfer_sar` with the transfer stall timeout) but route by segment-id
range before `ingest()` — the 4-byte header is public layout, so peeking `payload[0..2]` is spec'd,
not a hack. This preserves the handshake path bit-for-bit (its tests must not change) while giving
the file path its own timeout policy. This is the "wire at the seam, not a caller" rule from the
seam-gap audits: file frames arrive through the same production entry (`accumulate_capture` → rx
tick → `process_received_bytes`) as everything else, and the tests drive that entry (§11).

### 6.3 RuntimeControlState and send-side driving in `server::run`

`RuntimeControlState` additions (following the `logbook`/`handshake_sar` precedents):

```rust
/// Active file transfer (at most one per link in v1).
pub file_tx: Option<openpulse_filexfer::SenderSession>,
pub file_rx: Option<openpulse_filexfer::ReceiverSession>,
/// Reassembles inbound OPFX SAR segments (segment-id ≥ 1).
pub filexfer_sar: SarReassembler,
/// Policy snapshot from [file_transfer] config.
pub filexfer_policy: FileTransferPolicy,   // enabled, gates, dirs, quotas (from openpulse-config)
```

**Send driving** lives in `server::run`'s command arm, beside the `SendMessage` OTA interception
(`server.rs:614-651`) — for the same reason stated there: this is where `ptt_controller` lives, and
the half-duplex turnaround must sequence PTT around each burst. A `Command::SendFile` is intercepted
before `apply_command_to_engine`:

1. Read + validate the file (size cap, readable), sign the manifest with
   `runtime_state.station_seed`, build `SenderSession`, transmit the `FileOffer` (SAR + per-fragment
   `engine.transmit_with_fec_mode(..)`; the small control frames — offer, reject, ack, complete,
   cancel — fit `FecMode::ShortRs`'s ≤ 213 B budget even after the 4-byte SAR wrap, name/mime caps
   in §4.2 were chosen for exactly this; data fragments use the session/HARQ FEC).
2. Subsequent progress is driven from the **rx tick**: inbound `FileAccept`/`BlockAck` frames arrive
   via `process_received_bytes`, which returns/queues `FxAction`s; a small
   `drive_filexfer(&mut engine, &mut ptt_controller, &mut runtime_state, &event_tx)` step in the rx
   tick (right after `process_received_bytes`, before the station-ID block) executes pending
   actions: key PTT, transmit the next burst of fragments at `engine.ota_tx_mode()/ota_tx_fec()`
   (or fixed mode), release PTT, listen for the FSK4 ACK (`receive_ack_with_short_fec_within` +
   `apply_ota_ack`) when OTA is active — i.e., `ota_send_with_ptt`'s body, refactored to be callable
   per burst. Timeouts fire from the same tick via `poll_timeout(now_ms)`.

This keeps the select loop's shape: commands stay cheap, all long-running radio work happens inside
`block_in_place` on the rx tick exactly as OTA ACK replies do today.

**Coexistence**: file transfer requires no new lockouts — QSY frames still parse first in
`process_received_bytes` (a mid-transfer QSY simply pauses progress; block stall timers are sized ≥
QSY switchover), the repeater/mesh paths are orthogonal, and OTA rate stepping keeps running
underneath (that's the point). The one-transfer-per-link rule is enforced in the state machine
(`busy` reject). `ota_suppressed_by_peer()` (diverged ladders) degrades file transfer to fixed-mode
exactly as it degrades `SendMessage`.

### 6.4 PTT sequencing summary

Per burst: `ptt.assert_ptt()` → `PttChanged{active:true}` → transmit fragments → `ptt.release_ptt()`
→ `PttChanged{active:false}` → listen window. The existing PTT watchdog and the station-ID timer
(§10) require no changes — both key off `frames_transmitted` deltas and wall-clock, and file bursts
are ordinary transmissions to them.

---

## 7. Panel UX (`apps/openpulse-panel`)

### 7.1 Wiring

- `app.rs`: add `Files` to the `Tab` enum (`Tab { Info, Stats, Config, Messages, Files, Log }`,
  app.rs:38) and its button to the tab strip (ui.rs:845–866); dispatch `Tab::Files =>
  files_widget(app, snap, eff)` in the match at ui.rs:869.
- `app.rs` `Message` additions: `FilePathChanged(String)`, `SendFile`, `AcceptFileOffer(u32)`,
  `RejectFileOffer(u32)`, `CancelTransfer(u32)`, `RefreshFiles`, `OpenDownloadDir` — handled in
  `App::update` via the existing `self.send(ControlCommand::…)` pattern (app.rs:173). MVP file
  input is a text field + "send to connected peer" (pre-filled from `snap.rf_peer`); a native
  picker via the `rfd` crate is a phase-D nicety (decision D6).
- `state.rs` `PanelState` additions:
  ```rust
  pub pending_file_offer: Option<FileOfferView>,     // from FileOffered{auto_accepted:false}
  pub active_transfer: Option<TransferView>,          // from FileProgress (direction, fractions)
  pub files: Vec<openpulse_daemon::protocol::FileSummary>,  // from FileList / FileReceived
  ```
- `connection.rs` `apply_event` arms for the six new events, following the
  `MessageReceived`/`MessageList`/`QsyPending` patterns (connection.rs:305–356): update state +
  `push_log` a one-liner (`"FILE offer from DL1ABC: photo.jpg 42 KB"`,
  `"FILE verified ✓ photo.jpg"`).

### 7.2 Files tab mockup

```
┌─ Info ─ Stats ─ Config ─ Messages ─ [Files] ─ Log ──────────────────────────┐
│ SEND                                                                        │
│  Path [ /home/op/photos/antenna.jpg          ]  To [ DL1ABC ]  [ Send ▶ ]   │
│  est. 38 KB ≈ 2 min @ 350 B/s (QPSK500/SL6)                                 │
│                                                                             │
│ INCOMING OFFER                                                              │
│  ⚠ DL1ABC offers  field-notes.pdf   112 KB   sha256 9f3a…1c   sig ✓         │
│    est. receive time ≈ 6 min          [ Accept ]  [ Reject ]   (78 s left)  │
│                                                                             │
│ ACTIVE TRANSFER                                                             │
│  ▼ RX field-notes.pdf from DL1ABC                                           │
│  [██████████████░░░░░░░░░░░░]  block 5/12 · 46 %  · 41 KB/112 KB · 355 B/s  │
│                                                       [ Cancel ]            │
│ RECEIVED FILES                                        [ Refresh ] [ Open ↗ ]│
│  ✓ verified   antenna-plan.png   18 KB   DL1ABC   2026-07-09 14:02          │
│  ✓ verified   sked.txt            1 KB   ON4XYZ   2026-07-08 19:44          │
│  ✗ UNVERIFIED beacon.bin          4 KB   N0CALL   2026-07-07 08:10          │
└─────────────────────────────────────────────────────────────────────────────┘
```

Progress bar reuses the ladder/meter drawing style already in `ui.rs`; the offer prompt reuses the
QSY accept/reject two-button row; the received-files list is a `stats_widget`-style table
(ui.rs:1178) with `info_row` (ui.rs:1272) cells; the verify badge is `ColorRole::Locked` green vs
`ColorRole::TxActive` red from `theme.rs` — theme-safe in Dark/Light/Contrast.

---

## 8. Configuration (`crates/openpulse-config`)

New section following the `CompressionConfig`/`LogbookConfig` pattern exactly (`#[serde(default)]`
struct + `Default` impl + a commented block in `init_template()`, lib.rs:97–125, 982–996):

```toml
[file_transfer]
# Direct P2P file transfer (offer/accept over an RF session). Opt-in. When disabled,
# inbound offers are rejected on air with reason "feature-disabled".
enabled = false
# Directory for received files (per-peer subdirectories are created beneath it).
download_dir = "~/.local/share/openpulse/files"
# Auto-accept inbound offers at or below this many bytes; 0 = always ask the operator.
auto_accept_max_bytes = 0
# Hard per-file cap, both directions (offer/accept refused above it).
max_file_bytes = 1048576          # 1 MiB
# Total bytes retained per peer under download_dir before new offers are rejected (quota).
per_peer_quota_bytes = 16777216   # 16 MiB
# Require a verified signed handshake (CONREQ/CONACK) before accepting offers.
require_verified_peer = true
# Optional callsign allowlist; empty = any peer passing the trust policy above.
allow_from = []
# Seconds an unanswered offer stays pending before automatic rejection.
offer_timeout_secs = 120
# Abort a transfer with no forward progress for this long.
transfer_stall_secs = 180
# Target airtime per PTT keying while sending file blocks.
burst_max_secs = 20
# Hours to retain partial (resumable) transfers before purging.
partial_ttl_hours = 72
```

Defaults are deliberately conservative: **feature off, always-prompt, verified-peers-only** — the
EmComm preset idea from the gap analysis (V13) later flips `auto_accept_max_bytes` high in one
place. `compress_tx` (`[compression]`) is *not* consulted: file blocks are always `pack()`ed because
the frame is self-describing and passthrough-safe (worst case +5 B/block).

---

## 9. Security & integrity

- **The differentiator (REQ-FT-03)**: every file is verified with
  `verify_manifest_with_payload()` — Ed25519 signature *and* SHA-256 match, fail-closed — against
  the pubkey proven by the signed handshake, so the badge asserts *this exact content, from this
  exact key, which answered as this callsign*. VarAC offers none of this. The receiver's
  countersigned `FileComplete` gives the sender a non-repudiable delivery receipt.
- **Filename sanitization** (`filexfer/src/sanitize.rs`, pure + unit-tested): take the final path
  component only; strip/reject `/`, `\`, NUL and all control chars; reject `.`/`..`/empty; strip
  leading dots (no dotfiles); cap at 48 bytes UTF-8 (offer field limit, §4.2); collision → ` (2)` suffix.
  Files are written only under `download_dir/<sanitized-peer-callsign>/` (callsign itself sanitized
  to `[A-Z0-9/-]`, `/` mapped to `_`), created `0o700`, written to a temp name and atomically
  renamed after verification. Never execute, never follow symlinks (`create_new`).
- **Size/quota**: offer-time checks (`max_file_bytes`, `per_peer_quota_bytes`); reassembly aborts if
  received bytes exceed the offered `file_size` (a lying sender can't balloon memory); per-block
  `unpack()` is already OOM-safe (`MAX_DECOMPRESSED_SIZE` pre-allocation check,
  `DecompressedSizeTooLarge`).
- **Unknown-peer policy**: `require_verified_peer = true` default; `allow_from` allowlist for
  stricter stations; the trust store (`load_trust_store_from_file`, revocation via
  `PublicKeyTrustLevel`) already gates the handshake upstream.
- **No content encryption by default** — deliberate, per the gap-analysis "do not copy" list and
  `docs/regulatory.md`: content encryption over amateur RF is prohibited in most jurisdictions
  (obscuring meaning ≠ authentication). The transport is *authenticated* (signatures) but
  *plaintext*. If a non-amateur deployment ever needs confidentiality, the ML-KEM machinery from
  `pq_handshake.rs` exists — but it must stay behind an explicit non-default config with a
  regulatory warning, and is out of scope for this plan.
- **Protocol hygiene**: unknown OPFX version → clean reject; `transfer_id` mismatches and
  out-of-range `block_index` → drop + log; all parsers total (no panics on truncated frames —
  enforced by fuzz-ish decode tests over truncations of every frame type).

---

## 10. Regulatory

- **Station ID (REQ-REG-10 / §97.119)**: already handled structurally. `server::run` arms
  `StationIdTimer` from the `engine.frames_transmitted()` delta and keys the ID transmission on its
  own PTT cycle (server.rs:806–861); file-transfer bursts increment the same counter, so a long
  transfer gets the ≤ 10-minute interval ID and the sign-off ID with **no new code**. Two things the
  plan must verify by test: (a) the receiver tolerates the interleaved `DE <CALL>` frame mid-block —
  it is a non-SAR, non-OPFX payload that falls through `try_reassemble_sar` as an ignorable
  malformed fragment (`SarError` → return, `lib.rs:1237`), and (b) block/stall timers comfortably
  exceed one ID cycle. An integration test transfers a file sized to span an ID interval (with the
  timer interval shrunk) and asserts completion + at least one ID transmission
  (`station_id_txcount`-style).
- **Duty cycle / etiquette**: `burst_max_secs` (default 20 s) bounds each keying far under the 180 s
  PTT watchdog; the listen window between bursts keeps the channel breathable; the panel's airtime
  estimate discourages oversized sends. Document the courtesy guidance (advise ≤ 30 KB on shared
  frequencies) in `docs/cli-guide.md`/user docs when phase D lands.
- **Third-party traffic**: a P2P file between two control operators is ordinary amateur
  communication; the §97.115/third-party concern arises only for relayed content. The existing
  relay path (`RelayForwarder`) does not forward OPFX frames (they are not `OPHF` envelopes), so no
  new exposure. Note in user docs: operators remain responsible for content legality (no music,
  no broadcasting, no pecuniary interest) — same footing as any message traffic.
- **Openness (Germany §12 AFuV etc.)**: the OPFX wire format is documented in
  `protocol-wire-spec.md` in the same PR that implements it — emissions stay third-party decodable.

---

## 11. Testing strategy

All tiers run under `cargo test --workspace --no-default-features` (LoopbackBackend only; the
repo-wide "never require real audio" rule).

**Tier 1 — pure state machines** (`crates/openpulse-filexfer/tests/` + inline unit tests):
- wire codec round-trips for all 7 frame types + truncation/garbage totality tests;
- offer→accept→transfer→verify happy path with injected clock;
- reject (each reason), offer timeout, cancel from each side in each state, block-retry exhaustion,
  stall timeout;
- block math: split/join identity at boundary sizes (empty file, 1 B, exactly `block_size`,
  `block_size`+1, max `block_count`), per-block pack/unpack, fragment-bitmap accounting;
- resume: accept-with-bitmap skips held blocks; corrupted persisted block is re-fetched;
- sanitize: traversal (`../../etc/passwd`), dotfiles, control chars, Unicode, collisions;
- manifest binding: offer signed by key A rejected against key B; tampered `sha256` in the offer
  fails `verify_manifest`.

**Tier 2 — modem loopback round-trip** (`crates/openpulse-modem/tests/` or the filexfer crate with a
dev-dependency on `openpulse-modem`): drive both sessions through `ChannelSimHarness`
(`channel_sim.rs::route()` / `route_clean()`):
- small file (≤ 1 block) clean channel: SAR → manifest → transmit → decode → reassemble →
  `verify_manifest_with_payload` OK;
- multi-block file (≥ 3 blocks, > 64 005 B total — proving multi-object framing) over AWGN 20 dB;
- lossy channel (Gilbert-Elliott light): dropped fragments recovered via `BlockAck` selective
  retransmission;
- **integrity failure**: flip one byte in a reassembled block before verification → transfer
  completes at the ARQ layer but the badge is `verified: false`, `FileComplete(hash_mismatch)` goes
  back, file lands in quarantine.

**Tier 3 — daemon production-entry tests** (`crates/openpulse-daemon/tests/`, twin harness): per the
seam-gap checklist, the test must go through the **production entry** (`accumulate_capture` / the
twin daemon rig in `daemon/src/twin.rs`), not only the convenience seam:
- twin-daemon file round-trip: `SendFile` command on daemon A → `FileOffered` + `AcceptFile` on
  daemon B → `FileProgress` streams on both → `FileReceived{verified:true}` + `FileSent`; asserts
  the file bytes on disk equal the source;
- auto-accept path (`auto_accept_max_bytes` set) needs no `AcceptFile`;
- policy rejections: feature disabled, oversize, unverified peer;
- handshake coexistence: CONREQ/CONACK completes mid-setup and file frames don't poison
  `handshake_sar` (segment-id reservation test);
- station-ID interleave test (§10).

**Tier 4 — protocol/serde**: JSON round-trip of every new `ControlCommand`/`ControlEvent`
(mirroring `ota_commands_round_trip_via_json`); panel `apply_event` unit tests for the new arms
(pattern exists in `connection.rs` tests).

**Proposed acceptance-table rows** (CLAUDE.md format; add in the implementing PRs):

| Requirement | Acceptance test |
|---|---|
| File offer/accept/reject/timeout state machines (REQ-FT-01/04) | `cargo test -p openpulse-filexfer` |
| File round-trip over simulated channel, small + multi-SAR-object (REQ-FT-02) | `cargo test -p openpulse-filexfer --test loopback_roundtrip` |
| Tampered payload yields UNVERIFIED, never a false badge (REQ-FT-03) | `cargo test -p openpulse-filexfer --test integrity_failure` |
| Twin-daemon end-to-end SendFile → FileReceived{verified} (REQ-FT-02/05) | `cargo test -p openpulse-daemon --test filexfer_twin` |
| Filename sanitization blocks traversal (REQ-FT-06) | `cargo test -p openpulse-filexfer sanitize` |
| Station ID transmitted during a long transfer (REQ-REG-10) | `cargo test -p openpulse-daemon --test filexfer_station_id` |
| Resume skips already-held blocks (REQ-FT-07, phase E) | `cargo test -p openpulse-filexfer --test resume` |

Traceability: each PR carries the requirement → design (this doc §) → implementation files → tests →
actual run counts chain in the PR body and appends to `docs/dev/project/traceability.md`
(repo rule; see `feedback_traceability`).

---

## 12. Phased implementation plan

Dependency-ordered, PR-sized. Each milestone lands green on
`cargo test --workspace --no-default-features` + clippy `-D warnings`.

**A — `openpulse-filexfer` crate: wire + state machines + manifest wiring** *(M, ~1 PR)*
Deliverable: the crate with `wire.rs`/`offer.rs`/`sender.rs`/`receiver.rs`/`sanitize.rs`, Tier-1
tests, OPFX registered in `docs/dev/design/protocol-wire-spec.md`. No daemon changes. Acceptance:
`cargo test -p openpulse-filexfer` (offer/accept/reject/timeout/verify paths, tamper test at the
state-machine level).

**B — blocks over SAR + multi-object framing + loopback round-trip** *(M, ~1 PR)*
Deliverable: `blocks.rs` (split/pack/SAR segment mapping, fragment bitmaps), Tier-2
`ChannelSimHarness` tests including the > 64 005 B multi-object case and the Gilbert-Elliott
selective-retransmit case. Acceptance: `loopback_roundtrip` + `integrity_failure` tests.

**C — daemon surface: config, commands/events, PTT-sequenced delivery** *(M–L, 1–2 PRs — the big
one)*
Deliverable: `[file_transfer]` config section; `protocol.rs` variants; `filexfer.rs` glue;
`process_received_bytes` → `try_reassemble_sar` refactor with segment-id routing; `server::run`
SendFile interception + `drive_filexfer` in the rx tick (refactor `ota_send_with_ptt`'s burst body
for reuse); storage/quota/sanitize enforcement; Tier-3 twin tests + station-ID test. Acceptance:
`filexfer_twin`.
**⇐ MVP ship line**: after C the feature is operator-usable from the control port / CLI (optionally
a thin `openpulse-cli send-file` in the same PR): small files, stop-and-wait blocks, auto-accept
off, no resume. Tag the risk items below before this PR.

**D — panel Files tab** *(S–M, ~1 PR)*
Deliverable: `Tab::Files`, state/connection/ui wiring per §7, offer prompt, progress card, verify
badges, airtime estimate. Acceptance: panel `apply_event` unit tests + manual twin-rig walkthrough
(screenshot in PR).

**E — resume + selective-retransmit hardening + throughput** *(M, ~1 PR)*
Deliverable: partial-block persistence + `FileAccept.have_bitmap` resume; tune
`fragments_per_burst`; measure stop-and-wait vs. a 2-block window in the twin harness/linksim and
adopt the window only if the numbers justify it; HARQ soft-combining regression test on Watterson.
Acceptance: `resume` test + a recorded before/after goodput table in the PR (measured, per the
"tests → results must be a real run" rule).

**F — on-air validation** *(no code; joins the deferred Phase 5.5-reg batch)*
Twin-OTA scenario (`docs/dev/onair-twin-ota.md` rig): a real 20–40 KB file over the dual-daemon RF
path; verify badge, ID compliance, duty-cycle behavior; results appended to
`docs/dev/ota-hardware-validation.md`.

**Highest-risk piece**: phase C's delivery loop — the interaction between the file-plane
`BlockAck`, the OTA plane's per-burst FSK4 ACK, PTT turnaround timing, and the rx tick's
single-threaded select loop. De-risk order: build the twin-harness test *first* (test-first rule),
run it over `moderate_f1` Watterson early, and keep the MVP strictly stop-and-wait so there is only
ever one outstanding thing on the air.

---

## 13. Decisions — RESOLVED 2026-07-10 (recommendations accepted)

All five decisions are locked to the plan's recommendations; the rest of this doc is normative against them.

- **D1 — Delivery ride.** ✅ **Hybrid** (§5.1): file fragments as ordinary OTA data bursts (FSK4
  per-burst ACK keeps stepping the rate ladder) **plus** OPFX `BlockAck` for block-level integrity and
  selective retransmission. Reuses `ota_send_with_ptt` bodily. (Rejected: per-fragment
  `transmit_arq_ota_within` ~2× turnaround; fixed-mode-only.)
- **D2 — Size ceilings and block size.** ✅ `max_file_bytes` = **1 MiB** hard cap (both directions),
  `block_size` = **16 384**, `auto_accept_max_bytes` = **0** (always ask). Protocol ceiling stays ~3 GiB as
  headroom; documented etiquette guidance 15/30/100 KB.
- **D3 — `SendFile` input channel.** ✅ **Daemon-host path only for the MVP** (Phase C). `SendFileInline
  { name, bytes_b64 }` for remote/WebSocket panels is a later add (Phase D+) — the offer flow is
  path-agnostic, so it grafts on cheaply.
- **D4 — Resume granularity.** ✅ **Block-level bitmap in `FileAccept` + `.partial` block store, Phase E.**
  No fragment-level or cross-restart sender state (extra machinery for ≤16 KiB saved per drop).
- **D5 — Trust / auto-accept defaults.** ✅ **`require_verified_peer = true` + prompt-always.** Preserves
  the "every file is attributable" story; an EmComm preset may relax it explicitly later.
- **Settled (not vetoed):** fresh **OPFX** binary protocol rather than extending B2F FC/FS (§3.4); one
  transfer per link in v1; quarantine-not-delete on verify failure; per-block `pack()` always on.

---

## 14. Risks & mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| Half-duplex ACK timing races in the rx-tick loop (C) | stalls / duplicate bursts | Twin-harness test first; stop-and-wait MVP; timers derived from `ack_timeout_ms_for_snr` anchors; stall→cancel is always reachable |
| Shared SAR receive path cross-talk with handshake frames | corrupted handshake or file slots | Segment-id namespace reservation (§4.4/§6.2) + dedicated reassembler instances + an explicit coexistence test; handshake tests must pass unchanged |
| Long transfers on low rungs (SL2/SL3) are practically unusable | operator frustration, channel hogging | Airtime estimate before send + warning threshold; document etiquette; OTA ladder raises the rate when the channel allows |
| Station-ID frame interleaves mid-block | receiver-side parse confusion | Non-OPFX payloads fall through `try_reassemble_sar` harmlessly (verified by test §10); timers sized over an ID cycle |
| Filename/path handling on a hostile offer | file overwrite / traversal | Pure `sanitize.rs` with adversarial tests; per-peer dirs; atomic rename; `create_new` |
| Verify-failure UX misread as "modem broken" | support noise | Distinct UNVERIFIED badge + quarantine dir + log line naming the failed check (sig vs hash) |
| Protocol drift between `protocol.rs` and handlers (KEEP-IN-SYNC) | runtime `CommandError`s | serde round-trip tests for every new variant; single PR touches both files |
| Scope creep toward BBS/gallery/multi-transfer | schedule | Non-goals §1; one-transfer-per-link enforced in the state machine; gallery = a flat `ListFiles` |
| A "works in ChannelSimHarness, fails via daemon" seam gap | shipped-but-dead feature | Tier-3 tests drive `accumulate_capture`/twin rig (production entry), per the seam-gap checklist |

---

## 15. Key references

`docs/dev/research/varac-feature-gap-analysis.md` §4.1 · `crates/openpulse-core/src/sar.rs` ·
`crates/openpulse-core/src/manifest.rs` · `crates/openpulse-core/src/compression.rs` ·
`crates/openpulse-core/src/handshake.rs` · `crates/openpulse-modem/src/engine.rs` (`ota_*`,
`transmit_with_fec_mode`, `receive_with_llr_combining`) · `crates/openpulse-modem/src/harq.rs` ·
`crates/openpulse-daemon/src/{protocol.rs, lib.rs, server.rs, twin.rs, logbook.rs}` ·
`apps/openpulse-panel/src/{app.rs, state.rs, connection.rs, ui.rs}` ·
`crates/openpulse-config/src/lib.rs` · `crates/openpulse-b2f/src/{frame.rs, session.rs}` ·
`docs/dev/design/protocol-wire-spec.md` · `docs/regulatory.md` · `apps/openpulse-linksim`.
