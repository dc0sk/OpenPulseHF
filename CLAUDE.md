---
project: openpulsehf
doc: CLAUDE.md
status: living
last_updated: 2026-05-27
---

# CLAUDE.md — OpenPulseHF Agent Contract

This file is the authoritative guide for any coding agent working in this repository. Read it before touching code. Mandatory agent safety rules are in `AGENTS.md` (root) and `docs/dev/AGENTS.md`.

---

## Build and test commands

```bash
# Toolchain preflight (required: rustc >= 1.94.0)
./scripts/check-toolchain.sh

# Full workspace build (requires libasound2-dev on Linux)
cargo build --workspace

# Full test suite (no audio hardware required)
cargo test --workspace --no-default-features

# Run a specific test file
cargo test --package openpulse-modem --no-default-features --test fec_loopback

# Clippy (treat warnings as errors). --all-targets lints tests/benches too — without it,
# test code is never linted and rots (that is how an unused binding sat in session_key.rs).
cargo clippy --workspace --no-default-features --all-targets -- -D warnings

# Format check
cargo fmt --all -- --check

# Cross-compile check for Raspberry Pi (requires `cross` installed)
cross check --workspace --target aarch64-unknown-linux-gnu --no-default-features

# Run the benchmark and capture JSON output
cargo run -p openpulse-cli --no-default-features -- --backend loopback --log error benchmark run

# CI benchmark regression gate (run locally to verify before PR)
cargo run -p openpulse-cli --no-default-features -- --backend loopback --log error benchmark run >/tmp/bench.json
jq '.passed == .total and .mean_transitions <= 20.0' /tmp/bench.json  # must print true

# Run the quick-tier test matrix (virtual channels, no hardware) — outputs to docs/test-reports/
cargo run -p openpulse-testmatrix --no-default-features

# Run the full test matrix (all propagation channels and payload sizes)
cargo run -p openpulse-testmatrix --no-default-features -- --full --output docs/test-reports

# Fallback core gates when full workspace checks are blocked by local toolchain constraints
cargo clippy --workspace --exclude pki-tooling --no-default-features --all-targets -- -D warnings
cargo test --workspace --exclude pki-tooling --no-default-features
```

The `--no-default-features` flag disables the CPAL audio backend and is required for CI. All tests must pass with this flag. Never add tests that require real audio hardware.

---

## Crate map

### Core layer

| Crate | Path | Role |
|---|---|---|
| `openpulse-core` | `crates/openpulse-core` | Traits, frame format, CRC-16, `FecCodec` (RS+Conv), `HpxSession`/`HpxReactor` state machine, plugin registry, trust/signing, SAR, ACK, rate adaptation, relay, query propagation, peer cache, compression, PQ handshake, Hilbert I/Q |
| `openpulse-audio` | `crates/openpulse-audio` | `LoopbackBackend` (testing) and `CpalBackend` (hardware, feature-gated) |
| `openpulse-modem` | `crates/openpulse-modem` | `ModemEngine`, `PipelineScheduler`, benchmark harness, diagnostics, CSMA/DCD, channel sim harness |
| `openpulse-channel` | `crates/openpulse-channel` | Channel simulation (Watterson, Gilbert-Elliott, QRN/QRM/QSB/Chirp) |
| `openpulse-radio` | `crates/openpulse-radio` | `PttController` trait + `NoOpPtt`, `SerialRtsDtrPtt`, `VoxPtt`, `RigctldPtt`, `RigctldController` (CAT) |
| `openpulse-dsp` | `crates/openpulse-dsp` | DSP primitives: RRC filter, PLL, Gardner timing recovery, LMS/DFE adaptive equalizer |
| `openpulse-config` | `crates/openpulse-config` | Typed TOML schema; `load()`, `init_template()`, CLI-override pattern |
| `openpulse-gpu` | `crates/openpulse-gpu` | wgpu-backed BPSK DSP kernels; CPU fallback when GPU unavailable; gated by `gpu` feature in `bpsk-plugin` |
| `openpulse-keystore` | `crates/openpulse-keystore` | Secret storage (REQ-SEC-CTL-04): `FileKeystore` — named secrets encrypted at rest under an operator master password (Argon2id KDF → ChaCha20-Poly1305 AEAD) |
| `openpulse-linksec` | `crates/openpulse-linksec` | Control-channel link security (REQ-SEC-CTL-01/02): PSK-authenticated encrypted daemon↔client control link via Noise (`Noise_NNpsk0`, X25519); non-loopback auth gate |

### Protocol layer

| Crate | Path | Role |
|---|---|---|
| `openpulse-ardop` | `crates/openpulse-ardop` | ARDOP-compatible TCP TNC interface; `openpulse-tnc` binary; Pat-compatible command set |
| `openpulse-kiss` | `crates/openpulse-kiss` | KISS/AX.25 TNC interface; `openpulse-kisstnc` binary |
| `openpulse-b2f` | `crates/openpulse-b2f` | B2F/Winlink protocol state machine (banner, FC/FS/Ff/Fq frames, gzip/Type D compression; Type C/LZHUF unsupported — inbound proposals are rejected) |
| `openpulse-b2f-driver` | `crates/openpulse-b2f-driver` | High-level ISS/IRS session driver over ARDOP TCP; e2e loopback tests |
| `openpulse-gateway` | `crates/openpulse-gateway` | Direct TCP Winlink CMS gateway; `openpulse-gateway` binary |
| `openpulse-qsy` | `crates/openpulse-qsy` | QSY frequency-agility protocol: wire frame codec, Ed25519 signing, `QsySession` state machine, `QsyScanner` |
| `openpulse-discovery` | `crates/openpulse-discovery` | JS8-based station discovery + rendezvous (FF-15) — RX + beacon TX + rendezvous SHIPPED (Phases A–G): pure no-I/O protocol logic. `hint.rs` (`@OPULSE` OPHF codec), `station.rs` (StationTable), `scheduler.rs` (`Js8Clock`), `discovery_sm.rs`, `runtime.rs` (`DiscoveryRuntime` — beacon scheduler + rendezvous orchestration), `peer_map.rs` (→ shared `PeerCache`), `hint_assembler.rs` (cross-slot `@OPULSE` beacon → peer recognition), `rendezvous.rs` (`RendezvousMsg` Propose/Accept/Reject codec + `RendezvousInitiator` + `respond()`; channels are per-band table indices; no signature — post-QSY CONREQ is the auth), `rendezvous_assembler.rs` (cross-slot RX reassembly of overs directed at us). Only Phase H (on-air) remains |
| `openpulse-mesh` | `crates/openpulse-mesh` | Mesh broadcast daemon; beacon re-broadcast with TTL, `openpulse-mesh` binary |
| `openpulse-repeater` | `crates/openpulse-repeater` | Digipeater / relay node; configurable filter and forwarding policy |
| `openpulse-daemon` | `crates/openpulse-daemon` | Unified background daemon aggregating modem, PTT, and control-protocol services |
| `openpulse-freedv-auth` | `crates/openpulse-freedv-auth` | External shim adding Ed25519 frame signing to FreeDV via the codec2 data channel (FF-11) |
| `openpulse-filexfer` | `crates/openpulse-filexfer` | Direct P2P file-transfer protocol (`OPFX`): pure no-I/O `FxFrame` codec + `SenderSession`/`ReceiverSession` state machines + offer/manifest/policy/sanitize + `blocks.rs` (split/pack/SAR mapping, `BlockAssembler`, fragment bitmaps, block-level resume). FF-16 Phases A–E SHIPPED (crate + modem loopback + daemon SendFile/twin round-trip + panel Files tab + real-radio PTT burst queue/drain + airtime-bounded burst splitting + `.partial` resume + `ListFiles`/CLI surface; PRs #730–#743, #787); on-air (Phase F) deferred |

### UI and tooling layer

| Crate | Path | Role |
|---|---|---|
| `openpulse-cli` | `crates/openpulse-cli` | CLI binary; thin wrapper over modem engine and protocol crates |
| `openpulse-tui` | `crates/openpulse-tui` | ratatui TUI frontend: HPX state, AFC/rate meters, DCD energy bar, transitions log |
| `openpulse-testbench` | `apps/openpulse-testbench` | egui/eframe signal-path testbench: 4-column waterfall/spectrum/scatter, 7 channel models |
| `openpulse-panel` | `apps/openpulse-panel` | Operator panel GUI (**iced**; connects to openpulse-daemon control port). Controls band + spectrum/waterfall/ladder + tabbed info/config/messages/log; Dark/Light/Contrast/System themes. `theme.rs` has an iced-free, unit-tested theme core. The egui version was retired 2026-07 (REQ-UX-04). |
| `openpulse-testmatrix` | `apps/openpulse-testmatrix` | Automated mode × channel test matrix runner |
| `openpulse-twinview` | `apps/openpulse-twinview` | Side-by-side viewer for the twin-daemon validation rig |
| `openpulse-dict-trainer` | `tools/openpulse-dict-trainer` | Trains the shared HPX Zstd compression dictionary |
| `openpulse-linksim` | `apps/openpulse-linksim` | Two-station bidirectional ARQ link simulator (lib + CLI): effective two-way transfer rate under simulated SNR/noise, with FSK4 ACKs, turnaround, retransmission, and over-the-air rate adaptation |
| `pki-tooling` | `pki-tooling` | Key management, trust store, bundle signing, PKI web service |

### Plugins

| Crate | Path | Role |
|---|---|---|
| `bpsk-plugin` | `plugins/bpsk` | BPSK31/63/100/250 modulation plugin; optional GPU path; LMS equalizer on RRC path |
| `qpsk-plugin` | `plugins/qpsk` | QPSK125/250/500/1000 modulation plugin; `-D` differential (DQPSK) modes for HF fading (hard-only, no soft path) |
| `psk8-plugin` | `plugins/psk8` | 8PSK500/1000 modulation plugin |
| `qam64-plugin` | `plugins/64qam` | 64QAM500/1000/2000-RRC modulation plugin; Gray-coded 8×8 PAM-8; soft demodulator |
| `fsk4-plugin` | `plugins/fsk4` | FSK4-ACK modulation plugin (ACK channel) |
| `mfsk16-plugin` | `plugins/mfsk16` | Constant-envelope non-coherent 16-GFSK weak-signal sub-floor waveform (REQ-WSIG-01): mode `MFSK16`, 31.25 baud, 500 Hz, 4 bits/sym, one 255-byte RS block; self-acquiring (Costas-16 sync + timing×freq search, `estimate_afc_hz = None`); soft-capable, frame-median-calibrated LLRs. Measured to beat coherent BPSK31 by ~4 dB on moderate fade / decode where BPSK31 fails on fast fade, at a PAPR credit. Broadcast-first originally; the ACK path shipped since — MFSK16 is SL1 of `hpx_hf` and its K=3 union-decoded return channel is gated by `mfsk16_arq_subfloor` |
| `js8-plugin` | `plugins/js8` | JS8-compatible 8-GFSK weak-signal waveform (FF-15) — full TX+RX SHIPPED. `Js8Plugin` ModulationPlugin (submode/costas/GFSK/LDPC(174,87)/CRC-12/tones); native RX decoder (`decoder.rs` window multi-decode, `demodulate.rs` soft 8-FSK, `sync.rs` Costas, `ldpc174.rs` BP) — B-6 −18 dB go/no-go PASSES. Message layer: `frame.rs`/`grammar.rs` (callsign/grid/compound/directed unpack), `varicode.rs` (Huffman) + `jsc.rs` (full 262k JSC codebook) free-text decode. TX packers `encode.rs` (`pack_compound_frame`/`pack_alphanumeric50`/`pack_heartbeat_frame`/`pack_huff_frame`) + `beacon.rs` (`heartbeat`/`opulse_hint`/`directed` over assembly + `frame_audio`). Tables ported from GPL-3.0 JS8Call, validated vs real boost+Qt5 |
| `ofdm-plugin` | `plugins/ofdm` | OFDM16/52 + OFDM52-{8PSK,16QAM,32QAM,64QAM} multicarrier; Schmidl-Cox preamble, LS channel est + ZF equalization; soft demod |
| `scfdma-plugin` | `plugins/scfdma` | SC-FDMA16/52 + SCFDMA52/26-{8PSK,16QAM,32QAM,64QAM} single-carrier-FDM; DFT-CE pilot channel est + MMSE; per-symbol SFO deramp; soft demod |
| `pilot-plugin` | `plugins/pilot` | Pilot-framed `PILOT-{QPSK,8PSK,16QAM,32APSK}{500,1000}` (+ `-RRC`, + `2000-RRC`); pilot-aided carrier recovery (cycle-slip-immune, SRO-robust); soft demod; 32APSK = DVB-S2 |

---

## Current phase and execution order

**Completed**: Phases 1–9, Phase 7 (7.1–7.5), Phase 8 (8.1–8.3), FF series (FF-1 through FF-13), BL-FEC series (BL-FEC-1 through BL-FEC-6), all code stubs (PR #187–#189). See `docs/dev/project/roadmap.md` for full history.

**Active tracks**:
- **FF-15 JS8 discovery + rendezvous** — RX + **beacon TX (Phase E)** + **rendezvous → HPX handoff (Phase F) COMPLETE** (native TX+RX waveform, full message layer incl. JSC, discovery runtime, `@OPULSE` peer recognition, shared `PeerCache`, CLI + panel surfaces; TX packers/beacon assembly, `transmit_raw_audio` seam, slot scheduler + daemon wiring — off-by-default behind `[discovery] mode = "beacon"`/`"full"` + a callsign + ±2 s clock-skew/DCD/self-ID gates; §97.221 doc in `docs/regulatory.md`; PRs #744–#797). **Phase F** (PRs #798–#805): 2-message Propose/Accept/Reject rendezvous over JS8 directed free text → `RendezvousWith` daemon command → scheduled QSY (`switch_in_slots` delay) → `ConnectPeer` CONREQ handoff; channel-index table in config; two-runtime GFSK-audio end-to-end test. Remaining: **H on-air** only.
- **FF-16 file transfer** — Phases A–E SHIPPED (PRs #730–#743, + `ListFiles`/CLI #787); on-air (Phase F) deferred.

**Deferred (no target date)**:
- On-air regulatory validation (Phase 5.5-reg): on-air tests, station ID audit, compliance report

**Completed work (Groups 1–3, Phases 1–9, FF/BL-FEC series, PRs #187–#321):** the PR-by-PR
changelogs and ✅-Done phase entries that used to sit here are archived in
[`docs/dev/reviews/claude-md-completed-phase-history.md`](docs/dev/reviews/claude-md-completed-phase-history.md).
They are provenance, not guidance — what those phases built is described by the code, the tests, and
the acceptance-criteria table below. Consult the archive when you need to know *which PR* introduced
something or *why* a design went the way it did.

For the roadmap and phase gates see [`docs/dev/project/roadmap.md`](docs/dev/project/roadmap.md).

---

## Acceptance criteria

Each requirement below is done when the linked test passes. Add new links as tests are written.

| Requirement | Acceptance test |
|---|---|
| BPSK loopback correctness | `cargo test -p openpulse-modem --test bpsk_hardening` (the SNR sweep now round-trips through AWGN) + `--test channel_loopback` + `--test psk31_longframe_acquisition` |
| QPSK loopback correctness | `cargo test -p openpulse-modem --test channel_loopback_multimode qpsk500_awgn_20db` (real decode) + `--test qpsk500_acquisition`; `--test qpsk_hardening` covers the TX/state-machine paths only |
| FEC RS encode/decode | `cargo test -p openpulse-modem --test fec_loopback` |
| The scanning FEC receive finds a frame inside a capture LONGER than the frame (the defect that blocked every long coded frame on real audio) | `cargo test -p openpulse-modem --test fec_scan_long_capture` |
| A mode never advertises a soft-demod capability it refuses at call time (`supports_soft_demod(mode)` agrees with `demodulate_soft`) | `cargo test -p qpsk-plugin --test differential_soft_capability` |
| The plain 8PSK pulse refuses a sample rate it cannot decode at (≥5 samples/symbol) instead of emitting undecodable audio — floor pinned from BOTH sides | `cargo test -p psk8-plugin --test plain_pulse_sps_floor` |
| HPX state machine transitions | `cargo test -p openpulse-modem --test hpx_conformance_integration` |
| Benchmark 100% pass, mean_transitions ≤ 20 | `cargo test -p openpulse-modem --test benchmark_integration` |
| Session persistence | `cargo test -p openpulse-cli --test local_state_integration` |
| Block interleaver round-trip | `cargo test -p openpulse-core --no-default-features --lib fec::tests::interleave` |
| Gilbert-Elliott bursts span whole symbols (mean 1/p_bg symbols) | `cargo test -p openpulse-channel --lib bursts_span_whole_symbols_with_mean_one_over_pbg` |
| Watterson fading envelope non-trivial | `cargo test -p openpulse-channel` (`f1_envelope_has_non_trivial_variation` in `watterson.rs`) |
| Watterson continuous fade correlates across `apply()` calls | `cargo test -p openpulse-channel --lib continuous_fade_correlates_across_calls` |
| SC-FDMA channel estimator vs. selective channels | `cargo test -p openpulse-modem --test scfdma_multipath_timing` (asserts; `scfdma_ce_sweep` is a **manual research harness**, not a gate — it is `#[ignore]`d and asserts nothing) |
| SC-FDMA decodes a stronger delayed ray to a 2 ms (16-sample) spread inside the CP | `cargo test -p openpulse-modem --test scfdma_multipath_timing` |
| Symbol-domain SNR tracks true SNR past M2M4's ~15 dB saturation | `cargo test -p openpulse-modem --test symbol_domain_snr` + `--test symbol_snr_ladder_climb` |
| 64QAM soft LLRs are calibrated (worst-bin error ≤ 4× the promised rate) | `cargo test -p qam64-plugin --test llr_reliability` |
| OFDM soft LLRs are calibrated (worst-bin error ≤ 4× the promised rate) | `cargo test -p ofdm-plugin --test llr_reliability` |
| Pilot-plugin soft LLRs are calibrated (worst-bin error ≤ 4× the promised rate) | `cargo test -p pilot-plugin --test llr_reliability` |
| QPSK1000-HF-RRC forward-only LMS holds the good_f1 coded floor | `cargo test -p openpulse-modem --test qpsk_hf_rrc_forward_only` |
| Differential QPSK (`-D`, hpx_hf SL6) survives moderate_f1 where coherent QPSK250 dies (#923) | `cargo test -p openpulse-modem --test qpsk_differential_fading` + `cargo test -p qpsk-plugin differential` |
| The `hpx_hf` rung table in `docs/mode-fec-ladder.md` matches `SessionProfile::hpx_hf` (mode/FEC/floor/ceiling) | `cargo test -p openpulse-core --test ladder_doc_matches_profile` |
| The roadmap SessionProfile table matches every profile, and its "manual-select only" modes are in no profile | `cargo test -p openpulse-core --test roadmap_profile_table` |
| Every `hpx_hf` **single-carrier/OFDM** rung decodes on a Watterson `moderate_f1` fade; no rung is uncoded; the entry rung works AT its floor. (SL1/MFSK16 is excluded — ~17 s/frame — and covered by `mfsk16_engine`/`mfsk16_harq`; the LdpcHighRate rungs share a waveform with their SC pair, which is swept) | `cargo test -p openpulse-modem --test hpx_hf_rungs_survive_fade` + `--test mfsk16_engine` |
| BPSK's SNR estimate still carries channel information on a fade (M2M4 read a flat constant) — #934 | `cargo test -p openpulse-modem --test bpsk_snr_tracks_a_fade` + `cargo test -p openpulse-dsp additive_snr` |
| The rate ladder climbs on decode-evidence, not only on an SNR estimate, and never demotes below a level that just decoded — #934 | `cargo test -p openpulse-core --test success_based_climb` + `cargo test -p openpulse-linksim psk_ladder_climbs_off` |
| Small frames get free RsStrong on the weak rungs (block-count-equal), and the OTA receiver dual-decodes it on a fade | `cargo test -p openpulse-core free_rs_strengthening` + `cargo test -p openpulse-modem --test free_rs_strengthening_ota` |
| The rate ladder's SNR scales are per-waveform-family (single-carrier ≈ true SNR; OFDM saturates) — a physical boundary, not a wart, pinned so it can't be "unified" into the v0.14.0 stall | `cargo test -p openpulse-modem --test snr_scale_boundary` |
| CI goodput regression gate (linksim effective_bps ≥ 65 % of baseline) | `cargo test -p openpulse-linksim goodput_gate` |
| JS8 NORMAL native decode reaches the −18 dB weak-signal gate (FF-15 Phase-B go/no-go) | `cargo test -p js8-plugin --test snr_sweep gate_at_minus_18_db` |
| JS8 discovery MVP: the daemon rx-tick activates, dwells, decodes an injected heartbeat, caches the station + emits `StationHeard` | `cargo test -p openpulse-daemon --no-default-features discovery_tick` |
| File-transfer protocol edges (offer/accept/reject/timeout/cancel/verify/tamper) | `cargo test -p openpulse-filexfer` |
| File-transfer blocks survive the modem (loopback round-trip + tamper→verify-fail) | `cargo test -p openpulse-modem --test filexfer_loopback` |
| File-transfer multi-object >64 KB split/reassemble | `cargo test -p openpulse-filexfer --test blocks multi_object_over_64kb` |
| File transfer crosses two real daemons (twin round-trip) | `cargo test -p openpulse-daemon --test twin_daemon_bridge a_file_crosses` |
| PTT assert/release ≤ 50 ms | `cargo test -p openpulse-radio --no-default-features --test rigctld_integration rigctld_ptt_round_trip_under_50ms` (real socket I/O; the `noop.rs` timer flips a bool and cannot fail) |
| Periodic station ID at interval (REQ-REG-10) | `cargo test -p openpulse-core --lib station_id` + `cargo test -p openpulse-core --lib cw_id` + `cargo test -p openpulse-modem --test station_id_txcount` |
| MFSK16 sub-floor waveform: loopback + acquisition + calibrated LLRs (REQ-WSIG-01) | `cargo test -p mfsk16-plugin` + `cargo test -p openpulse-modem --test mfsk16_engine` |
| MFSK16 sub-floor HARQ: combining adds diversity, and a stale message neither dilutes it nor false-delivers | `cargo test -p openpulse-modem --test mfsk16_harq` |
| OTA decode + HARQ gain reach the daemon's **production** capture entry (`accumulate_capture`), not just `ota_decode_burst` | `cargo test -p openpulse-modem --test ota_production_capture_path` |
| Receiver AGC: decode level-invariant on/off + AGC tracks level (REQ-AGC-01) | `cargo test -p openpulse-modem --test agc_amplitude_sweep` |
| Simultaneous multi-mode receive monitor (REQ-RX-01) | `cargo test -p openpulse-daemon --no-default-features monitor::` |
| Hotplug-safe audio device resolution (REQ-DEV-01) | `cargo test -p openpulse-core --no-default-features audio::tests` |
| Every device the backend LISTS can also be selected by name (cpal's ALSA enumeration truncates when devices are retained — needs a real audio host, so not in the `--no-default-features` gate) | `cargo test -p openpulse-audio --features cpal-backend --test device_enumeration` |
| CM108 / GPIO PTT backends (REQ-PTT-02/03) | `cargo test -p openpulse-radio --no-default-features -- cm108 gpio` |
| Relay authenticates envelope origin — rejects forged/unsigned `src_peer_id` (audit E3) | `cargo test -p openpulse-core --lib relay::` + `cargo test -p openpulse-mesh --test mesh_loopback -- impersonated_origin_rejected_at_relay authenticated_relay_forwarding` |
| Handshake replay-freshness — signed timestamp; stale/future/missing rejected | `cargo test -p openpulse-core --lib handshake::tests` (fresh/stale/future/missing/none/stale-conack) |
| SAR reassembly resists poison — conflicting fragment stream doesn't block the legit one | `cargo test -p openpulse-core --lib sar::tests` (poison/wrong-total/flood) + `cargo test -p openpulse-daemon --lib poison_fragment_does_not_block_conreq_verification` |
| OTA rate ACK is authenticated — ECDH-derived keyed MAC; forged/foreign-key ACK rejected (audit E7) | `cargo test -p openpulse-core --lib -- session_key ack::tests` + `cargo test -p openpulse-modem --test ack_exchange_integration authenticated_ack_round_trips_and_forgery_is_rejected` |
| Authenticated ACK composed with the **sub-floor K=3 union** return channel (E7 × REQ-WSIG-01) | `cargo test -p openpulse-modem --test mfsk16_arq_subfloor authenticated_k3_subfloor_ack_round_trips_and_forgery_is_rejected` |
| FSK4-ACK decodes **correctly** under noise at its operating point (and degrades below its floor) | `cargo test -p fsk4-plugin --test fsk4_integration` |
| Winlink session bounds total decompressed bytes, and a normal batch still passes | `cargo test -p openpulse-b2f --no-default-features -- session_bounds_aggregate session_aggregate_cap_does_not_trip` |
| Winlink Type C (LZHUF) is unsupported — inbound proposals are rejected, Type D accepted | `cargo test -p openpulse-b2f --no-default-features irs_rejects_a_type_c_proposal_and_accepts_type_d` |
| Winlink header fields are capped, and a realistic multi-recipient message still decodes | `cargo test -p openpulse-b2f --no-default-features -- header_decode_caps header_decode_allows_a_realistic` |
| B2F driver survives a hostile peer — line cap, per-operation read deadlines, framing edges | `cargo test -p openpulse-b2f-driver --no-default-features --test cmd_hardening` + `--test timeout_hardening` + `--test data_framing` |
| B2F driver reports a refused or fully-rejected ISS transfer instead of silent success | `cargo test -p openpulse-b2f-driver --no-default-features --test iss_failure_paths` |
| CI gates are defined and correct (Linux core/full/gpu/pi5 + `macos-build`) — **but the `CI` workflow is `disabled_manually` by the maintainer, so they do NOT run on a PR; the gates above are run locally before every merge** | `.github/workflows/ci.yml` `on: pull_request` (definition only; check state with `gh api repos/dc0sk/OpenPulseHF/actions/workflows`) |

For any new Phase 1 feature: write the test first, confirm it fails, implement until it passes. Do not mark a task done if its test does not exist.

---

## Coding conventions

### Rust style
- `thiserror` for error types in library crates; `anyhow` in CLI and test code
- No `unwrap()` or `expect()` in library crate production paths (`openpulse-core`, `openpulse-audio`, `openpulse-modem`, `openpulse-channel`, `openpulse-radio`). `expect()` is acceptable in tests and CLI.
- Derive `Debug`, `Clone`, `PartialEq` on config and result types
- Derive `serde::Serialize, Deserialize` on any type that crosses an API boundary or is emitted as JSON
- Use `tracing::{debug, info, warn, error}` for structured logging; no `println!` in library code
- Integer field sizes: use the smallest type that covers the domain (`u8` for counts ≤ 255, `u16` for sequence numbers, `f32` for audio samples and DSP)
- `Arc<RwLock<T>>` for shared state read by multiple threads; `crossbeam_channel` for inter-thread messaging

### Module organisation
- One concept per file; prefer small focused modules over large files
- Traits defined in `mod.rs` or a dedicated `traits.rs`; implementations in separate files named after the implementation
- Test modules inline for unit tests (`#[cfg(test)] mod tests { ... }`); integration tests in `tests/` directory

### Documentation
- All public types and functions get a one-line doc comment
- No multi-paragraph docstrings
- No comments explaining what the code does; only comments explaining why when the reason is non-obvious

### Commit style
- One logical change per commit
- Prefix: `feat:`, `fix:`, `refactor:`, `test:`, `docs:`, `ci:`
- Imperative mood: "add block interleaver" not "added block interleaver"

### PR hygiene
- Every PR must pass `cargo test --workspace --no-default-features` locally before opening
- Every PR that adds a feature must include at least one test
- Link the roadmap task in the PR description

### Traceability (required for substantive changes)
Carry the full chain in the commit message and PR body, and append an entry to
`docs/dev/project/traceability.md`:

**requirement/change → architecture/design decision (+ rationale) → implementation (files/functions) → tests → test results (actually run).**

- The "tests → results" link must be a real run (pass/fail counts), never "covered" asserted from a callers-grep.
- Keep the acceptance-criteria table above current (requirement ↔ acceptance test).
- Don't build a separate heavyweight matrix that rots — bake the chain into the artifacts that already travel with the change (commits, PRs, the `traceability.md` ledger, the acceptance table).

---

## Known sharp edges

**QPSK dependency scope mismatch (resolved).** `qpsk-plugin` is now in `[dependencies]` for both `openpulse-modem` and `openpulse-cli`, so production wiring can use QPSK paths without dependency-scope surprises.

**Coherent QPSK/8PSK is fade-fragile on HF; differential (`-D`) is the fix, not a better tracker (#923).** Coherent QPSK250+Rs decodes **0% on Watterson `moderate_f1` at every SNR up to 40 dB** — an absolutely phase-encoded waveform cannot hold a carrier reference through a 1 Hz Doppler fade, so a decision-directed cycle-slip at a fade null ruins the whole frame tail. The ablation is decisive: removing the Doppler rescues it (0.82), removing the delay spread does not (0.00) — it is carrier tracking, not ISI or noise. **Two plausible fixes are dead ends, both measured to 0.00:** porting 8PSK's 2-pass acquire-then-track (`dd_track_seeded`) to QPSK, and routing to the pilot waveform (PILOT-QPSK500). The only survivors on that channel are **BPSK250 (differentially decoded) and MFSK16 (non-coherent)** — which is the tell. `QPSK250-D` encodes each dibit as a phase *increment* so the fade rotation cancels symbol-to-symbol and a slip costs one dibit instead of the tail; it recovers the rung to ~0.65 at 20 dB (`hpx_hf` SL6). Constraints baked in: differential **requires FEC** (no-FEC differential is also 0.00 — the per-slip dibit error must be corrected), it has **no soft-LLR path** (`qpsk_demodulate_soft` errors on `-D` rather than emit miscalibrated coherent LLRs), and it costs ~2 dB of AWGN floor (both decode 100% by 4 dB, well under SL6's operating SNR). Before "improving QPSK carrier tracking for HF", note that the 45°-margin coherent modes are the wrong tool for a fading channel — the margin, not the tracker, is the limit (BPSK250's *longer* frame still beats QPSK250 3×).

**An HF ladder calibrated on AWGN is not an HF ladder.** `hpx_hf`'s floors came from AWGN sweeps, and on a routine Watterson `moderate_f1` fade most of the ladder did not work at the floors it advertised. **Uncoded BPSK31 — SL2, the `initial_level` every session starts on — decoded 0.00 of fading frames at 3, 6 AND 9 dB**; the coherent single-carrier mid rungs decoded ~0 % at *any* SNR, so effective throughput (decode × net bps) at 20 dB read 346 (SL6) → 0 → 125 → 0 → 395 → 1816 — a four-rung dead zone the rung-by-rung adapter had to cross to reach the rungs that worked. Three rules fell out, all measured:
1. **There is no useful uncoded rung on a fade.** #923's "differential needs FEC" is not a QPSK quirk — it is the ladder's law, because BPSK is differential too. Coded, those rungs work (BPSK31 @3 dB 0.00 → 0.25, BPSK63 @7 dB → 1.00).
2. **`RsInterleaved` is inert; code strength is the lever.** A ≤223-byte payload is ONE RS block and a single block is position-agnostic, so there is nothing to spread (BPSK250 @5/8 dB: 0.17/0.58 — *identical* to `Rs`). The docs say the opposite (§2 "RsInterleaved — best for HF burst/fading"); measure the rung, don't trust the table.
3. **"`RsStrong` is free" is TRUE ONLY ≤191 B — check the block boundary before generalising.** RS(255,223) and RS(255,191) both emit one 255-byte block, so below 191 B t=32 costs nothing and roughly doubles the fading decode (BPSK31 @3 dB 0.25 → 1.00). At **192–223 B it needs a second block and doubles the airtime** — ordinary traffic — dropping `hpx_hf`'s AWGN goodput 310 → 199 bps, through the CI goodput floor. I measured "free" at 64 B and generalised straight past the boundary that made it true; the linksim goodput gate caught it. `Rs` is the ladder-wide default; `RsStrong` is right only where frames are known to stay under 191 B.

Gates: `hpx_hf_rungs_survive_fade` (fade), `goodput_gate` (the clean-channel counterweight — it is what stops a fade fix from quietly costing 1.5× the AWGN throughput).

**Differential does NOT scale to 8PSK — built, measured, REJECTED.** The obvious follow-on to #923 is "give SL9 (`8PSK500+Rs`, also 0.00 on `moderate_f1`) the same `-D` treatment". It was prototyped and measured: **8PSK500-D reaches only 0.050 @20 dB / 0.125 @40 dB** on `moderate_f1` (vs QPSK250-D's 0.675) — not a usable rung. The implementation was *correct*, not broken: its AWGN control decodes 1.000 by 16 dB. But that same control exposes the cost — coherent 8PSK500 is at 0.975 by **8 dB** while the differential needs **~12–16 dB**, a **~4–6 dB** penalty versus QPSK's ~2 dB. Differential detection roughly doubles the effective noise, and at 8PSK's ±22.5° margin that eats more than the fade immunity returns: strictly worse on AWGN *and* still useless on fading. This is the same ordering the whole issue follows — **robustness tracks phase margin**: MFSK16 (non-coherent) > BPSK (±90°) > QPSK (±45°) > 8PSK (±22.5°). SL9 is not rescuable by differential encoding; the ladder's answer is that the rate adapter steps down to SL6, which now works. Don't re-attempt it without a *different* mechanism (pilot symbols dense enough to track the fade, or a non-coherent waveform).

**Watterson Doppler envelope resolution (resolved).** `WattersonChannel::make_envelope` now auto-sizes the shaping FFT so `σ_bins ≥ 2.0` even for low-Doppler profiles (e.g. Good F1 at 0.1 Hz), capped at 2^16 samples (`MAX_FFT` in `crates/openpulse-channel/src/fading.rs`). The envelope shows meaningful temporal variation across a full call instead of collapsing to the 0.5 floor. Regression test: `f1_envelope_has_non_trivial_variation` in `crates/openpulse-channel/src/watterson.rs`.

**FEC short-payload waste (resolved for loopback / well-framed paths).** `FecCodec::encode` still emits multiples of 255 bytes; for ACK frames and small (≤ 213 B) **data** frames, callers can now select `FecMode::ShortRs` via `transmit_with_fec_mode` / `receive_with_fec_mode`. That path wraps the payload in the standard `Frame` envelope (10 B), then `ShortFecCodec::with_ecc_len(32)` (t = 16) appends 32 ECC bytes — so the wire carries `Frame(payload) + 32` bytes (≈ `payload + 42` total) instead of a 255-byte block. The receiver runs the normal `stage_decode_frame` + `HpxStateUpdate` routing after RS correction, so seq/CRC validation and HPX state updates work identically to other FEC modes. Only plugins whose demodulator emits the exact byte count of the transmitted frame are supported (loopback and well-framed half-duplex paths); OFDM/SC-FDMA padded modes are not. Regression tests: `short_fec_data_frame_engine_loopback`, `short_fec_data_frame_rejects_oversized_payload` in `crates/openpulse-modem/tests/fec_loopback.rs`.

**Audio backend opt-in (`--no-default-features` footgun).** All workspace tests and CI assume `--no-default-features` so the CPAL audio backend is **off**. Building the ARDOP TNC, KISS TNC, or testbench without their `cpal` feature produces a binary that silently falls back to `LoopbackBackend` regardless of any `[audio] backend = "cpal"` line in `config.toml`. To get real audio, build with `cargo build --release -p openpulse-kiss --features cpal` (or the equivalent for `openpulse-ardop` / `openpulse-testbench`). **`openpulse-cli` is the exception**: its audio feature is `cpal-backend` and it is **on by default**, so `cargo build -p openpulse-cli` already includes CPAL — pass `--features cpal-backend` only if you have disabled default features (`--features cpal` does not exist for the CLI and will error). The `--backend cpal` CLI flag will emit a warning at startup when the feature is absent.

**SAR is now implemented** (`crates/openpulse-core/src/sar.rs`). Objects up to 64 005 bytes can be segmented into 255-byte frame payloads and reassembled. PQ handshake (Phase 3.1) is unblocked.

**The rectangular QPSK/8PSK pulse is a crossfade — the one-slot demod loses ⅓ of the next symbol.** The "plain" modulator blends adjacent symbols with a raised cosine, so `demodulate_symbols` recovers `sym_k + ⅓·sym_{k+1}`. That `β²=−9.5 dB` ISI floor is invisible to any BER test (45° QPSK margin) but caps every soft consumer — it stalled `mean(|LLR|)` above ~12 dB and floored recovered-symbol EVM at −9.7 dB regardless of SNR. `cancel_crossfade_isi` removes it by stable backward substitution (PR #695; QPSK500 soft-FEC floor was stuck at 0.00 and now decodes). It is *anti-causal* ISI, so the DFE cannot reach it. **8PSK has the same defect and is now fixed too** (PR pending): its matched demod integrates against the *squared* window `w_tail²`, so `β = Σ w_head·w_tail² / Σ w_tail³` is **n-dependent** (0.182 at 16 sps, 0.167 at 8 sps) — computed from the window, not a constant — and 8PSK500 EVM cleared −13.7→−20.0 dB @40 dB. **The cancellation must be gated to the plain (crossfade) pulse only**: the `cosine_overlap`/`-HF` pulse is a per-symbol `sin²` bump with no inter-symbol overlap, so cancelling there injects ⅓ of the next symbol as *error* — the shipped QPSK #695 ran it unconditionally on the non-RRC path (latent soft-path corruption on `QPSK1000-HF`), fixed by the same `!cosine_overlap` guard.

**Soft combining does not dominate plain retry — take the union.** On a fading channel, summing HARQ attempts wins when every attempt is partially ruined and they carry complementary information, and *loses* when one attempt is simply clean and the sum dilutes it (measured on `moderate_f1`: SCFDMA52 @20 dB, plain retry 0.97, combining alone 0.95). `receive_with_llr_combining` therefore decodes each attempt standalone before falling back to the MAP sum — one extra RS decode over LLRs already in memory, and success becomes a strict superset of both (PR #694; SCFDMA52-16QAM @28 dB: 0.43 / 0.48 → **0.67**). Deep-fade outage is what limits SC-FDMA on HF, and diversity is the only thing that touches it.

**An uncoded-BER win is not a win.** SC-FDMA's IBDFE halved uncoded BER on a static notch and moved coded frame success by *zero*, because iterative feedback trades average residual for **confidently-wrong bits** — and soft FEC is destroyed by exactly those. Its own model noise variance was 90× optimistic (`v̄` comes from max-log LLRs, and the feedback error correlates with the noise it is subtracted from); the calibration-safe choice is to keep the pre-iteration variance and claim only a better symbol *estimate*. Measured, then reverted — see `docs/dev/research/scfdma-improvements.md` → *Rejected — P7*. Always take the **coded** number, and check `plugins/scfdma/tests/llr_reliability.rs`.

**Code rate is the last lever, not the first.** Higher-rate FEC buys throughput by *spending* SNR. Measured on SC-FDMA: `LdpcHighRate` (r≈8/9) costs +4…+8 dB of floor over `SoftConcatenated` (r≈0.437) for 2.03× the rate — a worse trade than climbing one modulation order (8PSK→16QAM: 1.33× for ~2 dB). So a rate swap on a rung that still has a denser constellation above it *loses* throughput at that rung's operating SNR. LDPC earns rungs only at the ladder's top, where 64QAM is already the densest constellation the plugin has (PR #692, SL16–SL19). Before proposing "stronger/faster FEC on rung X", measure the floor delta and compare it against the next modulation order.

**Test what an LLR *means*, not just its sign.** A true LLR `L` predicts `P(bit wrong) = 1/(1+e^{|L|})`. Bin the emitted LLRs by `|L|`, count actual errors, compare. SC-FDMA's `mmse_llr_noise_var` modelled only the additive noise — omitting channel-estimate error and the residual-ISI term `var(α_k)` — and bits with `|L| ≈ 12` were wrong **71× more often than promised**, on a *flat* channel. No frame-success metric in the repo could see it: soft Viterbi, min-sum LDPC and max-log turbo are all scale-invariant, and the missing terms were nearly a per-frame constant. Fixed in PR #690 with **no measured decode gain** — it matters for HARQ combining and for any iterative equalizer that derives feedback reliability from LLR posteriors. `plugins/scfdma/tests/llr_reliability.rs` is the gate.

**An SNR estimator that measures a residual counts the FADE as noise (#934).** On a fading channel `z ≈ h·s + n`; any estimate built from the raw residual `z − ŝ` (or its orthogonal component, or M2M4's moments) folds the *multiplicative* `h` into "noise" and **stops tracking SNR entirely**. Measured on `moderate_f1`: BPSK had no `estimate_snr_db` at all, so the engine's M2M4 fallback read **a flat ≈ −4 dB from 15 dB of true SNR to 35 dB** — the same number across 20 dB of channel. `hpx_hf`'s SL2–SL5 are all BPSK, so the rate controller was deciding on a constant. The fix is always the same shape — **remove a per-window least-squares complex gain first** (`constellation::additive_snr_db_windowed` in the symbol domain, where decisions are aligned by construction and no time alignment is needed; `openpulse_channel::estimate_additive_snr_db` for raw audio). **This is the third occurrence of the identical bug**: PR #484 fixed it in the linksim's tx-vs-rx estimator, the linksim was then migrated onto the plugin estimators which had it too, and BPSK never had an estimator at all. Two traps to know: (1) the estimate is symbol-domain **Es/N0**, so it sits above channel SNR by the mode's processing gain and saturates at its residual-EVM floor — fine for a rate decision (which needs *movement*, not accuracy), but **never compare one mode's reading against another mode's floor**; (2) at low baud the fix cannot work — at 31 baud a 1 Hz fade decorrelates in ~6 symbols, so no window is both short enough to track `h` and long enough to average `n`, and BPSK31's estimate stays flat *in principle*. Gates: `bpsk_snr_tracks_a_fade`, `constellation::additive_snr_*`. **And a good estimator is not enough**: the rate controller's *only* climb path was `snr >= ceiling`, so a flat estimate pinned `hpx_hf` on its entry rung at ~5 bps **while delivering 20/20 frames** on a fade. The fix is two rules — the ladder climbs on `ACK_CLIMB_THRESHOLD` consecutive clean decodes even with a useless SNR reading (evidence, not just the model), and it **never demotes below a level that just decoded** (a decode is proof the rung works; demotion belongs on the *failure* path where SNR explains something). A decode is an observation; the SNR is a model; the observation wins. Gate: `psk_ladder_climbs_off_the_entry_rung_on_a_fade` — through the **controller**, which is what every prior demod-only fade gate could not see.

**The rate ladder's SNR scales are per-waveform-family by physical necessity — do NOT try to unify them.** `rx_snr_db` dispatches to each plugin's `estimate_snr_db`, and they report different quantities: single-carrier PSK (BPSK, post-#934) reads ~true channel SNR; OFDM/SC-FDMA read a *saturation-bounded plugin-domain* SNR that flattens near ~16 dB (ZF noise-enhancement on faded subcarriers) and **physically cannot** report the 20–30 dB the dense rungs run at. So `hpx_hf`'s SL2–SL6 floors are true channel SNR and SL7–SL14 floors are plugin-domain — two scales, one ladder. This looks like a bug; it is not. Forcing OFDM onto a true-SNR scale (or "unifying the estimators") would put the top rungs' floors above anything the estimate can read → the SNR climb never reaches them → the exact v0.14.0 "AWGN-scale floors never clear" stall. The **evidence-based climb** (#934) is what makes two scales safe: it advances on decode success where SNR saturates. Gate: `snr_scale_boundary` — it fails if OFDM starts tracking true SNR without the floors being re-derived in the same change (the two are one decision). Full single-carrier→true-SNR unification of QPSK/8PSK was scoped and DECLINED: high-risk (floor recalibration across 5 profiles) for churn-reduction only, since the evidence climb already self-corrects the mismatch.

**LLRs already carry `1/σ²` — do not weight them by it again.** `openpulse_dsp::constellation::symbol_llrs` divides every distance by `noise_var`, so a calibrated plugin (SC-FDMA, OFDM) emits true log-likelihood ratios whose magnitude is ∝ 1/σ². For repeated observations of the same bits, LLRs **add**: `combine_llrs_map` is the MAP combine and *is* inverse-noise weighting. The engine used to re-weight that sum by a `1 / mean(|LLR|)` proxy — a second 1/σ² — costing 0.75 dB on graded HARQ attempt sets (fixed in PR #686). `combine_llrs_weighted` is only for LLRs with a noise-blind scale (the ±1.0 trait default). Every shipped plugin is now calibrated (PR #687); `crates/openpulse-modem/tests/llr_calibration.rs` fails any plugin whose `mean(|LLR|)` stops growing with SNR. **Choosing the noise estimator is the hard part**: a demodulator's residual is not all thermal noise — pulse-shaping ISI and equalizer misadjustment vary the symbol *amplitude* with no SNR dependence, so a moment (M2/M4) or distance-to-nearest-point estimator stops tracking SNR entirely. Use the component *orthogonal* to the hard decision (`psk_symbol_noise_var`), or for a differential detector the quadrature companion, where the amplitude cancels exactly (`differential_llr_scale`).

**Acquire on the normalised correlation, not the unnormalised score.** `IqMatchedFilter::search`'s argmax favours high *energy*, on the reasoning that "a deep-fade low-energy window cannot win". When the **preamble** is the faded part that is exactly backwards — SC-FDMA lost frames to data-region windows 4896 samples later that merely shared the pilot comb (ρ = 0.994 at the true offset with energy 19.4, versus ρ = 0.657 with energy 83.0). Use `search_normalized` with an energy floor (ρ is meaningless on a silent window). This looked like "fade dynamics" for a release and was slated as a channel-estimate fix; **ablating** `smooth_ce` entirely (temporarily, to test the hypothesis) left the numbers bit-identical (PR #689) — the sync was the mechanism. `smooth_ce` itself stays in the SC-FDMA demod (an EMA channel-estimate smoother, live at 6 call sites in `plugins/scfdma/src/demodulate.rs`); it just wasn't the thing that was broken.

**Sync must lock ahead of the correlation peak, never on it.** A matched filter's argmax sits on whichever multipath ray is instantaneously strongest — the delayed one about half the time. A late FFT-window start pulls the next symbol in; the cyclic prefix only protects an **early** start (there the window begins inside the symbol's own prefix, a circular shift that `deramp_timing` removes). SC-FDMA locked on the argmax and lost half of all Watterson frames for it (PR #688; `good_f1` sum 9.19 → 29.57 of 42, AWGN bit-for-bit unchanged). OFDM already scanned back for the leading tap. Note the asymmetry that hid it: with the *direct* ray stronger the argmax is already right, so a **symmetric** static two-ray test passes either way — the reproduction needs `a_delayed > a_direct`.

**The RX capture LEVEL is a decode gate — too hot and the energy gate can never shut.** `EnergyGate` (`crates/openpulse-modem/src/engine.rs`) picks `threshold = clamp(idle_floor*3, ABS=0.0001, MAX=0.0032)` and only hands audio to the demodulator above it. If the **idle** noise floor itself exceeds `MAX/3 = 0.00107`, the threshold clamps *below* the noise: the gate fires continuously on noise, the receiver settles AFC on that noise, and the stale bogus correction destroys the real frame arriving seconds later — **"invalid magic" at every retry position, with strong RF and the rigs aligned to ~1 Hz**. Measured on air (2026-07-28, IC-9700 ↔ FT-991A, 2 m): at PipeWire source volume 1.00 the IC-9700 read idle `mean_sq` **0.0154** (4.7× over the clamp) and `BPSK250|none|64` **FAILED** with `AFC settling done: correction=81.2Hz` logged **12 s before the frame arrived**; at 0.55 it read idle 0.00042 / signal 0.0024 (threshold ~0.0013 *between* them) and the identical case **PASSED**. The FT-991A needed no change (idle 0.000125 at 1.00) — its USB AF output is simply quieter, so this is a **per-rig measurement, never a volume to copy**. Two traps: (1) it presents as an AFC/frequency bug — the visible symptom is a large bogus AFC correction, so the reflex is to re-trim the rigs, but a Gate-5 FFT showed the carrier was already at **+1.2 Hz** and a re-trim would have been pure damage; (2) PipeWire volume is **cubic**, so capture *power* scales ≈ `v^6` — 1.00→0.55 is a 32× power drop, and small volume edits move the level enormously. Gates: `scripts/onair-rx-level-check.sh` (fails when the threshold would clamp; wired into the on-air runner's preflight as `verify_rx_level`) and the per-side `A_RX_SOURCE_VOLUME`/`B_RX_SOURCE_VOLUME` set+read-back in `scripts/onair-setup-audio-routing.sh`.

**A rig setting you did not verify is a variable you did not control.** Eight modes — `64QAM{500,1000,2000-RRC}`, `SCFDMA52-{16,32,64}QAM`, `SCFDMA52-64QAM-P4`, `PILOT-QPSK500` — were classified as **analog-path limited** by a clean-looking three-rung comparison: they passed in-process, passed on the virtual (snd-aloop) rig, and failed on the dual-card rig, whose stated difference is "a real analog cable". Six of the eight pass on `main` **with no code change** once `scripts/setup-dualcard-loopback.sh` is re-applied. The dual-card rung was measuring a **live capture AGC**: unplugging the USB adapters resets their mixers, and the runner's `_normalise` `continue`s past an unresolved card with every `amixer` call ending in `|| true`, so it cannot report that it failed. Ablated directly, `SCFDMA52-16QAM` and `-32QAM` each **FAIL 2/2 with the AGC on and PASS 2/2 with it off**. A capture AGC moves the level *during* a frame — near-harmless to a phase-only waveform, destructive to one carrying bits in amplitude — which is exactly why the failure set looked like a waveform property and why "the amplitude modes fail, the phase modes pass" read as physics. **Three lessons, all cheap:** (1) "passes on rig A, fails on rig B" isolates a variable only if everything *else* is genuinely equal, and mutable rig state is not automatically equal; (2) the AGC had already been named as the leading candidate and was struck off by **reading** the control (`sget` said off) rather than **ablating** it — but the read happened after runs that each call `_normalise`, so it described the rig *then*, not during the failing measurements; (3) a script that *sets* state must *verify* it — `run-loopback-dualcard.sh` now refuses to sweep while any card's AGC is live (`AGC_PREFLIGHT=0` overrides) and `setup-dualcard-loopback.sh` reads it back instead of printing an unverified claim. Still open after the correction: `SCFDMA52-64QAM-P4` (0/8) and two genuinely marginal rungs — and `-P4` is *not* a separate defect (it is the **better** mode in-process: 8/8 vs 6/8 uncoded AWGN at 25 dB), just the same cliff with ~0.6 dB less margin.

**Delete the mechanism; if the number doesn't move, it was never the mechanism.** Three accepted explanations in this codebase were falsified in a row by removing the impairment they depend on. "Dense QAM can't hold coherence on HF" died against a noiseless in-CP two-ray channel (#685). "Notch smearing" died at 60 dB SNR — it is a noise-enhancement mechanism, and the selective-vs-flat gap was 0.50 at 32 dB and 0.51 at 60 dB (#688). "The CE lags a moving channel" died when `smooth_ce` was **ablated** (temporarily removed to test it) and the flat-fade numbers came back bit-identical (#689) — the CE smoother stays in the shipped SC-FDMA demod; it just wasn't the mechanism. Run the ablation *before* building the fix the explanation implies.

**A modem that fails at *every* SNR has a bug, not a limitation.** The SC-FDMA `dft_ce_estimate` mis-reconstructed every frequency-selective channel (coarse 3.94-sample delay grid + negative taps read as large positive ones). Its signature was a *flat* 2–7% Watterson decode rate from 8 to 32 dB, and the tests recorded that as "correct and by design" for two releases. It was found by **taking the noise away**: a static two-ray FIR inside the cyclic prefix, no Doppler, 90 dB SNR — a receiver that cannot decode that has nowhere to hide. The same trick then falsified the *next* accepted explanation: "notch smearing" predicts the selective-vs-flat gap shrinks with SNR, but it measured 0.50 at 32 dB and 0.51 at 60 dB, which is how the sync bug above surfaced. Uncoded BER, flat-channel CE MSE, and all 58 unit tests were green throughout. Replacement is `channel::DelayCe` (physical delay basis, f64 normal equations, Wiener ridge with an exponential delay-power prior, and a σ² read off the pilot comb rather than a fit residual). Two lessons: (1) a metric reading "fails at all SNR" is a bug signature — write the noiseless test first; (2) when a DSP change regresses, swap **one** component behind a switch and hold the rest — that is what showed the AWGN loss belonged to the missing ridge, not to the delay basis.

**`ChannelSimHarness` hands the receiver a buffer that IS the frame — the easiest case that exists.** A real receiver listens for seconds and the frame sits *somewhere inside*. Every `route*` variant filled the RX loopback with exactly the transmitted samples, so an entire class of frame-*location* bug was structurally invisible to the in-process suite. It hid a live one: the scanning FEC receive slices a **fixed-length** window (`end = (start + max_frame_samples).min(accumulated.len())`), so the demodulated byte count is a function of the window rather than the frame, and `FecCodec::decode`'s exact-multiple-of-255 gate rejected every attempt before RS ran. On the dual-card rig `QPSK250 + rs` **passed at a ~7 s capture window and failed at the default 45 s one** — same mode, same FEC, same level, same payload; the only variable was how much audio was captured around the frame. Fix: `FecCodec::decode_prefix` tries successively longer block prefixes and returns the first that decodes (safe because `decode` validates its own 4-byte length prefix against the decoded size), wired into the `Rs`/`RsStrong` arms of `receive_from_samples_with_fec`; the single-shot and LLR-combining paths stay strict, and `RsInterleaved` is untouched since it deinterleaves first and needs the exact length. Use `route_embedded(lead, trail)` for anything that must prove the receiver can *locate* a frame. **Two traps this burned:** (1) it presents as a waveform/channel problem — eight hypotheses (SRO, level, RS capacity, TX underrun, scan granularity, LMS, frame airtime, physical corruption) were falsified first, all of them about the *signal*, when the productive move was to vary one thing about the **receiver**; (2) `QPSK250-D` failed at the tight window where coherent passed, which read as a second differential-specific defect — it was the same defect, and the tight window was a coin flip that coherent won. There was one bug, not two.

**RX capture has two entry families — wire RX front-end DSP at the seam, not a caller.** Captured audio reaches demod by two distinct routes: the `receive*` family (`stage_capture_input` → `receive_from_samples`) and the **daemon's streaming path** (`accumulate_capture` → `accumulate_routed`, the one `server::run`'s `rx_ticker` actually uses). They both funnel through exactly one shared seam: **`route_audio_stage(PipelineStage::InputCapture)`** (~19 call sites). The receiver notch lives there so every path gets it by construction; `ModemEngine::notch_blocks_processed()` is a tripwire that stays 0 if an enabled feature never runs on a path. The original notch bug put the transform in `stage_capture_input` only, so it covered the `receive`-family tests but never ran in the daemon. Lesson: a receiver/transmitter front-end transform belongs at the single pipeline-stage seam, and must be tested through the **production entry function** (`accumulate_capture` / the `twin` harness), not only `ChannelSimHarness`/`receive()`.

**Cross-cutting RX/TX feature checklist (avoids the gap above).** When adding a feature that must run on *every* receive or transmit: (1) trace **top-down from the binary** — `server::run`'s `rx_ticker`/`tx` path, not just the engine API — to find what the running daemon actually calls; (2) place the transform at the single shared seam (`route_audio_stage(InputCapture)` for RX), never in one of the many caller functions; (3) never claim "covers all paths" from a callers-grep — prove it with a test that **fails without the wiring**; (4) add a runtime tripwire (a processed-block counter) and assert it increments on the production path; (5) add at least one test through the production entry (`accumulate_capture` or the `twin` daemon harness), not only the convenience seam.

---

## DSP acquisition & carrier-recovery playbook

Blind acquisition — recovering timing, frequency, **and** phase simultaneously from a 16-symbol preamble — is the single most-churned and most-misdiagnosed area of the modem (60+ AFC/carrier commits). These are hard-won, load-bearing practices; read them before touching any plugin's demod or the engine acquisition path.

1. **Diagnose an "AFC" failure with the swept-applied-correction experiment FIRST.** When a mode won't acquire through an offset, modulate at `fc+Δ`, then demodulate with a *manually swept* `afc_correction_hz` (and matching `center_frequency`). If it fails even at the exactly-correct Δ, the estimator/AFC is **innocent** — the bug is in timing, onset, or the carrier tracker. This one check relocated the 8PSK gap (PR #417) from "AFC precision" (where earlier sessions spent days on FLL / preamble-redesign / liquid-dsp ports) to a broken drift-fit branch in `carrier_phase_correct`.

2. **AFC is the usual suspect, rarely the culprit.** The acquisition chain is `energy gate → refine_onset → afc_mini_settle → decode → carrier tracker` (`crates/openpulse-modem/src/engine.rs`); a weakness in *any* link reads as "doesn't decode → must be AFC." Historically these were: onset landing (BPSK31 #406, QPSK500 #413), timing metric at 90° carrier phase (`5dded08`/`866b085`), sample-rate offset on the dual-clock rig (#391/#392/#397), and carrier tracking (8PSK #417) — **not** the AFC estimate.

3. **Settle AFC on the refined-onset window, never the coarse energy-gate window** (it may be mostly silence → a confident-but-bogus estimate, e.g. QPSK500's spurious ~257 Hz). And **don't apply sub-noise-floor (<2 Hz) settled corrections** (`AFC_SETTLE_DEADBAND_HZ` in `engine.rs`).

4. **Carrier recovery is acquire-then-track, not one loop.** A gentle (low-BW) loop holds lock but **cannot acquire** even a ~1 Hz residual over a short (~60–200 symbol) frame. Use two passes: pass 1 wide BW to acquire the frequency, pass 2 narrow BW *seeded* with it to track cleanly. 64QAM (`dd_carrier_track_2pass`) and 8PSK (`dd_track_seeded`, #417) both do this. A single high-BW loop fixes the offset but regresses clean/dense modes (8PSK9600) — the split keeps both.

5. **Don't try to extract sub-Hz CFO from the 16-symbol preamble by a magnitude-peak frequency search.** Its frequency resolution is only ~baud/16 (31–62 Hz) and the magnitude metric is sidelobe-ridden; a coarse scan locks to spurious peaks (−100…−256 Hz observed). Use a scan only for *coarse* acquisition; leave the fine residual to the 2-pass tracker. The data-aided mean-phase-increment estimator is the precise stage (ISI-biased ~0.9 Hz, which the tracker now absorbs).

6. **Dense constellations are the regression canaries.** 8PSK (±22.5° margin) and 64QAM surface every timing/phase/AFC weakness that BPSK/QPSK hide. Validate acquisition changes against them, not just BPSK.

7. **Rebuild BOTH ends for any loopback test** — the preamble sequence and frame geometry are shared protocol; a one-sided rebuild fails silently with "invalid magic."

8. **Test FEC-protected modes WITH their FEC.** Dense modes (SCFDMA-HOM, 64QAM) only ever run FEC-protected, so a no-FEC loopback is an unrealistic bar — use the loopback `FEC=` env / CLI `--fec`. Soft FEC (~+6 dB) was the bigger lever that the loopback had never exercised.

External modem/DSP references (gnuradio FLL band-edge, liquid-dsp framesync, daniestevez/qo100-modem) are catalogued in `docs/dev/research/references.md`. Recurring lesson: those references all use **RRC pulse shaping + a dedicated frequency-acquisition stage**; our rectangular single-Costas PSK is the outlier, which is why band-edge techniques don't drop in cleanly.

---

## Key documents by topic

| Topic | Document |
|---|---|
| Channel models (Watterson, Gilbert-Elliott) | `docs/dev/benchmark-harness.md` |
| Testbench design (channel models, DSP, UI) | `docs/dev/design/testbench-design.md` |
| WSJTX weak-signal techniques | `docs/dev/research/wsjtx-analysis.md` |
| JS8Call speed ladder and ARQ commands | `docs/dev/research/js8call-analysis.md` |
| VARA architecture and ACK taxonomy | `docs/dev/research/vara-research.md` |
| PACTOR Memory-ARQ, interleaver, FEC | `docs/dev/research/pactor-research.md` |
| ARDOP research | `docs/dev/research/ardop-research.md` |
| HPX waveform design | `docs/dev/design/hpx-waveform-design.md` |
| HPX state machine | `docs/dev/hpx-session-state-machine.md` |
| Protocol & handshake wire format (frame/SAR/CONREQ/CONACK/ACK/manifest) | `docs/dev/design/protocol-wire-spec.md` |
| Peer query and relay wire format | `docs/dev/peer-query-relay-wire.md` |
| Regulatory compliance | `docs/regulatory.md` |
| Roadmap and phase gates | `docs/dev/project/roadmap.md` |
| What 1.0 requires (draft criteria + explicit non-goals) | `docs/dev/project/release-1.0-criteria.md` |
| Requirements | `docs/dev/requirements.md` |
| Architecture | `docs/dev/design/architecture.md` |
| PKI tooling | `docs/dev/pki/pki-tooling-architecture.md` |
| CLI usage | `docs/cli-guide.md` |
| Full technical book (waveforms, DSP/physics, architecture, use cases) | `docs/openpulse-book.md` |
| Benchmark harness spec | `docs/dev/benchmark-harness.md` |
| External modem/DSP references (FLL, liquid-dsp, qo100-modem) | `docs/dev/research/references.md` |
| JS8 discovery & rendezvous plan (D1–D7 locked; Phases A–G shipped — RX + beacon TX + rendezvous → HPX handoff; only H on-air remains) | `docs/dev/design/js8-discovery-rendezvous-plan.md` |
| Direct P2P file-transfer plan (D1–D5 locked; Phases A–E shipped, on-air deferred) | `docs/dev/design/file-transfer-plan.md` |
| VarAC feature-gap analysis (ideas we're missing; research, not scheduled) | `docs/dev/research/varac-feature-gap-analysis.md` |
| GPU LDPC BP prototype findings | `docs/dev/gpu-ldpc-prototype.md` |
| OTA adaptive rate-stepping hardware validation | `docs/dev/ota-hardware-validation.md` |
| On-air twin-OTA scenario (two daemons + twinview over RF) | `docs/dev/onair-twin-ota.md` |
| Loopback transports (virtual → hardware → on-air) | `docs/dev/virtual-loopback.md` |
| Loopback re-validation at HEAD (why the recorded evidence is stale, and the plan to close it) | `docs/dev/loopback-revalidation-plan.md` |
| Pre-1.x completeness audit (2026-07-18) — what is claimed done but is not proven | `docs/dev/reviews/pre-1x-completeness-audit-2026-07-18.md` |
| Transmit-safety / supply-chain audit (2026-07-19) — PTT-asserted, pre-auth reach, sibling front-ends | `docs/dev/reviews/audit-2026-07-19-transmit-safety-and-supply-chain.md` |
| Agent safety rules | `AGENTS.md`, `docs/dev/AGENTS.md` |
