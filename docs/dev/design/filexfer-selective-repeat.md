---
project: openpulsehf
doc: docs/dev/design/filexfer-selective-repeat.md
status: resolved
last_updated: 2026-10-02
---

# File transfer survives a lost fragment, a lost ack and a long block

Work plan M2. Found by the #1461 review (`reviews/review-multi-frame-burst-decode.md`, finding 2) and by
reading the timers while designing the fix. REQ-FX-05 requires "a block-level acknowledgement bitmap
for selective retransmission"; the bitmap exists on the wire and nothing sends a useful one.

## Problem — three defects (read from source; the tests below demonstrate each before the fix)

1. **No NACK.** The sender has a selective-repeat arm: `BlockAck { complete: false,
   missing_frag_bitmap }` → `SendBlock { missing }`, up to `max_block_retries` = 4
   (`filexfer/src/sender.rs`). Nothing can trigger it. The daemon's `on_block_fragment`
   (`daemon/src/filexfer.rs`) sends a `BlockAck` only on a completed block, always with
   `complete: true`. One lost fragment therefore means silence until both stall timers fire.
2. **A lost `BlockAck` is fatal.** The sender's only reaction to silence is
   `poll_timeout` → `Failed { Stall }` at `block_stall_ms` = 120 s. A `BlockAck` is one short frame,
   and losing one on HF is ordinary.
3. **The stall timers do not scale with airtime.**
   - **Sender.** `begin_block` arms the 120 s deadline **before** the block is transmitted. Each
     receive tick runs `poll_timeouts`, then `drain_filexfer_tx`, which transmits the block
     synchronously (`server.rs`). A default 16 KiB block is 66 fragments, about 9.5 min at BPSK250
   (filexfer fragments go out uncoded through `transmit`: 8.6 s each, measured in the #1461 twin),
     so the first tick after the drain finds the deadline long past. The transfer fails before the
     peer can answer.
   - **Receiver.** It re-arms its deadline only on a completed block, so a block whose fragments
     take longer than 120 s also fails.
   - **Consequence:** any block with more than about 120 s of airtime fails on a perfect channel.
     The twin cannot show this, because loopback transmits in zero wall-clock time; the
     state-machine tests below inject time.

## Decision (proposed, revised after review)

No wire change: every frame used already exists. Review: `reviews/review-filexfer-selective-repeat.md`.

**R1 — the receiver answers fragments; it never transmits on a timer.** For each fragment of the
active transfer:
- **The block is already held** (complete, or seeded on resume): re-send
  `BlockAck { complete: true }`, and do **not** ingest it. SAR drops a segment on completion, so a
  lone fragment would start a candidate that is never expired (review 5).
- **Otherwise** ingest it. If the block is still incomplete, NACK
  (`BlockAck { complete: false, missing_frag_bitmap }`) when any of these holds:
  - its index is `frag_total − 1`, the end of the first round;
  - its index is `round_last`, the highest index of the last NACK sent, i.e. the end of a resend
    round;
  - it is a **duplicate** (already set in the arrival bitmap). That is the answer to a probe whose
    NACK was lost (review 1): without it, a lost NACK leaves the two ends waiting on different
    `round_last`s until the retries run out.
- **Transfer already finished** (`FileComplete` sent): the receiver keeps a short-lived record of
  the finished transfer, `transfer_id` → status and countersignature, for the receive stall time.
  A fragment or probe for that transfer re-sends `FileComplete` (review 2).

The ack is queued after every frame of the burst is processed, because the drain runs once per tick
after the frame loop (#1461).

**S1 — the sender probes instead of failing when the ack does not come.**
- When the ack-wait expires in `Sending`, it re-sends the last fragment of the round it just sent,
  `SendBlock { missing: {round_last} }`. By R1 that fragment draws a complete ack, a NACK or a
  `FileComplete`.
- **`FileComplete` while in `Sending` on the last block** is terminal success. The block's ack was
  lost, but the receiver verified the whole file (review 2).
- **Retry accounting** (review 6): a probe answered by nothing counts against `max_block_retries`.
  A NACK whose missing count is below the previous one resets the counter, because progress is
  being made. A NACK with no fewer missing counts. After `max_block_retries` the transfer fails
  `Stall` as today.

**T1 — timers measure silence, not airtime.**
- The sender arms its ack-wait when the round has finished **transmitting**. The daemon calls a new
  `note_round_sent(now)` after `drain_filexfer_tx` returns, if a round was in the queue. It is no
  longer armed when the block is queued.
- If the drain failed part-way, the rest of the queue is dropped (review 7). The probe and NACK
  path recovers that at the cost of one probe, and the event is logged.
- `ack_wait = max(120 s, 3 × airtime(one control frame at the active mode) + 30 s decode margin)`,
  computed with `estimate_air_secs` (review 4: at BPSK31 a `BlockAck` alone is tens of seconds).
- The receiver re-arms on every accepted fragment.
- The receiver's stall must outlast the sender's probe cycle:
  `rx_stall = (max_block_retries + 2) × ack_wait`.

**B1 — no reactive keying into a busy channel** (review 3, 4). `drain_filexfer_tx` defers while
`engine.is_channel_busy()`: the queue is kept, not dropped, and the next tick retries, as the beacon
path does.
- This closes the station-ID case: the sender's §97.119 ID runs after its drain and could otherwise
  collide with the receiver's immediate NACK.
- It also covers a probe whose ack-wait expires while the NACK is still in the air.
- A persistently busy channel (QRM) then stalls the transfer through the existing timers rather than
  keying over it.

## Risks and how each is checked

- **A collision: the receiver keys while the sender is still transmitting.** R1 keys only after a
  decoded fragment whose burst has ended, never on a timer. If the sender's queue held more frames
  after `round_last` in the same keying, the receiver would decode them in the same burst first. Can
  a sender queue anything after a block round in one drain? A control frame queued in the same tick
  would ride the same drain. Checked by test: the NACK is queued only after the whole burst is
  processed.
- **A probe storm.** At most `max_block_retries` (4) probes per block, each separated by `ack_wait`,
  and each one fragment long.
- **A stale `BlockAck` from an earlier round.** The sender already drops an ack for a different
  block. Within one block, an older NACK arriving after a newer round only repeats missing indices
  the sender then resends; it is bounded by the retry count.
- **Resume interplay** (`seed_held_block`, resumed transfers): a seeded block counts as complete, so
  a fragment for it draws a complete ack, which is correct.
- **The receive-side stall gets longer.** A dead peer now pins the receive slot for 12 min instead of
  2. That is acceptable: it is a slot, not an airtime cost, and the operator can cancel.

## Tests

- **State machine, injected time** (`openpulse-filexfer`):
  - the sender does not fail while a long round is still transmitting, and fails after its retries;
  - a probe follows ack-wait expiry;
  - the receiver re-arms per fragment.

  Each fails on today's code (sabotage control).
- **Daemon twin over a lossy bridge.** A bridge that drops chosen frames by index (a channel wrapper
  around the twin's AWGN):
  - drop fragment 2 of 4 → the file arrives after one NACK round;
  - drop the receiver's `BlockAck` → the file arrives after a probe;
  - drop the round's last fragment → the file arrives after a probe and then a NACK;
  - drop the NACK → a duplicate fragment draws a fresh NACK (review 1);
  - drop the last block's `BlockAck` → `FileComplete` ends the send as success (review 2).

  Each asserts the file arrives byte-exact and counts the extra keyings.
- **The daemon tick order** (`note_round_sent` after the drain): a twin with a slowed loopback is not
  available, so the daemon-level order is covered by a unit test of the call sequence. This is
  UNCHECKED end to end until the hardware-loopback stage.

## Consumer

`server.rs` receive tick (`process_received_bytes` → `filexfer::on_frame` → `on_block_fragment`;
`poll_timeouts`; `drain_filexfer_tx`); `filexfer::drive_tx_actions` (`SendBlock` → `enqueue_block`).
Found by `grep -n "fn on_block_fragment\|fn poll_timeouts\|fn drain_filexfer_tx\|fn drive_tx_actions"
crates/openpulse-daemon/src/*.rs`.

## Prior art

- `BlockAck` / `missing_frag_bitmap` / `encode_block(.., missing)` / `BlockAssembler::missing_bitmap`.
  The wire and the bitmap plumbing exist; only the triggers are missing.
- `SenderSession`'s retry counter.
- REQ-FX-05.

Found by `grep -n "missing_frag_bitmap\|fn missing_bitmap\|max_block_retries" crates/openpulse-filexfer/src/*.rs`.

## Twins

The PQ handshake's SAR (`transmit_handshake_frame` / `try_reassemble_handshake`) has no
retransmission at all: a lost fragment fails the handshake, and the operator retries. Not Release 1
(linksec ships disabled), so it is listed and not changed. Found by `grep -n "fn try_reassemble_handshake"
crates/openpulse-daemon/src/lib.rs`.
