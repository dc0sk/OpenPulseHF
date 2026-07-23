---
project: openpulsehf
doc: docs/dev/project/changelog.md
status: living
last_updated: 2026-07-22
---

# Changelog

> Phase/roadmap history lives in [roadmap.md](roadmap.md); this file tracks
> user-visible changes. "Unreleased" = merged to `main`, not yet in a tagged release.

## Unreleased

Merged to `main` since v0.16.0; not yet tagged. Two arcs dominate: the **transmit-safety audit
fix-down** (#971–#988) and the **loopback re-validation on real audio** (#989–#1012), which found
several live defects and — at the end — retracted a long-standing misattribution.

### Fixed — on-air tooling currency

- **The on-air scripts were brought current with the CLI**, which had drifted 6–8 weeks behind the
  loopback-revalidation arc. `deploy-rpi-pair.sh` cross-built no-audio binaries that key nothing and
  now refuses by default; `run-onair-tests.sh` called a `send` subcommand and TNC flags that no
  longer exist and is rewired to the real `transmit`/`receive` surface with `--fec`; the two mature
  runners never applied `--fec` (evidence claimed a FEC never used) and now do; receiver-settle
  timings corrected; and `onair-preflight.sh` gained a probe that catches a loopback-only build
  before it wastes an on-air window transmitting nothing.

### Fixed — safety and correctness

- **PTT is released when a keyed ARDOP client disconnects** ([#971]) and the PTT watchdog safety core
  moved out of the daemon so the sibling front-ends share it ([#972]).
- **The repeater honours `full_duplex` before keying** for a whole session ([#973]).
- **Audio capture loss is reported instead of read as a quiet band** ([#979]) — a lost device
  previously produced an unbroken run of successful empty reads, so the station went deaf in silence.
- **Stale QSY sessions expire**, so one spoofed frame cannot disable auto-QSY ([#981]); the rendezvous
  responder is rate-limited per peer ([#982]); inbound file-transfer frames are gated on a valid
  station callsign ([#983]); an unusable station identity key now fails closed ([#985]).
- **Two silent-drop paths closed** — PTT assert failure and ARDOP event lag ([#986]) — and file
  transfer no longer reports a file that was never written or acks an un-accepted transfer ([#987]).
- **The daemon never holds two capture streams open on one device** ([#1007]). The OTA ACK-wait opened
  its own stream while the receive tick's persistent one sat unread; on an exclusive ALSA device the
  second open fails and the ACK is never heard. Closes the last item of #917.

### Fixed — modem and DSP

- **The scanning FEC receive can find a frame inside a capture longer than the frame** ([#995]). It
  sliced a *fixed-length* window, so the demodulated byte count tracked the window rather than the
  frame and the multiple-of-255 gate rejected every attempt before RS ran. This is what had blocked
  `QPSK250-D` — `hpx_hf`'s SL6 fade rung — on real audio.
- **A 60 s flush clamp made `BPSK31` untransmittable** ([#997]) — the ladder's entry rung, and the
  clamp existed for exactly the slow modes it made inert.
- **`long_frame` is classified after the FEC widening, not before** ([#1000]); the full-buffer retry is
  budgeted by scan cost rather than frame geometry ([#1005]); the SC-FDMA timing deramp is enabled for
  the localized block-pilot layout ([#1001]); the plain 8PSK pulse now refuses a rate below 5
  samples/symbol instead of emitting undecodable audio ([#999]); `supports_soft_demod` is per-mode so
  `-D` stops advertising a capability it refuses ([#996]).
- **64QAM tracks its amplitude reference across the frame** ([#1008]). The reference was fitted once
  from the 16-symbol preamble, so nothing tracked a level that moves *during* a frame — measured
  noiselessly, gain wander is an order of magnitude more damaging than the same fractional phase
  wander at slow rates. Scoped honestly: validated in-process, **not** on hardware.
- **cpal enumeration truncates when devices are retained** ([#998]) — the listing advertised devices
  that could not be opened, which made the virtual-loopback rung unrunnable.

### Added

- **Phase G0 idle-floor gate** (`scripts/onair-rx-idle-floor.{sh,py}`) — a runnable check that a rig's
  receive USB-audio floor is clean of the conducted-RFI birdies that blocked every prior rig-to-rig
  link, gain-independent (prominence-based) and validated against synthetic captures before use.
- **On-air execution plan** (`docs/dev/onair-execution-plan.md`) — the sequenced 1.0 group-A campaign,
  grounded in the recorded ground truth that the modem, waveforms, decoder and transmitters are
  already SDR-proven on real RF, and the rig→rig blocker is conducted USB-audio RFI (fix = galvanic
  isolation), not a modem defect. Critical path: kill the RX RFI → signal-chain gates → one rig→rig
  decode → ladder on a real fade → Winlink over RF.
- **Per-subcarrier EVM measured before the DFT de-spread** ([#1009], `scfdma_subcarrier_evm_db`).
  SC-FDMA's IDFT averages every subcarrier into every output symbol, so a post-despread measurement
  cannot separate a narrowband impairment from a broadband one. Diagnostic only.

### Changed — a retraction worth reading

- **The "analog path" mode classification is withdrawn** ([#1010], [#1011], [#1012]). Eight modes had
  been recorded as limited by the dual-card rig's analog path. Re-run on a rig whose setup script had
  been re-applied, **six of the eight pass on `main` with no code change**. The comparison had been
  measuring a **live capture AGC** — unplugging the USB adapters resets their mixers — which moves the
  level *during* a frame and so destroys exactly the amplitude-carrying modes. Ablated directly:
  `SCFDMA52-16QAM` and `-32QAM` each FAIL 2/2 with the AGC on and PASS 2/2 with it off.
  `run-loopback-dualcard.sh` now **refuses to sweep** while any card's AGC is live, and
  `setup-dualcard-loopback.sh` reads the setting back instead of printing an unverified claim.
- **`hpx_wideband_hd` SL14 (`SCFDMA52-64QAM`) is documented as marginal** (3/5 on a 71 dB-SNR cable);
  `64QAM2000-RRC` is preferred where bandwidth allows. `SCFDMA52-64QAM-P4` remains in no profile.

[#971]: https://github.com/dc0sk/OpenPulseHF/pull/971
[#972]: https://github.com/dc0sk/OpenPulseHF/pull/972
[#973]: https://github.com/dc0sk/OpenPulseHF/pull/973
[#979]: https://github.com/dc0sk/OpenPulseHF/pull/979
[#981]: https://github.com/dc0sk/OpenPulseHF/pull/981
[#982]: https://github.com/dc0sk/OpenPulseHF/pull/982
[#983]: https://github.com/dc0sk/OpenPulseHF/pull/983
[#985]: https://github.com/dc0sk/OpenPulseHF/pull/985
[#986]: https://github.com/dc0sk/OpenPulseHF/pull/986
[#987]: https://github.com/dc0sk/OpenPulseHF/pull/987
[#995]: https://github.com/dc0sk/OpenPulseHF/pull/995
[#996]: https://github.com/dc0sk/OpenPulseHF/pull/996
[#997]: https://github.com/dc0sk/OpenPulseHF/pull/997
[#998]: https://github.com/dc0sk/OpenPulseHF/pull/998
[#999]: https://github.com/dc0sk/OpenPulseHF/pull/999
[#1000]: https://github.com/dc0sk/OpenPulseHF/pull/1000
[#1001]: https://github.com/dc0sk/OpenPulseHF/pull/1001
[#1005]: https://github.com/dc0sk/OpenPulseHF/pull/1005
[#1007]: https://github.com/dc0sk/OpenPulseHF/pull/1007
[#1008]: https://github.com/dc0sk/OpenPulseHF/pull/1008
[#1009]: https://github.com/dc0sk/OpenPulseHF/pull/1009
[#1010]: https://github.com/dc0sk/OpenPulseHF/pull/1010
[#1011]: https://github.com/dc0sk/OpenPulseHF/pull/1011
[#1012]: https://github.com/dc0sk/OpenPulseHF/pull/1012

## v0.16.0 — 2026-07-18

A documentation and test-integrity release. Two new documents ship with it — a full technical **book**
and a rewritten **operator manual** — and a documentation audit corrected a long tail of claims that
had outrun the code. The modem itself is unchanged: no wire-format change, no config change, and a
v0.16.0 station interoperates with v0.15.0 exactly as before.

The version is a **minor** bump for one small reason, recorded honestly: `MockTransport.write_log` in
`openpulse-radio` changed type. Everything else is additive or documentation.

### Breaking changes

- **`openpulse-radio`: `MockTransport.write_log` is now `Arc<Mutex<Vec<u8>>>`** (was `Vec<u8>`), with a
  new `MockTransport::log_handle()` accessor. The field is documented as "inspectable by tests", but
  once the transport was boxed into a `GenericSerialCat` the log went with it and no test could reach
  it — which is why five `*_sends_correct_bytes` tests never checked any bytes. Sharing the log fixes
  that. ([#957](https://github.com/dc0sk/OpenPulseHF/pull/957))

### Features

- **The OpenPulseHF book** (`docs/openpulse-book.md`, 4414 lines) — a complete technical account for
  licensed operators, electronic engineers and software developers at once: abstract, per-audience
  reading routes, the waveform catalogue and rate ladders, the physics and DSP with their governing
  mathematics, cryptography and trust, the software architecture, and twelve copy-pasteable use-case
  scenarios. ([#964](https://github.com/dc0sk/OpenPulseHF/pull/964))
- **1.0 release criteria** (`docs/dev/project/release-1.0-criteria.md`) — a draft definition of what
  1.0 asserts, with explicit non-goals and four open questions for the maintainer. "Pre-1.x" was
  previously undefinable. ([#960](https://github.com/dc0sk/OpenPulseHF/pull/960))
- **`--profile` now validates at parse time** and lists every accepted profile. The help previously
  advertised 7 of the 12 profiles `SessionProfile::by_name` accepts; the list is now sourced from
  `PROFILE_NAMES` and cannot drift. Case-insensitive input with interchangeable `-`/`_` still works.
  ([#963](https://github.com/dc0sk/OpenPulseHF/pull/963))

### Fixes

- **The operator manual is current again** and documented three CLI commands that do not exist
  (`openpulse relay status|routes|policy`), showed unrelated commands in its Winlink section, and
  claimed 8 plugin families where there are 10. It now leads with the v0.15.0 both-ends interop
  notice. ([#962](https://github.com/dc0sk/OpenPulseHF/pull/962))
- **Six vacuously-passing test gates now test what they are named for** — including the FF-7 TX
  limiter gate, which never inspected an amplitude, and five CAT tests that never inspected the bytes
  they were named for. ([#957](https://github.com/dc0sk/OpenPulseHF/pull/957))
- **The acceptance-criteria table is runnable.** Three rows could not execute as written (cargo takes
  one positional test filter), and two named tests that never called the receive path they claimed to
  gate. ([#955](https://github.com/dc0sk/OpenPulseHF/pull/955))
- **`profile.rs`'s own `hpx_hf` comment table was 3–10 dB wrong on every OFDM rung** and is now gated
  against the executable floors. ([#961](https://github.com/dc0sk/OpenPulseHF/pull/961))
- **The Type C/LZHUF removal is complete.** README still advertised "Winlink Type C wire-compatible"
  148 lines below its own retraction of that claim, and the root manifest still declared the
  dependency. ([#954](https://github.com/dc0sk/OpenPulseHF/pull/954))
- **A reversed CE-SSB conclusion was still documented as current** — the gate excludes dense OFDM-HOM
  and all SC-FDMA, and the test cited to lock the old claim now asserts the opposite.
  ([#958](https://github.com/dc0sk/OpenPulseHF/pull/958))
- **`CLAUDE.md` said the Watterson envelope FFT is capped at 2^18**; `fading.rs` sets `1 << 16`.
  ([#964](https://github.com/dc0sk/OpenPulseHF/pull/964))
- **`mode-fec-ladder.md` contradicted itself on `RsInterleaved`**, billing it "best for HF
  burst/fading" in §2 while §7 recorded the measurement that it is inert on a single-block payload.
  ([#963](https://github.com/dc0sk/OpenPulseHF/pull/963))

### Documentation

- Status trackers consolidated: `backlog.md` listed two shipped subsystems as open work and omitted
  the only two genuinely open items. ([#956](https://github.com/dc0sk/OpenPulseHF/pull/956))
- A reproducible SBOM (`SBOM.spdx.json` + `scripts/generate-sbom.sh`).
  ([#953](https://github.com/dc0sk/OpenPulseHF/pull/953))

### Known limitations

- The v0.13.0 → v0.16.0 HF-fade work remains validated against the Watterson channel simulator only.
  On-air validation is the first gate in the new 1.0 criteria and has not been passed.
- Winlink Type C (LZHUF) is unsupported.

## v0.15.0 — 2026-07-18

Hardening release. A multi-agent audit of the Winlink network stack (`openpulse-b2f`,
`openpulse-b2f-driver`, `openpulse-gateway`) found the stack broadly solid — no panics, no
memory-safety bugs, no auth bypass — but turned up one live denial-of-service and a cluster of ways an
untrusted or broken peer could hang or balloon the process. All of them are fixed here, along with the
deletion of a Winlink Type C code path whose advertised compatibility was never real.

**Interop:** one change affects the air interface. A v0.15.0 station opportunistically strengthens the
FEC on small frames (see below), and a pre-v0.15.0 receiver does not know to try that code — so a
v0.15.0 → older link can lose small frames on the Rs rungs. **Update both ends.** Everything else in
this release is host-side (TCP/Winlink) with no wire impact.

### Breaking changes

- **`openpulse-b2f`: the Type C (LZHUF) codec is gone.** `compress_lzhuf`, `decompress_lzhuf`,
  `compress_lzhuf_winlink`, `decompress_lzhuf_winlink`, `decompress_lzhuf_compat` and
  `B2fSession::queue_message_type_c` are removed, along with the `oxiarc-lzhuf` dependency. Its
  external-Winlink compatibility was never verified and could not have held: this used LHA `LH5`,
  while FBB/Winlink use the classic Okumura LZHUF — a *different bitstream*. An inbound Type C
  proposal is now answered `Reject`, an honest "cannot decode this" that leaves the peer free to
  re-propose as Type D (Gzip). The CMS gateway path is unaffected; it has always used Type D.
  ([#948](https://github.com/dc0sk/OpenPulseHF/pull/948))
- **`openpulse-b2f-driver`: `CmdPort::set_timeout` / `DataPort::set_timeout` now take `&mut self`**
  (they record the deadline as well as setting the socket option), and `DriverError` gains
  `AllProposalsRejected { count }`. `B2fDriver::run_iss` now returns an error instead of `Ok(())` when
  the peer rejects every proposal. ([#947](https://github.com/dc0sk/OpenPulseHF/pull/947),
  [#950](https://github.com/dc0sk/OpenPulseHF/pull/950))

### Features

- **Free FEC strengthening on the weak rungs.** `RsStrong` (t=32) roughly doubles the weak BPSK rungs'
  fading decode (BPSK31 @3 dB: 0.25 → 1.00) and costs nothing on the wire for small frames — the same
  255-byte RS block as `Rs`. The sender now upgrades `Rs` → `RsStrong` per frame **only when it costs
  no extra RS block**, so airtime can never regress, and the receiver tries both codes with the CRC
  disambiguating. ([#941](https://github.com/dc0sk/OpenPulseHF/pull/941))

### Fixes

- **Denial of service: unbounded proposal retention and a non-terminating FC flood.** The IRS proposal
  cap gated the *answer* rather than the *push*, so a peer streaming FC frames grew a `Vec` without
  limit, and a flood that never sent FF spun the receive loop forever. Retention is now bounded by an
  overflow count and a per-session frame ceiling.
  ([#943](https://github.com/dc0sk/OpenPulseHF/pull/943))
- **Memory amplification: no aggregate decompression cap.** Each message was capped at 16 MiB and the
  count at 32, but never their product — ≈ 512 MB of transient allocation from ~2 MB of wire, a
  plausible OOM on the Pi target. A running total is now enforced at the shared seam both the driver
  and the gateway call. ([#945](https://github.com/dc0sk/OpenPulseHF/pull/945))
- **The command port could be grown without limit.** A TNC that never sends a newline drove the
  client's memory. The server side already had this fix; the client-side twin did not.
  ([#946](https://github.com/dc0sk/OpenPulseHF/pull/946))
- **Read timeouts were per-syscall, not per-operation.** `SO_RCVTIMEO` restarts on every partial read,
  so it bounded the gap between bytes and never the operation — a peer sending one byte per interval
  held a read open indefinitely. Reads now run against a single deadline. `run_irs` also cleared its
  command-port timeout and never restored it, leaving session teardown able to hang forever; ports now
  carry a default timeout and the prior value is restored.
  ([#947](https://github.com/dc0sk/OpenPulseHF/pull/947))
- **Header fields could amplify.** `To:` and `File:` accumulated without a per-field cap. They are now
  bounded — and *rejected* rather than truncated, since silently dropping recipients would deliver a
  message to fewer addressees than it names. ([#949](https://github.com/dc0sk/OpenPulseHF/pull/949))
- **A fully-rejected transfer reported success.** `run_iss` returned `Ok(())` when the peer rejected
  every proposal, reporting "sent" for messages that never left the queue — while the gateway had
  always treated that as a failure. A refused CONNECT is also now reported immediately as `Aborted`
  rather than surfacing as a timeout 60 s later.
  ([#950](https://github.com/dc0sk/OpenPulseHF/pull/950))

### Documentation and tests

- **The rate ladder's per-waveform-family SNR scales are documented as deliberate and gated.** Single-
  carrier PSK reports ~true channel SNR; OFDM/SC-FDMA report a saturation-bounded plugin-domain SNR
  (their equaliser enhances noise on faded subcarriers, so the estimate flattens near ~16 dB and
  cannot report the 20–30 dB the top rungs run at). Unifying the two would put the dense rungs' floors
  above anything the estimate can read — the v0.14.0 stall. A new gate fails if OFDM starts tracking
  true SNR without the floors being re-derived in the same change.
  ([#944](https://github.com/dc0sk/OpenPulseHF/pull/944))
- **Adversarial coverage for the Winlink stack**: `DataPort` framing edges, malformed
  banner/frame/header input, non-UTF8, tamper and truncation on an accepted blob, silent-peer
  timeouts, and hostile-CMS gateway cases. ([#951](https://github.com/dc0sk/OpenPulseHF/pull/951))
- Roadmap tables reconciled against the code and gated; the HF-fade release arc recorded as RF-6; the
  missing controller-side fade gate added. ([#938](https://github.com/dc0sk/OpenPulseHF/pull/938),
  [#939](https://github.com/dc0sk/OpenPulseHF/pull/939),
  [#940](https://github.com/dc0sk/OpenPulseHF/pull/940))

### Known limitations

- The v0.13.0 → v0.15.0 HF-fade work is validated against the Watterson channel simulator only.
  On-air validation with real radios remains outstanding.
- Winlink Type C (LZHUF) is unsupported. Restoring it requires a captured RMS Express / RMS Gateway
  Type C blob to validate the bitstream and length-prefix convention against.

## v0.14.1 — 2026-07-17

Patch release: the fade-aware ladder from v0.14.0 fixed the *rungs*, but the rate controller still
could not drive them there. On a routine moderate HF fade `hpx_hf` sat pinned on its entry rung at
**~5 bps while delivering every frame** — now it climbs into the OFDM rungs. Everything here is
receiver-side rate-control and SNR-estimation; no wire-format, config, or API change, and nothing to
do to upgrade. Two v0.14.0 stations and a v0.14.1 station interoperate exactly as before.

| `hpx_hf` on Watterson `moderate_f1` @20 dB | v0.14.0 | v0.14.1 |
|---|---|---|
| mean speed level reached | 1.5 (pinned near SL1) | **4.9** |
| final level | SL1 (MFSK16 sub-floor) | **SL11 (OFDM)** |

### Fixes

- **The rate ladder now climbs on decode evidence, not only on an SNR estimate.** The controller had
  exactly one way up — the measured SNR clearing a rung's ceiling — and on a fade the SNR estimate is
  uninformative *in principle* (at 31 baud a 1 Hz Doppler fade decorrelates faster than any usable
  averaging window), so the link stayed on its entry rung even while **every frame decoded**. Worse, a
  decoded frame with a low SNR reading was answered by dropping *below* the rung that had just
  decoded. The controller now climbs after three consecutive clean decodes regardless of the SNR
  reading, and never demotes on a frame that decoded — a decode is direct proof the rung works, so the
  fast-downshift belongs only on an actual failure, where the SNR estimate genuinely explains
  something. ([#936](https://github.com/dc0sk/OpenPulseHF/pull/936))
- **BPSK gained an SNR estimator that works on a fade.** BPSK had none and fell back to a
  constant-modulus moment estimator that a fade defeats — it read a flat ≈ −4 dB from 15 dB of true
  SNR to 35 dB, the same number across 20 dB of channel, and `hpx_hf`'s four weak-signal rungs are all
  BPSK. The new estimate removes the multiplicative fade with a per-window gain before measuring the
  residual, so it tracks the channel again. ([#935](https://github.com/dc0sk/OpenPulseHF/pull/935))
- **The link simulator can now transmit the sub-floor rung.** `openpulse-linksim` never registered the
  MFSK16 plugin, so once a ladder demoted to its SL1 sub-floor the sim could not transmit at all and a
  fading run read as a total link failure — a harness artifact, not modem behaviour. Every fade
  measurement the simulator produced for `hpx_hf` before this was suspect. ([#935](https://github.com/dc0sk/OpenPulseHF/pull/935))

### Notes

- The rate controller's policy shifted slightly toward throughput: it will now probe one rung above
  what a conservative SNR estimate permits, since a rung's advertised floor carries a fading margin
  and coded rungs decode below it on a clean channel. The climb is self-correcting — a rung that
  starts failing is dropped on the failure — and cannot run away into the dense high-throughput rungs
  a poor channel could not carry.

## v0.14.0 — 2026-07-17

Minor release: the HF rate ladder is now calibrated for a **fading** channel instead of a clean one.
Minor rather than patch because the ladder's rungs, their FEC and their numbering all change on air —
see the interop note below.

Measured on Watterson `moderate_f1` (1 Hz Doppler, 1.0 ms delay — a routine ITU-R moderate HF path),
each rung **at its own advertised SNR floor**:

| rung | before | after |
|---|---|---|
| **SL2 `BPSK31` @3 dB** — the rung every session *starts* on | **0.00** | **0.25** |
| SL3 `BPSK63` @4 dB | 0.00 | **0.83** |
| SL3 `BPSK63` @7 dB | 0.00 | **1.00** |
| the SL7–SL10 mid rungs | ~0 at *any* SNR | replaced with OFDM |

### Fixes

- **The weak-signal rungs now decode on a fade.** `hpx_hf`'s SL2–SL5 (BPSK31/63/100/250) shipped
  **uncoded**, and its SNR floors came from AWGN sweeps — so on a fading channel they decoded ~0 % of
  frames at the floors they advertised. Worst of all, SL2 is `initial_level`: **every session starts
  there, and uncoded it decoded nothing at 3, 6 or 9 dB**, so a fading link could not reliably get
  going at all. All four rungs are now Reed-Solomon coded. This is the same law
  [#923](https://github.com/dc0sk/OpenPulseHF/issues/923) established — *differential encoding needs
  FEC* — applied to the whole ladder, since BPSK is differentially decoded too.
- **The dead mid-ladder is gone.** `QPSK250`/`QPSK500` (uncoded) and `8PSK500` decoded ~0 % on that
  fade at **any** SNR up to 40 dB, and none is rescuable: FEC does not help (the defect is carrier
  tracking, not errors), and differential encoding does not scale to 8PSK's ±22.5° margin. Above SL6
  the ladder is now OFDM, whose cyclic prefix rides the delay spread and whose per-subcarrier pilots
  track the fade — `OFDM52` decodes 0.58/0.75/0.83 at 8/12/16 dB where `8PSK500` decodes 0.00 at all
  three. The ladder is 14 rungs instead of 17, and every rung is measured to work on a fade.
- **The dense OFDM rungs are reachable again.** The ladder compares SNR floors against the receiver's
  *plugin symbol-domain* SNR, which for OFDM is conservative and saturates near ~17 dB — a true 20 dB
  link reads ~14.4. `hpx_hf`'s OFDM floors were AWGN-scale numbers, so those rungs could never be
  climbed to; a strong link stalled below them. They are re-based on the same plugin-scale calibration
  `hpx_ofdm_hf` already used, and a 35 dB channel now climbs to the top of the ladder.

  > **On-air interop:** SL2–SL5 gain FEC, the rungs above SL6 change waveform, the rungs re-index, and
  > the ladder fingerprint changes accordingly. **Both ends must run v0.14.0** to share a ladder. No
  > config or API change.

### Known limitations

- The single-carrier and OFDM rungs' SNR floors are on **different scales** (true-SNR vs plugin
  symbol-domain SNR). Both are now correct for their own rungs and documented in
  `docs/mode-fec-ladder.md`, but the mismatch itself is unresolved.
- `RsStrong` (RS(255,191)) is roughly twice as good as `Rs` on a fade at the weak rungs and costs
  **nothing** for payloads ≤191 B — but at 192–223 B it needs a second RS block and doubles the
  airtime, which costs more clean-channel throughput than the fade robustness is worth ladder-wide.
  It remains the better code for a rung whose frames are known to stay under 191 B.
- The OTA ACK-wait opens a second capture stream on the cpal (real-audio) backend; loopback hides it.
  Parked pending hardware. Tracked in [#917](https://github.com/dc0sk/OpenPulseHF/issues/917).

## v0.13.0 — 2026-07-17

Minor release: the HF rate ladder's mid rung now works on a fading channel. Minor rather than patch
because **SL6's on-air waveform changes** — see the interop note below.

Measured on Watterson `moderate_f1` (1 Hz Doppler, 1.0 ms delay), `hpx_hf` SL6, Rs FEC:

| SL6 @ 20 dB | decode rate |
|---|---|
| before (`QPSK250`, coherent) | 0.000 |
| after (`QPSK250-D`, differential) | **0.650** |

### Fixes

- **The `hpx_hf` SL6 rung now decodes on a fading HF channel.** It was `QPSK250 + RS`, which decoded
  **0% on a Watterson `moderate_f1` fade at every SNR up to 40 dB** — a coherent, absolutely
  phase-encoded waveform cannot hold a carrier reference through a 1 Hz Doppler fade, so a cycle slip at
  a fade null ruins the rest of the frame. SL6 is now **`QPSK250-D`**, a differential (DQPSK) waveform
  that encodes each dibit as a phase *increment*, so the fade rotation cancels between adjacent symbols
  and a slip costs one dibit instead of the frame tail — the same immunity BPSK has always had. Measured
  on `moderate_f1` at 20 dB: **0.00 → 0.65**, which makes the rung out-throughput the BPSK250 rung below
  it (284 vs 237 effective bps) instead of being dead weight. The AWGN cost is ~2 dB at the extreme floor
  only (both decode 100% by 4 dB, well under SL6's ~7 dB operating point). New selectable modes
  `QPSK250-D` / `QPSK500-D`. ([#923](https://github.com/dc0sk/OpenPulseHF/issues/923))

  > **On-air interop:** this changes the waveform SL6 transmits. Both ends must run this version to use
  > SL6; the ladder fingerprint changes accordingly. No config or API change.

### Known limitations

- The sibling coherent rung `8PSK500 + RS` (`hpx_hf` SL9) has the same fade-fragility and is unchanged.
  Differential 8PSK was prototyped and **measured — it does not rescue it**: `8PSK500-D` reaches only
  0.125 on `moderate_f1` even at 40 dB, and costs ~4–6 dB of AWGN floor (vs QPSK's ~2 dB), because
  differential detection roughly doubles the effective noise and 8PSK's ±22.5° margin cannot absorb it.
  Robustness tracks phase margin; SL9 needs a different mechanism, not `-D`. The rate adapter steps down
  to SL6, which now works.
- The coherent uncoded rungs (SL7/SL8) are high-SNR rungs by design: differential encoding requires FEC
  to correct the dibit a slip costs, so it cannot rescue them.

## v0.12.2 — 2026-07-17

Patch release: the weak-signal receive path. HARQ soft-combining across retransmissions was in place but
throwing away most of its own gain, and the weak-signal sub-floor rung was excluded from it entirely. All
receiver-side — no wire-format, config, or API change, and nothing to do to upgrade.

Measured on a Watterson `moderate_f1` fade at 10 dB, SL12 `OFDM52-16QAM`, 400 fade realisations
(a single burst decodes 0.582 of the time):

| | decode rate |
|---|---|
| before | 0.840 |
| after | **0.957** |

### Fixes

- **HARQ combining no longer discards retransmissions whose soft-data length differs.** The combiner only
  accepted retained soft-decision data of *exactly* the current burst's length — but a faded demodulator
  recovers a varying symbol count for the same frame (576–4320 observed on one frame), so genuine
  retransmissions were being dropped and their diversity thrown away, having already been paid for in
  airtime. Retained data is now aligned onto the current burst's grid (truncated if longer, zero-padded if
  shorter, which is the correct "this burst never recovered that symbol"). Worth **+0.117** decode rate.
  (#921)
- **An abandoned message's soft data no longer dilutes the next message's combine.** Retained data was
  isolated by length only, and the intended per-session guard never fired on the daemon (it pins the
  session id to the local callsign), so two consecutive same-length messages — fixed-size SAR fragments
  are exactly this — combined a stale message's data into the live one. No false delivery (RS/CRC still
  gated), but it cost **-0.067** of the diversity gain HARQ exists to provide, about a quarter of it.
  (#920)
- **`openpulse session --diagnostics` reports a real `afc_offset_hz`.** The field was serialized but never
  written, so the JSON always carried `"afc_offset_hz": null` while the engine held the value all along.
  Same schema; the value is simply no longer a constant null after a demodulation. (#926)

### Features

- **HARQ combining now covers the plain-RS rungs, including the MFSK16 weak-signal sub-floor** — worth
  about **2.5 dB** there. At -4 dB the sub-floor's decode rate goes 0.117 → 0.750, and at -6 dB it
  recovers frames that no single transmission ever does. This is the rung a link falls back to under
  sustained failure, so the gain lands where there is nothing else left. It was held out because every
  MFSK16 frame is one fixed-size block, so an abandoned message's data could not be told apart from a
  retransmission — worst case, delivering the wrong message. The fix in #920 contains that hazard by
  construction; it is gated by tests for both zero dilution and zero false deliveries. (#922)

### Tests

- OTA receive and HARQ are now exercised through the daemon's **production** capture entry
  (`accumulate_capture`, tick-sized chunks, DCD-decided burst boundaries) rather than only the
  direct-decode seam — confirming the diversity gain reaches the real receive path (0.633 → 0.967). (#924)
- Closed the audit's deferred test-coverage gaps: FSK4-ACK now has a **correct-decode-under-noise** gate
  (its only noise test asserted the ACK *breaks*, and its stated -16 dB rationale was wrong by ~14 dB
  against measurement); the MFSK16 sub-floor ACK is tested with authentication enabled; and the SC-FDMA
  acceptance row now cites the test that actually asserts instead of an `#[ignore]`d zero-assertion
  harness. (#925)

### Known limitations

- The `QPSK250 + RS` rung of the `hpx_hf` ladder does not decode on a Watterson `moderate_f1` fade at any
  SNR (0.033 even at 40 dB). Diagnosed to carrier phase tracking under fading — not ISI, not noise. The
  rate ladder steps off the rung, so this costs throughput rather than correctness. Tracked in
  [#923](https://github.com/dc0sk/OpenPulseHF/issues/923).
- The OTA ACK-wait opens a second capture stream on the cpal (real-audio) backend; loopback hides it.
  Parked pending hardware. Tracked in [#917](https://github.com/dc0sk/OpenPulseHF/issues/917).

## v0.12.1 — 2026-07-16

Patch release: internal fixes from the first formal loose-ends audit of the `openpulse-modem` crate
(the core modem engine / ARQ / OTA rate ladder / DSP). The audit found the core broadly solid; these
are the top confirmed items. No wire-format or config change.

### Fixes

- **The daemon control loop no longer freezes when a peer goes silent mid-send.** An OTA send to a peer
  that stopped answering retried through its full budget of ACK windows (up to ~36 s on the MFSK16
  sub-floor) synchronously in the daemon's control loop, blocking `Abort`/`Disconnect` and data RX. It now
  abandons after two consecutive silent ACK windows (a NACKing — i.e. present — peer still gets the full
  retry budget). (#918)
- **The OTA candidate-decode loop no longer re-runs the receiver front-end per candidate.** With the notch
  or AGC enabled, each rate candidate re-applied the notch/AGC/DCD, which could advance the notch
  persistence counter and prematurely trigger an auto-QSY. (#914)
- **The daemon no longer logs "OTA aggressiveness preset applied" for knobs it doesn't use.**
  `ota_aggressiveness` / `ota_min_backlog` / `ota_upgrade_hold_frames` configure a rate policy the daemon's
  receiver-led ladder doesn't run; it now warns instead of falsely confirming. Bound the OTA rate with
  `ota_min_level` / `ota_max_level` / `ota_lock_level`. (#915)
- **Consistent hard-decision LLR rule** across the decode paths (they disagreed only at exactly +0.0), and
  a corrected internal DSP note. (#916)

### Known limitations (tracked)

- Deferred modem-audit findings — HARQ cross-message LLR isolation, a cpal-only second-capture-stream
  buffering, and some test-coverage gaps — are tracked in GitHub issue #917. Full report:
  `docs/dev/reviews/2026-07-16-modem-loose-ends-audit.md`.

## v0.12.0 — 2026-07-16

Closes the last two implementable findings from the handshake-trust audit: the OTA rate ACK is now
cryptographically authenticated, and the verified-peer store is consolidated to a single per-callsign
source of truth. Minor bump for the new signed handshake field and the ACK behaviour change; no config
break.

### Security fixes

- **The OTA rate-control ACK is now authenticated (audit E7).** The tiny FSK4 rate ACK carried no
  authentication — its only filter was a 16-bit hash of the `session_id`, which travels in the cleartext
  connection request, so any listener could forge ACKs and manipulate a link's rate ladder. The handshake
  now performs an ephemeral **X25519** key agreement (its public keys ride inside the Ed25519-signed
  CONREQ/CONACK, so a man-in-the-middle can't substitute them), and the ACK carries a **keyed MAC** derived
  from the shared secret. The MAC reuses the existing 5-byte ACK layout, so there is **no change to the ACK
  waveform or airtime**. This is authentication, not encryption — the ACK content stays in the clear
  (§97.309 compatible; see `docs/regulatory.md`). (#911)
- **Verified peers are a single per-callsign source of truth (audit E5).** File-transfer offer
  verification already bound to the offer's true sender (v0.8.0); this removes the redundant global
  "most-recently-handshook" slot so all verified-identity reads go through the authoritative per-callsign
  map. (#912)

### Behaviour changes

- When both stations advertise a key-agreement key in the handshake, OTA rate ACKs are authenticated and a
  forged or foreign-key ACK is dropped. Legacy peers that don't advertise one keep the unauthenticated ACK
  (no interop break). (#911)

## v0.11.0 — 2026-07-16

Two more deferred security-audit fixes on the signed-handshake path: replay-freshness and SAR-poison
resilience. Minor bump for the handshake behaviour change (below); no wire-incompatible change beyond a
new signed timestamp field, and no config break.

### Security fixes

- **Handshake replay-freshness.** CONREQ/CONACK carried no timestamp, so a captured, validly-signed
  handshake could be replayed indefinitely. Both frames now carry a **signed `timestamp_ms`**, and the
  verifier rejects a frame that is stale, future-dated, or timestampless beyond a clock-skew window (the
  daemon uses ±120 s). Because the timestamp is inside the signed body, an attacker cannot refresh a
  captured frame. (#908)
- **SAR reassembly poison resilience.** The handshake reassembled every frame under a single constant key,
  so one crafted fragment could seed a bogus fragment count (legitimate fragments then bounced) or fill an
  index with garbage (the real fragment was dropped as a duplicate) — blocking a legitimate handshake for
  the whole reassembly timeout. Two genuine handshakes interleaving on the same key could also poison each
  other by accident. The reassembler now keeps conflicting fragment streams as **separate candidates**, so
  a poisoned or interleaved fragment cannot corrupt an in-flight reassembly; the bogus candidate fails
  verification and is dropped while the good one completes. (#909)

### Behaviour changes

- **The daemon rejects a handshake with no timestamp or one outside ±120 s.** Both ends stamp a current
  time, so an upgraded pair interoperates; a peer running an older (timestampless) build is rejected.
  Assumes both stations keep roughly correct wall-clock time. (#908)

## v0.10.0 — 2026-07-16

Envelope origin authentication for relayed traffic — the last major deferred finding from the
handshake-trust audit series (E3). A relay now cryptographically authenticates the originator of every
relay-data frame it forwards, so a station can no longer impersonate another originator at a relay. Minor
bump for the control-plane wire-format change (envelope v2) and the behaviour change (relays reject
unsigned/forged relay frames); no config break.

### Security fixes

- **A relay no longer forwards forged or unauthenticated relay traffic (audit E3).** The control-plane
  envelope's 16-byte `auth_tag` was never verified and had no key-distribution scheme, so any station could
  forward a frame claiming any originator (`src_peer_id`) — impersonation at the relay. Because a peer id
  *is* its Ed25519 verifying key, the envelope now carries an optional Ed25519 origin signature verifiable
  against `src_peer_id` with no external key store, and a relay drops any relay-data / hop-ack frame whose
  signature is absent or invalid (on by default). The originator allow-list from v0.9.0 now sits on top of
  real cryptographic authentication rather than a spoofable id. (#906)

### Behaviour changes

- **Relays reject unsigned or forged relay frames by default.** A `relay_data_chunk` / `relay_hop_ack`
  without a valid origin signature is dropped. The only relay-frame originators in the project (mesh nodes)
  now sign the frames they originate, so normal traffic is unaffected; there is no opt-out. (#906)

### Breaking changes

- **Control-plane wire schema → v2** (the `OPHF` mesh/relay/peer-query/route-discovery envelope). The fixed
  16-byte `auth_tag` is replaced by an *optional* 64-byte Ed25519 signature (present only on signed
  relay-data frames). v1 and v2 envelopes are **not interoperable**. This affects only the mesh / relay /
  discovery control plane, all of which are off by default. (#906)

### Resolved limitations

- The v0.9.0 "envelope-level authentication of relayed traffic" limitation is **resolved**. It was expected
  to require a mesh reception-model change (to carry always-signed, therefore fragmented, envelopes);
  instead the signature was made *optional* — only relay-data frames are signed, so authenticated frames
  stay within one modem frame and the control responses that carry their own payload-level signatures stay
  compact and unfragmented. (#906)

## v0.9.0 — 2026-07-16

Second security release in the audit series — a fresh RX-decode and protocol-bridge sweep plus follow-ups.
Fixes one **CRITICAL** remote-panic DoS on the receive path, hardens the network-facing protocol bridges,
and lands the relay originator allow-list. The minor bump reflects the file-offer signature wire-format
change (below) and the new relay config; no config break.

### Security fixes

- **A crafted transmission could crash the receiver (CRITICAL, audit RX-1).** The short-FEC decode path
  (used by the OTA acknowledgement listener and short-FEC receive) passed attacker-length-controlled
  demodulator output to a Reed–Solomon decoder backed by a fixed 256-byte buffer, which **panics** on any
  input ≥ 256 bytes. Any station on-air could be crashed by transmitting enough audio. The decoder now
  rejects over-length input before it can panic. (#903)
- **Two more receive-path panics on 32-bit / WASM builds (audit RX-2/RX-3).** Length-prefix arithmetic in
  the FEC / convolutional / soft-Viterbi decoders could overflow a 32-bit index on a crafted frame and
  panic; now uses checked arithmetic. (#903)
- **The SAR reassembler is now bounded (audit RX-4).** A sender flooding distinct, never-completed segments
  could grow reassembly memory unbounded; capped at a fixed number of pending segments. (#903)
- **The ARDOP TNC could be driven out of memory (audit A-1).** A client that streamed bytes with no newline
  grew the command buffer without limit; the read is now length-bounded. (#901)
- **The Winlink gzip decompressor had no size cap (audit B-1).** A decompression bomb from a malicious CMS
  could allocate without bound (the LZHUF path was already capped); gzip is now capped too. (#901)
- **Unbounded B2F proposals (audit B-2).** A peer could make the receiver accept, decompress, and retain an
  unbounded number of messages per session; capped. (#901)
- **A signed file offer's metadata is now covered by its signature (audit F-2).** Previously only the
  content hash, size, and sender were signed; the filename, MIME hint, and block geometry rode along
  unauthenticated, so an on-path attacker could replay a legitimately-signed offer with a spoofed filename
  while it still showed as signature-valid. The offer now carries its own signature over the whole offer.
  (File content was already protected by the signed hash.) **Wire-format change** to what the offer
  signature covers; direct file transfer is off by default. (#900)

### Features

- **Relay originator allow-list (audit E1).** `[relay] allow_list` restricts an enabled relay to forwarding
  only frames from listed originator peer IDs (alongside the existing deny-list) — a defense-in-depth
  control for scoping a club/mesh relay to known stations. (#902)
- **The mesh can now carry control responses larger than one modem frame** by SAR-fragmenting and
  reassembling them. Transparent to current traffic; groundwork for future signed control messages. (#904)

### Known limitations (tracked)

- Envelope-level authentication of relayed traffic (route/query floods) remains future work: the modem's
  255-byte frame cap means a signed envelope must fragment, which needs a mesh reception-model change.
  Documented in `docs/dev/reviews/2026-07-15-handshake-trust-audit.md` (finding E1/E3).

Full audit write-ups: `docs/dev/reviews/2026-07-15-rx-decode-audit.md`,
`docs/dev/reviews/2026-07-15-protocol-bridge-audit.md`,
`docs/dev/reviews/2026-07-15-filexfer-relay-seams-audit.md`.

## v0.8.0 — 2026-07-15

Security release folding in three back-to-back adversarial audits of the handshake/trust, session, and
file-transfer subsystems. Fixes two CRITICAL and one SEVERE issue plus supporting hardening. Several fixes
deliberately change runtime behaviour (see Behaviour changes / migration in the [release notes](../../releasenotes.md)) —
hence the minor bump — but no wire-protocol or config break.

### Security fixes

- **A station could impersonate any trusted callsign (CRITICAL).** The classical signed handshake verified a
  CONREQ/CONACK signature against the frame's *own* public key and then consulted the trust store by
  callsign — but never checked that the frame key matched the trusted key for that callsign. Any station
  could therefore present its own key under a trusted callsign and be accepted at full trust, defeating the
  file-transfer signature gate. The frame key is now bound to the trusted key (mirroring the post-quantum
  path, which already did this). (#896)
- **A file offer could land far more data on disk than it declared (CRITICAL).** Received transfer blocks
  were never checked against the offer's declared size, so a small, quota-approved offer could write blocks
  that each expand to ~64 KB — up to gigabytes on disk, bypassing the file-size cap and per-peer quota. Each
  block is now rejected unless its decoded length matches the geometry the offer declared. (#898)
- **A file send to a silent peer locked up the whole transfer subsystem (SEVERE).** Transfer timeouts were
  implemented but never actually fired in the daemon, and there was no way to cancel an outbound send. A
  `send` to a peer that never answered — the norm on HF — pinned the subsystem until a restart, refusing
  every later send. Timeouts now fire each receive tick, and cancelling a transfer also cancels an outbound
  send. (#898)

### Fixes

- **A racing station could be recorded as the peer you dialled.** The connection initiator accepted a CONACK
  that merely echoed the (guessable, time-based) session id; it now also requires the reply to come from the
  callsign it actually dialled. (#896)
- **A no-callsign daemon kept the transmitter identified.** Autonomous responders (handshake reply, QSY, OTA
  acknowledgement, relay forward) no longer key the transmitter when no valid callsign is configured, so the
  station can never transmit unidentified (§97.119). (#897)
- **The QSY trust filter now works.** The frequency-move responder was pinned at "unverified", so a trust
  allowlist either did nothing or rejected everyone; it now reflects the peer's over-air trust. (#897)
- **A trusted signed offer is verified against its own sender's key**, not whoever completed a handshake most
  recently, so a legitimate offer is no longer checked against the wrong key on a multi-peer frequency. (#898)
- **A malformed peer-query response can no longer force a large allocation** from a tiny frame. (#898)
- **A received file can no longer overwrite an existing one** after a large number of same-name collisions;
  the write fails instead. (#898)
- **A trust store that fails to load now stops startup** instead of silently continuing with an empty store
  (which would have dropped revocations). (#896)
- **A transfer with the maximum block count no longer stalls** on its last block (a SAR id collided with the
  control channel). (#898)

### Behaviour changes

- A daemon configured with a trust-store path that can't be read now **refuses to start** (previously it
  started with an empty store). A *missing/unset* path is still fine.
- A daemon with no callsign (or `N0CALL`) **will not transmit** autonomous responses.
- `[discovery] group` is now documented as **reserved** — it was never wired, and setting it now logs a
  warning. The `@OPULSE` group is used regardless.

### Known limitations (tracked)

- A signed offer's filename/geometry are not yet covered by the signature (content is — the payload hash is
  signed), so an on-path attacker can spoof the displayed filename under a "verified" badge; closing this
  needs a manifest wire-format change (next cycle).
- Relay forwarding and OTA rate adoption still act on unauthenticated traffic when their (default-off)
  features are enabled — the signed handshake is an identity label, not an access gate.

Full audit write-ups: `docs/dev/reviews/2026-07-15-handshake-trust-audit.md` and
`docs/dev/reviews/2026-07-15-filexfer-relay-seams-audit.md`.

## v0.7.3 — 2026-07-15

Final hardening patch for the MFSK16 sub-floor ARQ rung — the last open finding from the audit. No breaking
changes; **all ARQ-seam audit findings are now addressed.**

### Fixes

- **Weak-rung acknowledgements now reach a peer running a different rate profile.** When a station drops to
  the MFSK16 sub-floor rung and answers with the robust three-copy acknowledgement, a peer whose profile
  doesn't include that rung previously couldn't decode it — its return channel went dark. The
  acknowledgement now leads with a short standard (FSK4) copy that any peer can hear, followed by the
  three-copy weak-signal version for a deep fade; the receiver acquires the leading copy out of the combined
  transmission. (#894)

The full audit and its resolutions are recorded in `docs/dev/research/mfsk16-arq-seam-audit.md`.

## v0.7.2 — 2026-07-15

Hardening patch for the MFSK16 sub-floor ARQ rung — the findings deferred from the v0.7.1 audit. No breaking
changes.

### Fixes

- **Anti-babble on the weak-signal ACK channel.** The receiver used to answer *every* undecodable burst with
  an acknowledgement; two adaptive stations (or repetitive co-channel QRM) could keep each other keying
  those replies. A consecutive-Nack budget now stops the negative acknowledgements after a few in a row (and
  resets on any real decode), so the station can't become a babbling transmitter — while a genuine
  retransmission still gets through, since the sender retries on its own timeout. (#892)
- **Cross-session acknowledgement filtering.** During the (up to 9 s) weak-rung ACK listen, a *different*
  station pair on the same frequency could have its acknowledgement adopted, silently marking the message
  delivered when the intended peer never got it. The sender now only accepts an acknowledgement carrying the
  addressed peer's session hash. (#892)
- A station whose ARDOP TNC is configured with an adaptive profile that includes the MFSK16 sub-floor rung
  now warns at startup that the rung is a background-daemon feature, not supported on the ARDOP adaptive
  path. (#892)

One known limitation (a rare mixed-profile acknowledgement blackout) remains tracked in
`docs/dev/research/mfsk16-arq-seam-audit.md`.

## v0.7.1 — 2026-07-15

Correctness patch for the v0.7.0 MFSK16 sub-floor ARQ rung. A 4-finder adversarial audit found the rung was
**non-functional on real (sound-card) hardware** — three independent breaks that every v0.7.0 test missed
because they shared masking artifacts (a buffered loopback, a 40 dB / level-locked twin test, a slope-only
SNR check). No breaking changes.

### Fixes

- **The weak-signal ACK is now capturable on real audio.** The sender's ACK-listen re-opened a fresh audio
  capture per read, discarding everything a real sound card buffered between reads — so the ~5 s three-copy
  ACK arrived as unusable fragments and never decoded off-loopback. It now holds one capture stream open for
  the whole listen. (#890)
- **The ACK decodes across turnaround timing at the rung's real SNR.** The copy-alignment step keyed on
  broadband energy, which at the sub-floor's low SNR just locked onto noise; the ACK then decoded for only
  ~28% of turnaround timings at the 0 dB design point. It now aligns on the waveform's own sync (Costas)
  acquisition, which is robust at low SNR. (#890)
- **The rung no longer immediately abandons itself.** The MFSK16 SNR estimate read ~21 dB too high, so the
  rate ladder always thought the link had recovered and jumped straight back to a mode that can't decode at
  that fade — bouncing off the sub-floor rung after every frame. The estimate is now on the true channel
  scale. (#890)
- **Oversized messages on the sub-floor rung fail loudly, not silently.** A message too large for the single
  small MFSK16 frame previously burned transmit airtime on a doomed larger-mode attempt and then dropped
  without a word; it now logs a clear "waiting for the link to climb off the sub-floor rung" and skips. (#890)
- **Removed an unsafe HARQ optimisation** for the sub-floor rung that could, in a corner case, combine soft
  data from an abandoned message into a later one. (#890)
- A malformed `[modem] ota_lock_level` now warns instead of silently leaving the station adaptive. (#890)

Known limitations tracked but not addressed in this patch are listed in `docs/dev/research/mfsk16-arq-seam-audit.md`.

## v0.7.0 — 2026-07-15

The `MFSK16` weak-signal waveform (shipped broadcast-only in v0.6.0) becomes a full **adaptive-ARQ sub-floor
rung**: the receiver-led OTA rate ladder now has a robust deep-fade rung *below* BPSK31. No breaking changes.

### Features

- **MFSK16 sub-floor ARQ rung (SL1)**: on the `hpx_hf` HF profile the rate ladder gains a non-coherent
  constant-envelope MFSK16 rung at **SL1** — the deep-fade rung the ladder drops to when the link falls
  below BPSK31's 3 dB floor. It carries data with RS FEC (one 255-byte block, ≤ 209 B/frame) and climbs
  back out to BPSK31 automatically once the SNR recovers past SL1's ceiling. (#886)
- **Robust K=3 union-decoded ACK**: the sub-floor rung can't be acknowledged over FSK4 (which dies far above
  the MFSK16 floor), so the receiver answers with **three time-spaced MFSK16-ACK copies** and the sender
  **union-decodes** them (decode each copy standalone, MAP-combine only as a fallback). Measured to clear
  ≥ 0.99 at 3 dB below the data floor where a single ACK held only ~0.6. The sender **union-listens** for
  both the FSK4 and the K=3 ACK on one window, so crossing the SL1 boundary can't desync the link. (#885,
  #886)
- **Payload-capacity guard**: a message larger than one MFSK16 frame (209 B) is transmitted on the next rung
  that fits, instead of hard-erroring and being silently dropped. (#887)
- **HARQ soft-combining across MFSK16 retransmissions**: failed sub-floor bursts are retained and
  MAP-combined with later ones, decoding more often than a single attempt on a faded channel. (#887)

### Notes

- The robust ACK was resolved by measurement: the earlier claim that the ACK was the binding constraint
  (~0.6 decode) was a **40-trial small-sample artifact** — at 400 trials it decodes ~0.9, and the winning
  fix (K=3 union, no frequency hop, stays 500 Hz) is cheap. The "longer contiguous frame" alternative was
  measured and *loses*. (#885)
- End-to-end validated across two real daemons (`twin_daemon_bridge::subfloor_sl1_message_crosses_with_k3_ack`),
  plus an `OTA_LOCK` knob on the snd-aloop real-audio rig for sound-card validation. (#888)

## v0.6.0 — 2026-07-15

Post-v0.5.0 block-B/D backlog plus the reference-derived requirements track (PTT backends, hotplug device
resolution, multi-mode monitor, AGC gate, the `MFSK16` weak-signal waveform). No breaking changes.

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
- **`MFSK16` weak-signal waveform** (`plugins/mfsk16`, mode `MFSK16`): a constant-envelope non-coherent
  16-GFSK sub-floor mode that decodes on deep-fade HF where coherent BPSK31 fails — measured ~4 dB better
  on moderate multipath and decoding on fast fade where BPSK31 fails entirely, at a PAPR credit. Registered
  in the CLI/daemon; usable now as a robust broadcast/beacon and explicit `--mode MFSK16` data mode. (The
  ARQ-rung integration — an MFSK16 ACK channel + ladder placement — is deferred.) (REQ-WSIG-01)
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
