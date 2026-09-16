---
project: openpulsehf
doc: docs/dev/reviews/artifacts/1310-capture-ticker-adoption.md
status: review
last_updated: 2026-09-16
---

# #1310 — adopting `CaptureTicker` in the ARDOP and KISS front ends

Design-class: changes the receive shape of two shipping front ends and adds a public method to
`CaptureTicker`.

## Prompt

Sent to Fable 5.1 as an adversarial review of the five-edit plan recorded on #1310, with five
questions asked for falsification rather than confirmation, plus three framing assumptions. The
packet stated the plan's own claims with `file:line` and asked which were generalised. Read at
`3807105b`.

## Verdict

**Ordering overturned, two of five edits refuted, one claim found generalised past its boundary.**

**The ordering was wrong in both directions.** #1315 does NOT have to land before #1310's ARDOP half,
and KISS should go FIRST — which the plan did not say. My concern that a held `CaptureTicker` stream
and the engine's own `stage_capture_input` (which does `open_input` per call) would be two streams on
one device was **confirmed in mechanism and refuted in consequence**: both live on the worker thread
(`Box<dyn AudioInputStream>` is `!Send`), so they are never concurrent in execution. What actually
happens is easier to fix and worse-shaped — the ticker's stream sits UNREAD for the whole ARQ loop,
its buffer accumulates our own TX plus the ACK audio, and the next `tick()` hands that blob to
`accumulate_capture`. That is #1007/#1319 verbatim, and the fix is `drop_stream()` before every keyed
transmit, not an ACK-path rewrite.

Routing the ACK through the shared accumulator was also rejected: `accumulate_capture` gates on DCD
and flushes on carrier drop, so a ~0.5 s FSK4 ACK would emerge as a burst tied to `rx_mode` — which
also aims the notch band. #1315's correct shape is the one already shipped at `receive_ota_ack_within`:
its own stream for the listen window, trial-decoding the accumulated buffer.

**Item 4 was incomplete.** The plan listed two ARDOP receive sites and missed a third:
`receive_with_ack_hint`, the adaptive IRS arm, which runs every 5 ms iteration when
`enable_adaptive_arq` is on — with a ticker held that is a second stream opened ~200x/s. It has no
burst equivalent (it derives `AckType` from `select_rx_ack_type(snr_db)` after a soft demod), so the
ARDOP PR must either convert it or gate the ticker on `!adaptive` and say so plainly.

**Item 1 was the wrong primitive.** `ota_decode_burst` is unusable for ARDOP by construction (it
requires an OTA session, takes candidates from the session profile, and updates the rate controller
and HARQ state on success — which `a_control_frame_does_not_touch_the_rate_controller` exists to
forbid). But a NEW `decode_burst_with_fec` is also wrong, because the coded onset scan already exists
inline in the OTA arm, twice. The right edit is to add `fec: FecMode` to `scan_burst_onsets` and
`decode_burst_inner` — a parameterisation, not a fourth copy.

**Item 2's justification was generalised past its boundary.** My claim that a raw-sized slice cannot
hold a coded frame is true only ABOVE 213 B of payload (wire = payload + 10; RS(255,223) is one block
iff wire <= 223). Below it the raw slice already holds the coded frame, which is what the shipped OTA
coded arm relies on today. The widening is still right — `decode_prefix` rescues a slice longer than
the frame and never one shorter — but the gate for it must use a >213 B payload at a non-zero onset
or the widening is untested.

**Item 5 is both halves, not either/or.** The explicit `drop_stream()` before each keyed site is
load-bearing; a counter read on the next tick cannot prevent the problem, because the ACK listen
happens inside the transmit loop before any next tick. ARDOP has no transmit off the worker thread
today, so the explicit drop is complete and the counter is a backstop.

**The gate's discriminator is not what the plan said.** "Off the chunk grid" is irrelevant here:
alignment mattered in #1247 because that path also runs a whole-buffer decode per raw chunk, whereas
the ticker path decodes nothing until a flush. The real property is that no single read holds a
decodable frame. The model to copy is `repeater_relays_a_daemon_burst.rs` — lead-in silence, the
frame in chunked reads, then silence reads so the flush comes from DCD energy dropping below the
squelch. And the PR claim must be narrowed to "accumulates across reads and flushes on carrier drop
(silent fixture)"; "receives on hardware" is an on-air-tier claim no in-process test reaches.

## Revised ordering, adopted

1. **PR1a — KISS + `drop_stream`** (this change). Needs none of items 1-3.
2. **PR1b — engine `fec` threading**, gated with a >213 B payload at a non-zero onset.
3. **PR1c — ARDOP**: non-adaptive IRS arms + `do_receive` + drop before all six keyed sites;
   adaptive arm converted or explicitly excluded; `[Rs, None]` when `fec_rx` is set.
4. **#1315 after, independent.**

## Two findings filed out of scope

`burst_onset_scan_bounds` returns the RAW `max_frame_samples` and the OTA coded arm slices with it at
every non-zero onset, so a two-block `Rs` frame may be sliced ~1.77x too short in the daemon — filed
as #1384, **code-read and not measured**, with the measurement stated. And ARDOP accepts 4096-byte
host frames while `Frame::new` rejects over 255 B, with no segmentation anywhere in the crate — filed
as #1385.

## Consumer

`openpulse-kiss`'s `worker_loop` (`crates/openpulse-kiss/src/bridge.rs`), the TNC's only receive path,
reached from `spawn_worker` off `main.rs`. `drop_stream`'s other consumer-to-be is the ARDOP bridge in
PR1c. The existing `CaptureTicker` consumer is the cross-band repeater's carrier sense.

Command: `grep -rn "CaptureTicker" --include=*.rs crates/` → `openpulse-modem/src/capture_ticker.rs`,
`openpulse-repeater/src/lib.rs`, and now `openpulse-kiss/src/bridge.rs`.

## Prior art

`CaptureTicker` itself (#1297) is the shipped pattern and was adopted rather than re-implemented.
`server.rs`'s `rx_ticker` is the open-coded original and the source of the drop-before-keying move
(#1319). `repeater_relays_a_daemon_burst.rs` is the fixture shape reused for the chunked backend.
`FecCodec::decode_prefix` already handles a slice longer than the frame, which is why the slice only
needs to be long ENOUGH — that is what bounds item 2.

## Twins

`openpulse-ardop`'s four receive sites (two non-adaptive IRS arms, `do_receive`, and the adaptive
`receive_with_ack_hint`) plus its ISS ARQ `receive_ack_with_short_fec` — all still on the one-shot
`receive`/`stage_capture_input` shape, deliberately left to PR1c. `server.rs`'s `rx_ticker` is the
third copy and is left alone. The module doc in `capture_ticker.rs` was corrected in this change
because it named only two of those sites and cited four line numbers that had drifted.
