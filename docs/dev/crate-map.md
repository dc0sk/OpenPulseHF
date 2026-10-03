---
project: openpulsehf
doc: docs/dev/crate-map.md
status: resolved
last_updated: 2026-09-30
---

# Crate map, feature tracks and document index

> Moved verbatim from `CLAUDE.md` on 2026-09-30 (work plan decision 7). `CLAUDE.md` keeps a short crate map; this is the full one.

## Crate map

### Core layer

| Crate | Path | Role |
|---|---|---|
| `openpulse-core` | `crates/openpulse-core` | Traits, frame format, CRC-16, `FecCodec` (RS+Conv), `HpxSession`/`HpxReactor` state machine, plugin registry, trust/signing, SAR, ACK, rate adaptation, relay, query propagation, peer cache, compression, PQ handshake, Hilbert I/Q |
| `openpulse-audio` | `crates/openpulse-audio` | `LoopbackBackend` (testing) and `CpalBackend` (hardware, feature-gated) |
| `openpulse-modem` | `crates/openpulse-modem` | `ModemEngine`, `PipelineScheduler`, benchmark harness, diagnostics, CSMA/DCD, channel sim harness |
| `openpulse-channel` | `crates/openpulse-channel` | Channel simulation (Watterson, Gilbert-Elliott, QRN/QRM/QSB/Chirp) |
| `openpulse-radio` | `crates/openpulse-radio` | `PttController` trait + `NoOpPtt`, `SerialRtsDtrPtt`, `VoxPtt`, `RigctldPtt`, `RigctldController` (CAT) |
| `openpulse-dsp` | `crates/openpulse-dsp` | DSP primitives: RRC filter, PLL, Gardner timing recovery, LMS/DFE adaptive equalizer, `noise_floor` (poison-resistant passband floor driving the adaptive squelch) |
| `openpulse-config` | `crates/openpulse-config` | Typed TOML schema; `load()`, `init_template()`, CLI-override pattern |
| `openpulse-gpu` | `crates/openpulse-gpu` | wgpu-backed BPSK DSP kernels; CPU fallback when GPU unavailable; gated by `gpu` feature in `bpsk-plugin` |
| `openpulse-keystore` | `crates/openpulse-keystore` | Secret storage (REQ-CTL-04): `FileKeystore` — named secrets encrypted at rest under an operator master password (Argon2id KDF → ChaCha20-Poly1305 AEAD) |
| `openpulse-linksec` | `crates/openpulse-linksec` | Control-channel link security (REQ-CTL-01/02): PSK-authenticated encrypted daemon↔client control link via Noise (`Noise_NNpsk0`, X25519); non-loopback auth gate |

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

## Feature-track status (as of 2026-09-30)

**Completed**: Phases 1–9, Phase 7 (7.1–7.5), Phase 8 (8.1–8.3), FF series (FF-1 through FF-13), BL-FEC series (BL-FEC-1 through BL-FEC-6), all code stubs (PR #187–#189). See `docs/dev/project/roadmap.md` for full history.

**Active tracks**:
- **FF-15 JS8 discovery + rendezvous** — RX + **beacon TX (Phase E)** + **rendezvous → HPX handoff (Phase F) COMPLETE** (native TX+RX waveform, full message layer incl. JSC, discovery runtime, `@OPULSE` peer recognition, shared `PeerCache`, CLI + panel surfaces; TX packers/beacon assembly, `transmit_raw_audio` seam, slot scheduler + daemon wiring — off-by-default behind `[discovery] mode = "beacon"`/`"full"` + a callsign + ±2 s clock-skew/DCD/self-ID gates; §97.221 doc in `docs/regulatory.md`; PRs #744–#797). **Phase F** (PRs #798–#805): 2-message Propose/Accept/Reject rendezvous over JS8 directed free text → `RendezvousWith` daemon command → scheduled QSY (`switch_in_slots` delay) → `ConnectPeer` CONREQ handoff; channel-index table in config; two-runtime GFSK-audio end-to-end test. Remaining: **H on-air** only.
- **FF-16 file transfer** — Phases A–E SHIPPED (PRs #730–#743, + `ListFiles`/CLI #787); on-air (Phase F) deferred.

**Deferred (no target date)**:
- On-air regulatory validation (Phase 5.5-reg): on-air tests, station ID audit, compliance report
- **REQ-PHY-05 audio-to-PTT-drop timing** (#1112): "transmitter release within 50 ms of the last
  transmitted sample" needs the rig. No in-process test can bound it — sound-device buffering and
  the rig's own CAT/serial latency are the dominant terms and are absent from any localhost
  harness. The existing `rigctld_ptt_round_trip_under_50ms` covers the *control-path* half only

**Implementation chronology** — the per-PR "Recently/Previously shipped" blocks, the Phase 1–5.7
group listings, and the two resolved design questions (SAR wire format, `PttController` location) —
is archived in `docs/dev/project/claude-md-completed-phase-history.md`. It records finished work
only; nothing in it is live guidance. Narrative history is in `docs/dev/project/roadmap.md`.

## Key documents by topic (full list, moved from CLAUDE.md)

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
