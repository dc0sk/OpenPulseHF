---
project: openpulsehf
doc: docs/releasenotes.md
status: living
last_updated: 2026-07-14
---

# Release Notes

## Unreleased

Post-v0.5.0 improvements. **No breaking changes.**

**Reliability & safety**

- **The transmitter watchdog can no longer be blocked.** The safety timer that force-releases PTT after the
  maximum key-down duration now runs on its own independent thread, so it still fires even while the daemon
  is busy inside a long operation (a frequency scan, or a message/file send-retry burst) — previously the
  release could be delayed until that operation finished. And if the radio's own release fails (a stuck
  rig), the watchdog keeps retrying rather than telling clients the transmitter is down when it isn't.
- **Faster fail-safe on unexpected errors.** Every automatic transmit now releases PTT the instant its
  scope ends — including if the code hits an unexpected error mid-transmit — instead of possibly holding
  the transmitter keyed until the watchdog timer expires.

**Radio interfaces**

- **CM108 USB-HID PTT.** You can now key transmit through the GPIO on CM108/CM109/CM119 USB sound-card
  interfaces (DMK URI, RepeaterBuilder RA-series, AIOC, homebrew) — `--ptt cm108` on the CLI or
  `[modem] ptt_backend = "cm108"` in the daemon config. It auto-detects a C-Media device (or set the
  `/dev/hidrawN` path and GPIO pin), and needs no extra libraries.

**Mesh & TNCs**

- **Multi-hop mesh routes.** A mesh route discovery now records the full path it traverses, so the
  destination replies with the real end-to-end route instead of only the last hop.
- **KISS full-duplex control.** A KISS host can now toggle carrier-sense (CSMA) with the standard
  `FullDuplex` control frame.

**Under the hood**

- A proposed weak-signal **frequency-diversity** mode was measured end to end and **deliberately not
  shipped**: its diversity gain is consumed by the transmit-peak (PAPR) cost of a two-carrier waveform, so
  the net on-air benefit is ~break-even at twice the bandwidth — the existing options (drop to a slower
  mode, or retransmit-and-combine) do better. The measurements and analysis are kept in the repo for any
  future revisit.

## v0.5.0 — 2026-07-14

A hardening release: the deferred tail of a whole-codebase "what isn't nailed down" audit, worked to
completion. It's mostly correctness, regulatory-compliance, and robustness fixes, with a few new
capabilities. **No breaking changes** — existing configs and workflows keep working; the new behaviour is
either automatic (safer defaults) or opt-in.

**New capabilities**

- **Mesh route discovery, end to end.** Mesh nodes can now discover a route to a destination they can't
  reach directly, remember it, and use it to forward relay traffic — including keeping routes fresh (signed
  route updates) and tearing down a route a hop declines to carry (route rejects). Previously only the wire
  format existed; now the whole request → answer → apply → maintain flow works on air.
- **Per-band transmit attenuation.** Setting TX attenuation with a band now remembers a per-band value and
  re-applies it automatically when you retune to that band (like the existing per-band squelch). Setting it
  without a band still sets the global default.
- **Declared transmit power** (`[station] tx_power_watts`) and the operator callsign are now recorded in
  the station's transmit log on every path (daemon, ARDOP/KISS TNCs, mesh) — previously the log showed a
  blank callsign and 0 W outside two CLI commands.
- **PTT resync.** A new `openpulse daemon ptt-state` command (and `GetPttState` control command) lets a
  reconnecting client recover the current transmit state if it missed a change.

**Compliance & safety (§97.119)**

- The **ARDOP** and **KISS** TNCs now refuse to key the transmitter without a valid station identifier —
  ARDOP needs your `MYID`, and KISS requires a real AX.25 source callsign in the frame (no `N0CALL`). The
  mesh daemon already refuses to run as `N0CALL`, and the cross-band repeater now identifies its
  transmitting rig. This prevents an unidentified transmission from a misconfigured station.

**Reliability & robustness**

- A flood of control commands can no longer starve the receiver or the PTT safety watchdog, and a
  CONNECT/DISCONNECT or a long scan no longer stalls other commands the way it could before.
- The control WebSocket now fails closed when authentication is required, the ARDOP data port no longer
  silently drops frames under load, and several signal-processing reliability figures (soft-decision
  calibration for the dense QAM/OFDM/pilot modes, weak-signal JS8 decoding of real off-air transmissions,
  rendezvous timing) were corrected.

Full technical detail and PR links are in `docs/dev/project/changelog.md`.

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
