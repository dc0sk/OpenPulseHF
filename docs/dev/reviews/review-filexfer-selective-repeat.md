---
project: openpulsehf
doc: docs/dev/reviews/review-filexfer-selective-repeat.md
status: resolved
last_updated: 2026-10-02
---

# Adversarial review — file-transfer selective repeat (M2)

One round by Fable, held before implementation. **Mandatory** (decision 8): the design changes when a
station keys its transmitter. The reviewer was read-only and was prompted for falsification. Target:
the draft [`design/filexfer-selective-repeat.md`](../design/filexfer-selective-repeat.md).

## Consumer

`server.rs` receive tick (`process_received_bytes` → `route_inbound_fragment` → `on_block_fragment`;
`poll_timeouts`; `drain_filexfer_tx`); `drive_tx_actions`. Found by `grep -n "fn on_block_fragment\|fn
poll_timeouts\|fn drain_filexfer_tx\|fn drive_tx_actions" crates/openpulse-daemon/src/*.rs`.

## Prior art

`BlockAck.missing_frag_bitmap`, `encode_block(.., missing)`, `BlockAssembler::missing_bitmap`, the
sender's retry counter, REQ-FX-05. Found by `grep -n "missing_frag_bitmap\|fn missing_bitmap\|max_block_retries"
crates/openpulse-filexfer/src/*.rs`.

## Twins

The PQ handshake's SAR has no retransmission; not Release 1, left unchanged. Found by
`grep -n "fn try_reassemble_handshake" crates/openpulse-daemon/src/lib.rs`.

## Prompt

Falsify, do not agree, with file:line evidence:
1. Are the three defects real as stated, especially the timer armed before a synchronous drain? Are
   the airtime numbers right?
2. Can R1 key while the sender is still transmitting (multi-keying rounds, station ID, control
   frames sharing the queue)?
3. Does a one-fragment probe reach the receiver as something R1 can recognise? What does SAR do
   with a lone fragment of a completed segment?
4. Can the retry accounting exhaust while progress is being made, or loop forever? Does the
   receiver's `round_last` match what the sender resends?
5. Are the timer values right for slow modes, and what happens when a drain fails part-way?
6. Is there anything simpler with less keying risk?
7. How do resume and cancel interact with it?

Does this block Release 1? If not, say so.

## Verdict

**Release 1 (reviewer):** not an acceptance item, but file transfer is unusable at the default block
size until this lands, and the RC UI client (decision 19) depends on it.

**Defects confirmed:** all three, with file:line evidence (tick order `poll_timeouts` → drain;
`begin_block` arms at queue time; acks only on completion).

**Disputed:** the airtime figure. The reviewer assumed RS-coded fragments (about 17 s each), but
filexfer fragments go out uncoded through `transmit`. The #1461 twin measured 8.6 s per fragment, so
the design keeps that figure and states why.

| # | Finding | Class | Outcome |
|---|---|---|---|
| 1 | A lost NACK deadlocks: the two ends hold different `round_last`s, so the probe never triggers a NACK | BLOCKING | Receiver also NACKs on a duplicate fragment; twin case "drop the NACK" |
| 2 | A lost ack on the last block is fatal: the receiver is terminal and `FileComplete` is ignored in `Sending` | BLOCKING | Sender takes `FileComplete` on the last block as success; receiver keeps a finished-transfer record that re-sends `FileComplete`; twin case added |
| 3 | The sender's station ID runs after its drain and can collide with the receiver's immediate NACK | FIX | B1: drain defers while the channel is busy |
| 4 | Ack-wait is a fixed 120 s; at BPSK31 a NACK's airtime approaches it, so the probe keys over the NACK | FIX | Ack-wait sized from airtime; B1 gates the probe |
| 5 | "Fragment of a held block" is not distinguishable after ingest, and its candidate leaks (never expired) | FIX | Check `block(idx)` before ingest; do not ingest |
| 6 | Probes and NACK resends share one retry budget, which runs out while progress is being made | FIX | Reset on a shrinking missing set; count only unanswered probes |
| 7 | A partial drain arms ack-wait for a round never sent | NOTE | Recovered by one probe; logged |
| 8 | Timer margins thin but harmless; resume consistent; receiver-side `FileCancel` has no busy gate (pre-existing) | NOTE | Rx stall widened to `(retries + 2) × ack_wait` |
| 9 | Simpler receiver: NACK on end-of-round or duplicate, complete-ack on a held block, finished-transfer record | NOTE | Adopted, keeping `round_last` as one of the NACK triggers |
