# OTA adaptive rate-stepping — hardware validation runbook

Validate receiver-led OTA adaptive rate-stepping end-to-end on two stations. This
is the on-air / dual-clock counterpart to the in-process loopback tests
(`crates/openpulse-modem/tests/ota_rate_lockstep.rs`,
`crates/openpulse-modem/tests/modcod_ladder.rs`) that already prove the mechanism
single-clock and hardware-free.

> **Author's note:** this runbook is for an operator with the two stations in
> hand — it is not run by CI and cannot be executed without RF hardware (or one of
> the audio-loopback rigs below). Each step lists what to do and what to expect.

## What is already in place (no hardware needed)

- Engine: `start_ota_session` / `respond_arq_ota` / `poll_ota_rx` /
  `transmit_arq_ota` / `apply_ota_ack`, the lockstep candidate fallback, and the
  M2M4 RX SNR estimator.
- Daemon: starts an OTA session from `[modem] ota_enabled` and applies
  bounds/lock/A2/A3 from config; emits `OtaStatus` ~1 Hz. **The RX tick now routes
  to OTA when a session is active** — see below.
- Control: `openpulse daemon ota-start|ota-stop|ota-bounds|ota-lock|ota-unlock|ota-hysteresis|ota-status`;
  panel toolbar OTA line + Lock/Unlock (PR #497).

## RX→OTA routing (now wired — validate with hardware in the loop)

The daemon's RX tick routes to OTA automatically: when `engine.ota_active()`, it
calls `engine.poll_ota_rx(session_id, None)` instead of `engine.receive(...)`.
`poll_ota_rx` captures one window, and **only if the window carries energy** (an
idle squelch gate so the daemon never keys PTT to ACK silence) it decodes with the
candidate fallback and returns the decoded payload plus the ACK frame to send
**without transmitting it**. The daemon then asserts PTT via the configured
`PttController`, transmits the ACK with `transmit_ack_with_short_fec`, and releases
PTT — so a half-duplex radio receives with PTT down and only keys to answer.

What still needs hardware/peer validation (not code): the **turnaround timing** —
PTT key-up delay, ACK timeout, and CSMA against a real peer. On the data-sending
station, drive `engine.transmit_arq_ota(...)` from the message/transmit path and
observe the keying with both stations connected. The session id stamped into the
ACK is the station callsign (`[station] callsign`), falling back to `"ota"`.

## Station setup

Two stations (e.g. `rpi51`, `rpi52`) — real radios, or one of:

- **Virtual single-clock** (`scripts/` snd-aloop rig, `~/.asoundrc` aloop_tx/aloop_rx)
  — isolates DSP from analog/dual-clock effects. Both "stations" on one host.
- **Dual-card hardware loopback** (`scripts/setup-loopback-dualcard.sh` +
  `run-loopback-dualcard.sh`) — both USB soundcards on the dev PC, a true dual-clock
  rung without two Pis. Use `CAPTURE_GAIN=16` (not max) to avoid clipping.
- **On-air / cabled radios** — the real target; FT-991A must use **CAT PTT**
  (`B_PTT_TYPE="CAT"`), not RTS.

Config (`~/.config/openpulse/config.toml`) on both:

```toml
[station]
callsign = "<yours>"

[modem]
ota_enabled = true
ota_profile = "hpx_modcod"   # or hpx_hf; the MODCOD ladder exercises modulation×FEC
# Optional guardrails:
# ota_max_level = "SL10"     # regulatory bandwidth / robustness cap
# ota_min_level = "SL3"
ptt_backend = "rigctld"      # or rts/dtr/vox per rig; CAT for FT-991A
```

Build with real audio: `cargo build --release -p openpulse-daemon --features cpal`.

## Procedure

1. **Bring up both daemons.** Confirm each logs `OTA adaptive rate-stepping enabled`
   with the configured profile.
2. **Confirm idle status.** On each: `openpulse daemon ota-status` (or watch the
   panel OTA line). Both should report `active: true`, `tx_level: SL2` (profile
   initial), `is_locked: false`.
3. **Start a sustained transfer** from station A to B (a multi-frame message).
   Watch B's `ota-status`: `rx_confirmed_level` should climb as the M2M4 SNR
   estimate crosses each rung's ceiling, and A's `tx_level` should follow B's
   `recommended_level` one frame behind.
4. **Lockstep under ACK loss.** Attenuate the **reverse** (B→A ACK) path (lower B's
   TX, or detune) so some ACKs are lost. **Expectation:** A never desyncs — every
   forward frame still decodes at B (the candidate set covers A's actual level);
   the climb pauses on lost ACKs and resumes. No cascade of NACK downgrades from
   ACK loss alone.
5. **Asymmetric SNR (per-direction).** Make forward and reverse SNR differ. Confirm
   A→B and B→A settle at independent levels.
6. **Down-step on degradation.** Increase forward-path attenuation/noise.
   `rx_confirmed_level` should step down (SNR floor crossing and/or NACK threshold);
   confirm it recovers when conditions improve.
7. **Operator controls.** `openpulse daemon ota-lock --level SL4` → both
   `tx_level`/`rx_*` pin at SL4 and stop moving; `ota-unlock` resumes.
   `ota-bounds --max SL6` → the climb caps at SL6.
8. **MODCOD rungs.** With `hpx_modcod`, confirm the ladder traverses FEC steps at a
   fixed modulation (BPSK250+LDPC → BPSK250+RS) before the modulation changes —
   visible as `tx_fec` changing while `tx_mode` holds.

## Pass criteria

- [ ] Rate climbs from the initial rung to a steady level matched to the link SNR.
- [ ] No desync: forward frames keep decoding through ≥30 % reverse-ACK loss.
- [ ] Per-direction levels adapt independently under asymmetric SNR.
- [ ] Down-step on degradation; recovery on improvement.
- [ ] Lock/unlock and min/max bounds behave as commanded.
- [ ] MODCOD: FEC rungs are used between modulation steps.
- [ ] Station ID / regulatory compliance per `docs/regulatory.md` and
      `docs/on-air_testplan.md`.

## Diagnostics

- `openpulse daemon ota-status` (JSON snapshot) and the panel OTA line (~1 Hz).
- Daemon NDJSON event stream: `OtaStatus`, `EngineEvent::RateChange`,
  `EngineEvent::FrameReceived`.
- If the climb stalls at a low rung on a clearly-good link, suspect the M2M4 SNR
  estimate vs the profile ceilings — log `record_rx_snr` / `last_rx_snr_db` and
  compare against a known-SNR injection (`set_rx_snr_estimate`).

## Related

- In-process proofs: `ota_rate_lockstep.rs`, `modcod_ladder.rs`.
- Loopback transports: `docs/dev/virtual-loopback.md`.
- On-air test plan + regulatory checklist: `docs/on-air_testplan.md`,
  `docs/regulatory.md`.
