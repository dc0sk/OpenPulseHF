---
project: openpulsehf
doc: docs/dev/project/changelog.md
status: living
last_updated: 2026-07-14
---

# Changelog

> Phase/roadmap history lives in [roadmap.md](roadmap.md); this file tracks
> user-visible changes. "Unreleased" = merged to `main`, not yet in a tagged release.

## Unreleased

Post-v0.5.0 block-B/D backlog. No breaking changes.

### Features

- **PTT watchdog preempts a blocked command loop**: the transmitter's max-keyed-duration force-release now
  runs on an independent watchdog thread, so it fires even while the daemon's async command loop is blocked
  inside a long handler (a QSY scan or an OTA send-retry burst) — the previous `select!`-arm watchdog (#853)
  could not, because the loop never re-enters `select!` during such a handler. A stuck rig (release keeps
  failing) is retried every tick and never falsely reported as released. (#863)
- **Transmitter-release RAII guard (unkey-on-Drop)**: every automatic transmit scope now releases PTT on
  scope exit — including on an early return or a panic/unwind — so an unexpected key-down is bounded to the
  current scope instead of waiting up to the 180 s watchdog. (REQ-PTT-01, #872)
- **CM108 USB-HID PTT backend** (`--ptt cm108` / `[modem] ptt_backend = "cm108"`): key PTT via the
  CM108/CM109/CM119 sound-chip GPIO on cheap USB interfaces (DMK URI, RA-series, AIOC, homebrew). A plain
  `/dev/hidrawN` write — no extra dependency; `ptt_device` selects the path (empty = auto-detect a C-Media
  device) and `ptt_gpio` the pin (default 3). (REQ-PTT-02)
- **GPIO-line PTT backend** (`--ptt gpio` / `[modem] ptt_backend = "gpio"`, `gpio` feature): key PTT via a
  Linux GPIO line (e.g. a Raspberry Pi header pin) over the `gpiocdev` char-dev uAPI; `ptt_device` carries
  a `chip:line[:active_low]` spec (e.g. `gpiochip0:17`). (REQ-PTT-03)
- **Daemon serial PTT**: the daemon now supports `rts`/`dtr` serial PTT (behind its new `serial` feature)
  using `[modem] ptt_device` as the port path, instead of silently disabling it.
- **Hotplug-safe audio device selection**: `[audio] device` is now resolved with a match ladder (exact
  name → ALSA `CARD=` token → case-insensitive substring, ambiguity is an error), so a device the OS
  renames or reorders (e.g. gains a `(2)` suffix, or its `hw:N` index shifts) still resolves instead of
  failing with `DeviceNotFound`. (REQ-DEV-01)
- **Simultaneous multi-mode receive** (`[monitor]`, off by default): the daemon can decode a list of extra
  modes from every capture burst in parallel with the active session, emitting a `MonitorFrame { mode,
  bytes }` event per decode — a monitor/discovery role for seeing what else is on frequency. (REQ-RX-01)
- **Receiver AGC config gate** (`[modem] agc_enabled`, off by default): the existing receiver AGC can now
  be enabled from the config file (with `agc_target_rms`/`agc_bandwidth`/`agc_max_gain_db`). Decode is
  already level-invariant, so the AGC stabilises the level through deep QSB and provides a metering
  readout rather than rescuing a decode. (REQ-AGC-01)
- **Mesh route discovery — source-accumulated multi-hop paths**: a `RouteDiscoveryRequest` now accumulates
  the traversed path as it floods (each forwarder appends itself), so the destination answers with the real
  end-to-end route instead of only `destination → [self]`. (#861)
- **KISS FullDuplex → CSMA**: the KISS `FullDuplex` control frame now toggles the engine's carrier-sense
  channel access (non-zero → full duplex → CSMA off; zero → CSMA on). The keying-delay control frames
  (TXDELAY/TXtail/P/SlotTime) remain no-ops — this TNC has no PTT-keying layer. (#862)

### Library

- **`ModemEngine::combine_and_decode_llrs`**: the audio-free union LLR decode (decode-each-alone, then
  MAP-combine) extracted from `receive_with_llr_combining` and made public — for an external diversity/HARQ
  combiner that has already demodulated its branches. Behaviour-preserving refactor. (#869)

### Notes

- **Weak-signal frequency-diversity rung: measured, not shipped.** A dual-carrier frequency-diversity rung
  (#864) was measured end to end (a ρ=0 ideal upper bound + the real-waveform net). The ideal cleared the
  kill-gate (~4 dB on slow fade), but the real waveform's ~2.6 dB two-tone PAPR consumes almost all of the
  ~1–2.6 dB matched-power gain → net on-air ≈ break-even at 2× bandwidth, dominated by the existing
  baud-drop and HARQ levers. The reproducible measurements and the analysis
  (`docs/dev/research/weak-signal-diversity-measurement.md`) landed; the rung did not. (#869)

## v0.5.0 — 2026-07-14

The 2026-07-13 "loose-ends" audit fix-down (issue #830, roadmap Phase 12): a 10-dimension refute-by-default
audit whose deferred tail was worked to completion. No breaking changes.

### Features

- **Route discovery — fully driven (0x03–0x08)**: the wire codecs had no driver. Added the request/response
  drive — a node originates a `RouteDiscoveryRequest`, answers when it is the destination or holds a cached
  route (self-authenticating Ed25519), and applies the `RouteDiscoveryResponse` into a bounded/TTL route
  table; the mesh daemon originates (`discover_route`), applies, and **consumes** a route for relay send
  (`send_via_route`, scored via `select_best_scored_route`). Plus the route-**maintenance** drive: signed
  `RelayRouteUpdate` (0x07, authoritative table refresh) and on-path-authorized `RelayRouteReject` (0x08,
  teardown only from a hop actually on the route), with `send_route_update`/`send_route_reject`. (#840,
  #841, #850, #856)
- **Per-band TX attenuation**: `SetTxAttenuation { band }` now honors the optional band — an engine-side
  per-band store applied on retune (mirrors per-band DCD squelch); a matching override wins on the current
  band. (#851)
- **PTT state resync**: a new `GetPttState` control command (and `openpulse daemon ptt-state`)
  re-broadcasts the current PTT state so a client that missed an edge can recover. (#843)
- **Declared TX power**: new `[station] tx_power_watts` config, recorded in the §97 regulatory TX log. (#849)

### Fixes

- **Regulatory (§97.119)**: the ARDOP TNC refuses on-air TX without a valid host `MYID` (host data / IRS
  ACK / auto-ID / relay), and the KISS TNC gates on the AX.25 source callsign per frame; the mesh daemon
  refuses to run as `N0CALL`, and the cross-band repeater station-IDs its transmitting rig. The regulatory
  TX-metadata log now records the operator callsign + declared power on the daemon/ARDOP/KISS/mesh paths
  (previously empty/0 W). `transmit_iq` is routed through the same compliance bookkeeping as the audio seam.
  (#847, #848, #827, #819, #849, #852)
- **DSP soft-LLR calibration**: 64QAM / OFDM / pilot / GPU soft demods emitted over-confident LLRs on dense
  grids (up to ~1599×); recalibrated from a known preamble/pilot residual plus a channel-estimation-error
  term — matters for HARQ combining. (#833, #834, #835, #837)
- **Robustness / concurrency**: the ARDOP CONNECT/DISCONNECT engine lock and the daemon PTT watchdog no
  longer block on / get starved by the async command loop (`spawn_blocking`; a dedicated watchdog
  `select!` arm; `biased` removed); the WebSocket control port fails closed when auth is required; the
  ARDOP data port no longer silently drops frames; the filexfer per-peer quota counts the `.partial`
  subtree; the InputCapture seam is not re-applied per decode-burst slice. (#846, #853, #817, #818, #820,
  #842, #826)
- **Discovery / JS8**: real off-air overs decode via a time search; the rendezvous timing/RxOnly cluster is
  fixed; `jsc_decompress` is guarded against a u32 overflow; the clock-skew TX gate is now live. (#814,
  #815, #816, #822)
- **Validation / correctness**: inconsistent file-offer geometry is rejected; `SetMode`/`SetConfig`
  validate before mutating shared state; the BPSK crossfade-ISI cancellation is kept off the soft
  (differential) path so it doesn't break HARQ LLR calibration. (#824, #823, #821, #832)

### Tests & docs

- New coverage for the command-path PTT hardware-failure guard, the discovery `server::run` handoffs
  (DCD-defer, dwell-tee, rendezvous-connect), and the daemon filexfer resume composition. (#836, #839,
  #845, #844)
- `docs/cli-guide.md` gains the daemon / FF-15 / FF-16 control CLI; the README `hpx_hf` ladder row, the
  panel mode list (12 PILOT modes), and roadmap Phase 12 are brought current. (#838, #828, #854, #857)

## v0.4.0 — 2026-07-12

- **JS8 station discovery + rendezvous (FF-15)**: a native JS8-compatible weak-signal waveform (8-GFSK, 79 symbols, Costas 3×7 sync, LDPC(174,87), CRC-12 — ported bit-exact from GPL-3.0 JS8Call, validated against compiled Boost/Qt5, **not** a JS8Call bridge) plus an idle-time discovery service in the new `crates/openpulse-discovery`. When enabled, an idle station QSYs to the current band's JS8 calling frequency, participates as a real JS8 station, marks itself with an in-band `@OPULSE` capability hint, and folds recognized OpenPulse stations into the shared `PeerCache`. Operator surface: `openpulse daemon {enable-discovery, disable-discovery, stations, peers}` + a panel `Tab::Discovery`; `[discovery]` config. **Beacon TX** (heartbeat + `@OPULSE` hint via a new `transmit_raw_audio` seam, Phase E) and **rendezvous → HPX handoff** (a 2-message Propose/Accept/Reject over JS8 directed free text → scheduled QSY → the signed CONREQ/CONACK handshake, Phase F, via the `RendezvousWith` control command) are **off by default**, gated behind `[discovery] mode = "beacon"`/`"full"` + a configured callsign + ±2 s clock-skew/DCD/self-ID gates; §97.221 automatic-control documentation in `docs/regulatory.md`. RX-only until opted in. Only on-air validation (Phase H) is deferred. (PRs #744–#805)
- **Direct P2P file transfer (FF-16)**: send a file to a connected peer over an RF session with an offer/accept handshake, progress, and size-gated auto-accept — plus an inline signed `TransferManifest` + SHA-256 verified against the peer's handshake key, so a tampered or wrong-key file is quarantined with an UNVERIFIED badge (verification VarAC's file transfer lacks). New `OPFX` wire (`crates/openpulse-filexfer`), files split into ≤48 KiB blocks over SAR (multi-megabyte transfers, config-capped at 1 MiB), hybrid delivery (OTA per-burst rate + block-ack bitmap selective retransmit) with block-level `.partial` resume and airtime-bounded PTT bursts. `[file_transfer]` config; `SendFile`/`AcceptFile`/`RejectFile`/`CancelFile`/`ListFiles` control + CLI; panel `Tab::Files`. On-air validation (Phase F) deferred. (PRs #730–#743, #787)
- **Adaptive rate ladder + DSP (signal-chain audit, Phase 11)**: the OTA rate ladder now climbs into the dense high-throughput rungs on good links — the M2M4 SNR estimator was waveform-blind and capped it mid-ladder (replaced by a per-plugin symbol-domain estimate that tracks to ~SL17). The dispersive-HF ladder (`hpx_hf`) is re-seated from SC-FDMA to **OFDM**, which measurably beats SC-FDMA on selective fading at matched rate; HARQ soft-LLR combining now engages across retransmissions in the daemon OTA path; and a batch of correctness fixes landed (inverted DFE feedback sign, AGC/DCD seam-ordering, SC-FDMA sync back-off / delay-cliff, CE-SSB whitening on low-entropy frames). Channel-model measurement fidelity was corrected (Watterson unity-power, Gilbert-Elliott per-symbol bursts, opt-in continuous fade) and a real-modem CI goodput-regression gate added. (PRs #697–#717)

## v0.3.0 — 2026-06-29

- **Security/Identity**: The daemon now performs the Ed25519 signed handshake over RF on connect — the initiator sends a signed `ConReq`, the responder verifies it and replies with a signed `ConAck`, and the initiator verifies that (both SAR-fragmented, since the frames exceed one modem frame). The verified peer callsign + Maidenhead grid are stored, a `PeerVerified` event is emitted, and the verified grid is written to the ADIF logbook (ahead of the `[logbook.peer_grids]` fallback). New `[station] identity_key_path`; 30 s handshake timeout (PR #584).
- **ARDOP TNC**: Opt-in adaptive ARQ session via `[ardop] enable_adaptive_arq` / `adaptive_profile`. With it on, the host `ARQBW` hint now caps the adaptive rate ladder by occupied bandwidth and `ARQTIMEOUT` drops an idle connection (both were accepted-and-echoed no-ops before). New rate-policy bandwidth-cap API (`set_arq_max_tx_level`), distinct from the OTA bounds (PR #585).
- **Radio/CAT**: The generic serial CAT backend is now selectable from the daemon for rigs Hamlib/rigctld doesn't support — `[radio] cat_backend = "generic"` with `serial_port` + `rig_file`, built with `--features generic-serial` (Unix). `RigctldController` gained its `CatController` impl (PR #586).
- **Logbook**: Automatic opt-in ADIF logbook — one record per contact (connect→disconnect); worked-station `GRIDSQUARE` from the verified handshake or a `[logbook.peer_grids]` config map; runtime `SetLogbook` toggle (CLI + panel).
- **Receiver auto-notch**: Productionized into the engine (multicarrier-aware, persistence, user controls); automatic QSY on a confirmed in-band interferer a notch can't remove; three seam-gap fixes from the notch-class audit and a single RX front-end seam.
- **Operator Panel**: AGC on/off toggle (PR #583); controls moved to a resizable right side-panel with a full-width waterfall and status below it (PR #579); `SetFreq` panel control; control-surface parity closed on both CLI and panel sides; `daemon set-tx-attenuation` (PR #587).
- **linksim**: I/Q constellation views with symbol-spaced (crisp-dot) sampling (PRs #574/#575), regrouped Station B views with waterfall/constellation toggles (PR #578), QR-branded info band, CE-SSB toggle, SNR plot, LDPC/Turbo/RS-Strong/Concatenated FEC modes, and a `--serve` mode so the panel attaches with no radio (PRs #580/#581).
- **Fix**: CE-SSB is gated off for dense OFDM higher-order modes (8PSK and above), where it caused a ~6 dB decode regression.
- **Docs**: Sorted `docs/dev/` into topic subfolders (`design/`, `pki/`, `research/`, `project/`) with all references updated (PR #582); manual + changelog + release notes brought current (PRs #588, #589).

## v0.2.2 — 2026-06-25

- **Drive tuning**: Live rig-meter polling (ALC / power-out / SWR) over a dedicated rigctld connection, surfaced as panel `RigStatus`; guided ALC drive tuning via `openpulse calibrate drive` (steps TX attenuation toward a target ALC band).
- **Tooling**: On-air SDR spectral-measurement script set and a one-shot twin-station demo.

## v0.2.1 — 2026-06-24

- **CE-SSB**: Controlled-envelope SSB TX conditioning (`openpulse_dsp::cessb`) — an adaptive, per-mode, default-on conditioner for the high-PAPR multicarrier modes (OFDM / SC-FDMA) that raises average TX power at fixed PEP. `[modem] cessb_enabled`, `SetCessb` control, `openpulse daemon set-cessb`, panel toggle. Channel-sim **+1.6 / +2.7 / +3.8 dB** on OFDM52 at zero BER cost; on-air confirmed **+1.18 dB** (FT-991A). Tests: `cessb_benefits_hold_on_ofdm_hom`, `cessb_acpr_spectral_regrowth`.
- **Operator Panel**: Messages presented as a tab alongside the Event Log.

## v0.2.0 — 2026-06-21

- **`openpulse-linksim`** (new crate): two-station ARQ link simulator proving the effective two-way transfer rate under simulated SNR / noise / fading — real forward frames + real FSK4 ACKs over a reverse channel, over-the-air rate adaptation, honest goodput accounting, compression modes; CLI sweep + GUI.
- **Signal-path testbench**: explicit 2×4 spectrum/waterfall grid (fixes unrendered waterfalls), all modes with measured per-mode bitrates, and new sources (virtual loop, dual-card hardware loop, test-matrix runner, adaptive ladder).
- **Bandplan Guardrails**: occupied-bandwidth coverage for active `-RRC` variants and `SCFDMA52-64QAM-P4` (no longer rejected as `UnknownOperatingMode`); `BandplanPolicy::default()` → `HamIaruRegion1`; Region 3 exposes a conservative-proxy warning.
- **Regulatory Logging**: `TxSessionLog::log_frame` rejects cross-station metadata.
- **Session Metrics**: throughput labeled as an upper-bound proxy with a dedicated `throughput_bps_note` field.
- **Waveform Validation**: BL-TP-7 SC-FDMA pilot-density Doppler review test (`plugins/scfdma/tests/pilot_density_review.rs`).
- **Performance**: cached benchmark corpus via `LazyLock` (PR #275); `qpsk-plugin` demod hot-path reduction (single-pass sin/cos + phase-step accumulation); `QPSK1000-HF` LMS pinned to `mu=0.015`.
- **Quality**: clippy `needless_borrow` fix and `HamIaru` → `HamIaruRegion1` in tests (PR #276); benchmark cached-corpus stability assertions.
- **Rate adaptation**: ACK-UP skips unmapped reserved profile rungs (e.g. HPX wideband SL9 → SL11); SNR-gated admission limited to HPX wideband-HD SL13 → SL14.
- **On-air tooling**: `onair-preflight.sh`, `run-onair-tests.sh` (default preflight), `onair-bundle-evidence.sh` — all with `--help`; preflight metadata in reports; strict validation flags; repo-state traceability (`git_dirty` + `git-status.short.txt`) in evidence bundles.

## 0.1.0

- Initial OpenPulseHF workspace with core modem architecture; BPSK plugin and CLI transmit/receive; audio backends (loopback + CPAL).

- Added `FecCodec` to `openpulse-core`: Reed-Solomon GF(2^8) codec (ECC_LEN=32, corrects up to 16 byte errors per 255-byte block).
- Added `ModemError::Fec` variant for FEC-specific error propagation.
- Added `ModemEngine::transmit_with_fec` and `receive_with_fec` for transparent FEC-protected transmission.
- Added FEC loopback hardening tests: 20-scenario fixture matrix (2 modes × 10 payloads) plus BER-injection correctness and capacity-exceeded failure tests.

- Added `qpsk-plugin` crate with Gray-mapped QPSK modulation and demodulation.
- Registered QPSK plugin in CLI engine, exposing modes `QPSK125`, `QPSK250`, and `QPSK500` via `openpulse modes`.
- Added QPSK loopback fixture matrix (3 modes × 14 payload profiles = 42 scenarios).
- Added spectral efficiency benchmarks confirming QPSK250 carries more bits per sample than BPSK250 at equal baud rate.

- Added documentation framework with standardized frontmatter.
- Added docs CI checks and automated last_updated stamping for pull requests.
- Expanded `openpulse-modem` BPSK hardening coverage with a deterministic
  loopback fixture matrix executing 56 scenarios across supported modes and
  payload profiles.
- Strengthened `openpulse-modem` structured HPX event logging so diagnostic
  entries preserve `event_source`, `session_id`, and `reason_string`, and
  transition events are counted consistently in session diagnostics.
- Improved `openpulse session state --diagnostics` output so text mode renders
  a readable summary plus event lines while JSON mode keeps the raw structured
  diagnostics payload and uses persisted peer context when available.

### HPX conformance & session audit (2026-04-25)

- Added 10 HPX spec conformance integration tests in `openpulse-modem` covering
  all major state-machine paths (happy path, timeouts, signature rejection,
  quality recovery, ARQ exhaustion, local/remote teardown, relay activation).
- Fixed missing `RelayActive + TrainingOk → ActiveTransfer` state-machine transition
  in `openpulse-core::hpx` required by the relay conformance scenario.
- Added `hpx_session_id()` and `hpx_transitions()` public accessors to `ModemEngine`.
- Added `POST /api/v1/session-audit-events` endpoint to `pki-tooling` that validates
  and persists HPX transition logs to the `audit_events` table.
- Added `PkiClient::create_session_audit_event` and `record_handshake_session_audit`
  to the CLI, wiring `diagnose handshake` to post audit events on every execution.
- Added `openpulse session` CLI subcommand group with four commands:
  `start`, `state`, `end`, and `log`, exposing the full HPX lifecycle through the CLI.
- Added 5 integration tests for the `session` command group using mockito.
- Added `live_pki_integration.rs` test suite that spins up the real `pki-tooling`
  axum router on a random TCP port and validates CLI commands end-to-end against
  a live Postgres database (skips gracefully when `PKI_TEST_DATABASE_URL` is unset).
