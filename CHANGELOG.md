# Changelog

All notable changes to OpenPulseHF are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

---

## [Unreleased]

### Added
- Observability / audit mode (REQ-OBS-01/02/03) — the full opt-in capability: `[observability]
  audit_mode` makes `openpulse-daemon` record its control-event stream (engine events, metrics,
  PTT/RF/QSY/OTA state) as NDJSON to `<archive_dir>/events.ndjson` (tapping the same broadcast
  clients subscribe to — no live client needed) and write `<archive_dir>/snapshot.json` at startup
  (version, git SHA, OS/arch, and the running config with secret values redacted); and a new
  `openpulse audit-bundle` command packages the archive + rolled logs into a single `.tar.gz` (with
  a `metadata.json` manifest) for handoff to a developer. Off by default.
- Persistent rotating file logging (REQ-OBS-02, first slice of the observability/audit-mode
  plan): opt-in `[logging] file` config path; a shared `openpulse_config::logging::init_tracing()`
  helper tees `tracing` to a daily-rolled, non-blocking file appender in addition to stdout (level
  precedence unchanged: `RUST_LOG` > config > default). Wired into `openpulse-daemon`; `~` in the
  path is expanded. Off by default.

### Security
- Operator panel control-channel encryption (REQ-SEC-CTL-01/02): the panel's TCP transport now
  performs the PSK Noise initiator handshake and routes commands/events/spectrum through the encrypted
  channel when `OPENPULSE_CONTROL_PSK` is set — with a resumable partial-frame reader so it fits the
  panel's non-blocking poll. Combined with the daemon-side wiring, the operator↔daemon control link is
  now authenticated + encrypted end-to-end over TCP (WebSocket + keystore-backed PSK loading follow).
- Daemon control-channel PSK authentication + encryption over TCP (REQ-SEC-CTL-01/02): the daemon's
  TCP control server now performs a Noise responder handshake per connection when auth is required
  (`[control_security] require_auth`, or any non-loopback bind), routing all commands/events/spectrum
  frames through the encrypted channel; a wrong or absent handshake **drops the connection (fail
  closed)**, and the daemon **refuses to start** if auth is required but no PSK is set. The 32-byte PSK
  comes from `OPENPULSE_CONTROL_PSK` (64 hex). The default loopback path stays plaintext (unchanged).
  Integration-tested against a real client. *Behavior change:* a non-loopback bind without a PSK now
  fails to start instead of running plaintext. (Panel client, WebSocket, and keystore-backed PSK
  loading follow.)
- Control-channel Noise socket channels (REQ-SEC-CTL-01/02): `openpulse-linksec` gains
  `sync_channel::SyncNoise` (blocking, for the panel/CLI) and `async_channel::AsyncNoise` (tokio,
  `tokio` feature, with `into_split` for concurrent read/write, for the daemon) — a `u32`-length-framed
  PSK-authenticated encrypted message channel, tested over real TCP sockets (handshake, round-trip,
  wrong-PSK-fails-closed). The daemon/panel connection-loop wiring is the remaining, live-validated step.
- Control-channel PSK link-security core (REQ-SEC-CTL-01/02, fourth slice of the control-channel
  security plan): new `openpulse-linksec` crate — a pure-Rust **Noise `NNpsk0`** channel
  (`NoiseHandshake`/`NoiseTransport`; X25519 + ChaCha20-Poly1305 + BLAKE2s via `snow`) giving PSK
  mutual authentication + AEAD encryption with forward secrecy, plus the non-loopback `auth_required`
  gate and a `[control_security]` config section. **Pivoted from TLS-PSK** — rustls has no external
  PSK and OpenSSL would add a C dependency. The transport wiring into the daemon/panel is a separate,
  live-validated step (the channel is still plaintext until then, safe on the default loopback bind).
- OS keychain secret-store backend (REQ-SEC-CTL-03, third slice of the control-channel security
  plan): a `SecretStore` trait in `openpulse-keystore` with a `KeychainStore` (OS Secret Service /
  Keychain / Credential Manager via `keyring`, default-on `keychain` feature) and a `FileStore`
  fallback wrapping the master-password keystore. `KeychainStore::available()` lets a caller fall
  back on headless hosts.
- Master-password file keystore (REQ-SEC-CTL-04, second slice of the control-channel security
  plan): new `openpulse-keystore` crate — `FileKeystore` encrypts named secrets at rest under an
  operator master password (Argon2id KDF → ChaCha20-Poly1305 AEAD), with a fresh salt+nonce per
  save, the master held only in memory, and an owner-only file. For hosts without a system secret
  store.
- Shared owner-only permission checks on secret files (REQ-SEC-CTL-05, first slice of the
  control-channel security plan): `openpulse_config::secret_file` validates that key/secret files
  are owner-only (`0600`) on load and enforces it on write. Wired into the identity-key read path
  (so the daemon and CLI now **refuse a group/world-readable `identity.key`**) and the CLI trust
  store now delegates to the shared helper.

### Fixed
- SC-FDMA hard-demodulation QAM amplitude bias: the hard demod now divides equalized symbols by the
  MMSE attenuation factor before demapping (mirroring the soft path), so 16/32/64QAM outer-ring hard
  decisions are no longer pushed toward the origin near threshold. RX-only; PSK unaffected.
- `cli_mode_advisor` integration test asserted stale speed-level expectations (12 dB → SL6,
  15 dB → SL7) that predated the FEC-protected `hpx_hf` upper-ladder recalibration; corrected to
  the current floors (12 → SL7/8PSK500, 14 → SL8/SCFDMA52-8PSK, 16 → SL9) and extended. The SL7
  11→16 dB gap-filler was reassessed with fresh AWGN + fading sweeps: **8PSK500+RS kept** — a swap
  to the pilot-aided `PILOT-8PSK500` was measured and rejected (it wins on AWGN but loses good_f1
  fading). Added a `calibrate_pilot_gap_candidate` calibration sweep to keep this re-derivable.

### Changed
- `hpx_hf` adaptive-ladder upshift ceilings normalized to a uniform `ceiling(L) = floor(L+1) + 2 dB`
  hysteresis (the old table mixed +1 and +4 dB, over-dwelling the lowest-throughput rungs), so the
  ladder climbs off the slow rungs sooner. Data-only; the SNR→mode mapping (floor-based) is unchanged.
- Operator panel (`openpulse-panel`) re-implemented on **iced** and made the default,
  retiring the egui/eframe version (REQ-UX-04). New layout: a controls band, live
  spectrum + waterfall + rate ladder, and a tabbed lower panel (Additional info /
  Daemon config / Messages / Event log). Adds selectable **Dark / Light / Contrast /
  System** themes (theme core is iced-free and unit-tested) and hover tooltips on every
  control; reuses the previous panel's transport/connection/state core. The binary name
  is unchanged (`openpulse-panel`); the former egui web (`Trunk`) build is gone.

### Added
- Controlled-Envelope SSB (CE-SSB) TX envelope conditioning (`openpulse_dsp::cessb`):
  look-ahead peak-stretcher that raises average TX power at fixed PEP on high-PAPR
  multicarrier modes. Per-mode, default-on (`ModemEngine::cessb_benefits` → `OFDM*`/
  `SCFDMA*`); `[modem] cessb_enabled` config, `SetCessb` control command, `openpulse
  daemon set-cessb` CLI, and a panel "CE-SSB" toggle. Channel-sim +1.6/+2.7/+3.8 dB on
  OFDM52 at zero BER cost; confirmed **+1.18 dB on-air** (FT-991A). Software ACPR and an
  on-air SDR spectral-mask check (SDRplay RSP2pro) show no added splatter on QPSK OFDM;
  on dense OFDM-HOM the larger average-power boost only splatters if the PA's ALC is
  over-driven, so set audio drive for moderate ALC (#521–#533).
- `cessb_benefits_hold_on_ofdm_hom` and `cessb_acpr_spectral_regrowth` measurement tests
  for the per-mode gate and the conditioner's (negligible) spectral regrowth.
- Operator panel: Messages presented as a tab alongside the Event Log in the bottom pane.

### Added (earlier, since 0.2.0)
- Turbo codec: rate-1/3 PCCC with RSC K=3 component codes (G1={1,1,1} G2={1,0,1}),
  3GPP QPP interleaver (K=40–6144), Max-Log-MAP BCJR, 8 iterations, CRC-16 early exit;
  `FecMode::Turbo` wired into `transmit_with_fec_mode` / `receive_with_fec_mode` (#337).
- On-device calibration wizard: `openpulse calibrate audio|ptt|afc` subcommands; all
  three run against the loopback backend with no hardware required; `--output <path>`
  writes a machine-readable JSON summary (#336).
- SC-FDMA adaptive pilot density via coherence-BW estimation: `AdaptivePilotState`
  (EMA α=0.3), `ScFdmaParams::with_pilot_density()`, `estimate_coh_bw_hz()` (#335).
- `QsyIncoming` `ControlEvent` variant: emitted when the daemon receives a `QSY_REQ`
  frame over RF, surfacing the incoming token and candidate count to panel clients.
- SC-FDMA GPU soft demodulator (`scfdma_demodulate_soft_gpu`): batches all per-symbol
  256-pt FFTs in a single wgpu dispatch; channel estimation, MMSE equalization, IDFT,
  and LLR computation remain on CPU. `ScFdmaPlugin::demodulate_soft` dispatches to it
  when built with the `gpu` feature.
- Token length bound (max 64 bytes) in the QSY frame decoder, rejecting oversized tokens
  before they reach session state.
- E2E `qsy_initiator_req_drives_responder_session_and_emits_event` test: encodes a real
  `QSY_REQ` from an initiator `QsySession`, feeds it to `process_received_bytes`, and
  asserts both session creation and the `QsyIncoming` broadcast event.
- GPU RRC FIR convolution kernel (`rrc_fir.wgsl`/`rrc_fir.rs`) and 256-point complex
  FFT/IFFT kernel (`fft256.wgsl`/`fft256.rs`) in `openpulse-gpu`; wired into BPSK,
  64QAM, 8PSK, and SC-FDMA plugins via `#[cfg(feature = "gpu")]` dispatch (#325).

### Fixed
- Turbo codec: `WrappedTurboCodecTest` log-domain underflow at low SNR; `alpha`/`beta`
  arrays use `f32::NEG_INFINITY` sentinels; BER test passes at Eb/N0 = 2 dB (#339).
- Calibration wizard: AFC calibration exit criteria widened from ±0.5 Hz to ±2.0 Hz
  to match BPSK31 AFC convergence time; test no longer flaky (#338).

### Changed
- SC-FDMA `ScFdmaPlugin` gains `gpu` feature flag (optional `openpulse-gpu` dep) and
  `with_gpu()` constructor for both hard and soft demodulation paths.

---

## [0.2.0] — 2026-05-20

### Added
- GPU soft-demodulation kernels for 64QAM and 8PSK via wgpu (`gpu_soft_demod`);
  `Qam64Plugin::with_gpu` and `Psk8Plugin::with_gpu` constructors (#324).
- QSY responder path: `process_received_bytes` wires incoming RF frames into a
  `QsySession`; `execute_qsy_actions` refactored into a shared helper used by both
  initiator and responder roles (#322).
- SNR fill in `accept_qsy`: `engine.last_rx_snr_db()` now supplies observed SNR to
  QSY channel-selection scoring instead of a uniform 0.0 dB (#322).
- QSY RF wiring: `AcceptQsy` command triggers `QsySession::new_initiator()` and
  transmits `QSY_REQ`/`QSY_LIST` frames via the modem engine (#321).
- `CrossBandRepeater` thread wired into `EnableRepeater`/`DisableRepeater` daemon
  commands (#321).
- SC-FDMA DFT-CE pilot-aided channel estimation; SCFDMA52-16QAM, SCFDMA52-32QAM,
  SCFDMA52-64QAM, SCFDMA52-64QAM-P4 modes; MMSE equalization (#316).
- ARQ retry loop with soft LLR accumulation across retransmissions; runtime mode
  switching between registered plugins (#318).
- SC-FDMA HOM modes (SCFDMA52-8PSK, SCFDMA52-32QAM); QPSK2000-RRC Quick tier;
  daemon PTT controller wired from config (#319).
- `hpx_wideband_hd()` profile updated to SL12–SL15 (SCFDMA52-16QAM → 64QAM2000-RRC);
  ACK-UP gate at SL14 protecting SL15 admission (#320).
- B2F Type C ISS sending: `queue_message_type_c()` with `compress_lzhuf_winlink` (#320).
- `hpx_narrowband_hd()` profile (SL8=QPSK9600-RRC, SL9=8PSK9600-RRC); QPSK2000-RRC
  and 8PSK2000-RRC narrow-HD modes (#319).
- SCFDMA52-64QAM promoted to `hpx_wideband_hd` SL14 (#320).
- QSY enable toggle and bandplan selector in panel and TUI (#295).

### Changed
- Daemon PTT controller skips dispatch on PTT hardware assertion failure (#319).
- `BandplanPolicy::default()` uses `HamIaruRegion1` instead of deprecated `HamIaru`.
- Bandplan guardrails recognise `-RRC` waveform variants and `SCFDMA52-64QAM-P4`.

---

## [0.1.0] — 2026-05-01

### Added
- Plugin-based modem architecture in a Cargo workspace.
- BPSK31/63/100/250, QPSK125/250/500/1000, 8PSK500/1000, 64QAM500/1000/2000-RRC
  modulation plugins.
- FSK4-ACK control channel plugin.
- `openpulse-gpu`: wgpu-backed BPSK DSP kernels (modulation, IQ demodulation, timing
  search); CPU fallback when GPU unavailable.
- `openpulse-core`: RS(255,223) and RS(255,191) FEC, block interleaver, convolutional
  codec, concatenated FEC, short-block FEC, Memory-ARQ soft combining, LDPC rate-1/2
  min-sum BP.
- HPX state machine (`HpxSession`/`HpxReactor`), `RateAdapter`, adaptive session
  profiles `hpx500()`, `hpx2300()`, `hpx_narrowband()`, `hpx_wideband_hd()`.
- SAR (segmentation and reassembly), ACK taxonomy, rate adaptation, relay forwarding,
  peer cache, compression (LZ4), post-quantum handshake (ML-DSA-44 + ML-KEM-768).
- `openpulse-channel`: Watterson fading, Gilbert-Elliott burst channel, AWGN, QRN/QRM.
- `openpulse-radio`: PTT backends (NoOp, RTS/DTR, VOX, rigctld), `RigctldController`.
- `openpulse-ardop`: ARDOP-compatible TCP TNC interface, `openpulse-tnc` binary.
- `openpulse-kiss`: KISS/AX.25 TNC interface, `openpulse-kisstnc` binary.
- `openpulse-b2f`: B2F/Winlink protocol state machine; LZHUF and gzip codecs.
- `openpulse-b2f-driver`: ISS/IRS session driver; e2e loopback tests.
- `openpulse-gateway`: Direct TCP Winlink CMS gateway, `openpulse-gateway` binary.
- `openpulse-daemon`: Unified background daemon with control port, event broadcast,
  modem engine, PTT, QSY, repeater, and B2F message store.
- `openpulse-qsy`: QSY frequency-agility protocol; wire frame codec, Ed25519 signing,
  `QsySession` state machine, `QsyScanner`.
- `openpulse-mesh`: Mesh broadcast daemon with beacon re-broadcast.
- `openpulse-repeater`: Digipeater/relay node.
- `openpulse-config`: Typed TOML configuration management.
- `openpulse-tui`: ratatui TUI frontend (HPX state, AFC/rate meters, DCD energy bar).
- `openpulse-panel`: egui/eframe operator panel GUI connecting to the daemon control port.
- `openpulse-testbench`: egui signal-path testbench with 7 channel models.
- `openpulse-testmatrix`: Automated mode × channel test matrix runner.
- `openpulse-cli`: CLI binary with transmit, receive, monitor, benchmark, config, and
  manifest subcommands.
- PKI tooling: key management, trust store, bundle signing, PKI web service.
- CSMA/DCD channel access; AFC loop; LMS/DFE adaptive equalizer on RRC paths.
- loopback-based CI test suite; cross-compile check for aarch64 (Raspberry Pi).
