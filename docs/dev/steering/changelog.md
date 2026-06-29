---
project: openpulsehf
doc: docs/dev/steering/changelog.md
status: living
last_updated: 2026-06-29
---

# Changelog

> Phase/roadmap history lives in [roadmap.md](roadmap.md); this file tracks
> user-visible changes. "Unreleased" = merged to `main`, not yet in a tagged release.

## Unreleased

## v0.3.0 — 2026-06-29

- **Security/Identity**: The daemon now performs the Ed25519 signed handshake over RF on connect — the initiator sends a signed `ConReq`, the responder verifies it and replies with a signed `ConAck`, and the initiator verifies that (both SAR-fragmented, since the frames exceed one modem frame). The verified peer callsign + Maidenhead grid are stored, a `PeerVerified` event is emitted, and the verified grid is written to the ADIF logbook (ahead of the `[logbook.peer_grids]` fallback). New `[station] identity_key_path`; 30 s handshake timeout (PR #584).
- **ARDOP TNC**: Opt-in adaptive ARQ session via `[ardop] enable_adaptive_arq` / `adaptive_profile`. With it on, the host `ARQBW` hint now caps the adaptive rate ladder by occupied bandwidth and `ARQTIMEOUT` drops an idle connection (both were accepted-and-echoed no-ops before). New rate-policy bandwidth-cap API (`set_arq_max_tx_level`), distinct from the OTA bounds (PR #585).
- **Radio/CAT**: The generic serial CAT backend is now selectable from the daemon for rigs Hamlib/rigctld doesn't support — `[radio] cat_backend = "generic"` with `serial_port` + `rig_file`, built with `--features generic-serial` (Unix). `RigctldController` gained its `CatController` impl (PR #586).
- **Logbook**: Automatic opt-in ADIF logbook — one record per contact (connect→disconnect); worked-station `GRIDSQUARE` from the verified handshake or a `[logbook.peer_grids]` config map; runtime `SetLogbook` toggle (CLI + panel).
- **Receiver auto-notch**: Productionized into the engine (multicarrier-aware, persistence, user controls); automatic QSY on a confirmed in-band interferer a notch can't remove; three seam-gap fixes from the notch-class audit and a single RX front-end seam.
- **Operator Panel**: AGC on/off toggle (PR #583); controls moved to a resizable right side-panel with a full-width waterfall and status below it (PR #579); `SetFreq` panel control; control-surface parity closed on both CLI and panel sides; `daemon set-tx-attenuation` (PR #587).
- **linksim**: I/Q constellation views with symbol-spaced (crisp-dot) sampling (PRs #574/#575), regrouped Station B views with waterfall/constellation toggles (PR #578), QR-branded info band, CE-SSB toggle, SNR plot, LDPC/Turbo/RS-Strong/Concatenated FEC modes, and a `--serve` mode so the panel attaches with no radio (PRs #580/#581).
- **Fix**: CE-SSB is gated off for dense OFDM higher-order modes (8PSK and above), where it caused a ~6 dB decode regression.
- **Docs**: Sorted `docs/dev/` into topic subfolders (`design/`, `pki/`, `research/`, `steering/`) with all references updated (PR #582); manual + changelog + release notes brought current (PRs #588, #589).

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
