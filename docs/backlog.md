---
project: openpulsehf
doc: docs/backlog.md
status: living
last_updated: 2026-05-10
---

# Backlog

All scheduled phases (1–9), far-future items (FF-1 through FF-13), and FEC backlog items
(BL-FEC-1 through BL-FEC-6) are shipped and merged.  See `docs/roadmap.md` for the full
history with PR numbers.

---

## Open work items

### Release packaging 🔄

The only remaining scheduled item.

- **GitHub Actions release workflow**: on `v*` tag push, build a static x86-64 musl binary
  and an aarch64 `.deb` package and publish both as GitHub release assets.
  The `openpulse-tnc` / `openpulse-kisstnc` binaries are the primary packaging targets.

### Code stubs (tracked; not blocking any current work)

| Location | Description |
|---|---|
| `plugins/psk8/src/lib.rs` | `demodulate_soft()` falls back to hard ±1.0 pseudo-LLRs — Gray-coded max-log-MAP soft demapping (~1 dB SNR gain) not yet implemented |
| `crates/openpulse-cli/src/commands/session.rs` | `manifest verify` CLI path returns a stub response — `verify_manifest()` in `openpulse-core` is fully implemented; only the CLI wiring is missing |
| `crates/openpulse-core/src/ldpc.rs` | `LdpcCodec` is an intentional passthrough stub — GPU-backed LDPC decode is reserved for a future wgpu kernel; the `IterativeDecoder` trait and `FecMode::Ldpc` variant are in place |

### Deferred (no target date)

| Item | Reason |
|---|---|
| On-air regulatory validation (Phase 5.5-reg) | Requires licensed station and coordinated test schedule |
| Adaptive equalizer LMS/DFE | Follow-on to FF-3 RRC; needed for 1000 baud on Watterson Moderate/Poor |
| 64QAM / SL12–SL20 speed levels | Deferred pending equalizer and OFDM research |
| External Winlink Type C LZHUF compatibility | 4-byte length prefix differs from Winlink convention; deferred |

---

## Completed sprint history

### Phases 1–9 (core modem)

| Phase | Key deliverables | PR |
|---|---|---|
| Phase 0 | Benchmark harness, signed envelopes, session persistence, trust-store CLI, CI | #49, #50 |
| Phase 1 | SAR, block interleaver, channel models, radio interface, AFC, PTT CLI | #67–#71 |
| Phase 2 | ACK taxonomy, rate adapter, HPX profiles (HPX500/HPX2300), signed handshake, DCD/CSMA, peer cache, relay, compression | various |
| Phase 3 | Post-quantum handshake (ML-DSA-44 + ML-KEM-768), Convolutional FEC eval, GPU acceleration, ARDOP TNC, channel sim harness | #88–#91 |
| Phase 4 | Structured JSON event stream, ratatui TUI, KISS/AX.25 TNC, B2F/Winlink, egui testbench | #92–#98 |
| Phase 5 | B2F session driver, LZHUF codec, TOML config, e2e loopback test, CMS gateway, CpalBackend, testbench live capture | #98–#108 |
| Phase 6 | AFC correction loop, pat/Winlink interop, network mesh daemon + peer cache | #116–#121 |
| Phase 7 | Full CAT control, dual-rig repeater, daemon control protocol, operator panel GUI, signed remote rig control, full-duplex mode | #various |
| Phase 8 | Waveform compliance: `hpx_wideband` rename, `hpx_hf` profile, cosine pulse shaping | various |
| Phase 9 | IQ scatter plot, asymmetric per-direction rate adaptation, SNR trend plot, SNR secondary rate input, broadcast/beacon mode | #138–#139 |

### FF series (far-future features)

| Item | Feature | PR |
|---|---|---|
| FF-1 | QSY frequency agility with rigctld | #140, #141 |
| FF-2 | I/Q complex baseband output for SDR upconversion | #150 |
| FF-3 | RRC matched filtering + Gardner TED + Costas PLL | #158 |
| FF-4 | OFDM multi-carrier plugin (OFDM16, OFDM52) with LS+ZF equalization | #167 |
| FF-5 | UHF/VHF narrowband/HD modes (2000 and 9600 baud QPSK/8PSK) | #159 |
| FF-6 | Binary spectrum channel (20 Hz waterfall in operator panel) | #157 |
| FF-7 | Tanh TX limiter for PA back-off | #149 |
| FF-8 | Per-band TX attenuation persistence via rigctld | #148 |
| FF-9 | HPX reactor pattern (event-driven session state machine) | #151 |
| FF-10 | Zstd dictionary compression | #156 |
| FF-11 | FreeDV authenticated voice shim (Ed25519 via codec2 data channel) | #162 |
| FF-12 | SC-FDMA waveform plugin (SCFDMA16, SCFDMA52) | #175 |
| FF-13 | Generic serial CAT (TOML-scripted, for rigs not in hamlib) | #173 |

### BL-FEC series (FEC improvements)

| Item | Feature | PR |
|---|---|---|
| BL-FEC-1 | Concatenated Conv+RS session mode | #169 |
| BL-FEC-2 | Strong RS(255,191) t=32 codec | #171 |
| BL-FEC-3 | Short-block RS for ACK/control frames | #170 |
| BL-FEC-4 | Memory-ARQ soft combining | #171 |
| BL-FEC-5 | K=7 soft-decision Viterbi + `demodulate_soft()` plugin API | #177 |
| BL-FEC-6 | `IterativeDecoder` trait + `LdpcCodec` stub (GPU path reserved) | #176 |
