---
project: openpulsehf
doc: docs/dev/onair-handover.md
status: review
last_updated: 2026-11-05
---

# M3 on-air handover: from the relayed sessions to one local session

Until 2026-11-05 the on-air work ran as two sessions:
- a cloud session wrote tasks in `docs/dev/onair-tasks.md` and reviewed results;
- the rpi51 station session ran them;
- they talked through pushes to `claude/affectionate-brahmagupta-sdn8qg`.

From here, **one local session does both, on a build machine that reaches both stations over SSH.**
This note is where it starts. The task file stays the record of every run. Keep committing to the
branch; push when it suits you, not as the transport.

## The rules that do not change

- **Ask the maintainer before every key-up, every run, every repeat.** A go covers exactly the runs it
  names.
- **FT-991A (station B): 5 W at most.** Never raise either rig's power.
- **FT-818** (if used): low power, never increased; the front antenna stays selected; check the mode.
- **No physical access to either station.** Every change is remote (CAT, SSH). Never touch an antenna
  socket: station B keys an ACK after every frame.
- **RSPdx port B stays empty** while the IC-9700 transmits.
- **The branch is public.** Never record IP addresses, logins, host keys, or anything about a host other
  than the project's own (rpi51, rpi53). That includes the build machine: docs say "a build machine",
  nothing more.
- **Process:**
  - `CLAUDE.md`, `AGENTS.md`, `docs/dev/AGENTS.md`.
  - The mandatory adversarial review (Fable, as a subagent) for anything that keys a transmitter or
    changes when or how it does, for wire and trait changes, and for eliminations.
  - Commits carry `Implements: REQ-…` (code) or `Verification-objective:` (docs and tests).
  - A ledger entry in `docs/dev/project/traceability.md` and an acceptance row per substantive change.
- **Merge only with the maintainer's explicit go.** Never force-push.
- **Shell:** never `pkill -f` with a pattern in the same command; kill by PID.

## The build machine: what to set up first

1. **Toolchain:** `scripts/check-toolchain.sh` (rustc ≥ 1.97.1), the `aarch64-unknown-linux-gnu` target.
2. **Cross-build the station binaries** (`openpulse-server`, `openpulse`) with the real audio backend.
   - CPAL needs ALSA and `serialport` needs libudev, so the cross sysroot needs `libasound2-dev:arm64`
     and `libudev-dev:arm64`.
   - With `cross`, that means a custom image (`Cross.toml` with a pre-build step or a Dockerfile).
   - `--no-default-features` builds compile, but they have no sound card.
3. **Prove the cross build matches the station build before it keys anything.**
   - Start the daemon dry on both Pis, with nothing keyed: the audio devices open, the plugins are
     listed, and `[modem]` loads.
   - Compare its startup log with a `deploy`-built daemon's.
4. **Deploy:** `scripts/deploy-rpi-pair.sh` builds on rpi51 today. Add a mode that copies prebuilt
   binaries, or copy them and check them by hand. Either way, record the sha256 of both binaries in
   every run's notes, as the task file does.
5. **Native checks on the build machine:**
   - `scripts/gate.sh` (quote only its `GATE:` line);
   - `scripts/slow-tests.sh` before notch, OTA-rate, carrier-detect or acquisition changes;
   - the env `CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0` keeps the debug
     target small (it reached 25 GB without it);
   - don't run the gate and a pre-push hook at the same time.

## Where M3 stands (2026-11-05)

**A1: two-station exchange on 2 m, both directions.** SL5 (`BPSK250|rs`) has passed both ways on one
build. Details and the off-ladder failures are on the work plan's M3 line
(`docs/dev/project/workplan.md`).

**A2: the OTA ladder.**
- **The climb is shown both ways.**
  - A→B on `fast` reaches SL9 (OFDM52-16QAM, 14.1 dB) since the notch fix; SL10 never decodes.
  - B→A tops at SL6 since SL7's peak fix. That is expected: the FT-991A shows no ALC compression,
    and the OFDM rungs go out quieter at the same peak.
- **The demotion is shown on NACKs:** SL10 → SL8 and SL9 → SL8 on `fast`.
- **Demotion through attenuation is not shown** (a2h, a2i). Lowering A's drive goes from "decodes
  everything" to "nothing reaches the air" within one step (0.008 → 0.007, about 9 dB). B's carrier
  detect stops seeing a frame before the decoder would fail it, so no NACK is keyed. A different
  inducement is the maintainer's call: a stepped noise source, or an in-band interferer.
- **Stability: the link stalled for good in f2c,** after a climb into silence.
  - The fallback after silence (`docs/dev/design/ladder-climb-into-silence.md`) is built for it.
  - **It did not fire in task 24's f2d,** and why is open. Task 25 (f2e) is running with the reason
    logged.

## Open items, in the order I would take them

1. **Task 25 (f2e): why the fallback declined on air.**
   - Read B's `OTA: the peer is silent; no fall-back` lines and their `why`, and B's ACK lines (B is
     at `openpulse_daemon=debug`).
   - Offline, the fallback fires on B's exact ACK sequence (an engine replay) and in a two-daemon
     twin. The code at `a845ce4` reads correctly.
   - The answer decides whether this is a bug or a case the design missed.
2. **P4, the failed-probe step-down** (task 22 review, item 4). `NackStepDown` lowers the *confirmed*
   rung even when the failures were at an unconfirmed probe, so each failed SL10 probe also costs SL9
   (SL10 → SL8, nine and ten times per run). It changes A2's demotion behaviour, so it needs the
   mandatory review and the maintainer's go.
3. **P2(a), a per-rung level prediction for climbs.** It is parked; the estimators need a per-mode
   calibration first. In f2c the FT-991A read SL6 at 9.6 dB and SL7 at 6.0 dB while SL7 was 3.7 dB
   louder. See the design note, "P2(a)".
4. **The demotion method for A2** (above).
5. **The twin cannot deliver SL6 (QPSK250-D) in a debug build.** Likely the receive loop's decode time
   missing the ACK window. Unconfirmed.
6. **A late-ACK question left from task 21:** whether a late ACK ever marked a later frame delivered in
   a2g. UNCHECKED.

## Station facts learned on air

- **Station B (FT-991A), the receive audio state:**
  - SQL 0 gives steady hiss between frames; SQL above 0 gates to digital silence.
  - AGC FAST (Hamlib 2) is set on both rigs and is a Release 1 operating requirement: with a slow
    AGC the DCD's floor slides under the gaps (`dcd-raised-floor.md`).
  - The preamp (PREAMP 20) does not change B's idle hiss.
  - **ATT is rejected by the rig on 144 MHz.**
- **Station A (IC-9700):**
  - runs in ALC on the loud frames (median 0.617; peaks 0.85–0.86 with SL7's old 2.82 peak, 0.75
    since);
  - RFPOWER 0.0117647, drive `A_TX_SINK_VOLUME` 0.15.
- **The audio path does not clamp f32:** cpal writes f32 and PipeWire applies the sink volume in float.
  A sample above 1.0 was therefore real RF, not clipping (SL7 before P1).
- **`auto_id_signoff_idle_secs = 30`** on both stations. The runner has no setting for it; the
  station session wrote it with a local, uncommitted variable.
- **Logging:** use `DAEMON_LOG_LEVEL=info,ofdm_plugin=debug` normally. Full `debug` held B's receive
  loop for 1.4–2.5 s at a time (task 22). `ACK not received` is a debug line.
- **`/tmp` on rpi51 is an 8 GB tmpfs.** It filled once during f2d's audio copy.

## Methods worth reusing (all used in tasks 21–24)

- **Per-symbol OFDM analysis of a recording.**
  - A temporary `#[cfg(test)]` module in `plugins/ofdm/src/demodulate.rs` runs the plugin's own
    `acquire` → `shift_hz` → `find_first_data_body` → per-symbol FFT, `deramp_timing`, `ls_estimate`
    and `zf_equalize` on segments cut from the WAV.
  - It reports the decision-free pilot SNR, the decision EVM per subcarrier band and per symbol, and
    where the energy ends.
  - It found the unwhitened padding and the broken last symbol (`ofdm-padding-whitening.md`).
  - Remove it after use.
- **The production receive path on recorded or synthetic audio:**
  - `accumulate_capture` → `ota_decode_burst` with the stations' configuration (the notch included);
  - `notch_protects_the_ladder.rs` is the template;
  - it found the notch cutting OFDM edge subcarriers (`notch-protects-the-ladder.md`).
- **Two daemons over the twin bridge** (`openpulse_daemon::twin::spawn_bridged_pair`) with a custom
  `ChannelModel`; `climb_into_silence_falls_back.rs` is the template.
  - A muted channel must output exactly what the channel makes of silence (`apply` on zeros). Louder
    noise is a burst the receiver NACKs.
- **Transmit levels per rung** through `transmit_with_fec_mode` on a loopback (RMS, dBFS, peak). This is
  how the 13 dB step SL7 → SL8 was found.
- **Sabotage has to remove the behaviour.** `x() && false` still calls `x()`; write `false && x()`. A
  sabotage that passes means the test proves nothing.

## Where everything is

- **The runs:** `docs/dev/onair-tasks.md`, tasks 1–25, each with its results and the review.
- **Evidence:**
  - `docs/dev/test-reports/onair-twin-ota-*-evidence/` (daemon logs, events, configs, the PTT/ALC
    watcher, notes);
  - receive audio in `docs/dev/test-reports/on-air/rx-audio-2026-11-01/` (8 kHz, gzip).
- **The designs from this stretch,** each with its review in `docs/dev/reviews/`:
  - `docs/dev/design/dcd-raised-floor.md`
  - `ofdm-padding-whitening.md`
  - `notch-protects-the-ladder.md`
  - `ladder-climb-into-silence.md`
- **The plan:** `docs/dev/project/workplan.md`, the M3 line.
