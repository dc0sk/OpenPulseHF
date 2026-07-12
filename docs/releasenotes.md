---
project: openpulsehf
doc: docs/releasenotes.md
status: living
last_updated: 2026-07-12
---

# Release Notes

## Unreleased

## v0.4.0 — 2026-07-12

- JS8 station discovery (new, opt-in): when your station is idle you can have it tune to the
  band's JS8 calling frequency and discover other OpenPulse stations there. It uses a native JS8
  waveform that interoperates with JS8Call — no separate JS8Call install — marks itself with an
  `@OPULSE` capability hint, and lists the stations it hears (`openpulse daemon stations` /
  `openpulse daemon peers`, or a new **Discovery** tab in the panel). Enable it under `[discovery]`.
  **Off by default, and receive-only until you explicitly opt into transmitting.**
- JS8 beacon + rendezvous (new, opt-in transmit): with a callsign configured and `[discovery]
  mode = "beacon"` or `"full"`, your station periodically announces itself (a heartbeat and the
  `@OPULSE` hint). In `full` mode it can also negotiate a working frequency with a discovered peer
  over JS8 (the `RendezvousWith` control command), QSY both stations there, and start an
  authenticated OpenPulse session. Every transmit path is off by default and gated behind an
  explicit mode + your callsign + a ±2 s clock-sync check; the automatic-control behaviour is
  documented in `docs/regulatory.md` (FCC §97.221) — you remain the control operator.
- Direct file transfer (new, opt-in): send a file to a connected peer over the air with an
  offer/accept prompt, a progress bar, and optional size-gated auto-accept. Every transfer is
  signed and checksummed and verified against the peer's identity key on the way in — a tampered
  or wrong-sender file is quarantined and flagged UNVERIFIED. Large files are split into blocks and
  can resume after a dropped session. Enable it under `[file_transfer]`; drive it from the panel's
  **Files** tab or `openpulse daemon send-file` / `openpulse daemon files`.
- Faster, more reliable links on good conditions: the adaptive rate ladder now climbs into the
  high-throughput dense modes (it was previously capped mid-ladder by a signal-quality estimator
  that flattened out), the HF ladder switched to OFDM for better performance on fading paths, and
  repeated retransmissions now combine their soft information instead of being retried cold.

## v0.3.0 — 2026-06-29

- Authenticated connections: when you connect to a peer, the daemon now exchanges a
  signed handshake over the air and verifies the peer's identity and Maidenhead grid.
  The verified grid is written to your ADIF logbook. Set your station signing key with
  `[station] identity_key_path` (an Ed25519 seed; auto-generated on first run).
- ARDOP adaptive ARQ (opt-in): set `[ardop] enable_adaptive_arq = true` to let the rate
  ladder adapt over the link and make the host `ARQBW` (bandwidth cap) and `ARQTIMEOUT`
  (idle disconnect) hints take effect. Off by default (fixed-mode, unchanged behaviour).
- Generic serial CAT: drive a transceiver that Hamlib/rigctld doesn't support by setting
  `[radio] cat_backend = "generic"` with a serial port and a rig-definition TOML (build
  with `--features generic-serial`, Unix).
- Automatic ADIF logbook (opt-in, `[logbook]`): one record per contact
  (connect→disconnect), with the worked station's grid taken from the verified handshake
  or a `[logbook.peer_grids]` config map. Runtime `SetLogbook` toggle (CLI + panel).
- Receiver auto-notch productionized into the engine: multicarrier-aware, with persistence
  and user controls, plus automatic QSY on a confirmed in-band interferer a notch can't
  remove.
- Operator panel rework: controls moved to a resizable right side-panel with a full-width
  waterfall and the session status below it; new AGC on/off toggle alongside the
  Notch / CE-SSB / Logbook toggles and the squelch slider. Full control-surface parity
  across daemon / CLI / panel.
- linksim: I/Q constellation views (symbol-spaced crisp dots) flanking a QR-branded info
  band, regrouped Station B views with waterfall/constellation toggles, a CE-SSB toggle,
  an SNR plot, extra FEC modes (LDPC / Turbo / RS-Strong / Concatenated), and a `--serve`
  mode so the operator panel can attach to a live simulation with no radio.
- New CLI command `openpulse daemon set-tx-attenuation <db> [--band <label>]` for
  headless/scripted TX-attenuation control.
- Fix: CE-SSB is now gated off for dense OFDM higher-order modes (8PSK and above), where
  it caused a ~6 dB decode regression.

## v0.2.2 — 2026-06-25

- Live rig-meter polling (ALC / power-out / SWR) over a dedicated rigctld connection,
  surfaced as panel status for drive tuning.
- Guided ALC drive tuning: `openpulse calibrate drive` steps TX attenuation until the
  rig's ALC sits in a target band (keeps CE-SSB on dense OFDM-HOM from over-driving the PA).
- On-air SDR spectral-measurement toolset (scripts) and a one-shot twin-station demo.

## v0.2.1 — 2026-06-24

- CE-SSB transmit envelope conditioning (controlled-envelope SSB, Hershberger W9GR, QEX
  2014): an adaptive, per-mode, default-on TX conditioner for the high-PAPR multicarrier
  modes (OFDM / SC-FDMA) that raises average TX power at fixed PEP. `[modem] cessb_enabled`,
  a `SetCessb` control command, `openpulse daemon set-cessb`, and a panel toggle.
  Channel-sim **+1.6 / +2.7 / +3.8 dB** average power on OFDM52 at zero BER cost; on-air
  confirmed **+1.18 dB** (FT-991A). Believed to be the first open-source HF *data* modem to do this.
- Operator panel: Messages presented as a tab alongside the Event Log.

## v0.2.0 — 2026-06-21

- Two-station link simulator (`openpulse-linksim`, new crate): proves the **effective
  two-way transfer rate** under simulated SNR / noise / fading — real forward data frames
  through a channel, real FSK4 ACKs over a reverse channel, over-the-air rate adaptation
  along a profile ladder, and honest goodput accounting (forward + ACK air time +
  turnaround over retransmissions). CLI sweep → effective-rate table/JSON; GUI with live
  spectra/waterfalls and an SNR slider.
- Signal-path testbench: explicit 2×4 spectrum/waterfall grid (fixes unrendered
  waterfalls), all modes with **measured** per-mode bitrates, and new sources — virtual
  loop, dual-card hardware loop, test-matrix runner, and an adaptive-ladder view.
- Bandplan guardrails now recognize active `-RRC` variants and `SCFDMA52-64QAM-P4` in
  occupied-bandwidth checks; `BandplanPolicy::default()` uses `HamIaruRegion1`; Region 3
  exposes an explicit conservative-proxy warning.
- TX compliance logs reject cross-station frame metadata; session metrics publish
  throughput as an explicit upper-bound proxy with a dedicated note field.
- BL-TP-7 SC-FDMA pilot-density Doppler review coverage (dense vs sparse pilots under
  deterministic Watterson channels).
- `qpsk-plugin` demodulation uses lower-overhead carrier/downmix loops; the `QPSK1000-HF`
  equalizer profile is pinned to `mu=0.015` to match validated Watterson characterization.
- On-air orchestration scripts (`onair-preflight.sh`, `run-onair-tests.sh`,
  `onair-bundle-evidence.sh`) with `--help`, default local preflight, preflight metadata
  in reports, and structured evidence bundles (incl. repo-state traceability).
- Adaptive-rate ACK-UP progression skips unmapped reserved profile rungs; SNR-gated
  admission limited to HPX wideband-HD SL13→SL14.
- Project docs organized under `docs/` with consistent frontmatter; PR docs validation and
  automatic `last_updated` stamping.

## v0.1.0

- First public OpenPulseHF release.
- Introduced plugin-based modem architecture in a Cargo workspace.
- Included BPSK mode support and loopback-based testing path.
