> **Provenance.** 2026-07-19 multi-agent audit across 9 dimensions never previously audited here
> (memory-safety/unsafe/FFI, secrets, pipeline + supply chain, observability, threat model,
> architecture fitness, fuzz targets, concurrency, newest subsystems). 48 agents, refute-by-default
> verification, synthesis on a separate model. **32 findings kept, 6 refuted.** Agent output — treat
> each item as a lead until re-derived.
>
> Deliberately does NOT re-cover requirements coverage, acceptance-table executability, test vacuity,
> loopback evidence or docs status-drift; those were audited on 2026-07-18 (see
> `pre-1x-completeness-audit-2026-07-18.md`).
>
> **Hand-verified by the maintainer session** (greps shown in the PR): the repeater keying PTT with
> `full_duplex=false`; the ARDOP TNC having no PTT watchdog or release-on-disconnect while the daemon
> has both; `audit.toml` at the wrong path; 0/15 actions SHA-pinned in `release.yml`;
> `mfsk16-plugin`'s production dependency on `js8-plugin`; and the missing X25519 contributory check.
> Remaining items are NOT independently confirmed.
>
> **Caveat:** three finder agents ran while the safety classifier was unavailable. Their findings were
> spot-checked (4/4 accurate) but that is a sample, not a proof.

# What Isn't Nailed Down — Audit Report (9 previously-unaudited dimensions)

## 1. Executive summary

**Broadly solid on the low-level axes; a real, clustered problem on the regulatory/transmit-safety axis in the non-daemon front-ends.**

- **Memory-safety / unsafe / FFI**: effectively clean. There is no `unsafe`, no FFI of concern; the only defects are two *latent* panic paths (`unpack_callsign`, `wire_query` read helpers) that are guarded by their sole callers today. Nothing exploitable.
- **Secrets / key material**: no leak, but a genuine fail-open — the daemon silently swaps in a random identity key when the station key fails the owner-only check, while the sibling trust-store path refuses to start. Zeroization is absent workspace-wide, but the keystore crate that most needs it has **zero production consumers**, which deflates most of that surface.
- **Supply chain / pipeline**: `cargo audit` is red today (4 advisories) and the one gate that catches it fires only at release-dispatch, so nobody noticed; none of the 4 advisories actually reach the three shipped binaries and the release gate correctly blocks. Dead `audit.toml`, drifted ignore list, no SHA-pinned actions, CI unlocked. All process/hygiene, none shipping a vulnerability.
- **Observability**: several "the counter/event exists but never reaches the operator" gaps — RX tripwires are test-only, relay events are never drained, disk-full reports success, PTT-assert failure on the OTA-ACK path is silent.
- **Threat model / attack surface & the newest subsystems (filexfer, discovery)**: this is where the weight is. The daemon received the §97.119 callsign gate, the `SharedPtt` watchdog, and the `openpulse-linksec` auth gate; its **sibling front-ends and newest features did not**. The result is a cluster of unattended-transmission / PTT-left-asserted / pre-auth-reachable gaps.

**The organizing insight: almost every high/medium finding is the same shape — "the daemon was hardened, its twin was not."** ARDOP TNC (no PTT watchdog, no auth), the cross-band repeater (PTT asserted with no watchdog on the default config), filexfer offers (no callsign gate), the ARDOP command reader (unswept `Lagged` bug). This is precisely the cross-cutting-seam / sibling-front-end failure class the repo already documents — it just hasn't been swept across the non-daemon binaries.

Two outcomes the brief asked me to weight most, and where they land:
- **Could leave PTT asserted / cause unattended transmission**: findings **#1, #2, #3 (high)** and **#4, #5, #6 (medium)** below all touch this. Two of them (ARDOP TNC disconnect, repeater default config) can key a real transmitter with no time bound.
- **Reachable pre-authentication from network or air**: **#3 (LAN)**, **#4 (one spoofed RF frame)**, **#5 (RF Propose)**, **#6 (RF offer)** are all unauthenticated-reachable.

---

## 2. Ranked top findings

Ranked by severity × blast-radius; unattended-TX / PTT-asserted / pre-auth reachability weighted up. All are **[confirmed]**.

| # | Severity | Finding | Fix type | One-line fix |
|---|---|---|---|---|
| 1 | **HIGH** | ARDOP TNC leaves the transmitter **keyed forever** if the host disconnects after `PTT TRUE` — no watchdog, no release-on-disconnect (`openpulse-ardop/src/command.rs:212`) | desk | Give `ModemBridge` the daemon's `SharedPtt` (watchdog + RAII guard) and release unconditionally on `handle_client` exit. |
| 2 | **HIGH** | Cross-band repeater asserts PTT for the whole session **even when `full_duplex=false` (the default)**, outside any watchdog — unbounded dead-air carrier on a quiet band (`openpulse-repeater/src/lib.rs:156`) | desk | Guard line 156 (and the matching release at 169) with `if self.config.full_duplex`. |
| 3 | **HIGH** | ARDOP & KISS TCP ports have **no auth and no non-loopback gate** — a one-line `--bind 0.0.0.0` (a documented recipe) hands transmit control to the LAN; `PTT TRUE`/`MYID` are on the same unauthenticated port (`openpulse-ardop/src/data.rs:32`, `openpulse-kiss/src/server.rs:34`) | desk | Call `openpulse_linksec::auth_required(&bind_addr, false)` in each `main.rs` and bail/require-PSK as the daemon does; gate PTT/MYID arms too. |
| 4 | **MED** | One spoofed, unsigned 18-byte `QSY_REQ` **permanently disables auto-QSY** (the anti-jam response) for the daemon's lifetime — no TTL, no terminal-state reset (`openpulse-daemon/src/lib.rs:1348`) | desk | Add a terminal-state accessor to `QsySession`, clear `qsy_session` on terminal, add a session TTL; pair the gate with a negative control. |
| 5 | **MED** | Rendezvous responder transmits a full ~45 s JS8 over for **every inbound Propose** — no dedup, no cooldown, explicit bypass of the cadence gate; drives an unattended rig pre-auth (`openpulse-discovery/src/runtime.rs:522`) | design | Seen-token/per-peer cooldown before `enqueue_directed`; consider silently dropping when `rendezvous_channels` is empty. |
| 6 | **MED** | Inbound OPFX file-offer keys the transmitter with **no §97.119 callsign gate** — the one air-triggered TX path in the daemon that skips the audit-F6 gate every sibling has; fires while identifying as N0CALL (`openpulse-daemon/src/lib.rs:1325`) | desk | Gate `route_inbound_fragment` on `local_callsign_valid()` (and the offer `enabled` flag) at the single inbound seam. *(De-duped: two findings, one root cause.)* |
| 7 | **MED** | Daemon **silently substitutes a random ephemeral identity key** when the station key fails the owner-only (REQ-SEC-CTL-05) check — the default startup path, fail-open where the trust-store 15 lines below fails closed (`openpulse-daemon/src/server.rs:530`) | desk | Return `Err` and refuse to start on `InsecureSecretPermissions`/`IdentityKeyLength`, matching the trust-store policy; add a server-level startup test. |
| 8 | **MED** | OTA ACK **silently skipped on PTT assert failure** — receiver-led ARQ stalls with no log at all; 3 of 5 `keyed()` sites log, this one and the **station-ID transmit** (`server.rs:962`) do not (`openpulse-daemon/src/server.rs:785`) | desk | `let Ok(_guard) = ptt.keyed(..) else { warn!("OTA ACK PTT assert failed"); .. };` at :785 and :962. |
| 9 | **MED** | Disk-full/permission write failure reports **`VerifiedOk` to the sender**, emits no client event on the ListFiles path, and `clear_partials` has already deleted the resume blocks (`openpulse-daemon/src/filexfer.rs:692`) | desk | Derive `status` from the write result, emit `FileFailed`, move `clear_partials` to *after* a successful write. |
| 10 | **MED** | The ±2 s JS8 clock-skew TX gate is **structurally unreachable** (decoder window bounds the input to ±750 ms) and **fails open at zero observations**; its unit test asserts a value production can't produce (`openpulse-discovery/src/scheduler.rs:85`) | desk + doc | Reconcile the ±2 s doc claim with the ±0.75 s observable range (or widen the search window); add a runtime-level suppression test. |
| 11 | **MED** | Receiver **persists peer bytes and transmits BlockAcks while still `AwaitingDecision`** (state check runs after the disk write and on-air ack); rejected/cancelled/stalled transfers never clear their `.partial` dir (`openpulse-daemon/src/filexfer.rs:559`) | desk | Move `persist_block`/`enqueue_ctrl` after `note_block_complete`; call `clear_partials` on reject/cancel/inbound-cancel/stall. |
| 12 | **MED** | ARDOP command port **silently discards TNC event lines on broadcast lag** (`Ok(x)=rx.recv()` disables the branch on `Lagged`) — the exact bug already fixed in its two sibling loops; can drop `DISCONNECTED` and the §97 `FAULT no MYID` line (`openpulse-ardop/src/command.rs:82`) | desk | Mirror `data.rs:71-87`'s `match { Ok / Lagged→warn / Closed }`. |
| 13 | **MED** | Canonical plugin registrar lives in a **bin-only crate**, so 9 consumers duplicate the list by hand and 4 have drifted; the **testmatrix omits MFSK16** (hpx_hf's SL1), so published fade reports measure a profile with its sub-floor rung absent (`openpulse-cli/src/plugins.rs:17`) | design | Move `register_all` into `openpulse-modem`; add a test that every `SessionProfile::mode_for` resolves in each front-end's registry. |
| 14 | **MED** | **Nothing enforces the layering** — no `deny.toml`, no xtask, no dependency test, no CI check; the predicted drift (mfsk16→js8 plugin edge) has already landed (`Cargo.toml:1`) | design | `deny.toml [bans]` or a `cargo metadata` test: no plugin depends on another plugin, `openpulse-core` has no workspace deps; sabotage-verify. |
| 15 | **MED** | **No fuzzing/proptest/corpus anywhere** despite ~15 binary decoders reachable from unauthenticated RF; prior hand-audits found real bugs here (F-1, F-4, A-1, JSC saturation) — the surface is productive and scrutiny was never automated (`Cargo.toml:1`) | design | Stand up `cargo-fuzz` over `WireEnvelope::decode` + payload decoders, `FxFrame::decode`, `jsc_decompress`, `SarReassembler::ingest`; sabotage-verify the harness. |
| 16 | **MED** | `cargo audit` is **red today** (4 advisories, 3× CVSS 7.5) and the only live gate fires at release-dispatch, not merge; none reach the 3 shipped binaries and the gate blocks the release (`.cargo-husky/hooks/pre-push:5`) | desk | Add `cargo audit` to pre-push/xtask; fix the hook comment pointing at a disabled workflow; drop the dead `--ignore RUSTSEC-2023-0071`. |
| 17 | **MED** | Root `audit.toml` is **inert** (cargo-audit reads only `.cargo/audit.toml`); ci.yml hand-duplicates a drifted ignore list — the repo's own "hardcoded list mirroring config" anti-pattern (`audit.toml:1`) | desk | Move to `.cargo/audit.toml`; replace the ci.yml `--ignore` flags with a bare `cargo audit` so all call sites share one policy. |
| 18 | **MED** | **No action is SHA-pinned**; `softprops/action-gh-release@v2` runs in the `contents:write` release job and `dtolnay/rust-toolchain@stable` (a force-pushed *branch*) runs in every job — the sharper vector is artifact poisoning of the unsigned `.deb`/musl binaries (`release.yml:158`) | desk | SHA-pin all six actions with a version comment + Dependabot; add checksums/cosign to `files: dist/*`. |
| 19 | **MED** | Audio capture device loss surfaces **only at `debug`** with no `ControlEvent`; the real gap is *recovery* — `read()` never errors, so a dead cpal stream is never re-acquired even after replug (`openpulse-daemon/src/server.rs:760`) | desk | `warn!`-escalate + emit an `AudioFailed`/`CaptureDegraded` event on the open path; plumb the cpal stream-error callback into a dead-stream flag that clears `rx_stream`. |

**Lower-severity (LOW), full detail in §3**: relay events never drained (#13-area), RX tripwires test-only, daemon control-port slowloris/unbounded line reader, `PSK` discarded silently on loopback, no zeroization, non-atomic keystore save, `unpack_callsign` panic, `wire_query` read-helper panics, `modem`→concrete-plugin dep, mfsk16→js8 plugin edge, CI unlocked, `DisableRepeater` blocking join.

---

## 3. Full detail by area

### A. Regulatory / transmit-safety — PTT lifecycle & unattended TX (highest weight)

**A1 — ARDOP TNC leaves PTT keyed forever on host disconnect** *[confirmed, HIGH]*
`openpulse-ardop/src/command.rs:212` exposes unpaired `PTT TRUE`/`PTT FALSE` over `ModemBridge.ptt`. `handle_client` (command.rs:42-88) has three exit paths (EOF `break`, oversized-line `return Err`, write `?`) and none touches PTT; there is no `impl Drop` and no watchdog in the crate. Worse than reported: the ARDOP worker's data-TX path never keys PTT itself, so host `PTT TRUE` is the primary keying mechanism. `main.rs:210 build_ptt` wires a real `RigctldPtt`. **Scenario**: a Pat client sends `PTT TRUE`, then crashes/SIGKILL/TCP-drop → rig transmits indefinitely, no in-process recovery. This is exactly what the daemon's `SharedPtt::spawn_watchdog` (issue #863) exists for; the separate binary never got it. *Mitigation bounding it to HIGH not critical*: `ptt_backend` defaults to `"none"` (NoOpPtt), so it needs deliberate rigctld config — an expected on-air setup.

**A2 — Cross-band repeater keys PTT on the default `full_duplex=false`** *[confirmed, HIGH]*
`openpulse-repeater/src/lib.rs:156` calls `rig_b.assert_ptt()` with no `full_duplex` guard, unlike the four guarded sites at 92/104/128/135. It's the only runner and `EnableRepeater` spawns it regardless of config. `rig_b` is built from `[radio.rig_b]` — **not** the daemon's `SharedPtt`, so no watchdog reaches it. **Scenario**: with the shipped default `full_duplex=false`, `EnableRepeater` keys `rig_b` immediately and holds an unmodulated carrier until the first relayed frame — unbounded on a quiet band, on a controller no watchdog covers, with no station ID. Every `run_full_duplex` test sets `full_duplex:true` or `enabled:false`, so the shipped-default combination is untested. Requires two opt-ins (`enabled=true` + `[radio.rig_b]`), the documented way to run the feature.

**A3 — QSY responder wedged permanently by one spoofed frame** *[confirmed, MEDIUM]*
`openpulse-daemon/src/lib.rs:1348` parses `QSY_REQ` with `decode_qsy_frame` (unsigned — the crate has `decode_signed`/`verify_line` but the RF path never uses them). `get_or_insert_with` creates a responder that is never reset and has no TTL; `maybe_qsy_on_interference` early-returns on `is_some()`. **Scenario**: one 18-byte `QSY_REQ tok 2` parks the responder in `State::Rejected` (or `WaitingForList` when enabled), so every later QSY_REQ fails `InvalidTransition` and auto-QSY — the anti-jam escape — is suppressed for the daemon's lifetime. `QsySession` has no `is_terminal()` at all (the finding's suggested fix names a method to add). Scoped to opt-in features, recoverable by restart, manual QSY unaffected.

**A4 — Rendezvous responder: unbounded keying per Propose** *[confirmed, MEDIUM]*
`openpulse-discovery/src/runtime.rs:522` — in `TxMode::Full`, `respond()` returns `Send(..)` on **both** the Accept and Reject branches (rendezvous.rs:262), and `maybe_transmit` drains `rendezvous_tx` *before* the cadence check ("no cadence gate"). Measured (probe test): 1 Propose → 3 `TransmitBeacon`, 5 → 15; ~45 s keying per proposal, linear, no dedup/cooldown/allowlist. Downstream the Accept branch schedules an unauthenticated retune + `ConnectPeer` CONREQ (server.rs:1237-1364) — all before "the CONREQ is the auth." Off by default (`mode="rx_only"`) and DCD-gated, and the dial is confined to the operator's own channel table — but once `full` is opted into, a replayed/looping remote party drives ~45 s of keying per frame, defeating the "terminable/bounded" half of the repo's regulatory rule.

**A5 — Inbound OPFX offer replies with no callsign gate** *[confirmed, MEDIUM — de-duped from two submitted findings]*
`openpulse-daemon/src/lib.rs:1325` routes any SAR fragment with `segment_id != 0` into `filexfer::route_inbound_fragment` **before** the audit-F6 §97.119 gate six lines below (lib.rs:1335). `decide()` returns `Reject(FeatureDisabled)` even with the feature off (documented design — the reject goes on air intentionally), and `drain_filexfer_tx` keys PTT to send it. Verified by a temporary probe test: with `enabled=false` + `local_callsign="N0CALL"`, the tx queue holds a `FileReject` fragment (EXIT 101). `local_callsign_valid()` is checked at five sibling sites and never on the filexfer path. **The real defect is the missing callsign gate** (an N0CALL station emits an unidentified frame); the `enabled` reject-on-air is by design. One-line fix at the shared inbound seam.

**A6 — OTA ACK & station-ID silently skipped on PTT assert failure** *[confirmed, MEDIUM]*
`openpulse-daemon/src/server.rs:785` — `if let Ok(_guard) = ptt.keyed(..) { .. }` with no `else`, no log; `key()` (ptt.rs:72-85) propagates `assert_ptt()`'s error without tracing. Three of five `keyed()` sites log (filexfer :1068, discovery :1422, OTA-send :1550); the OTA-ACK path (:785) and the **periodic station-ID transmit (:962)** do not. **Scenario**: rigctld drops / serial unplugged → every receiver-led ACK and every §97.119 ID silently fails; the operator reads a dead PTT as a bad band. The genuinely undiagnosable case is *intermittent* assert failure. NoOpPtt returns `Ok`, so reachable only on configured hardware.

### B. Secrets & key material

**B1 — Ephemeral identity key fail-open** *[confirmed, MEDIUM]* — see top finding #7. `server.rs:522-539` swallows every `load_identity_from` error (permission failure, truncation) with a `warn!` and generates a random seed; the trust store 15 lines below refuses to start. This is the **default startup path** (empty `identity_key_path` still routes through it). Failure direction is toward *less* trust (station becomes unrecognised, sessions downgrade to Unverified), not impersonation — hence medium. The ephemeral seed also signs filexfer manifests/countersignatures. No test can see the daemon override.

**B2 — `OPENPULSE_CONTROL_PSK` discarded on loopback; panel/CLI mismatch** *[confirmed, LOW]*
`server.rs:350` discards a set PSK when `require_auth` is false (the loopback default) with no `warn!`; the info line is inside `if control_psk.is_some()`. The panel keys Noise solely on the env var being present (transport.rs:154); the CLI has no linksec dep at all (always plaintext). The discard is **documented intentional** (book:2849). The residual defect: an operator enabling security the obvious way (export PSK, stock config) gets a panel that silently fails to connect (`connect()` returns `None` after the framed read trips) with no diagnostic on either side, plus two untested paths (hex parser, discard gate). Fail-obscure, not fail-open — no plaintext session is established.

**B3 — No zeroization anywhere** *[confirmed, LOW]*
Zero `zeroize`/`Zeroizing` hits tree-wide. `FileKeystore` holds `master: String` + decrypted secrets as plain heap; `derive_key` returns a bare `[u8;32]`; `PendingHandshake` derives `Debug` and holds `pub kex_secret: [u8;32]` (a future `debug!(?pending)` would print a live ECDH secret). **Deflated because the keystore is entirely unconsumed** (no crate depends on `openpulse-keystore`); live sites hold ephemeral per-session keys, and the attacker model (gcore/`/proc/pid/mem`) already implies same-uid. Highest-value cheap wins: enable `ed25519-dalek`'s `zeroize` feature (identity keys are un-wiped too), and drop `Debug` from `PendingHandshake`.

**B4 — `FileKeystore::save` non-atomic and non-durable** *[confirmed, LOW]*
`lib.rs:157-163` truncates then `write_all` with no temp+rename, no backup, **no fsync** — an interrupted save leaves a short file that `open` rejects with `Format` and no recovery. `set`/`delete` re-save on every mutation. The finding's 0644-window half is **refuted** (`open` validates owner-only first, so a loose keystore is refused at load). No production consumer today, so "destroys the operator's PSK" is hypothetical. Fix: temp-file at 0600 → fsync → rename → fsync parent dir.

### C. Supply chain / pipeline

**C1 — `cargo audit` red; gate fires only at release** *[confirmed, MEDIUM]* — see #16. Real run: EXIT 1, 4 advisories (crossbeam-epoch, quick-xml ×2 @7.5, quinn-proto @7.5). The pre-push hook delegates to a `disabled_manually` workflow. **But** `release.yml` (active) runs a bare `cargo audit` in a `test` job that both builds `need`, so the gate blocks — and none of the 4 advisories reach the 3 shipped binaries (they enter via cli/panel/testbench/criterion dev-deps). Process latency, not shipped exposure.

**C2 — Root `audit.toml` inert + drifted mirror** *[confirmed, MEDIUM]* — see #17. A/B verified: cargo-audit reads only `.cargo/audit.toml`. The ci.yml hand-duplicated ignore list still carries `RUSTSEC-2023-0071` (rsa is gone from the lockfile) and lacks the 4 live IDs. Fail-closed (unread allowlist = everything reported), so no posture weakened.

**C3 — No SHA-pinned actions** *[confirmed, MEDIUM]* — see #18. Every `uses:` is a mutable ref; `@stable` is a force-pushed branch. **Refuted**: the "build jobs hold excess token scope" claim — repo default token is already read-only, and `release.yml` is `workflow_dispatch`-only (no pwn_request path). The real vector is artifact poisoning of the **unsigned** `.deb`/musl binaries that amateur operators install as root; needs no write token.

**C4 — CI gates build without `--locked`; release uses it** *[confirmed, LOW]*
`release.yml` locks (:26/:60/:108); every ci.yml gate and the pre-push hook omit it. **Reasoning refuted**: cargo does *not* opportunistically upgrade with a valid lockfile present, so there's no silent dependency drift. The narrow real gap: a PR that edits a Cargo.toml without committing the regenerated lock passes CI and fails loudly at release `--locked` — late feedback, not a silent ship. `cargo install cross` at ci.yml:65 is unpinned in version *and* `--locked`.

### D. Observability / diagnosability

**D1 — Disk-full write reports `VerifiedOk`** *[confirmed, MEDIUM]* — see #9. `filexfer.rs:655-692`: status computed before I/O, `clear_partials` runs before `write_file`, the failure arm is a lone `warn!` that doesn't mutate status. **Impact partly refuted**: `emit_terminal`'s fallback arm *does* send `FileReceived{verified:true, path:""}` (not a UI hang), but the file isn't in `received_files`, so `ListFiles` disagrees with the event, and the sender gets a valid countersignature for a file never written. Resume is destroyed. `auto_accept_max_bytes` defaults to 0 (opt-in receive) bounds exposure.

**D2 — Audio capture loss at `debug`, no recovery** *[confirmed, MEDIUM]* — see #19. **Mechanism corrected**: both shipped `read()` impls are infallible, so the `Err→reopen` branch at :757 is dead; mid-run unplug surfaces via cpal's error callback as `warn!` (cpal_backend.rs:182 — visible at default level). The sharp real defect is *recovery*: `rx_stream` is never cleared, so a dead stream is drained forever, never re-acquired. Startup with an absent/wrong device does hit the permanently-deaf loop at info-level silence. Needs a cpal build.

**D3 — RelayForwarder events written, never drained in ARDOP/KISS** *[confirmed, LOW]*
`relay.rs` pushes a `RelayEvent` on every `forward()` return; `drain_events` has zero callers in ardop/kiss (mesh drains). Two consequences: unbounded `Vec` growth (attacker-repeatable via replay flood → `DuplicateSuppressed` each) and no per-cause visibility. **Impact half-corrected**: the `debug!("{e:?}")` fallback *does* discriminate the six causes via the error enum — the residual is that it's below default level, so `AuthenticationFailed`/`PolicyRejected` on a relay node leave no default-verbosity record. Off by default; ~1 MB/month growth, a leak not a DoS. The sibling `seen` table beside it *is* bounded to 4096 — the event Vec was overlooked.

**D4 — RX pipeline tripwire counters are test-only** *[confirmed, LOW]*
`agc_/notch_/dc_/dcd_blocks_processed` have zero production consumers — no `ControlEvent`, no status, no log. The doc comment frames them as *runtime* checks ("if the notch is enabled but this stays 0 while the daemon runs..."), but no code performs that check at runtime. **Deflated**: the seam-gap regression class they guard is already gated at *build* time (the daemon cfg(test) drives `accumulate_capture` and asserts the counter), so a refactor killing a stage fails local gates before merge. Enhancement: fold the four into the 1 Hz `Metrics` event + `warn!` when an enabled stage's counter stalls; sabotage-verify.

### E. Network listeners / attack surface

**E1 — ARDOP/KISS ports unauthenticated** *[confirmed, HIGH]* — see #3. Neither crate depends on `openpulse-linksec`; a bare TCP data-frame goes straight to the modem TX queue (works in `Disc` state). `PTT TRUE`/`PTT FALSE` and `MYID <any callsign>` are on the same open command port (command.rs:212/99) — an attacker keys the rig *and* stamps a callsign of their choosing into the §97 log. The daemon treats the identical off-loopback bind as fail-closed. Exposure is the *documented* recipe (`manual.md:1451` gives `--bind 0.0.0.0` with no caveat), not a warned-against misconfiguration. Requires deliberate bind + LAN adjacency → high, not critical.

**E2 — Daemon control port: no pre-auth handshake deadline, unbounded plaintext line reader** *[confirmed, LOW]*
`handle_client` (lib.rs:839) awaits `AsyncNoise::responder` with no `timeout`; the accept loop has no connection cap. The plaintext reader is uncapped `.lines()` while the ARDOP twin was explicitly capped (`read_capped_line`). **Impact deflated**: `read_frame` checks length *before* allocating, so a trickling client costs one task+fd (fd exhaustion), not OOM; the uncapped-`.lines()` OOM is loopback-only (auth forces a PSK off-loopback), where the attacker can already issue authenticated commands. The useful observation is the asymmetry — the authenticated steady-state reader and the ARDOP twin are both bounded; only the daemon's pre-auth window has neither deadline nor cap. Fix mirrors the repo's own #942 precedent.

**E3 — ARDOP command port drops event lines on `Lagged`** *[confirmed, MEDIUM]* — see #12. `command.rs:82` `Ok(event)=recv()` self-heals but silently loses messages; `data.rs` and kiss `server.rs` carry the deliberate `match` fix with a comment naming this exact pattern. Trigger is *structural*: the data queue is `sync_channel(64)` against a `broadcast(32)` event ring, so a burst of data frames before `MYID` emits up to 64 `FAULT` lines into a 32-slot ring. Fail-closed on the regulatory axis (TX still refused, still logged server-side) — harm is operator diagnosis (the `FAULT` explaining the refusal can vanish; a missed `DISCONNECTED` staler Pat's state).

### F. Architecture fitness

**F1 — Plugin registrar duplicated across 9 consumers; testmatrix omits MFSK16** *[confirmed, MEDIUM]* — see #13. `register_all` lives in the bin-only `openpulse-cli`; 9 crates re-list by hand, 4 Cargo.toml manifests have drifted. The **strongest instance the submission missed**: `apps/openpulse-testmatrix` runs `AdaptiveHpxHf` against `hpx_hf` but has zero mfsk16 references and no dep — so published `docs/test-reports/` measure hpx_hf with its SL1 deep-fade sub-floor rung *structurally absent*, and a demotion there reads as link failure. The linksim already documented this exact artifact (issue #934) and was fixed; the testmatrix copy wasn't. Touches evidence quality on published reports — the repo's #1 defect class. Fix the testmatrix gap first, independent of the larger refactor.

**F2 — No layering enforcement mechanism** *[confirmed, MEDIUM]* — see #14. No `deny.toml`, no xtask, no `cargo metadata` test, no CI check; the only structural statement is prose in CLAUDE.md. The predicted drift has landed (mfsk16→js8). **Honest correction**: `modem`→plugin edges are plausibly intentional (static linking needs the registry populated), so a naive `[bans]` list fires on all ten. Enforceable clean rules: no plugin→plugin, `openpulse-core` has no workspace deps. `openpulse-core` holds by luck, not gate.

**F3 — No fuzzing/proptest/corpus** *[confirmed, MEDIUM]* — see #15. Confirmed null: no `fuzz/`, no proptest, no corpus. All 5 named entry points reachable from production RX (WireEnvelope::decode invoked on untrusted bytes at 5 non-test sites). Randomized testing that exists is FEC noise injection, not adversarial parser input. No failing input demonstrated (the variable-length decoders are hand-audited clean), so this is an assurance-methodology gap, not a live vuln — but the historical bug yield (F-1/F-4/A-1/JSC saturation, all landed) is direct evidence the surface is productive.

**F4 — mfsk16-plugin reaches into js8-plugin internals** *[confirmed, LOW]*
`plugins/mfsk16/Cargo.toml:14` + lib.rs:23-24 import `js8_plugin::{demodulate::goertzel_energy, modulate::{modulate_tones, GfskParams, DEFAULT_BT}}` — the only plugin→plugin edge. **No rule is actually violated** (CLAUDE.md's crate map is a description, no fitness check exists), the imports are genuinely M-agnostic misfiled DSP primitives, and "silently breaks" is false (both are on shipped paths, so a change fails the build or mfsk16's own gates). Tidiness item: hoist the three symbols to `openpulse-dsp`.

**F5 — `openpulse-modem` hard-depends on two concrete plugins** *[confirmed, LOW]*
`bpsk-plugin`/`qpsk-plugin` in `[dependencies]`; `qpsk_plugin` has **zero** `src/` references (only tests use it). `bpsk-plugin` has one real use (`benchmark.rs:8`, a non-cfg(test) `pub mod`). The CLAUDE.md "resolved" note is half-backed — the real registry wiring is in `openpulse-cli`, not the modem. Compile-time only. Fix: move `qpsk-plugin` to dev-deps, feature-gate `pub mod benchmark`.

### G. Memory-safety (latent panics)

**G1 — `js8_plugin::unpack_callsign` panics above 276,349,320** *[confirmed, LOW]*
`frame.rs:193` `ALPHANUMERIC[t as usize]` (39-byte table); `word[0]=idx(v)` at :205 is the one unbounded index. Verified by a targeted run (index 39, len 39). Safe today: the sole caller masks `& 0x0fff_ffff` (28 bits → max index 37). **Corrections**: the precondition *is* documented ("Unpack a 28-bit standard-callsign value"), just unenforced; 28 bits is the protocol wire-field width, not a "3% accident." js8-plugin isn't in the no-panic library list. Fix: `.get(..).unwrap_or(b' ')` matching the `% 32` guard next door.

**G2 — `wire_query` read helpers panic on short input; error branch is dead code** *[confirmed, LOW]*
`read_u64`/`read_u32`/`read_arr32` (`wire_query.rs:78-104`) slice before checking, so the `.map_err(MalformedPayload)` arm is unreachable and the only failure mode is a panic — while `read_u16` in the same file checks first (the inconsistency is accidental). These decoders run pre-signature-verification on RF bytes. **Latent, not live**: all 13 private call sites hand-derive a covering length guard today (traced, all correct), and hop counts derive from `u8`/`u16` so no overflow. `openpulse-core` is a no-panic path. Fix: add explicit `if bytes.len() < off+N` to each helper (matching `read_u16`) + a truncation loop test over every decoder (only `WireEnvelope` has one today).

### H. Concurrency

**H1 — ARDOP PTT / repeater PTT** — covered as A1/A2 (the concurrency dimension surfaced them as lifecycle-across-threads issues).

**H2 — `DisableRepeater` joins a blocking thread inside the async control loop** *[confirmed, LOW]*
`lib.rs:2210-2215` calls `thread.join()` bare in an `async fn` awaited in the single `select!` loop, while the same file uses `block_in_place` at 7 other sites. **Mechanism corrected**: cpal `read()` sleeps a fixed 10 ms and returns (not "blocks until samples"), so a typical stall is ~10 ms, not a decode cycle; the real worst case (seconds) is if the stop flag races an in-flight `transmit`. The suggested `spawn_blocking(join)` fix doesn't restore the rx tick (the await still serializes the arm), and main-rig PTT safety is already covered by the independent watchdog OS thread (#863). Hygiene, off by default.

---

## 4. What this pass did NOT cover

Explicitly out of scope here, audited separately in prior passes: **requirements coverage** (REQ-ID → test mapping), **acceptance-table executability**, **test vacuity** (name-vs-assertion mismatch) as a standalone sweep, **loopback evidence tiers**, and **docs status-drift**. Where a finding here touches those (e.g. the ±2 s clock-skew *vacuous test* in #10, the testmatrix *evidence-quality* gap in #13, the untested control-auth/hex-parser paths), it's included only because the underlying defect is architectural or regulatory, not because I re-ran the vacuity/coverage sweep.

These nine dimensions still leave untouched, that I could see but did not fully exercise:
- **Dynamic/runtime concurrency behavior** — I read for structural hazards (blocking-in-async, channel lag, PTT lifecycle) but ran no `tokio-console`, no loom, no stress/race testing. Data races under real multi-station load are unassessed.
- **The DSP/waveform correctness core** — every finding here is at the wiring/protocol/pipeline layer; the modulation math, LLR calibration, and fade behavior are covered by the repo's own acceptance gates and are all **Watterson-simulator tier, never on-air** (per the repo's own evidence-tier rule).
- **`pki-tooling`** — the web service / DB migration / bundle-signing crate got only incidental reads (it's excluded from the fallback core gates); its HTTP surface, SQL, and key handling warrant their own pass.
- **Actual fuzzing** — F3 reports the *absence* of fuzzing; I did not stand up a harness, so no decoder here has been adversarially exercised. Treat "the variable-length decoders are hand-audited clean" as a unit-test-tier claim, not an exhaustive one.
- **On-air / hardware-in-the-loop** anything — no finding was validated against a real rig; the PTT-lifecycle findings (A1/A2/A6) are read-confirmed and probe-tested, not observed keying a transmitter.