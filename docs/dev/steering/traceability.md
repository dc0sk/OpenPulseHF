# Traceability ledger

Running record of substantive changes as a full chain:
**requirement/change → architecture/design decision → implementation → tests → test results.**

Newest first. See `CLAUDE.md` → *PR hygiene → Traceability* for the standing rule. The per-feature
acceptance gates live in `CLAUDE.md` → *Acceptance criteria*; this ledger adds the design rationale
and the actually-observed results per change.

---

## 2026-06-29 — Signed handshake over RF into the daemon connect (+ verified logbook grid)

- **Requirement/change:** wire the Ed25519 signed `ConReq`/`ConAck` handshake into the daemon's
  `ConnectPeer`/RF path (it was a tested library primitive the daemon never exchanged — `ConnectPeer`
  was a local trust eval), store the verified peer identity, and feed the verified grid to the ADIF
  logbook (closing logbook item B). The keystone that also unblocks host-driven ARQ bounds.
- **Design decision:** *additive* exchange, not a rewrite of connect — `begin_secure_session` still
  runs; `ConnectPeer` additionally signs+sends a `ConReq` and records a `PendingHandshake`. Frames
  are ~530 B > the 255 B modem-frame cap, so they're **SAR-fragmented** (`sar_encode`) on TX and
  reassembled (`SarReassembler`) on RX; the reassembly is a fall-through after relay/QSY dispatch
  (handshake fragments are binary, not QSY ASCII / relay envelopes) and is confirmed by the
  reassembled `HSCQ`/`HSAK` magic, so no wire marker is needed (which wouldn't fit anyway). The
  responder verifies + replies `ConAck` + records the peer; the initiator verifies the `ConAck`
  against its in-flight `ConReq` (session-id gated) + records the peer + clears pending. Grid is a
  `skip_serializing_if`-empty signed field on `ConReq`/`ConAck` so legacy zero-grid frames and their
  signatures stay byte-identical; added `create_with_grid` constructors leaving the 25 existing
  `create` callers untouched. Station key from `[station] identity_key_path` (default
  `~/.config/openpulse/identity.key`, auto-generated; explicit path lets the twin rig hold distinct
  identities). New `ControlEvent::PeerVerified`; 30 s CONACK timeout via `expire_pending_handshake`.
  Verification uses `PolicyProfile::Permissive` (signature proves key possession; first-seen peers
  still connect, mirroring the optimistic `ConnectPeer`).
- **Implementation:** `crates/openpulse-core/src/handshake.rs` (grid field + `create_with_grid`);
  `crates/openpulse-config/src/lib.rs` (`StationConfig.identity_key_path` + template);
  `crates/openpulse-daemon/src/lib.rs` (`PendingHandshake`/`VerifiedPeer`, `RuntimeControlState`
  fields incl. `handshake_sar`, `transmit_handshake_frame`, `try_reassemble_handshake`,
  `handle_inbound_conreq`/`handle_inbound_conack`, `record_verified_peer`,
  `expire_pending_handshake`, `ConnectPeer` CONREQ send, RX dispatch);
  `crates/openpulse-daemon/src/logbook.rs` (`set_pending_peer_grid`);
  `crates/openpulse-daemon/src/server.rs` (load identity seed at startup; expiry tick);
  `crates/openpulse-daemon/src/protocol.rs` + `apps/openpulse-panel/src/connection.rs`
  (`PeerVerified` event + panel log).
- **Tests:** `crates/openpulse-core/src/handshake.rs` inline (grid round-trip, grid is
  signature-covered, empty-grid byte-identical to legacy); `crates/openpulse-daemon/src/lib.rs`
  `handshake_rf_tests` (responder reassembles+verifies+records; initiator verifies+stamps logbook
  grid into the ADIF record; mismatched-session CONACK ignored; ConnectPeer initiates; full-size SAR
  fragment survives BPSK250; pending-handshake timeout).
- **Test results (run):** `cargo test -p openpulse-core -p openpulse-config -p openpulse-daemon
  --no-default-features` → all green (core lib 226, handshake_integration 17, daemon lib incl.
  `handshake_rf_tests` 6, config 16, …; 0 failed). `cargo clippy -p openpulse-core -p
  openpulse-config -p openpulse-daemon -p openpulse-panel --no-default-features --all-targets -D
  warnings` → 0 warnings. `cargo fmt` clean on the touched crates.

## 2026-06-29 — Panel: AGC on/off toggle (control-surface parity)

- **Requirement/change:** close the last open control-surface parity gap — the receiver streaming
  AGC (`ControlCommand::SetAgc`, shipped 2026-06-28 in the daemon + CLI) had no panel button, so the
  GUI operator couldn't toggle it. (Re-audit found the CLI `SendMessage`/`SetMode`/PTT/QSY-accept/
  reject gaps and the panel Squelch gap from the 2026-06-27 audit were already closed; AGC was the
  only one left.)
- **Design decision:** mirror the existing Notch/CE-SSB/Logbook toggle pattern exactly — a single
  `AGC: ON/OFF` button in the right-hand controls column that flips local `agc_enabled` and sends
  `ControlCommand::SetAgc { enabled }`. Default off, matching the daemon's `[modem] agc_enabled`
  default and the engine's opt-in AGC. No new state machinery; one bool field + one button.
- **Implementation:** `apps/openpulse-panel/src/app.rs` — `agc_enabled: bool` field (default false),
  `AGC: ON/OFF` button next to the Notch toggle in `draw_controls`, with hover text noting the
  active-span gating. Stale docs corrected: roadmap §10.6 "panel toggle parity" marked done; the
  control-surface parity table updated to show all flagged gaps closed.
- **Tests:** GUI toggle (no unit test — the `SetAgc` daemon/engine path is covered by
  `crates/openpulse-modem/tests/agc_loopback.rs`).
- **Test results:** `cargo build -p openpulse-panel --no-default-features` green;
  `cargo clippy -p openpulse-panel --no-default-features --all-targets -- -D warnings` 0 warnings;
  `cargo fmt -p openpulse-panel --check` clean. Visual confirmation pending (held before merge per
  the GUI-change rule).

## 2026-06-29 — Docs: sort `docs/dev/` into topic subfolders + fix all references

- **Requirement/change:** the loose files under `docs/dev/` were moved into four topic
  subfolders — `design/` (architecture, design, freq-acquisition-design, hpx-waveform-design,
  testbench-design), `pki/` (the 11 `pki-tooling-*` docs), `research/` (ardop/freedv-auth/js8call/
  ofdm/pactor research, reference-mining-plan, references, vara-research, wsjtx-analysis), and
  `steering/` (backlog, changelog, roadmap, traceability). All inbound and outbound references had
  to follow the move so no link or doc-path breaks.
- **Design decision:** keep the moves as `git mv` renames (history-preserving) and rewrite every
  reference in two classes: (1) full-path mentions `docs/dev/<base>` → `docs/dev/<subdir>/<base>`
  across the whole tree (markdown link text, `doc:` frontmatter self-pointers, and `.rs` doc/line
  comments); (2) relative markdown link *targets* fixed per linking-file location — the dev README
  index (`](base.md)` → `](subdir/base.md)`), the manual's `dev/<base>` targets, and the moved
  files' own outgoing relative links that gained/needed a directory level
  (`../mode-fec-ladder.md` → `../../mode-fec-ladder.md`, siblings via `../`).
- **Implementation:** 29 `git mv` renames; a 29-substitution `sed` script applied to all tracked
  text files for class (1); targeted per-file `sed` for class (2) in `docs/dev/README.md`,
  `docs/openpulse-manual.md`, `docs/{features,mode-fec-ladder}.md`,
  `docs/dev/{hpx-session-state-machine,vara-parity-execution-board}.md`,
  `docs/dev/reviews/review-26050{8,17}.md`, and the moved `design/`/`steering/` files. Five `.rs`
  doc-comment path mentions updated (`openpulse-ardop`, `openpulse-config`, `openpulse-dsp`,
  `openpulse-kiss`, `pilot` plugin) — comment-only, no code change.
- **Tests:** a Python link-integrity walker that resolves every relative `.md` link target against
  the filesystem; a basename grep for any surviving old `docs/dev/<base>` path.
- **Test results:** old-path grep → 0 hits. Link walker → only 3 broken links remain
  (`docs/README.md` → `marketing/{banner,flyer,presentation}.md`), which are pre-existing (those
  targets were never committed) and unrelated to this move. `cargo fmt --all --check` shows 5
  pre-existing deviations, all in files outside this change set (`gui.rs`, `channel/lib.rs`,
  `agc_loopback.rs`, `verification.rs`); no new fmt regression (rustfmt does not reflow comments).

## 2026-06-28 — Panel: controls to a right side-panel; status below the waterfall

- **Requirement/change:** move the right-column (session status) elements below the waterfall; make
  the waterfall as wide as the spectrum; move all controls except connection / PTT / callsign+Connect-RF
  into the right column.
- **Design decision:** keep only connection (transport/server/Connect), PTT, RF-connect
  (callsign/Connect RF), and the connection indicator in the top toolbar. Everything else (Mode,
  Freq/Tune, Repeater, CE-SSB, Notch, Logbook, OTA, TX Atten, Squelch, Config, Messages, QSY) moves
  to a resizable right `SidePanel` rendered by a new `PanelApp::draw_controls`. The `CentralPanel`
  drops its 2-column split and stacks the spectrum pane (now full width) then the session status
  below it, inside a vertical `ScrollArea`. Waterfall widened to the full pane width (was capped at
  512 px) in `draw_spectrum_pane`.
- **Implementation:** `apps/openpulse-panel/src/app.rs` — toolbar slimmed; `draw_controls` method;
  right `SidePanel` + stacked `CentralPanel`. `apps/openpulse-panel/src/ui.rs` — waterfall size to
  `available_width × 96`.
- **Tests:** GUI layout (no unit test).
- **Test results:** `cargo fmt -p openpulse-panel --check` clean; `cargo build -p openpulse-panel`
  green; `cargo clippy -p openpulse-panel --all-targets -- -D warnings` 0 warnings. Visual
  confirmation pending (held before merge per the GUI-change rule).

## 2026-06-28 — Linksim: regroup Station B views + waterfall/constellation toggles

- **Requirement/change:** swap the Station B RX spectrum/waterfall with the ACK/NACK
  spectrum/waterfall; keep Station B's constellation at the far right; add controls to
  enable/disable the waterfalls and the constellation diagrams.
- **Design decision:** swap the middle and far-right signal columns so the column order is
  `[A TX | ACK (B→A) | B RX]` — grouping all of Station B's RX views (spectrum, waterfall, and the
  far-right I/Q constellation in the branding band) on the right, with the ACK in the middle. Two
  toolbar checkboxes (`ui_show_waterfall`, `ui_show_constellation`, both default on): `draw_panel`
  gains a `show_waterfall` flag (early-returns after the spectrum when off); the branding band
  conditionally renders the two `constellation_plot`s and recomputes the flanking text width
  (`3×qr_side` → `1×qr_side`) so the QR stays centered when constellations are hidden.
- **Implementation:** `apps/openpulse-linksim/src/gui.rs` — toolbar checkboxes; column swap
  (panels[2] ACK middle, panels[1] B RX far right); `draw_panel(.., show_waterfall)`; gated
  branding band.
- **Tests:** GUI layout/visualization (no unit test); existing gui unit tests unaffected.
- **Test results:** `cargo build/clippy/test -p openpulse-linksim --features gui` green (3/3 gui
  unit tests). Visual confirmation pending (held before merge per the GUI-change rule).

## 2026-06-28 — Linksim: symbol-spaced (crisp-dot) constellations

- **Requirement/change:** sharpen the I/Q constellations (#574) from a full-rate cloud to discrete
  per-symbol dots so the clean-TX vs noisy-RX contrast reads as a real constellation.
- **Design decision:** parse samples/symbol from the mode's trailing baud (`samples_per_symbol`,
  order/suffix-stripped; `None` for OFDM/SCFDMA/PILOT/FSK which have no PSK symbol grid), then
  sample the Hilbert baseband once per symbol at the **best timing phase** (peak mean magnitude —
  symbol centers carry full amplitude, transitions dip). No full timing/carrier recovery — a cheap
  estimate that's honest about being a viz, not a demod. Multicarrier/FSK keep the full-rate cloud.
- **Implementation:** `apps/openpulse-linksim/src/gui.rs` — `samples_per_symbol()`, `baseband_iq()`
  best-phase symbol-spaced path, `PanelView::push(samples, sps)`, `sps` threaded from `fs.mode`.
- **Tests:** `apps/openpulse-linksim/src/gui.rs` unit tests (gui feature): baud parsing
  (order/suffix/multicarrier cases), and symbol-spaced-vs-cloud (far fewer points + tighter
  Q-spread on synthetic BPSK).
- **Test results:** `cargo test -p openpulse-linksim --features gui --bin openpulse-linksim-gui`
  3/3 pass; `cargo clippy -p openpulse-linksim --features gui --all-targets` 0 warnings.

## 2026-06-28 — Streaming AGC rollout to the PSK ladder (active-span gated)

- **Requirement/change:** roadmap 10.6 — roll the existing `openpulse-dsp::agc::Agc` out as a
  receiver front-end level normaliser for the PSK/QAM ladder, with active-span gating.
- **Design decision:** place it at the **single `route_audio_stage(InputCapture)` seam** (after the
  notch: remove interference, then normalise) so every capture path — `receive*` family and the
  daemon's `accumulate_capture` streaming path — gets it by construction. Opt-in (default off), like
  the notch, so dense-mode canaries can't regress unless enabled. The AGC's own docs forbid running
  on the raw capture (leading silence ramps the gain to its clamp); satisfied via **active-span
  gating** — `Agc::lock()` freezes the gain on sub-squelch (silent) blocks, `unlock()` adapts on
  carrier-present blocks (RMS ≥ DCD threshold). Tripwire counter mirrors the notch's. Exposed for the
  running daemon (no dead capability): `ControlCommand::SetAgc` + CLI `daemon set-agc`.
- **Implementation:** `crates/openpulse-modem/src/engine.rs` (`agc`/`agc_enabled`/
  `agc_blocks_processed` fields, `enable_agc`/`disable_agc`/`configure_agc`/`is_agc_enabled`/
  `agc_gain_db`/`agc_blocks_processed`, `apply_rx_agc`, seam wiring); `openpulse-daemon`
  `protocol.rs` + `lib.rs` (`SetAgc`); `openpulse-cli` `cli.rs` + `commands/daemon.rs` (`SetAgc`).
- **Tests:** `crates/openpulse-modem/tests/agc_loopback.rs` — off-by-default+toggle; tripwire on the
  `accumulate_capture` (daemon) path; active-span gating (gain ~0 dB through silence, boosts a weak
  carrier); decode of a ~30 dB-attenuated QPSK500 frame with AGC on.
- **Test results:** `agc_loopback` 4/4; full `openpulse-modem --no-default-features` suite green;
  `notch_loopback` 4/4 (notch path unchanged); workspace build (excl. pki-tooling) green; clippy
  (modem/daemon/cli) 0 warnings.

## 2026-06-28 — Host-driven TNC control (ARQBW/ARQTIMEOUT): blocked, finding recorded

- **Requirement/change:** wire the ARDOP `ARQBW`/`ARQTIMEOUT` host hints into the engine for real,
  replacing the accepted-but-ignored no-ops the #571 audit flagged.
- **Investigation:** same blocked class as the signed handshake (B). `crates/openpulse-ardop/src/
  main.rs` never calls `start_adaptive_session`/`start_ota_session`, so `current_tx_level()` is
  always `None` and `worker_loop` always runs the **fixed-mode** path — the adaptive ARQ ladder is
  dormant. `ARQBW` has no ladder to cap; `ARQTIMEOUT` has no ARQ connection to time out (the worker
  does single-shot `receive(mode, None)`). The only bandwidth-cap lever (`ota_set_level_bounds`)
  targets the **OTA** controller, but the worker's adaptive path reads the **rate_policy** controller
  — different mechanisms; no rate_policy bandwidth cap exists.
- **Decision:** wiring no-ops into the dead fields would re-create the "defined-but-not-consumed" gap
  the audit removed, so it was deliberately NOT done. Real fix is a feature (TNC runs an adaptive ARQ
  session + rate_policy bandwidth cap + connection timeout), recorded in `docs/dev/steering/roadmap.md` under
  the TNC command-surface audit.
- **Implementation:** none (no speculative surface); roadmap finding only.
- **Test results:** docs-only; workspace gates unaffected.

## 2026-06-28 — Linksim: I/Q constellation views flanking the QR branding band

- **Requirement/change:** show a constellation diagram for Station A to the left of the QR code and
  one for Station B to the right, keeping the text closest to the QR.
- **Design decision:** the `FrameStep` carries only real passband waveforms, so derive baseband I/Q
  via the existing `openpulse_core::iq::hilbert_iq` (fc=1500 Hz, fs=8 kHz — the `ModemEngine`
  defaults), trim the 31-sample group-delay edges, RMS-normalize, and decimate to ≤700 points — the
  same "viz straight from passband samples" approach the spectrum/waterfall already use. Map
  Station A = `forward_tx` (clean TX, panel 0), Station B = `forward_rx` (post-channel RX, panel 1),
  giving a clean-vs-noisy contrast that matches the app's "Station A | Channel | Station B" framing.
  Branding band reordered to `[const A | wordmark | QR | tagline | const B]` so the text stays
  nearest the QR and the constellations sit on the outer edges.
- **Implementation:** `apps/openpulse-linksim/src/gui.rs` — `PanelView.iq`, `baseband_iq()`,
  `constellation_plot()` (egui_plot `Points`, fixed unit bounds, no axes/grid), branding-band
  rewrite.
- **Tests:** GUI visualization (no unit test); `baseband_iq` reuses the unit-tested `hilbert_iq`.
- **Test results:** `cargo build -p openpulse-linksim --features gui` green; `cargo clippy -p
  openpulse-linksim --features gui --all-targets` 0 warnings. Visual confirmation pending (held for
  the user before merge, per the GUI-change rule).

## 2026-06-27 — Logbook peer GRIDSQUARE via handshake (B): blocked, finding recorded

- **Requirement/change:** carry the worked station's grid in the signed handshake so the logbook
  fills `GRIDSQUARE` from a verified, on-air source (the richer-fields item B, follow-on to A).
- **Investigation:** the Ed25519 signed handshake (`ConReq`/`ConAck`, `openpulse-core/src/
  handshake.rs`) is a tested library primitive that the **daemon never exchanges**. The
  `ConnectPeer` path runs `ModemEngine::begin_secure_session`, a *local* trust evaluation
  (`evaluate_handshake` over locally-supplied params) — it sends no `ConReq` and verifies no peer
  `ConAck` over RF. `ConReq`/`ConAck` are referenced only by the handshake lib + its tests.
- **Decision:** B is blocked on a larger prerequisite — wiring the over-the-air signed
  `ConReq`→`ConAck` exchange into the daemon connect — not a field add. Adding a grid field to a
  primitive the daemon never exchanges would create a fresh "defined-but-not-consumed" gap (the
  exact anti-pattern the TNC/config audits just removed), so it was deliberately NOT done. The
  config `[logbook.peer_grids]` map (A, shipped) remains the interim source.
- **Implementation:** none (no speculative surface). Finding + real-fix path recorded in
  `docs/dev/steering/roadmap.md` ("Signed handshake not wired into the daemon connect").
- **Test results:** docs-only; workspace gates unaffected (no code change).

## 2026-06-27 — Logbook peer GRIDSQUARE via config map (A)

- **Requirement/change:** populate the ADIF `GRIDSQUARE` (worked station's grid). The audit found
  the grid is NOT carried by the handshake/peer-cache/engine today, so "from the handshake" needs a
  protocol change (tracked as B). Deliver the outcome now via a config lookup.
- **Design decision:** a `[logbook.peer_grids]` callsign→grid map (case-insensitive), consulted at
  `begin_qso` by peer callsign — what most logging software does. Composes with B later (the
  handshake-exchanged grid would take precedence over the map).
- **Implementation:** `openpulse-config` `LogbookConfig.peer_grids`; `logbook.rs` (lookup at
  begin_qso → `Pending.gridsquare` → `GRIDSQUARE`); `server.rs` passes the map; TOML template.
- **Tests:** logbook unit (GRIDSQUARE from a lowercase-key map + uppercase connect), daemon
  integration (`connect_then_disconnect…` asserts `<GRIDSQUARE>`), config default (empty map).
- **Test results:** logbook 5/5, daemon integration passes, config 9/9, clippy 0.

## 2026-06-27 — ARDOP/KISS TNC command-surface audit

- **Requirement/change:** audit the ARDOP + KISS TNCs for the "accepted/advertised but not applied"
  gap class (a command the TNC accepts but no-ops, or a doc claim the code doesn't honour).
- **Finding:** ARDOP — `GRIDSQUARE`/`ARQBW`/`ARQTIMEOUT` are validated + echoed but never read by
  the engine (the modem self-manages bandwidth/timeout via its adaptive ladder); `CWID`/`SENDID`
  are honest warn-logged stubs. KISS — only `KISS_DATA` is applied; the 6 control frames
  (TXDELAY/P/SlotTime/TXtail/FullDuplex/SetHardware) were *silently* dropped.
- **Design decision:** the no-ops are defensible (self-managed rate/PTT) but were silently
  misleading. Make them honest, don't implement host-driven control speculatively, track the real
  wiring. KISS: log dropped control frames (`debug!`) instead of silent. ARDOP: code comment +
  corrected `docs/non-gpl-interfacing.md` (split "implemented" vs "accepted-not-applied" vs "stub").
  Roadmap "TNC command-surface audit" records the real-wiring follow-ups.
- **Implementation:** `crates/openpulse-kiss/src/server.rs` (log); `crates/openpulse-ardop/src/
  command.rs` (comment); `docs/non-gpl-interfacing.md`; `docs/dev/steering/roadmap.md`.
- **Test results:** ardop + kiss build; clippy 0; no behavior change beyond a debug log.

## 2026-06-27 — Adaptive-profile FEC audit (+ a permanent gate)

- **Requirement/change:** audit every adaptive profile's FEC assignment for the `cli_adaptive`
  bug class (a profile assigning no/wrong FEC to a mode that needs it — `hpx_ofdm_hf` had OFDM52-8PSK
  with no FEC).
- **Finding:** all 12 profiles are now **correct** — every modulatable rung decodes a clean loopback
  with its assigned FEC. The only rungs that don't decode are `hpx_narrowband_hd`'s SL8/SL9
  (QPSK9600-RRC / 8PSK9600-RRC), which can't modulate at 8 kHz — but `profile.rs` already documents
  that profile as **requiring a 48 kHz audio path**, so that's by design, not a gap.
- **Design decision:** promote the audit probe into a permanent CI gate rather than a one-off — it
  would have caught the `cli_adaptive` bug. The gate iterates every profile × rung, asserts clean
  decode with the assigned FEC, and pins the count of known-unmodulatable (48 kHz) rungs at 2 so a
  new unreachable rung trips it.
- **Implementation:** `crates/openpulse-modem/tests/channel_loopback.rs`
  `every_profile_rung_decodes_clean_with_its_fec` (no source change — the profiles were correct).
- **Test results:** gate passes; clippy 0.

## 2026-06-27 — ADIF logbook follow-ups (runtime toggle + parity + richer fields)

- **Requirement/change:** complete the ADIF logbook — a runtime `SetLogbook` control with CLI/panel
  parity (config-only before), and richer fields (RST/COMMENT from the RX SNR).
- **Design decision:** mirror the `SetNotch`/`SetCessb` pattern (control command + thin CLI
  `simple()` wrapper + panel toggle). `Logbook::set_enabled` for runtime control. At disconnect,
  read `engine.last_rx_snr_db()` → `RST_RCVD` (coarse SNR→RST bucket) + a `COMMENT` carrying the
  mode and SNR. Peer `GRIDSQUARE` from the handshake deferred — not exposed on the engine yet.
- **Implementation:** `crates/openpulse-daemon/src/logbook.rs` (`set_enabled`/`is_enabled`,
  `end_qso(now_ms, rx_snr_db)`, `rst_from_snr`); `protocol.rs` `SetLogbook`; `lib.rs` handler +
  disconnect passes the SNR; CLI `daemon set-logbook`; panel `Logbook: ON/OFF` toggle.
- **Tests:** logbook unit (runtime-toggle writes, RST/COMMENT present, `rst_from_snr` buckets);
  existing connect→disconnect integration still passes; CLI parse.
- **Test results:** daemon lib + logbook **all pass**; CLI `set-logbook` parses; clippy 0; full
  workspace green. Panel button → held for visual confirm.

## 2026-06-27 — WS-vs-TCP control-port parity audit (no gap)

- **Requirement/change:** audit another surface — does a `ControlCommand` reach the daemon on the
  TCP control port but not the WebSocket port (or vice versa)?
- **Finding:** **parity holds.** Both `lib.rs::handle_command` (TCP) and `ws.rs` parse the same
  `ControlCommand` enum, handle the identical 6 request-response commands inline (SubscribeSpectrum,
  GetConfig, ListMessages, GetMessage, SendMessage, DeleteMessage), and route everything else
  through the same `dispatch_command` → `apply_command_to_engine`. No command is reachable on one
  transport but not the other.
- **Design decision:** no code gap to fix; the only risk is *future* divergence (the two inline
  chains are duplicated). Added cross-referencing "keep in sync" comments to both handlers as a
  tripwire; a full consolidation into one shared request-response handler is noted as future
  hardening (low priority — no current gap).
- **Implementation:** comments in `crates/openpulse-daemon/src/lib.rs` and `ws.rs`.
- **Test results:** daemon builds; fmt clean; no behavior change.

## 2026-06-27 — CE-SSB gated off for OFDM-HOM (8PSK+) — a real ~6 dB regression

- **Requirement/change:** investigate the CE-SSB-on-OFDM cost surfaced while greening the baseline
  (CE-SSB clipping corrupted OFDM52+full-RS on a clean channel). Does it hurt any *shipped*
  OFDM-HOM+RS rung at marginal SNR?
- **Finding:** yes — CE-SSB was **net-harmful on OFDM-HOM**. `cessb_benefits` gated off 16QAM+ but
  still applied CE-SSB to **OFDM52-8PSK** (the shipped `hpx_ofdm_hf` SL7 rung, default-on). The
  peak-fair `cessb_power_evm` shows OFDM52-8PSK BER **0.0000 → 0.0026** (power gain doesn't recover
  the in-band clipping distortion), and a marginal-SNR AWGN sweep has it fail entirely with CE-SSB
  on (**12/12 → 0/12 at 12–16 dB**), decoding once gated off. CE-SSB is genuinely zero-cost only on
  the QPSK-subcarrier OFDM (OFDM16/OFDM52, BER 0→0). The team's own gating principle —
  "favourable raw BER notwithstanding, real-path decode breaks" — applies to 8PSK too.
- **Design decision:** add `8PSK` to the `cessb_benefits` exclusion (CE-SSB now applies only to
  QPSK-OFDM). The on-air +1.2 dB power result was on QPSK-OFDM and is unaffected.
- **Implementation:** `crates/openpulse-modem/src/engine.rs` (`cessb_benefits`).
- **Tests:** updated `cessb_power_evm::cessb_benefits_hold_*` and `cessb_engine::benefits_only_*`
  to assert 8PSK gated off; new real-path guard
  `channel_loopback::ofdm52_8psk_rs_decodes_at_operating_snr_with_default_cessb`.
- **Test results:** new guard 8/8 at 16 dB; cessb suites pass; full workspace **no failures**;
  clippy 0.

## 2026-06-27 — Config-schema completeness audit (defined-but-not-consumed)

- **Requirement/change:** audit another surface for the seam-gap class — config fields that exist
  (and are in the TOML template) but are never read, so setting them does nothing.
- **Design decision:** 72/79 fields consumed; the 7 dead ones are all in `[radio]` —
  `[radio.rig_a]` (never read; the primary rig is the top-level `[radio]`) and the `"generic"` CAT
  backend (`backend`/`serial_port`/`rig_file`, documented in the manual but unimplemented). Don't
  remove documented/planned schema and don't undertake the feature in an audit; instead mark them
  accurately so the config stops looking wired, and record the real fixes in the roadmap.
- **Implementation:** `crates/openpulse-config/src/lib.rs` (field docs + TOML template mark
  rig_a "currently unused" and the generic-backend fields "reserved — not yet implemented";
  corrected the repeater comment); `docs/dev/steering/roadmap.md` "Config/feature gaps" entry.
- **Tests/results:** `openpulse-config` 9/9 (template still parses), clippy 0. The recently-added
  `[modem] notch_*`, `[qsy] auto_qsy_on_interference`, `[logbook] *` fields were each confirmed
  consumed by the daemon during the audit.

## 2026-06-27 — Auto-QSY end-to-end validation

- **Requirement/change:** validate the notch → in-band-interferer → auto-QSY loop end to end
  (the capstone of the notch arc, previously only unit-tested piecewise).
- **Design decision:** the TCP twin daemon harness can't inject a *standalone* interferer (channel
  models transform a signal, they don't generate a tone into B's silence), so validate the full
  logical loop deterministically via `ChannelSimHarness`: Station A confirms a persistent in-band
  tone through `accumulate_capture`, auto-QSY transmits a real `QSY_REQ`, it crosses the channel,
  Station B decodes it and `process_received_bytes` opens a responder session (+ `QsyIncoming`).
- **Implementation:** test only — `crates/openpulse-daemon/src/lib.rs`
  `auto_qsy_end_to_end_initiator_to_responder_over_rf`.
- **Tests/results:** the new test passes; daemon lib **29/29**; clippy 0; fmt clean. Remaining: a
  two-station **on-air** run (rpi53 + FT-991A / SDR) — genuine hardware, not reproducible here.

## 2026-06-27 — Automatic ADIF logbook (opt-in)

- **Requirement/change:** the roadmap-recorded feature — a per-QSO station log in ADIF for import
  into logging software / LoTW / eQSL, opt-in.
- **Design decision:** per-QSO (a connect→disconnect session), distinct from the per-frame
  `TxSessionLog`. ADIF writer in `openpulse-core` (pure, no time crate — Hinnant civil-date for
  UTC); a daemon `Logbook` helper holds in-flight QSO state and appends on disconnect, decoupled
  from the RF loop (io errors are logged, never propagated). Sourced from the `ConnectPeer`
  callsign, the active mode (→ `SUBMODE`, `MODE=DYNAMIC`), the last `SetFreq` (→ `FREQ`/`BAND`),
  UTC connect/disconnect timestamps, and station callsign/grid from config.
- **Implementation:** `crates/openpulse-core/src/adif.rs` (`AdifRecord`/`to_adif`, `utc_date_time`,
  `band_for_mhz`, header); `crates/openpulse-config` (`[logbook] enabled`/`adif_path`);
  `crates/openpulse-daemon/src/logbook.rs` (`Logbook`); daemon `ConnectPeer`/`DisconnectPeer`/
  `SetFreq` hooks + `server.rs` build from config.
- **Tests:** ADIF unit tests (record render, band map, UTC format, header); `Logbook` unit tests
  (write/append-no-dup-header, disabled/no-pending no-op); daemon integration
  (`connect_then_disconnect_writes_an_adif_logbook_record`); config default.
- **Test results:** core adif 4/4, daemon logbook+integration pass, full workspace **no failures**,
  `clippy --all-targets` **0 errors**. Follow-up: a control command + CLI/panel toggle/export
  (config-driven for now).

## 2026-06-27 — Green the test/clippy baseline (3 red items)

- **Requirement/change:** make `cargo test --workspace` and `clippy --all-targets` green (they had
  red items all session, undermining the "real green results" traceability rule).
- **Design decisions + findings (each probed by clean loopback before fixing):**
  - `cli_adaptive::adaptive_ofdm_hf_reaches_top_rung`: the `hpx_ofdm_hf` profile had `fec_modes:
    [None; 21]` and the `adaptive` command decoded with no FEC — but OFDM52-8PSK fails unprotected
    even on clean. Per-level FEC measured: OFDM16/OFDM52 base decode unprotected and *break* under
    full RS (padded 255-byte block spans too many OFDM symbols); OFDM52-8PSK+ need RS. → assign RS
    to SL7–SL10 only, and make the command apply `profile.fec_for(level)` via
    `transmit_with_fec_mode`/`receive_with_fec_mode`.
  - `repro::ofdm52_rs_clean_128b_engine`: red because **CE-SSB** (default-on PAPR conditioner,
    #521) clips OFDM52-base+full-RS past RS t=16. That combo is used by no profile (zero
    operational impact) and the shipped OFDM-HOM+RS rungs survive CE-SSB; the guard predates
    CE-SSB (#185) and tests the OFDM modulator path → disable CE-SSB in the guard, documenting the
    finding that CE-SSB is *not* zero-cost on every OFDM mode.
  - 3 testbench clippy `field_reassign_with_default` lints → struct-update syntax.
- **Implementation:** `crates/openpulse-core/src/profile.rs` (hpx_ofdm_hf fec_modes);
  `crates/openpulse-cli/src/commands/adaptive.rs` (per-level FEC); `apps/openpulse-testmatrix/
  tests/repro.rs` (CE-SSB off in the guard); `apps/openpulse-testbench/src/signal_path.rs` (lints).
- **Tests:** `cli_adaptive` (6), the repro guard, full workspace test + `clippy --all-targets`.
- **Test results:** `cli_adaptive` 6/6; the OFDM ladder climbs SL5→SL10 (6/6 frames, ~1153 bps);
  full `cargo test --workspace --exclude pki-tooling`: **no failures**; `clippy --all-targets`: **0 errors**.

## 2026-06-27 — SetFreq panel control + CLI rig-default fix

- **Requirement/change:** make CAT `SetFreq` reachable from the panel (the one parity item left
  panel-only after the prior round), and fix the CLI `set-freq` default that the daemon rejects.
- **Design decision:** the daemon's `SetFreq` handler only accepts `rig == "rigctld"` (single CAT
  target), not the display rig_a/rig_b labels — so no rig selector is needed. Panel: a `Freq:`
  DragValue in **kHz** (operator-ergonomic, HF-ranged 1500–30000) + a `Tune` button sending
  `freq_hz = round(kHz × 1000)` with `rig = "rigctld"`, placed next to the Mode selector. CLI:
  change the `set-freq --rig` default from the invalid `a` to `rigctld`.
- **Implementation:** `apps/openpulse-panel/src/app.rs` (Freq DragValue + Tune → `SetFreq`; new
  `freq_khz` field, default 14070.0); `crates/openpulse-cli/src/cli.rs` (`set-freq` default rig).
- **Tests:** panel build + clippy/fmt (GUI confirmed visually before merge); CLI build + `set-freq`
  parse/connection-stage reachability.
- **Test results:** panel builds, **0 clippy errors**, fmt clean; CLI builds, fmt clean, `set-freq`
  parses and reaches the connect stage. (PR #562.)

## 2026-06-27 — Control-surface parity (CLI + panel)

- **Requirement/change:** the control-surface audit (`docs/dev/steering/roadmap.md` → "Control-surface
  parity gaps") found `ControlCommand`s reachable from one surface but not another: CLI couldn't
  `SendMessage` / `SetMode` / PTT / accept-reject QSY; panel couldn't `SetDcdSquelch` / start-stop
  OTA. Close the real two-way-operability gaps.
- **Design decision:** mirror existing patterns rather than invent new surface plumbing. CLI →
  thin `simple()` wrappers over the existing `ControlCommand` (identical to `set-cessb`/`set-notch`).
  Panel → toolbar controls mirroring the TX-atten slider (squelch) and OTA lock/unlock block
  (start/stop). Keep the OTA hysteresis/aggressiveness/bounds CLI-only — intentional (panel offers
  the simplified lock/unlock).
- **Implementation:**
  - CLI (PR #559): `crates/openpulse-cli/src/cli.rs` (`DaemonCommands`: SetMode, SetFreq,
    PttAssert/Release, AcceptQsy/RejectQsy, SendMessage); `src/commands/daemon.rs` dispatch arms.
  - Panel (PR #560): `apps/openpulse-panel/src/app.rs` (Squelch slider → `SetDcdSquelch`; OTA
    Start/Stop + `ota_profile` field; new fields `dcd_squelch`, `ota_profile`).
- **Tests:** CLI subcommand parse + connection-stage reachability (manual invocations); daemon-side
  handlers for these commands are covered by `openpulse-daemon` lib tests; panel build + clippy/fmt
  (GUI confirmed visually before merge).
- **Test results:** CLI builds; all new subcommands parse and reach the connect stage;
  `openpulse-daemon` lib: **25 passed / 0 failed**; panel builds, **0 clippy errors**, fmt clean.
  CLI #559 merged; panel #560 merged after visual confirm.

## 2026-06-27 — Seam-gap audit fixes (RX/TX cross-cutting)

- **Requirement/change:** after the notch-on-daemon-path gap, audit every cross-cutting RX/TX
  behavior for the "wired at one entry, not the shared seam" pattern.
- **Design decision:** move each cross-cutting concern to its single shared seam; verify the rest
  are already uniform; record intentional exceptions.
- **Implementation (PR #557):** TX regulatory `log_frame` → `stage_emit_output` seam; RX SNR record
  added to `receive_from_samples_with_fec`; removed duplicate OTA `FrameReceived` emit
  (`crates/openpulse-modem/src/engine.rs`).
- **Tests:** `crates/openpulse-modem/tests/tx_logging_seam.rs` (plain/FEC/ACK paths log);
  existing `FrameReceived` tests use `.any()`.
- **Test results:** new tx_logging_seam tests pass; full modem + daemon suites pass; fmt/clippy
  clean. Verified-not-gaps: DCD unified, CSMA-broadcast intentional, FrameTransmitted on all data
  paths.

## 2026-06-27 — Single RX front-end seam + tripwire (notch gap structural fix)

- **Requirement/change:** the receiver notch ran only on the `receive()` family, not the daemon's
  `accumulate_capture` streaming path — a coverage gap invisible to the (wrong-seam) tests.
- **Design decision:** place the notch at the single convergence point all ~19 capture paths funnel
  through, `route_audio_stage(PipelineStage::InputCapture)`; add a tripwire counter so a feature
  that never runs on a path is visible; test through the production entry, not a convenience seam.
- **Implementation (PR #556):** `route_audio_stage` applies the notch for InputCapture keyed by a
  stored `rx_mode`; `notch_blocks_processed()` counter; removed the two duplicate call sites.
- **Tests:** `notch_runs_on_the_daemon_streaming_capture_path` (drives `accumulate_capture`, asserts
  the counter); auto-QSY daemon test asserts it too.
- **Test results:** notch + QSY + loopback suites pass; single-application preserved on both paths;
  fmt/clippy clean. Prevention checklist added to `CLAUDE.md` → *Known sharp edges*.
