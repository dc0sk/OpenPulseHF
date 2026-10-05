---
project: openpulsehf
doc: docs/dev/reviews/review-compression-dict-id.md
status: archive
last_updated: 2026-10-04
---

# Adversarial review — session compression: dictionary ID on the wire, corrupt packed frames refused

> **Superseded — archived for its findings, not its design.** This review covers the branch
> `claude/confident-dijkstra-h8wbb7` as of `1d26ff6` (2026-09-30), which was never merged. #1477
> (merged 2026-10-01) shipped the daemon half by another route and **deliberately made no wire
> change** (work plan decision 17): zstd already carries the dictionary ID in its frame header. The
> branch was then reset to `main` for the file-assembler twin fix (2026-10-04). The tag-3 container
> described below does **not** exist on `main`. What is still open on `main` is listed under *Open
> items*.

## Open items (still true on `main` at 21acf82, each measured on the superseded branch)

1. **Same-ID dictionary content skew decodes to garbage `Ok`.** The dictionary-ID check cannot see a
   dictionary whose content changed but whose ID did not. The reviewer measured it: second half
   XOR 0x5A → `Ok(318 bytes)` of garbage, with zstd's dictionary-ID flag on or off, because zstd's
   content checksum is off by default (FHD bit 2 = 0). `CParameter::ChecksumFlag(true)` makes it
   `Err("Restored data doesn't match checksum")`, at +4 B per zstd frame (or 0 B if the frame's own
   ID field is dropped in exchange, which needs the ID elsewhere: a wire change). Mitigation without
   a fix: retraining is content-keyed (zdict derives the ID from content; ~2⁻³² collision), so this
   needs a hand-edited or corrupted dictionary file, not a retrain. **A decision, not a bug fix.**
2. **A raw message body that begins with `OPZ1` is refused.** Since #1477 a magic-bearing frame that
   fails to unpack is dropped, so a sender with compression OFF whose body happens to begin with the
   four bytes `OPZ1` (followed by a non-tag byte) loses that message at the receiver. #1477 states
   this as an accepted cost ("the price of the magic"). The branch
   fixed it at both daemon SendMessage sites (`compression::outbound`: pack, tag 0, whenever the body
   begins with the magic). ARDOP/KISS peers send raw and stay exposed regardless. SAR, QSY, relay,
   filexfer and station-ID frames cannot collide (checked in review round 1, finding 4).
3. **#1477's drop is tested at the helper only.** `unpack_received` unit tests cannot see the server
   site bypassing the helper. #1477 says no end-to-end test is possible because "no command can put a
   corrupt packed frame on the air". A replay audio backend can, without any command. The branch's `crates/openpulse-daemon/tests/packed_frames_refused.rs`
   drove the real `server::run` (it waits for `FrameReceived`, then requires
   `Metrics.compress_ratio` to stay `None`, with packed and unpacked positive controls). Sabotaged by
   reverting the server site, only that test failed. Recoverable from commit `18f58ad` if the SHA
   survives on GitHub; it is not on `main`.
4. **The file-assembler twin** (`blocks.rs` `unwrap_or(packed)`): **fixed 2026-10-04** on this
   branch (see the ledger entry of that date). #1477 had parked it as bounded: the file-level verify
   fails the whole transfer. The fix turns that into a selective retransmit of the one block.

Reviews by Fable, read-only except for throwaway probes (deleted), prompted for falsification: the
design before implementation (round 1, revise), and the implementation with its write-up (round 2).
The task cited `docs/dev/reviews/governance-review-2026-09-30.md`, finding 6. That file is not in the
tree (`git ls-tree -r origin/main --name-only | grep -i governance` → empty; my filter), so the
defects were re-derived from the code.

## Consumer

- `pack`: `server.rs` SendMessage arm, `lib.rs` `apply_send_message` (both now via `outbound`),
  `openpulse-filexfer/src/blocks.rs` `encode_block`.
- `unpack`: the daemon rx tick (`server.rs`, now `unpack_received`), the file assembler
  (`blocks.rs` `ingest_fragment`), `openpulse-modem/tests/compression_wire.rs` (two tests), one
  daemon lib test.
- `decompress(_, Zstd(_))`: testmatrix / testbench / linksim runners (tool-local, not wire).

Found by `grep -rn "compression::\|unpack(\|pack(" --include=*.rs crates apps`. The modem test file
was missed in round 1 and found by the reviewer.

## Prior art

- `qsy_lines_refused` (daemon `lib.rs`): refuse-and-count for a self-identifying frame that fails
  verification. Reused as `packed_frames_refused`.
- The handshake negotiation deleted in #1166 carried a dict-id membership check
  (`zstd_dict_id_mismatch_rejected_in_negotiation`, per the header of `compression_integration.rs`).
  It is restored here in the frame itself rather than in a negotiation.
- No zstd frame-parameter setting existed in the tree (`grep -rn "DictIdFlag\|ChecksumFlag"`; my
  filter found nothing).

## Twins

- `openpulse-filexfer/src/blocks.rs` `unpack(&packed).unwrap_or(packed)` has the same shape as the
  daemon site. A corrupt packed block usually failed the F-1 length binding by accident, but a
  coincidental length match stored compressed bytes as file content (a test now pins this).
- `decompress(_, Zstd(id))` ignored `id`: the in-API twin of the unchecked tag.
- Both SendMessage send sites (`server.rs`, `lib.rs`) needed the escape for a body that begins with
  the magic.
- ARDOP and KISS never call `unpack` (reviewer's grep): no packed frames on those paths, unchanged.

## Prompt

Round 1 (design v1, parked at the session scratchpad): falsify (1) the measurement that the zstd
frame already carries and checks the dictionary ID; (2) retiring tag 2 for a new tag 3, compared with
reusing tag 2 or bumping the magic; (3) the `Result<Option<_>>` API and whether dropping via an empty
buffer in the rx tick is safe; (4) the magic-collision consequence and a sender-side escape; (5) the
filexfer twin; (6) whether each test can fail on `main`, and whether a better production-entry test
exists; (7) the REQ-CMP-03 rewrite against the trace tooling.

Round 2: the implementation diff, the sabotage table, and this artifact, the ledger entry and the
acceptance row as they will be committed.

## Verdict

**Round 1 — REVISE**, all adopted:

1. The dictionary ID IS on the wire today, inside zstd's own frame header, and zstd rejects a
   different ID ("Dictionary mismatch", reproduced). A retrained dictionary was therefore not silent
   inside `decompress`. It became silent at the daemon's `unwrap_or`, which delivered the raw
   compressed bytes. What the ID cannot catch is **different content under the same ID**. Measured:
   a dictionary with its second half XOR 0x5A decoded to garbage `Ok(318 bytes)` with the ID flag on
   or off, because zstd's content checksum is off by default. Adopted: drop zstd's in-frame ID (the
   container carries it and checks it) and spend those 4 bytes on `ChecksumFlag(true)`. The zstd frame
   stays the same size, and the container's explicit ID adds 4 bytes per zstd frame. Also adopted:
   reject `decoded.len() != claimed` (the old code used the size prefix only as a capacity).
2. Tag 3 plus a retired tag 2 is fine. Nothing outside `compression.rs` depends on tag numbers, and a
   magic bump would need a `signing_domain` registry entry without changing old-receiver behaviour.
3. BLOCKING (small): the consumer list missed `openpulse-modem/tests/compression_wire.rs`. The
   empty-buffer drop is safe: `process_received_bytes` returns on empty, and the metrics block is
   gated on `!bytes.is_empty()`. **Stated consequence:** on the OTA arm the ACK is keyed before the
   unpack, so a refused frame has already been ACKed, and the sender cannot see the refusal.
4. The sender-side escape is correct and sufficient. SAR fragments cannot begin with `OPZ1` (the
   fragment index would be 0x5A ≥ total 0x31), and QSY, relay, filexfer and station ID carry their own
   prefixes. Among daemon send sites, only SendMessage bodies can collide. A raw payload from a
   non-daemon sender (an ARDOP or KISS peer) that begins with `OPZ1` is still refused by a receiving
   daemon.
5. BLOCKING: the filexfer twin must skip the candidate (`continue`), not return `Ignored`. The loop
   walks every SAR completion so that a poisoned completion sharing the key cannot shadow the real one.
   An early return would regress that property.
6. Test through `server::run` (the `monitor_during_ota.rs` pattern), not only through the helper. The
   helper tests cannot see the server site bypassing the helper. The source-scan idea (T9) was dropped
   in favour of this.
7. Traceability: edit both sides of CAP-01 ↔ REQ-CMP-03 (BIDIR-DRIFT), keep CAP-08's test list
   non-empty, and update `compression_integration.rs`'s header comment.

**Round 2 — APPROVE**, conditional on replacing the ledger's gate placeholder with a real `GATE:`
result (done after the gate ran). The reviewer re-ran the server-site sabotage (only the
`server::run` refusal test failed, both controls green) and confirmed that the OTA ACK is keyed at
`server.rs:1080–1097`, before `unpack_received` at `:1130`. Nothing else consumes the pre-unpack
bytes. Non-blocking notes, all adopted in the text:
- `pack`/`unpack` edges are sound: every malformed frame that carries the magic returns `Err`,
  never `Ok(None)`. The LZ4 arm's length check already exists inside `lz4_flex`.
- The `zstd_compress` fallback (u32::MAX prefix) can never be chosen by `pack`.
- The content-skew test drives `zstd::bulk` directly. Its attribution to the checksum rests on the
  "checksum off" sabotage, so that sabotage line in the ledger is load-bearing.
- The daemon test cannot pass vacuously: `compress_ratio` is `None` exactly while
  `raw_payload_bytes == 0`, and it is fed only after `unpack_received`.
- The escape covers daemon send sites only, so a raw `OPZ1…` payload from an ARDOP/KISS peer is
  refused. File transfer between an old and a new build stalls silently at the block level. Both
  are stated.
- The mixed endianness is a wart, not a design choice; the CAP-08 results column was stale; and the
  ledger's opening sentence overstated defect (1), which is latent.
