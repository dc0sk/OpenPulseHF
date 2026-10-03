---
project: openpulsehf
doc: docs/dev/reviews/review-1477-unpack.md
status: resolved
last_updated: 2026-10-01
---

# Adversarial review — compression unpack fix (#1477)

One round by Fable (read-only, no files edited), prompted for falsification, on branch
`claude/affectionate-brahmagupta-sdn8qg` at `377f1ad5` plus the two `last_updated` bumps.

## Consumer

`crates/openpulse-daemon/src/server.rs` receive loop → `unpack_received` →
`openpulse_core::compression::try_unpack`. Found by `grep -rn "unpack" crates/openpulse-daemon/src`.

## Prior art

`compression::unpack` (kept as the `Option` wrapper); #1166 removed the handshake compression fields
that the old REQ-CMP-03 text still described. Found by `git log -S "compression" --oneline -- crates/openpulse-core/src/handshake.rs`.

## Twins

`crates/openpulse-filexfer/src/blocks.rs:164` `unpack(&packed).unwrap_or(packed)` — the only other
non-test receive site. Found by `grep -rnE "compression::unpack|\bunpack\(|\bdecompress\(" crates tools`
(positive control: the same grep matched the daemon test caller `lib.rs:4537`). Parked: filexfer
ships disabled in Release 1; a wrong unpack is caught by the offer-length check when the length
differs, otherwise by the file-level verify.

## Prompt

Falsify, do not agree: (a) does zstd enforce the dictionary ID in general, and how does the trainer
assign it — can a retrained dictionary keep the same ID? (b) are there other receive-path
unpack/decompress sites that still deliver bytes on failure? (c) is the rewritten REQ-CMP-03 honest
about what ships? (d) can any production frame begin with `OPZ1`? (e) re-run the claimed test
commands. Does this block Release 1? If not, say so.

## Verdict

**Merge.** Does not block Release 1 (an M1 item, no wire change; no ladder, FEC, PTT or ARDOP code).

1. zstd enforcement holds for our frames, with a caveat: the decoder check is guarded by
   `fParams.dictID &&` (`zstd_decompress.c:717`), so a frame carrying dictID 0 skips it. Our sender
   always writes the ID (`ZSTD_c_dictIDFlag` default 1). **Folded** into the trace entry.
2. The trainer leaves dictID 0 and libzstd assigns a content hash (`zdict.c:879-881`), so a retrain
   changes the ID; the flipped-byte test exercises exactly the compared field. Pre-existing nit: the
   trainer comment at `main.rs:148` says big-endian, the code reads LE — **parked**.
3. One twin, filexfer (above). The trace wording "fails the offer-length check" overstated it —
   **softened** to "length check or file verify".
4. REQ-CMP-03 rewrite honest. Unstated interop gap: a packed body larger than one frame is not
   SAR-fragmented then unpacked — already #1461.
5. No valid production frame begins with `OPZ1` (SAR, QSY, `OPHF`, `OPFX`, ACK all ruled out); only a
   raw operator message starting `OPZ1` is lost, as the PR states.
6. Re-run: `cargo test -p openpulse-core --no-default-features --lib compression::` rc=0, 13 passed;
   `cargo test -p openpulse-daemon --no-default-features --lib unpack_received` rc=0, 2 passed.
7. Work plan decision 17 was listed before 16 — **fixed**.
