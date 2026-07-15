---
doc: docs/dev/reviews/2026-07-15-filexfer-relay-seams-audit.md
date: 2026-07-15
status: partial
scope: direct P2P file transfer, multi-hop relay / wire-envelope, RX/TX pipeline seams, control-surface parity
---

# File-transfer / relay / pipeline-seam security audit

Six-finder adversarial sweep (diverse lenses, refute-by-default, every finding source-verified) of the
file-transfer subsystem (`openpulse-filexfer` + `openpulse-daemon/src/filexfer.rs`), the relay /
wire-envelope code (`openpulse-core::{relay,wire_query,query_propagation}`), the RX/TX pipeline seams,
and the daemon control-surface parity. Findings ordered by severity within each area.

The DSP-seam and control-parity lenses came back essentially clean: every RX/TX front-end transform
(DC-removal, notch, DCD squelch, AGC, CE-SSB, TX limiter, station-ID `frames_transmitted` counter) is
implemented once at the shared `route_audio_stage(InputCapture)` / `stage_emit_output` seam, exercised
by both the receive-family and the daemon streaming path, each with a tripwire counter — the "wired at
one seam, not all paths" bug class does not reproduce. Control-command / event / config wiring is 1:1
except for one dead config field (below).

## Fixed — file transfer

### F-1 — [CRITICAL] Received blocks were never bounded by the offer's declared size

**Files:** `openpulse-filexfer/src/blocks.rs` (`BlockAssembler::ingest_fragment`), `openpulse-daemon/src/filexfer.rs` (`reassemble_verify_write`)

The size gate (`max_file_bytes`), per-peer quota, and `offer_geometry_ok` are all evaluated once, at
offer time, against the *declared* `file_size`/`block_size`/`block_count`. Nothing then constrained the
*actual* bytes a block carried: `ingest_fragment` stored `unpack(&packed)` with no length check, and
`reassemble_verify_write` wrote the file unconditionally (only the `.unverified` suffix differed on a
hash mismatch). A peer could offer a 1 KB file, pass the gate, then send blocks that each unpack to ~64 KB
(the decompression cap) — up to `65535 × 64 005 ≈ 4.2 GiB` on disk from one quota-approved offer.
*Confirmed independently by two finders.*

**Fix:** `BlockAssembler` now carries `block_size`/`file_size` and rejects any block whose decoded length
≠ its expected slot length (`block_size`, or the short remainder for the last block); `seed_block` (resume)
enforces the same. `reassemble_verify_write` additionally refuses to write a payload whose total ≠
`offer.file_size`. Tests: `blocks::an_oversized_block_is_rejected`.

### F-5 — [SEVERE] A send whose peer never answered pinned the subsystem forever

**File:** `openpulse-daemon/src/filexfer.rs`

`SenderSession::poll_timeout` / `ReceiverSession::poll_timeout` (offer/stall/verify deadlines) were
implemented and unit-tested in the crate but had **zero daemon call sites**, and `cancel_transfer` only
ever cleared `rs.file_rx`. So `SendFile` → peer silent (the HF norm) → `rs.file_tx` stayed populated
forever → every later `SendFile` failed "a file transfer is already active", recoverable only by a daemon
restart. No attacker required.

**Fix:** a new `filexfer::poll_timeouts(...)` fires both sessions' deadlines and is called each daemon rx
tick (next to `expire_pending_handshake`); `cancel_transfer` now also cancels an outbound send. Tests:
`a_stuck_send_clears_on_offer_timeout`, `cancel_clears_an_outbound_send`.

### F-6 — [MEDIUM] `block_count == 0xFFFF` collided the last block with the control channel

**File:** `openpulse-filexfer/src/lib.rs` (`block_count`)

The last block's SAR `segment_id` is `block_index + 1`; a 65535-block transfer made the final block's id
`0xFFFF` = `FX_CONTROL_SEGMENT_ID`, so `route_inbound_fragment` sent it to the control-frame reassembler
where it was dropped — the transfer hung on the last block (config-gated to ~64 MiB `max_file_bytes`).

**Fix:** `block_count` is capped at `MAX_BLOCK_COUNT = 0xFFFE` (returns `None` above it), so both send
(`from_manifest` fails) and receive (`offer_geometry_ok` mismatches) reject such geometry. Test:
`block_count_math`.

### E5 — [MEDIUM] `verified_peer` was a single global slot

**File:** `openpulse-daemon/src/{lib,filexfer}.rs`

A signed file-transfer offer was verified against whoever handshook most recently (the single
`verified_peer` slot), so peer A's offer could be checked against peer B's key (rejecting A's legitimate
offer, or mis-attributing).

**Fix:** a per-callsign `verified_peers` map (populated by `record_verified_peer`) — `on_offer` verifies
the offer against *its own sender's* key, and only trusts `sender_id` as the quota/display identity once
the signature validates (an unverified offer stays in the shared "unknown" bucket, so a spoofed
`sender_id` cannot carve out a fresh quota). The single slot is retained for OTA/QSY, which act on the
current link. Test: `offer_is_verified_against_its_senders_key_not_the_last_handshook_peer`.

### F-3 — [LOW] `unique_path` could overwrite after 10 000 collisions

**File:** `openpulse-daemon/src/filexfer.rs`

After `name`, `name (1)` … `name (9999)` all existed, `unique_path` returned `dir.join(name)` — the
already-existing path — and `write_file` truncated it, violating the "never overwriting" contract.

**Fix:** `unique_path` returns `Option`; `write_file` errors (`AlreadyExists`) rather than clobbering.

## Fixed — wire / config

### F-4 — [MEDIUM] `PeerQueryResponse::decode` pre-allocated from an unchecked count

**File:** `openpulse-core/src/wire_query.rs`

`decode` read `result_count` (attacker-controlled `u16`) and did `Vec::with_capacity(result_count)`
before validating the buffer held that many records — unlike its two sibling decoders. A ~130-byte frame
forced a ~6.5 MB transient allocation (reachable via `openpulse-mesh`). **Fix:** bound the capacity by the
bytes actually present (as the siblings do).

### discovery.group — [dead config] deserialized but never applied

**File:** `openpulse-config/src/lib.rs`, `openpulse-daemon/src/server.rs`

`[discovery] group` (default `"OPULSE"`, documented as a live knob) was parsed but never read: both the
beacon TX addressing and the RX hint filter hardcode the `OPULSE_GROUP` constant. An operator setting
`group = "MYCLUB"` was silently ignored. The `@OPULSE` group is baked into the JS8 beacon frame packing
(validated against JS8Call ground-truth vectors) and the RX assembler, so a configurable group is a real
on-air feature, not a string swap. **Fix (proportionate to an audit):** the field is marked RESERVED in
the config schema + template (matching its already-reserved neighbours) and a non-default value logs a
warning at daemon startup, so the config is honest; wiring the custom-group feature is tracked separately.

## Deferred (larger change — documented)

- **F-2 — [MEDIUM] offer metadata is unsigned.** The Ed25519 signature covers only
  `{payload_hash, payload_size, sender_id}`; `name`/`mime`/`block_size`/`block_count` ride unauthenticated,
  so an on-path attacker can replay a signed offer with a spoofed filename while the UI still shows
  `signature_valid: true`. Content is protected (the hash is signed); only metadata spoofs. Closing it
  means widening the signed `ManifestBody` — a wire-format change — tracked as follow-up.
- **F-7 — [LOW] selective-retransmit (NACK) path is unreachable in the daemon.** The receiver always
  sends `BlockAck { complete: true }` and never calls `missing_bitmap()`, so a single lost fragment can't
  be recovered by partial retransmission (it relies on the stall timeout, now functional via F-5). Feature
  completeness, not a correctness break.
- **E3 — [MEDIUM] relay forwards unauthenticated frames** (`min_trust_filter` unread, `auth_tag`
  unverified). Previously documented; needs trust threading + key material.

## Refuted / won't-fix

- **F-8** replayed completed-block idempotency: caught by the whole-payload SHA-256 + signature check —
  a robustness/airtime gap, not a verification bypass.
- **emit_cw_id skips `csma_check()`**: the only caller is the ARDOP TNC's `CWID` option, and a station ID
  arguably *should* transmit regardless of channel occupancy — not gated.
- Path traversal / symlink escape (sanitize covers `..`, absolute, drive-letter, NUL, mixed separators),
  TOCTOU verify→write, manifest-signature bypass, integer overflow in block/fragment math, hop-limit wrap,
  duplicate-suppression capacity, `score_route` underflow, all wire header/`read_u64` slicing — traced and
  safe.
