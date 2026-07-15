---
title: MFSK16 sub-floor ARQ seam — adversarial audit
status: living
last_updated: 2026-07-15
---

# MFSK16 sub-floor ARQ seam — adversarial audit (2026-07-15)

A focused 4-finder adversarial audit (concurrency/lockstep, DSP/signal, protocol/edge, regression/seam-gap;
Fable finders, refute-by-default, verified against source; Opus synthesis) of the code shipped in #885–#888
(v0.7.0). The headline: **as shipped, the rung was non-functional on real hardware** — three independent
critical breaks, each masked by a test artifact (LoopbackBackend buffering, 40 dB twin channel, locked-SL1
twin config, slope-only SNR assertion). The audit convergence across finders was strong; the criticals were
each confirmed by ≥2 finders and by direct source/numeric verification.

## Fixed (this pass)

- **C1 — K=3 ACK capture broken on cpal (CRITICAL; confirmed by 3 finders).** `receive_ota_ack_within` used
  per-iteration `stage_capture_input`, which opens a *fresh* input stream each call; on a real backend the
  audio buffered between reads (and during the 30 ms sleep) is dropped, so the ~4.84 s ACK was captured as
  discontinuous, hole-punched fragments the contiguous slot geometry can't align. **Fix:** hold ONE capture
  stream open across the window (as the daemon's data-RX tick already does), so reads are contiguous.
- **C2 — onset aligner broken at the operating SNR (CRITICAL; measured 15/45 vs 45/45).** `energy_onset`
  triggered on the first RMS ≥ 0.4×peak window; at full-band SNR ≤ ~7 dB *noise* crosses that, pinning onset
  to sample 0, so the fixed slot grid aligned only ~28 % of turnaround phases at the 0 dB design point. The
  ≥0.99 acceptance number (#885) was measured with the harness slicing ±220 around *known* copy starts —
  the measured mechanism was not the shipped one. **Fix:** anchor slots on **Costas acquisition**
  (`ModulationPlugin::acquire_copy_offset`, robust at low SNR); the 3 copies are a fixed span apart, so one
  anchor locates them all. Regression gate: `k3_ack_decodes_across_turnaround_phases_at_operating_snr`.
- **C3 — `estimate_snr_db` +21 dB hot (CRITICAL; measured 0 dB→+19.9).** The estimate was the per-Goertzel-
  bin SNR, ~10·log10(SPS/2) ≈ 21 dB above the true full-band scale the ladder's floors/ceilings use, so it
  could never fall below SL1's 5 dB ceiling and the rung self-ejected to dead BPSK31 after every decode.
  **Fix:** subtract the processing gain. Regression gate: `snr_estimate_intercept_is_on_the_channel_scale`.
- **H1 — payload-capacity bump was futile (HIGH; confirmed by 3 finders).** A >209 B body at SL1 bumped to
  SL2/BPSK31, but the peer's SL1-settled candidate set never includes SL2, *and* a >209 B BPSK31 frame is
  ~54 s > the 30 s burst-accumulator window — so the bump only burned airtime on a doomed frame, then
  silently dropped. **Fix:** replace the bump with an honest capacity guard — the daemon surfaces a warning
  and skips (the sub-floor rung is for ≤209 B traffic; a larger body needs the link to climb off SL1).
- **M1 — MFSK16 HARQ combining un-admitted (MEDIUM).** PR-2 admitted MFSK16+Rs to HARQ soft-combining, but
  `ota_retained_llrs`' length filter can't tell two MFSK16 frames apart (all one fixed 255-byte block) and
  the daemon session guard never fires (session id = local callsign), so a stale abandoned-message LLR set
  could pollute — worst case deliver — a later message. **Reverted;** re-admitting needs frame-identity-
  tagged retained LLRs.
- **L1 — a bad `ota_lock_level` silently disabled the lock (LOW).** Now warns.

## Deferred (documented, not fixed this pass)

- **session_hash is not cross-validated (MEDIUM; DSP#3).** During the 9 s union-listen the ISS adopts any
  valid ACK, including a co-channel session's, and returns success (dropping the message as delivered). A
  strict fix (validate `AckFrame.session_hash` against the peer's) risks rejecting *legitimate* ACKs on any
  callsign-format mismatch, and the connectionless OTA path has no negotiated shared session id. Needs a
  shared session identity, not the fragile per-callsign hash. Reachable only with two OpenPulse pairs on one
  frequency inside the ACK window.
- **Unconditional keyed Nack-ACK reply → storm (HIGH but pre-existing/broader).** The daemon answers *every*
  undecodable burst with an ACK; two OTA-active ends can answer each other's ACK bursts forever. Pre-existing
  with FSK4 (200 ms replies); #885–#888 escalate each cycle to a 5 s K=3 burst and make the seed (a late/
  replayed ACK landing in the rx tick) more likely. The proper fix (ACK-burst discrimination / Nack rate-
  limiting) is a broader daemon change beyond this seam.
- **ARDOP adaptive path not converted (LOW; R2).** The ARDOP RateAdapter path (opt-in `enable_adaptive_arq`
  + non-default `adaptive_profile = "hpx_hf"`) still uses the FSK4-only ACK, so an ARDOP IRS that reaches SL1
  answers MFSK16 with an FSK4 ACK that dies at that floor — degraded-but-safe (NACK-retry, not a desync; the
  ARDOP ISS `transmit_arq` floors at SL2, so only the IRS SNR path reaches SL1). A separate mechanism from
  the daemon receiver-led OTA; wiring K=3 into it is future work.
- **Mixed-profile / mixed-version ACK blackout (MEDIUM; D4).** A pair where one side carries MFSK16 and the
  other doesn't, with no verified handshake (fingerprint fail-open), can black out the ACK channel while one
  side recommends SL1. Bounded by the fingerprint suppression when a handshake completes.

## Method note

The three criticals were each invisible to every shipped test because the tests share masking artifacts:
LoopbackBackend drains its shared buffer atomically (hides C1), the twin e2e runs at 40 dB and pins
`ota_lock_level = "SL1"` (hides C2 and C3), and the SNR test asserted only slope (hides C3). Lesson
reinforced: for a receiver/timing feature, test through the production entry on a *realistic* channel and
turnaround phase, not the convenience seam on a clean loopback.
