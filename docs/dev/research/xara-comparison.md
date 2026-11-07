---
project: openpulsehf
doc: docs/dev/research/xara-comparison.md
status: review
last_updated: 2026-11-07
---

# xara: what an open VARA-compatible modem does, against OpenPulseHF

## Summary

**What xara is.** An open-source (MIT) Rust modem that describes itself as "a VARA-compatible
modem for linux, freebsd and macOS. tested against VARA HF 4.9.0 and FM 4.4.0" (`README.md:3`).
Its CONTRIBUTING calls it "a reference implementation of VARA's protocol" and "a stopgap until the
world moves on to more modern, open protocols". Read at **v0.1.3, commit `09a9201 xara 0.1.3 Sat Oct
31 05:17:08 2026 +0000`** (`git -C ~/research-untrusted/xara log -1 --format='%h %s %cd'`), a
shallow clone with one commit. Paths below are relative to that clone; OpenPulseHF paths to this repo.

**Clean-room stance.** This note compares behaviour and design ideas only. No xara constant, table,
codebook, sequence, vector or code is copied here or into OpenPulseHF; where a number is quoted it is
a behavioural range (a search span, a timeout) needed for the comparison, and the file:line is given
so a reader can inspect the rest. Anything VARA-specific (codebooks, scrambler states, frame layouts)
is out of scope for OpenPulseHF, which is a from-scratch protocol
(`docs/dev/research/vara-research.md:369-379`).

**Provenance.** Neither `README.md` nor `CONTRIBUTING.md` says how the VARA protocol or the 37 JSON
files under `vectors/` were obtained. Checked: a case-insensitive grep for
`reverse|provenan|captur(ed|e) from|derived from|obtained|disassembl|clean.?room|extract|interop|generated (by|from)|recorded`
over every `.md/.toml/.json/.1/.yml` file matched only Pandoc's man-page banner and the release
workflow's `attest-build-provenance` step (so the filter works, and found no statement). The one
commit message is "xara 0.1.3 / fixes #28 / closes #17 / closes #19 / closes #29". No `.rs` comment
mentions VARA except `crates/xara-arq/src/fm/timing.rs:22` ("deviates from VARA behavior"). The
nearest thing to a provenance statement is in the tests: eight `#[ignore]`d tests say "needs the VARA
lab captures in $XARA_CAPTURES" (e.g. `crates/xara/tests/monitor_captures.rs:41`,
`crates/xara-phy/tests/common/mod.rs:21-25`), and vector names such as `vara_frames.json`,
`kiss/vara_lab.json`, `textcomp_blackbox.json` and `hf/b2f_snoop.json` suggest recordings of real VARA
stations and black-box observation. **That is inferred from names; the project states no method.**
The `registered` build feature unlocks the levels above 4 "if you have a registered copy of VARA"
(`README.md`, *registered mode*; `crates/xara-arq/src/hf/frames.rs:10`).

**Key findings.**
1. **xara has no carrier-gated receive.** Every waveform engine scans a rolling baseband buffer
   continuously and decodes at the peak of a normalised correlation metric; its busy detector only
   reports `BUSY` to the host and gates unproto/KISS sends. OpenPulseHF's receive is gathered by a
   carrier detect — the mechanism the fast-AGC finding broke.
2. **Its frame detectors are level-independent by construction** (unit-normalised phase products,
   energy *ratios*). Inferred, not measured: fast AGC should not hide its frames.
3. **It carries the frequency offset across a session** from every decoded frame and control frame;
   OpenPulseHF rolls the AFC back on each failed attempt.
4. **Its ACK windows are learned per level** from the peer's observed turnaround; OpenPulseHF's is a
   fixed 4 s / 9 s.
5. **Its PTT has a dead-man release** (audio loop stalled → unkey) and a process panic hook.
   OpenPulseHF releases on panic unwind (a RAII key guard, REQ-PTT-01) and when the device reports
   the last sample played (#1367), but a stalled-but-alive transmit path is bounded only by the 180 s
   watchdog.

## 1. Busy / carrier detection

**xara, read in code.**
- **Two tests, one hang** (`crates/xara-phy/src/busy.rs:162-206`). Both run on the front end's
  decimated complex baseband (`crates/xara-phy/src/hf/rx/frontend.rs:28-65`).
  - *Tone test* (`busy.rs:88-93, 215-225`): per FFT block, the strongest in-band bin against a
    low-ranked bin of the same block. A ratio, so the receive level cancels. It is gated by a
    peak-hold "VU" level in dBFS that decays per block (`busy.rs:216-221`), an **absolute** gate.
  - *Signature test* (`busy.rs:95-107, 227-248`): a sum of short spectra, comparing the bins where
    xara's own multi-carrier PSK levels put carriers against the gaps between them, with a ±1-bin
    shift. A waveform-matched spectral template; it exists only for PSK levels with ≥2 carriers
    (`busy.rs:76-86`). It also has an absolute mean-power gate (`busy.rs:237`).
  - Any hit re-arms a hang counter of blocks; the state falls when it runs out (`busy.rs:192-202`).
- **What it drives.** `BUSY ON/OFF` to the host (`crates/xara/src/station/hf.rs:461-466`,
  `crates/xara-host/src/response.rs:55`), and the unproto/KISS send: on HF only in 500 Hz mode, with a
  short hold after busy and p-persistence (`crates/xara-arq/src/hf/broadcast.rs:144-151`); on FM
  always (`crates/xara/src/station/fm.rs:92`). It is forced on while a session is not listening or
  PTT is keyed (`station/hf.rs:462`). An HF `CONNECT` does not consult it (`station/hf.rs:479-492`).
- **How a frame is gathered** (`crates/xara-phy/src/hf/rx/receiver.rs:290-328`). There is no
  burst. The receiver keeps a rolling ~24 s baseband track (`receiver.rs:19, 225`). Each engine the
  session expects (`receiver.rs:247-260`) scans it with a **correlation** metric and decodes at a
  held peak above a fixed threshold on a normalised statistic (`crates/xara-phy/src/dsp.rs:78`):
  - OFDM: unit-magnitude products between consecutive symbol rows, correlated against the known
    marker cells, three sub-bin frequency hypotheses (`crates/xara-phy/src/hf/rx/ofdm.rs:271-299,
    301-328`);
  - PSK: unit-magnitude one-symbol differentials against known pilot differentials, plus an M-th
    power term for data (`crates/xara-phy/src/hf/rx/psk.rs:141-173`);
  - FSK: pilot-tone energy over the other tones' energy in the same row (`hf/rx/fsk.rs:100-118`);
  - control frames (ACK/NACK codewords): tone energy against its comb neighbours, plus a hit count
    (`hf/rx/ctrl.rs:290-309`).
  - The one level test is the end-of-frame check for an unannounced second frame: mean power after
    the frame against a floor taken as a low percentile of recent un-muted blocks, and it requires the
    frame to be several times that floor (`crates/xara-phy/src/receiver.rs:22-25`,
    `hf/rx/receiver.rs:354-361`).

**OpenPulseHF.** The carrier detect at the `InputCapture` seam is total power against an adaptive
squelch, OR the per-band spectral test (#1454), OR the OFDM preamble detect P (#1518/#1519)
(`crates/openpulse-modem/src/engine.rs:2619-2722`). The squelch and the spectral test take their floor
from `NoiseFloorTracker`, per-bin over time and held during a burst
(`crates/openpulse-dsp/src/noise_floor.rs:1-24`, `S_OPEN`/`S_HOLD_MEAN` at `:116, :137`). That decision
gathers the burst (`accumulate_routed`, `engine.rs:3204-3300`) that the decoder is handed.

**Under fast AGC (inferred from code, not measured).** On air an OFDM frame under AGC FAST sat at
1.15–1.22× the gap hiss (`docs/dev/design/ofdm-preamble-detect.md:20-35`). xara's frame detectors use
no level: phase products are unit-normalised and FSK/control metrics are ratios, and AGC is a real gain
that leaves phase alone. So its gathering should survive this. Its busy *report* should too: the tone
and signature tests are ratios, and AGC'd hiss clears the absolute gates. Two exceptions:
- the busy detector has **no OFDM template**, so it cannot report a third party's OFDM burst
  (`busy.rs:76-86`);
- the second-frame continuation check is energy-based and would read "nothing follows" at 1.2×. That
  fails toward closing the turn, so a single-frame turn is unharmed.

**Learn / better / n.a.**
- *Learn:* gather by correlation, not by a carrier gate. That is the direction P already took for
  SL7–SL14; xara applies it to every waveform and the control frames too.
- *Learn:* a waveform-matched spectral signature (carriers against gaps) as a level-free busy test.
- *OpenPulseHF does better:* its floor is per-bin, held during bursts and tracked over time; xara's
  absolute VU and power gates are the kind of level constant OpenPulseHF has removed four times.

## 2. Carrier-offset acquisition

**xara, read in code.**
- **Coarse.** Per waveform, inside detection:
  - OFDM: three STFTs at 0 and ±half a bin (`hf/rx/ofdm.rs:29, 39-44`). The detection metric is
    insensitive to a common rotation (a frequency offset multiplies every unit product by the same
    phasor), so each hypothesis covers part of a bin.
  - FSK and control frames: an integer-bin (FSK) or half-bin (control) shift search. The span is
    ≈ ±115 Hz unlinked and ≈ ±30 Hz linked (`hf/rx/receiver.rs:20-21, 243-245`;
    `hf/rx/fsk.rs:125-136`; `hf/rx/ctrl.rs:198-200`).
- **Fine.**
  - OFDM: the marker products' mean phase over one symbol period, then a ±1 alias test on coherence
    (`hf/rx/ofdm.rs:370-389`).
  - PSK: the pilot-differential phase (`hf/rx/psk.rs:251-259`), unambiguous to about half the symbol
    rate.
  - Control: parabolic interpolation over three shifts (`hf/rx/ctrl.rs:239-244`).
- **Carried across the session.** One running `afc_hz` derotates the whole baseband
  (`hf/rx/receiver.rs:293-298`). It is updated from every control-frame detection (`:302-309`) and
  from the mean residual of the good frames in each transmission (`:485-492`). When linked, only half
  of each correction is applied (`:305, :489`). *Inferred:* both ends correct, so each takes half and
  they do not chase.
- **Range, as far as the tests show.** Data frames at −30 and +25 Hz on every 2300 Hz level
  (`crates/xara-phy/tests/rx_loop.rs:126-147`); control frames up to +90 Hz, AFC within 6 Hz
  (`rx_loop.rs:150-175`). No test beyond ±117 Hz was found.

**OpenPulseHF.**
- BPSK: a dense spectral peak of the squared signal, ±400 Hz (`plugins/bpsk/src/demodulate.rs:432-488`,
  `crates/openpulse-modem/src/engine.rs:368-373`; `docs/dev/design/bpsk31-settle-sideband.md`).
- OFDM: its own ±50 Hz coarse grid, then the pilots (`docs/dev/design/ofdm-carrier-offset.md:40-50`).
- **The AFC is rolled back on every failed attempt**, so a receiver meets each burst cold
  (`bpsk31-settle-sideband.md`, *Problem*). Seeding it from the previous session is parked as not
  recommended for Release 1 (*Option 2* there).

**Learn / better / n.a.**
- *Learn:* a session-scoped offset carried from *decoded* evidence (good frames, ACK detections)
  rather than rolled back. It differs from Option 2: within a session, not across sessions.
- *OpenPulseHF does better on range:* its BPSK reach is wider, and it has a measured sideband fix;
  xara's PSK fine estimate is bounded by the symbol rate.
- *n.a.:* the halving matters only if the transmitter also follows the received offset.

## 3. Waveforms and FEC

**xara, read in code** (`crates/xara-phy/src/hf/level.rs`).
- Three bandwidths, 500/2300/2750 Hz, with 13/16/17 levels (`level.rs:90-96`). Each level is FSK,
  multi-carrier PSK, or OFDM (`level.rs:149-153`; tables at `:416, :456, :516`).
  - The low levels are MFSK;
  - the middle levels are 2–13 carrier BPSK/QPSK with RC/RRC pulses;
  - the top levels are OFDM: differential PSK on the carriers, coherent QAM with scattered pilots at
    the very top (`hf/rx/ofdm.rs:394-421`).
- **Net rates, computed with `Level::net_bps` (`level.rs:291`) from the tables.** About 18 bps at
  level 1; about 0.9–1.2 kbps at the PSK top; about 7.0 kbps (2300) and 8.5 kbps (2750) at the OFDM
  top; about 1.5 kbps in 500 Hz. Unregistered builds stop at level 4 (`hf/frames.rs:10`): MFSK in
  2300/2750 Hz, about 175 bps at most.
- **FEC.** One code family throughout: an 8-state parallel-concatenated (turbo) code, punctured per
  level from a rate-1/3 mother (`crates/xara-phy/src/turbo.rs:10-30`, `fec.rs:53-73`).
  - The decoder runs a fixed iteration cap with a CRC early stop (`decode.rs:7-9, 53-54`), in
    parallel windowed segments (`turbo/engine.rs:76-187`).
  - It has fixed- and floating-point kernels, the fixed one the default on ARM Linux (`build.rs`).

**OpenPulseHF** (`docs/mode-fec-ladder.md:235-250`).
- `hpx_hf` SL1–SL14 in one ≤2700 Hz span: MFSK16 (~9 bps), BPSK31–250, QPSK250-D, OFDM52 BPSK…64QAM.
  Its top is 7.7 kbps (SL14, LDPC 8/9).
- FEC per rung: RS on SL1–SL6, soft Viterbi + RS on SL7–SL11, high-rate LDPC on SL12–SL14
  (`mode-fec-ladder.md:111-125`). A rate-1/3 PCCC exists (`Turbo`) but is not on `hpx_hf`.

**Learn / better / n.a.**
- *Learn:* the multi-carrier PSK middle — several 93.75 Bd carriers, 0.4–1.2 kbps — fills the same
  gap OpenPulseHF's BPSK250 → QPSK250-D → OFDM52 steps cover. It is evidence that a parallel-carrier
  PSK rung is viable at Pi-class CPU. Whether it beats QPSK250-D on a fade is unmeasured.
- *Learn:* a single soft code with per-rung puncturing keeps one decoder hot. OpenPulseHF runs three
  code families.
- *n.a. for Release 1:* the ladder is frozen for v0.17.0.

## 4. ARQ, rate adaptation and session

**xara, read in code.**
- **Receiver-led, multi-step.** The receiver measures SNR and replies with a code that moves the
  sender: STAY, UP by 1–4, DOWN, "second frame only", NACK (`crates/xara-arq/src/hf/adapt.rs:163-177`).
  - Each up step needs a positive margin over that level's threshold and a large enough sender-queue
    "hint".
  - The margin grows on a loss — more after failed climbs and after a short run of good frames — and
    decays per good frame to a floor (`adapt.rs:83-131`).
  - A rising SNR is half-averaged and a falling one taken at once (`adapt.rs:32-38`).
- **Down after a NACK** scales with how far the sender climbed since its anchor: 1 to 3 steps
  (`adapt.rs:191-202`, `hf/session.rs:1170-1178`).
- **Candidate set.** The receiver listens on its level, the previous level and, after a NACK, the
  three levels below (`hf/session.rs:438-447, 1376-1378`).
- **Silence.** A sender that hears no reply sends an ACK *request* in a progressively more robust
  control format (`hf/session.rs:748-756`; `hf/ctrl.rs:83-89`). A receiver that missed the data
  answers it with NACK-missed: it counts the frames lost, steps 2 down and widens its candidates
  (`hf/session.rs:1210-1226`). A watchdog (30 s unlinked, 60 s linked) ends a dead link
  (`hf/timing.rs:9-10`, `session.rs:705-710`).
- **Timing.** The ACK window per level is the running mean of the peer's observed turnaround at that
  level. Unmeasured levels are extrapolated from a per-level decode allowance (`hf/timing.rs:90-160`).
- Two frames per turn, with selective first-/second-only acknowledgement (`hf/session.rs:1158-1169`).

**OpenPulseHF** (`crates/openpulse-core/src/ota_rate.rs:1-48`).
- Receiver-led with a **lockstep invariant**: at most one mapped step above the confirmed level, so
  the candidates are exactly {recommended, confirmed}. A lost ACK cannot desync the ends.
- Climbs on SNR or on 3 clean decodes (`:64`).
- The ladder moves only on a NACK that is keyed (`NackVerdict`, `:89-100`). The NACK budget caps
  keyed NACKs at 3 and leaks one per 10 min (`crates/openpulse-daemon/src/nack_budget.rs:14-17`).
- After a climb into silence, the sender falls back once to the level before the climb, then declines
  re-climbs with doubling backoff (`ota_rate.rs:66-71, 396-430`).
- The ACK timeout is fixed: 9 s with MFSK16 in the profile, else 4 s (`engine.rs:8666-8672`).

**Learn / better / n.a.**
- *Learn:* the **sender-initiated ACK request in a more robust format** gives the receiver a chance to
  say "I missed it" instead of the sender inferring from silence. It is a wire change (mandatory review).
- *Learn:* **learned per-rung ACK windows** instead of one fixed timeout: shorter waits on fast rungs,
  fewer false timeouts on slow decodes (the BPSK31 deadline misses in `bpsk31-settle-sideband.md`).
- *OpenPulseHF does better:* its lockstep invariant makes desync impossible by construction. xara
  climbs up to 4 levels at once and must widen to 5 candidates to recover, each a full decode per
  detection peak.

## 5. PTT and timing

**xara, read in code.**
- **Backends** (`crates/xara/src/ptt.rs:37-120`): RTS, DTR, CM108 HID (by device or USB id), rigctld,
  VOX. **Lead, tail and max-on** are configurable (`ptt.rs:260-276`).
- **The controller is one thread** (`ptt.rs:416-498`). It needs a heartbeat from the audio loop while
  keyed:
  - if none arrives for a short stall time, it unkeys (`ptt.rs:454-460`);
  - past max-on it unkeys and latches until the request drops;
  - a failed key counts as still keyed, and a release is retried three times (`ptt.rs:357-375`);
  - a process panic hook shuts every controller down and waits for release (`ptt.rs:316-334`).
- **The TX latency model** (CHANGELOG 0.1.3; `crates/xara/src/audio/txpath.rs:12-47, 120-200`) is
  an explicit budget: capture latency + capture backlog + PTT lead + keying + playback queue +
  interpolator + device. It is measured, smoothed and logged when it moves.
  - The station starts each burst that much early so the audio reaches the air when the protocol
    expects it (`crates/xara/src/station/scheduler.rs:90-92`; fed in `crates/xara/src/main.rs:476-478,
    562-563`).
  - The lead is zero samples ahead of the burst (`txpath.rs:95`).

**OpenPulseHF.**
- **Backends:** none, VOX, rigctld, CM108, GPIO, RTS, DTR (`crates/openpulse-radio/src/ptt_builder.rs:46-74`).
- `SharedPtt` has an independent watchdog: 180 s, an engineering bound on a hang
  (`crates/openpulse-radio/src/shared_ptt.rs:24-40`, `:435`). A failed release leaves it armed.
- The leader is `ptt_leader_ms` (`crates/openpulse-config/src/lib.rs:423-427`).
- The key-to-audio gap work cached device resolution out of the keyed window
  (`docs/dev/design/key-to-audio-gap.md`, *Round 2*).
- **Release on panic and at end of audio exist; a stall dead-man does not.**
  - A keyed scope holds a RAII key guard that releases during unwinding (REQ-PTT-01;
    `crates/openpulse-radio/src/shared_ptt.rs:452`, test `key_guard_releases_on_panic_unwind` at
    `:907-910`; `crates/openpulse-daemon/src/server.rs:2005`, `:2221`).
  - The cpal backend releases when the device reports that the last sample has left it, with an
    underrun's zero delay explicitly not trusted (#1367; `crates/openpulse-audio/src/cpal_backend.rs:379-442`,
    `crates/openpulse-audio/src/flush.rs:58`, `:178`).
  - There is no process panic hook (`set_hook` hits xara's `ptt.rs:319` and nothing in `crates/`).
    It is not needed for an unwinding panic, which the guard covers; it would matter only under
    `panic = "abort"` or for a panic outside a keyed scope.
  - **What is missing:** a transmit path that stalls while ALIVE (no panic, no drain: e.g. a hung
    audio write) stays keyed until the 180 s watchdog.
- **No end-to-end TX latency budget.** My grep for
  `tx.?latency|latency budget|playback latency|output latency` in `openpulse-daemon`, `-audio` and
  `-modem` found nothing; the same pattern hits xara's `txpath.rs:209`.
- *Correction, made before commit:* the first draft said OpenPulseHF has "no panic hook" and implied
  no panic release at all. A code check found the RAII guard and the drain-based release above.

**Learn / better / n.a.**
- *Learn:* **stall-release** — a transmit path that stops writing audio unkeys in well under a
  second, not after 180 s of dead carrier. It keys a transmitter (mandatory review).
- *Learn:* a **panic hook that releases PTT** before unwinding.
- *Learn:* a measured, logged latency budget turns the implicit leader into a number the operator can
  read.
- *OpenPulseHF does better:* fail-closed refusal of an unusable backend name (memory: maintainer
  ruling) and a key-generation guard in `SharedPtt`.

## 6. Host interface

**xara.**
- A VARA-style command port with the data port at command+1 (`crates/xara-host/src/server.rs:78-84`),
  with VARA's command vocabulary (`crates/xara-host/src/command.rs`).
- A KISS TCP port, 8100 by default (`README.md`). KISS frames go out as fragmented unproto broadcasts
  (`crates/xara-arq/src/kiss.rs:91-150`).
- A token-protected web UI, which can key the radio only with `--ui-control` (`README.md`).

**OpenPulseHF.**
- The ARDOP TNC on 8515/8516 (`crates/openpulse-config/src/lib.rs:777-784`).
- KISS on **8100** (`lib.rs:789-796`).
- Daemon control on TCP 9000 / WebSocket 9001, with PSK auth enforced off loopback
  (`lib.rs:176-183, 858-866`).

**Learn / better / n.a.**
- *Practical:* both default KISS to 8100, so running both on one host collides. Worth a line in the
  CLI guide.
- *n.a.:* a VARA command port. Pat and Winlink already speak ARDOP, and emulating VARA's interface
  invites the compatibility questions `vara-research.md:375-379` defers.

## 7. Engineering practice

**xara.**
- **CI on every push and PR** (`.github/workflows/ci.yml`): fmt, clippy `-D warnings` per feature set,
  `cargo test --release` on Linux x86/ARM64, macOS ×2, a FreeBSD check, an MSRV build, and ARMv6/v7
  tests under qemu, both turbo kernels.
  - Actions are pinned by tag (`@v4`), not by SHA.
- **Reproducible releases** (`README.md`, *reproducing a release*; `.github/workflows/release.yml`).
  The maintainer puts SHA-256 sums in the annotated tag, and CI rebuilds in `ubuntu:22.04` and fails
  unless its sums match (`release.yml`, "sums match the tag"). It then attests build provenance.
- **Tests.**
  - TX fidelity checks synthesized audio against stored fingerprints (`vectors/audio_2300.json`,
    `crates/xara-phy/tests/tx_fidelity.rs`).
  - Session behaviour is a **golden transcript** of a simulated two-station link, re-blessed with
    `UPDATE_GOLDEN` (`crates/xara-arq/tests/golden/mod.rs:10`, `tests/golden/hf_link.txt`).
  - RX against real captures is `#[ignore]`d, and the captures are off-repo.
  - My grep for `watterson|rayleigh|doppler|fading|multipath` found no fading-channel model in xara;
    the same pattern hits `crates/openpulse-channel/src/fading.rs`. Its RX tests are AWGN plus offset
    (`crates/xara-phy/tests/rx_loop.rs:13-18, 126-147`).

**OpenPulseHF.**
- `scripts/gate.sh` is the workspace verdict, run post-merge and before tags. It is not a required
  check (CLAUDE.md, *Verification mechanics*).
- Actions are SHA-pinned (`.github/workflows/coverage.yml:40`).
- On-air captures are committed and run in the default suite (`crates/openpulse-modem/tests/captures/`).
- Watterson fading is simulated, and traceability and acceptance tables are kept.
- No reproducible-build check on releases: `SOURCE_DATE_EPOCH` appears only in
  `scripts/generate-sbom.sh:33`.

**Learn / better / n.a.**
- *Learn:* the tag-recorded-sums reproducibility check is cheap and strong (the `reproducible-builds`
  and `release-checklist` skills).
- *Learn:* a golden transcript of a two-station session would make any ARQ behaviour change visible
  as a diff.
- *OpenPulseHF does better:* fading channels, committed real captures in the default run, SHA-pinned
  actions.

## 8. The passive monitor

**xara.** `xara-monitor` (`crates/xara/src/bin/xara-monitor.rs`, `crates/xara/src/monitor.rs:159`)
decodes other stations' VARA HF sessions and never transmits. VARA keys its control codewords to the
session's callsigns, so the monitor needs known calls or a **blind search** over the keying state
from observed tones (`crates/xara-phy/src/hf/rx/blind.rs:89-370`). Its README warns it "probably won't
run on a pi".

**OpenPulseHF.** The daemon's `[monitor]` (`crates/openpulse-daemon/src/monitor.rs:1-12`, REQ-RX-01)
decodes every registered mode from a tee of each completed burst, on a throwaway engine. The CLI has
`monitor` (`crates/openpulse-cli/src/cli.rs:212`). Neither has a key to recover: OpenPulseHF framing is
not callsign-keyed.

**Learn / better / n.a.** Equivalent in role. The blind key search is VARA-specific (n.a.). One point
transfers: xara's monitor uses the same continuous correlation receiver, so it has no carrier gate
for a fast AGC to defeat. OpenPulseHF's monitor sees only bursts the carrier detect gathered, and P
does not run for it (`docs/dev/design/ofdm-preamble-detect.md:49-70`, *Twins* `:487-505`).

## Ideas worth evaluating for OpenPulseHF (ranked)

1. **Carry the AFC within a session from decoded evidence; stop rolling it back on a failed attempt.**
   - xara: `hf/rx/receiver.rs:293-309, 485-492`.
   - *Evidence:* replay f2j bursts 1–4 (`bpsk31-settle-sideband.md`) with the AFC seeded from the
     last decoded frame or ACK. Measure time-to-decode and wrong settles, with a negative control: a
     seeded AFC that is wrong by one sideband must not lock in.
   - *Release 1:* bears on A2's slow starts, but the dense coarse stage already fixed the measured
     case. Not a blocker; maintainer call.
2. **PTT stall-release** (a dead-man on a stalled-but-alive transmit path; panic unwinding is already covered by the RAII guard).
   - xara: `ptt.rs:316-334, 454-460`.
   - *Evidence:* a test that stops the TX writer while keyed and asserts unkey within the stall bound
     through `SharedPtt`, sabotage-verified. Keys a transmitter, so a mandatory review.
   - *Release 1:* transmit safety on the release path (M2). Candidate, not proven necessary: no
     incident yet.
3. **Learned per-rung ACK windows.**
   - xara: `hf/timing.rs:90-160`.
   - *Evidence:* turnaround distributions per rung from the twin-OTA and on-air logs, against the
     fixed 4 s / 9 s. Count false timeouts and dead wait per turn.
   - *Release 1:* no. Post-release throughput.
4. **A sender-initiated ACK request in a more robust format on silence.**
   - xara: `hf/session.rs:748-756, 1210-1226`.
   - *Evidence:* a twin-daemon climb-into-silence run, recovery time against `fall_back_after_silence`.
     A wire-format change, so a design review first.
   - *Release 1:* no; v0.17.0's wire format is set.
5. **A waveform-matched spectral signature as a level-free busy test for third-party traffic.**
   - xara: `busy.rs:95-107`.
   - *Evidence:* on the committed IC-9700/FT-991A captures, the carrier/gap statistic for frames
     against idle p99, under AGC FAST.
   - *Release 1:* no; P covers SL7–SL14 for own-session gathering.
6. **A measured, logged TX latency budget.**
   - xara: `audio/txpath.rs:12-47`.
   - *Evidence:* SDR capture of the PTT edge to the first sample against the computed budget on rpi51.
   - *Release 1:* no.
7. **A reproducible-release check with sums in the tag.**
   - xara: `release.yml`.
   - *Evidence:* two clean container builds of v0.17.0 with identical sums.
   - *Release 1:* optional for the M4 cut; not a blocker.
8. **A golden two-station session transcript.**
   - xara: `crates/xara-arq/tests/golden/`.
   - *Evidence:* the transcript must diff on a known ARQ change (sabotage).
   - *Release 1:* no.

Not recommended: a VARA host interface, VARA codebooks or blind keying, or replacing the lockstep
candidate set with xara's widened one.

## Consumer

The maintainer, when triaging post-release work. Items 1–2 may be parked in the work plan's
Release 1 tail (`docs/dev/project/workplan.md`, triage rule 3). Nothing in the tree references xara
yet: `grep -rIil 'xara' docs crates scripts` returned nothing, and the same flags with `vara` list
seven research notes.

## Prior art

- `docs/dev/research/vara-research.md`: VARA from public material.
- `docs/dev/research/varac-feature-gap-analysis.md`: VarAC features.
- `docs/dev/research/acquisition-comparison-codec2-ft8.md`: the same comparison method against codec2
  and FT8.

Found with `ls docs/dev/research/` and `grep -rIil 'vara' docs/dev/research`. Code-level prior art
for each idea is cited inline. Whether any other open VARA implementation exists is UNCHECKED.

## Twins

- **Within this note:** the monitor and the main receiver share one gathering design in xara and
  differ in OpenPulseHF (§8).
- **Notes that make claims about VARA:** `vara-research.md`, and
  `crates/openpulse-modem/tests/harq_rate_selection_watterson.rs:128-190`, which uses a VARA HF peak
  figure as a throughput reference. Found with `grep -rIl 'VARA' crates`.

xara's level table does not contradict that figure's order of magnitude (§3: ~7–8.5 kbps top,
registered). Whether the two describe the same mode is UNCHECKED.
