---
project: openpulsehf
doc: docs/releasenotes.md
status: living
last_updated: 2026-07-18
---

# Release Notes

## v0.15.0 — 2026-07-18

A hardening release. Nothing in the modem changed shape; what changed is how the software behaves when
the thing on the other end of a connection is broken, hostile, or simply gone.

The trigger was a multi-agent audit of the Winlink network stack — the three crates that speak to a
real Winlink CMS over TCP (`openpulse-b2f`, `openpulse-b2f-driver`, `openpulse-gateway`). The verdict
was that the stack is broadly solid: no panics, no memory-safety bugs, no authentication bypass, and
the earlier gzip-bomb and framing fixes all hold. But it found one live denial-of-service and a cluster
of ways an untrusted peer could hang or balloon the process, plus a piece of code advertising a
capability it never had. This release closes all of it.

**Do I need to update both ends?** For one change, yes. A v0.15.0 station opportunistically strengthens
the error-correction code on small frames, and a pre-v0.15.0 receiver does not know to try that code —
so a v0.15.0 station transmitting to an older one can lose small frames on the weak rungs. Update both
ends. Everything else here is host-side and has no effect on the air interface.

### The denial of service

The receiving side of a Winlink session accepts up to 32 message proposals. The cap was applied when
*answering* the proposals rather than when *storing* them, so a peer that streamed proposal frames grew
an unbounded list in memory — and a peer that never sent the "finished" frame kept the receive loop
spinning forever. Either one is enough to take down a station remotely. Retention is now bounded by a
count, and a session-wide frame ceiling terminates a flood.

### Two caps that were each correct, and still left a hole

Every inbound message was capped at 16 MiB of decompressed output, and every session at 32 messages.
Both limits were right. Nothing enforced their **product**: 32 × 16 MiB is roughly half a gigabyte of
transient memory conjured from about 2 MB of network traffic — enough to matter on a Raspberry Pi. The
session now tracks a running total. This is a shape worth remembering: two individually sufficient
limits can still multiply into an exposure that neither one covers.

### A timeout that was not a timeout

The TCP read timeout used throughout the driver restarts on every *partial* read. It therefore bounds
the gap between two bytes and never the operation as a whole — a peer sending one byte every few
seconds could hold a read open indefinitely, and the code looked correct while doing it. Reads now run
against a single deadline measured from when the operation began. A related bug: the receive path set a
timeout to wait for a connection and then cleared it outright, leaving session teardown able to hang
forever on a peer that simply stopped talking.

### Type C (LZHUF) has been removed

The B2F protocol has two compression types: D (gzip) and C (LZHUF). This project shipped a Type C
implementation, and the documentation described it as Winlink-compatible.

Reading it closely showed that claim could not have been true. The implementation used LHA `LH5`, while
FBB and Winlink use the classic Okumura LZHUF — a *different bitstream*, not merely a different header
convention. No amount of testing would have made those interoperate. The code also had no production
caller: the CMS gateway has always used Type D.

Rather than keep an unverifiable decoder, it is deleted. An inbound Type C proposal is now answered
`Reject` — an honest "I cannot decode this", which leaves the sender free to re-propose as Type D. The
alternative, decoding it best-effort, risks the worst outcome available: a silently corrupted message
presented to the operator as delivered mail.

### Free error-correction on the weak rungs

The one air-interface change. The stronger Reed-Solomon code (`RsStrong`) roughly doubles how often the
weakest rungs decode on a fading channel — BPSK31 at 3 dB goes from 0.25 to 1.00 — and for small frames
it is genuinely free, occupying the same 255-byte block as the standard code. v0.14.0 could not adopt
it ladder-wide because at 192–223 bytes it needs a second block and doubles airtime.

The sender now upgrades per frame, but **only when the upgrade costs no additional block**, so airtime
can never regress; the receiver tries both codes and lets the CRC decide. This is what requires both
ends to be on v0.15.0.

### Migration

- **If you use `openpulse-b2f` as a library:** the LZHUF functions (`compress_lzhuf`,
  `decompress_lzhuf`, `compress_lzhuf_winlink`, `decompress_lzhuf_winlink`, `decompress_lzhuf_compat`)
  and `B2fSession::queue_message_type_c` are gone. There is no replacement — use Type D (gzip) via
  `queue_message`, which is what Winlink actually uses.
- **If you use `openpulse-b2f-driver` as a library:** `set_timeout` now takes `&mut self` on both
  `CmdPort` and `DataPort`; change your binding to `let mut port = …`. `B2fDriver::run_iss` now returns
  `Err(DriverError::AllProposalsRejected { count })` where it previously returned `Ok(())` for a
  transfer in which the peer accepted nothing — if you were treating `Ok` as "sent", that check was
  wrong before and is now correct. A refused connection surfaces as `DriverError::Aborted` instead of
  `DriverError::Timeout` after a minute.
- **Operators:** no configuration changes. Update both ends of a link to keep small frames decoding on
  the weak rungs.

### Known limitations

The HF-fade work spanning v0.13.0 through this release is validated against the Watterson channel
simulator only. It has not been flown on the air. That remains the honest next step and needs real
radios.

## v0.14.1 — 2026-07-17

A patch release that finishes what v0.14.0 started. v0.14.0 re-seated the HF rate ladder so every rung
*decodes* on a fading channel — but the rate controller could not actually **drive** the link up that
ladder on a fade, so in practice it stayed on the bottom rung. This release fixes the controller and
the SNR estimate it reads. Everything here is receiver-side; there is no wire-format, config, or API
change, and a v0.14.1 station interoperates with v0.14.0 exactly as before.

**Background.** On a two-way link the receiver leads the rate: it judges the channel and tells the
sender which rung to use. It judges partly from a measured SNR and partly from whether frames are
decoding. v0.14.0 made the rungs work on a fade; this release makes the *judgement* work on a fade.

**What was wrong.** The controller had a single way to climb: the measured SNR had to clear the current
rung's ceiling. That is fine when the SNR estimate is trustworthy — and on a fading channel it often is
not, for a fundamental reason. At the lowest baud rates the symbol rate is barely faster than the fade
itself (at 31 baud, a 1 Hz fade), so no measurement window is both short enough to follow the fade and
long enough to average out the noise. The estimate flattens into a constant that carries no information
about the channel. With the climb gated solely on that number, the link sat on its **entry rung** — the
one every session starts on — **while decoding every single frame**:

| `hpx_hf` on Watterson `moderate_f1` @20 dB | v0.14.0 | v0.14.1 |
|---|---|---|
| mean speed level reached | 1.5 (pinned near the bottom) | **4.9** |
| final level | SL1 (MFSK16 sub-floor, ~9 bps) | **SL11 (OFDM)** |
| frames delivered | 20/20 | 20/20 |

Delivery was never the problem — throughput was. The rungs that would have carried ~300–1200 bps were
right there, decoding in tests, and the controller would not use them because a number had not moved.

There was a second, sharper edge: on a frame that *did* decode, if the SNR reading was low the
controller would recommend a rung **below** the one that had just decoded. On a fade, where the reading
is flatly wrong, that meant every success was met with a demotion, and the link oscillated on its
bottom two rungs.

**The fix — a decode is evidence; an SNR estimate is a guess; the evidence wins.**

- **Climb on evidence.** After three consecutive clean decodes at a rung, the controller advances one
  step, whatever the SNR estimate says. A rung that keeps decoding has proven itself; refusing to move
  up because a number is stuck is the bug. SNR still climbs *faster* when it is informative — it is now
  an accelerator, not the only permission.
- **Never demote on a decode.** A frame that decoded is direct proof its rung works, so demotion moved
  to the failure path only, where a low SNR reading genuinely explains what went wrong.

Both climbs still advance exactly one rung at a time, which preserves the property that a lost
acknowledgement can never desync the two ends.

**Also fixed, underneath.** BPSK — which is the entire weak-signal core of the HF ladder — had no SNR
estimator at all and fell back to one that assumes a steady signal envelope, exactly what a fade
destroys; it read a flat ≈ −4 dB from 15 dB of true SNR all the way to 35 dB. It now removes the fade's
multiplicative distortion before measuring the noise, so it tracks the channel again. And the link
simulator never registered the sub-floor waveform, so once a ladder dropped to it the sim silently
stopped transmitting — which had made fading runs read as total link failures that were pure harness
artifact. Both are corrected here.

### Notes

- The controller's policy shifted slightly toward throughput. Because a rung's advertised SNR floor
  carries a fading margin, coded rungs actually decode below it on a clean channel — so the evidence
  climb will now probe one rung higher than a conservative SNR estimate would have allowed, and take
  the extra throughput when the frames keep landing. The climb is self-correcting (a rung that starts
  failing is dropped immediately) and cannot run away into dense rungs a poor channel could not carry.
- This is the first release in which the adaptive HF link works end-to-end on a fade *through the rate
  controller*, not just in per-waveform decode tests. The gate that proves it now drives the real
  controller on a fading channel — every earlier fade test called the demodulator directly and so could
  not see that the controller was leaving throughput on the table.

## v0.14.0 — 2026-07-17

A minor release about one thing: **the HF rate ladder was calibrated for a clean channel, and HF is not
a clean channel.** It is a minor rather than a patch release because the rungs, their coding and their
numbering all change on air — see *Interop*.

**Background.** `hpx_hf` is the profile for a real HF SSB link. It is a ladder of ~17 rungs from the
most robust weak-signal waveform up to the densest high-throughput one, and an adaptive session walks
up and down it as conditions change. Its SNR floors — the signal level at which each rung is declared
usable — were derived from AWGN (clean-noise) sweeps.

**What was wrong.** HF fades. Measured against Watterson `moderate_f1` — 1 Hz Doppler, 1.0 ms delay
spread, a *routine* ITU-R moderate path, not a worst case — most of the ladder did not work at the
floors it advertised:

| rung, at its own advertised floor | frames decoded |
|---|---|
| **SL2 `BPSK31` @3 dB** | **0 %** |
| SL3 `BPSK63` @4 dB | 0 % |
| SL5 `BPSK250` @5 dB | 0 % |
| SL7 `QPSK250`, SL8 `QPSK500`, SL9 `8PSK500` | **0 % at any SNR up to 40 dB** |

The first line is the serious one. **SL2 is `initial_level` — every session starts there.** On a fading
path a link could not reliably get started at all; it would fall back to the 9 bps sub-floor rung or
fail. And the middle of the ladder was a four-rung dead zone that the adapter — which climbs one rung
per successful exchange — had to cross to reach the rungs that did work.

Nothing caught this because every gate in the project measured AWGN.

**Two independent causes, both measured.**

*The weak rungs shipped uncoded.* BPSK is differentially decoded, so it rides a fade's slow phase
rotation — but a carrier slip still costs symbols, and without forward error correction there is
nothing to repair them. This is exactly the law [#923](https://github.com/dc0sk/OpenPulseHF/issues/923)
established for QPSK ("differential encoding needs FEC"), and it applies to BPSK for the same reason.
Coding them fixes it:

| at its own floor | uncoded | coded |
|---|---|---|
| BPSK31 @3 dB | 0.00 | **0.25** |
| BPSK63 @4 dB | 0.00 | **0.83** |
| BPSK63 @7 dB | 0.00 | **1.00** |

The floors did not move — they were always fading-appropriate. The rungs simply lacked a code.

*The coherent mid rungs cannot be saved.* `QPSK250`, `QPSK500` and `8PSK500` carry data in absolute
carrier phase, and a fade null makes the receiver's phase reference slip — ruining the rest of the
frame. FEC does not help (the defect is tracking, not errors), and the differential trick that rescued
SL6 does not scale to 8PSK, whose ±22.5° decision margin cannot absorb differential detection's noise
penalty. So above SL6 the ladder is now **OFDM**, which sidesteps the problem entirely: its cyclic
prefix rides the delay spread and its per-subcarrier pilots track the fade.

| on `moderate_f1` | 8 dB | 12 dB | 16 dB |
|---|---|---|---|
| `8PSK500` (was SL9) | 0.00 | 0.00 | 0.00 |
| **`OFDM52`** (now SL7) | **0.58** | **0.75** | **0.83** |

The ladder is **14 rungs instead of 17**, and every rung is measured to decode on a fade.

**A third fix fell out of the second.** The ladder compares its SNR floors against the *receiver's*
estimate of SNR, which for OFDM is deliberately conservative and saturates near ~17 dB — a true 20 dB
link reads about 14.4. `hpx_hf`'s OFDM floors were AWGN-scale numbers (up to 30 dB), which the receiver
could therefore never report, so **those rungs were unreachable** and a strong link stalled below them.
They now use the same receiver-scale calibration `hpx_ofdm_hf` already used, and a 35 dB channel climbs
all the way to the top of the ladder.

**Interop.** SL2–SL5 gain FEC, the rungs above SL6 change waveform, the rungs re-index, and the ladder
fingerprint changes accordingly. **Both stations must run v0.14.0** to share a ladder. There is no
config-file or API change.

### Known limitations

- **The single-carrier and OFDM rungs' floors are on different scales** — true channel SNR for the
  former, receiver plugin-domain SNR for the latter. Each is now correct for its own rungs and the
  split is documented in `docs/mode-fec-ladder.md`, but the underlying mismatch is unresolved.
- **A stronger code is available but not used ladder-wide.** `RsStrong` roughly doubles the weak rungs'
  fading decode (BPSK31 @3 dB: 0.25 → 1.00) and costs *nothing* for payloads ≤191 B, because both codes
  emit the same 255-byte block. At 192–223 B it needs a second block and doubles the airtime, which
  costs more clean-channel throughput than it is worth as a default. It remains the right choice for a
  rung whose frames are known to stay under 191 B.
- The OTA ACK-wait opens a second capture stream on the cpal (real-audio) backend; loopback hides it.
  Parked pending hardware. Tracked in [#917](https://github.com/dc0sk/OpenPulseHF/issues/917).

## v0.13.0 — 2026-07-17

A minor release about one rung of the HF rate ladder that didn't work on a fading channel, and now
does. It is a **minor** rather than a patch release for one reason: the waveform SL6 transmits has
changed, so this rung is not backward-compatible on air. See *Interop* below.

**Background.** The `hpx_hf` rate ladder walks a link from the most robust weak-signal mode up to the
densest high-throughput one, stepping up and down as conditions change. SL6 — `QPSK250 + RS` — sits just
above the BPSK rungs and is meant to be the first step into QPSK's doubled bit rate.

**What was wrong.** SL6 decoded **0% of frames on Watterson `moderate_f1`** — a standard ITU-R moderate
HF channel (1 Hz Doppler, 1.0 ms delay spread) — **at every SNR up to 40 dB**. Not "poorly at low SNR":
zero, at any signal strength. A flat failure across all SNR is the signature of a bug rather than a
physical limit, because noise is the thing SNR buys off, and this didn't care about noise at all.

**What was actually happening.** QPSK250 is *coherent* and *absolutely* phase-encoded: each symbol's
meaning is its absolute phase, so the receiver must track the carrier's phase for the whole frame. A 1 Hz
Doppler fade drives the signal through nulls, and at a null the decision-directed tracking loop loses its
reference and can slip by 90°. After the slip every remaining symbol is rotated, so the entire tail of the
frame decodes wrong. With RS padding, an SL6 frame is 4.08 s on air — far more than enough time to meet a
null.

Ablation pinned it down, and killed the obvious explanation first:

| QPSK250 @ 40 dB | frames decoded |
|---|---|
| `moderate_f1` (1 Hz Doppler, 1.0 ms delay) | 0% |
| **delay spread removed** (1 Hz, 0 ms) | **0%** ← so it is *not* multipath/ISI |
| **Doppler removed** (0 Hz, 1.0 ms) | **82%** ← the fade is the whole problem |

**Two plausible fixes were measured and rejected.** Both were the natural things to try, and both decode
0% themselves on this channel:

- Port the 2-pass acquire-then-track carrier loop already used by 8PSK — but **8PSK500 is itself at 0%** here.
- Route the rung to the pilot-aided waveform, which exists precisely because it is cycle-slip-immune — but
  **PILOT-QPSK500 is also at 0%**.

The only two modes that survive this channel are **BPSK250 (95%)** and **MFSK16 (100%)** — which are
exactly the differentially-decoded and the non-coherent modes. That was the tell.

**The fix.** SL6 is now **`QPSK250-D`**, differential QPSK. Each dibit is encoded as a phase *increment*
from the previous symbol rather than as an absolute phase, and the receiver recovers it from the phase
*difference* between adjacent symbols. A slow fade rotation is common to both symbols and cancels; a
carrier slip corrupts one dibit and then the stream re-references itself — it can no longer poison the
tail. This is the same immunity BPSK has always had.

| SL6 on `moderate_f1` @ 20 dB | frames decoded |
|---|---|
| v0.12.2 (`QPSK250`, coherent) | 0% |
| **v0.13.0 (`QPSK250-D`, differential)** | **65%** |

At 65% of 437 bps, SL6 now delivers ~284 effective bps against BPSK250's ~237 (95% of 250) — so the rung
finally earns its place in the ladder instead of being a step the adapter always falls off.

**What it costs.** Differential detection trades a little noise performance for fade immunity. On AWGN the
penalty shows up only at the extreme floor — coherent QPSK250 manages 68% at 2 dB where differential
manages 0% — but **both reach 100% by 4 dB**, comfortably below SL6's ~7 dB operating point. On the channel
this rung actually runs on, the trade is lopsided in its favour.

**Interop.** This changes the waveform SL6 transmits, so **both stations must run v0.13.0 to use SL6**;
the ladder fingerprint changes accordingly. Every other rung is untouched, and there is no config-file or
API change. New selectable modes `QPSK250-D` and `QPSK500-D` are available directly.

### Known limitations

- **`8PSK500 + RS` (SL9) has the same fade-fragility and is not fixed.** The obvious follow-on — give it
  the same differential treatment — was prototyped and measured: `8PSK500-D` reaches only 12.5% on
  `moderate_f1` even at 40 dB, and costs ~4–6 dB of AWGN floor (against QPSK's ~2 dB). Differential
  detection roughly doubles the effective noise, and 8PSK's ±22.5° decision margin cannot absorb that —
  it ends up worse on AWGN *and* still unusable on fading. Robustness tracks phase margin (non-coherent
  MFSK16 > BPSK ±90° > QPSK ±45° > 8PSK ±22.5°), so SL9 needs a different mechanism, not `-D`. In
  practice the rate adapter steps down to SL6, which now works.
- The coherent **uncoded** rungs (SL7/SL8) remain high-SNR rungs by design: differential encoding needs
  FEC to correct the dibit a slip costs, so it cannot rescue a rung that has no FEC.
- The OTA ACK-wait opens a second capture stream on the cpal (real-audio) backend; loopback hides it.
  Parked pending hardware. Tracked in [#917](https://github.com/dc0sk/OpenPulseHF/issues/917).

## v0.12.2 — 2026-07-17

A patch release about one thing: getting more frames out of a fading channel. Everything here is
receiver-side — there is no wire-format, config-file, or API change, and nothing you need to do to
upgrade. Two stations running v0.12.1 and v0.12.2 interoperate exactly as before; the newer one just
decodes more.

**Background.** When a message fails to decode, the sender retransmits it. HARQ soft-combining is the
technique that adds those attempts together instead of throwing the failed ones away: each burst is a
partially-ruined observation of the same bits, and on a fading channel they are ruined in *different*
places, so the sum can decode what no single copy could. OpenPulse has had this since v0.4, but it was
leaving most of its own gain on the floor.

**What was wrong.** Two things, both invisible without measuring:

- The combiner only accepted a retained burst if its soft-decision data was *exactly* the same length as
  the current one. That sounds harmless, but a faded demodulator recovers a varying number of symbols from
  the same frame — we measured 576 to 4320 on a single frame — so most genuine retransmissions were being
  rejected and their diversity discarded, after the airtime had already been spent sending them.
- If the sender gave up on a message and moved on, the abandoned message's soft data could be mixed into
  the *next* message's combine. It never delivered a wrong frame — the error-correcting code and checksum
  still gated everything — but it diluted the very gain the mechanism exists to provide.

**What it's worth.** On a moderate HF fade (Watterson `moderate_f1`) at 10 dB, over 400 independent fade
realisations, where a single burst decodes 58.2% of the time:

| | frames decoded |
|---|---|
| v0.12.1 | 84.0% |
| **v0.12.2** | **95.7%** |

**The weak-signal sub-floor gets HARQ too — worth about 2.5 dB.** MFSK16 is the rung the link falls back
to when everything else has failed: slow, non-coherent, built for deep fades. It was excluded from
combining because every MFSK16 frame is one fixed-size block, which meant an abandoned message's data
couldn't be told apart from a retransmission of the live one — and the worst case there is delivering the
*wrong message*. The fix above removes that hazard structurally (a stale message's bursts are always
older, so the decoder simply tries the recent ones first), so the rung could finally be let in. At -4 dB
its decode rate goes from 11.7% to 75.0%, and at -6 dB it recovers frames that no single transmission ever
does. That gain lands exactly where a link has nothing left to fall back on.

**Also:** `openpulse session --diagnostics` now reports a real `afc_offset_hz`. The field was always in
the JSON but nothing ever filled it in, so it read `null` no matter what — the carrier-offset estimate was
sitting in the modem the whole time. Same schema, so nothing that reads that JSON breaks.

### Known limitations

- **`QPSK250` on a moderate fade.** Investigating the above turned up that the `QPSK250 + RS` rung of the
  `hpx_hf` ladder never decodes on a Watterson `moderate_f1` fade — at *any* signal level, including 40 dB.
  An ablation points at carrier phase tracking through the fading (removing the multipath changes nothing;
  removing the fading rescues it), not noise or intersymbol interference. Because the rate ladder simply
  steps off a rung that isn't working, this costs throughput rather than correctness, and it is not a
  regression — it predates this release. Tracked in
  [#923](https://github.com/dc0sk/OpenPulseHF/issues/923).
- **Second capture stream during the ACK wait (real-audio backend only).** Parked pending hardware to
  verify against. Tracked in [#917](https://github.com/dc0sk/OpenPulseHF/issues/917).

## v0.12.1 — 2026-07-16

A patch release of internal fixes from the first formal loose-ends audit of the modem core (the
engine / ARQ / OTA rate ladder / DSP). The audit's verdict was that the core is broadly solid — no
memory-safety, no remote-input crash, no happy-path decode bug — with the real gaps clustered in the OTA
rate-ladder wiring. These are the top confirmed items. There is no wire-format or config-file change.

- **Silent-peer send no longer freezes the daemon.** Sending a message to a peer that has gone quiet used
  to retry through its full budget of acknowledgement windows — up to ~36 seconds on the weak-signal
  sub-floor — all synchronously inside the daemon's single control loop, so `Abort`/`Disconnect` and
  incoming data both stalled for the duration. It now gives up after two consecutive silent windows. A peer
  that *is* answering (with a negative-ACK asking for a resend) still gets the full retry budget — only a
  genuinely silent link is cut short.
- **Receiver front-end no longer re-runs per rate candidate.** When the notch filter or AGC is enabled, the
  OTA decoder was re-applying them once per candidate rate, which could make the notch's interference
  detector trip early and hand off to an unnecessary frequency change (QSY).
- **Honest config warnings.** Three OTA tuning knobs (`ota_aggressiveness`, `ota_min_backlog`,
  `ota_upgrade_hold_frames`) configure a rate policy the daemon's receiver-led ladder doesn't use — the
  daemon used to log that it had "applied" them. It now warns that they have no effect and points you at
  the knobs that do (`ota_min_level` / `ota_max_level` / `ota_lock_level`).
- Plus a couple of small internal-consistency fixes.

No action required to upgrade.

## v0.12.0 — 2026-07-16

Closes the last two implementable findings from the handshake-trust audit series. After this release the
only outstanding audit item is a larger architectural refactor (session-driving front-ends). The minor
bump reflects a new signed handshake field and the ACK behaviour change below; there is no config break.

### Authenticated rate-control ACK

The link's speed ladder is driven by a tiny 5-byte acknowledgement sent on a robust FSK4 channel between
data bursts. That ACK proved nothing about who sent it — its only filter was a 16-bit hash of the session
id, and the session id is sent in the clear in the connection request. So any station listening on the
frequency could forge ACKs and yank a link's data rate up (into a mode the channel can't hold) or down
(a slow-motion denial of service).

The fix adds a Diffie-Hellman key agreement to the signed handshake: each side sends an ephemeral X25519
public key *inside* the Ed25519-signed CONREQ/CONACK, so the exchange is bound to the authenticated
identity and a man-in-the-middle can't swap the keys. Both stations derive the same secret and the ACK now
carries a short keyed authentication tag. A forged ACK — or one from a different session — fails the tag
and is ignored. The tag fits inside the existing 5-byte ACK, so nothing about the ACK's on-air waveform or
timing changes.

This is **authentication, not encryption**: the ACK content is still fully readable; the key is used only
to prove authenticity, never to hide meaning — consistent with amateur-radio rules (see the regulatory
notes). A residual: a captured valid ACK could be replayed within the session, but the rate ladder is
receiver-led and absolute, which bounds the effect.

### Verified-peer consolidation

An earlier release already made file-transfer offer verification bind to the offer's real sender rather
than "whoever handshook most recently." This release removes the now-redundant global slot so every
verified-identity read goes through one authoritative per-callsign store.

### Behaviour changes / migration

1. **Rate ACKs are authenticated when both ends support it.** Two upgraded stations negotiate the ACK key
   automatically; a forged or wrong-key ACK is dropped. A peer running an older build simply doesn't
   advertise a key and falls back to the legacy unauthenticated ACK — no interop break. *Action:* none.

## v0.11.0 — 2026-07-16

Two more deferred fixes from the security-audit series, both on the signed-handshake path. The minor bump
reflects the handshake behaviour change below; there is no config-file break, and the only wire change is a
new (optional, signed) timestamp field.

### Replay-freshness

A signed CONREQ/CONACK proved *who* sent it, but carried nothing to prove *when* — so a captured, valid
handshake could be replayed later and re-accepted, its signature still good. Both frames now carry a signed
`timestamp_ms`, and the verifier rejects a frame whose timestamp is stale, future-dated, or missing beyond a
clock-skew window (the daemon uses ±120 seconds). The timestamp lives inside the signed body, so an attacker
cannot refresh a captured frame without invalidating the signature. This bounds the replay window from
"forever" to the clock-skew tolerance.

### SAR-poison resilience

Handshake frames are larger than one modem frame, so they are split and reassembled. The reassembler keyed
*every* handshake under one constant slot — so a single crafted fragment could poison it: claim a wrong
fragment count and the legitimate fragments bounced off it, or fill an index with garbage and the real
fragment was silently dropped as a duplicate. Either way a legitimate handshake was blocked for the whole
reassembly timeout, cheaply and silently. Two genuine handshakes arriving at once could also poison each
other by accident. The reassembler now keeps conflicting fragment streams as **separate candidates**: a
poisoned or interleaved fragment starts its own candidate instead of corrupting the in-flight one, and the
bogus candidate simply fails signature verification and is discarded while the good one completes.

### Behaviour changes / migration

1. **The daemon rejects a handshake with no timestamp or one more than ±120 s from its clock.** Both ends
   stamp a current time, so two upgraded stations interoperate — but a station running an older
   (timestampless) build will be rejected, and stations whose clocks drift more than two minutes apart will
   fail to connect. *Action:* upgrade both ends together and keep stations roughly time-synced (most HF
   digital software already assumes this).

### Under the hood

- The SAR reassembler's `ingest` now returns *all* frames a fragment completed (normally one; more only
  under a deliberate collision). Internal API only; no wire-format change.

## v0.10.0 — 2026-07-16

The follow-through on the security-audit series: this closes the last major deferred finding from the
handshake-trust audit, **envelope origin authentication for relayed traffic (E3)**. A relay now
cryptographically verifies who originated every relay-data frame it forwards, so a station can no longer
impersonate another originator at a relay. The minor bump reflects a control-plane wire-format change
(envelope v2) and the behaviour change below; there is no config-file break.

### The headline change

**A relay no longer forwards forged or unauthenticated relay traffic (E3).** The `OPHF` control-plane
envelope carried a 16-byte `auth_tag` that was *never verified* and had no scheme for distributing the keys
that would make it meaningful. So any station could transmit a relay frame stamped with any originator id
(`src_peer_id`) and a relay would forward it — impersonation, and the foundation a strong relay-access
policy would have to stand on. Because a peer id in this system **is** its Ed25519 verifying key (the same
self-authenticating trick the peer descriptors use), the fix needs no key exchange: the envelope now carries
an optional Ed25519 signature over its own fields, and the relay verifies it against the `src_peer_id` in the
frame. A relay drops any relay-data frame whose signature is missing or invalid. The originator allow-list
shipped in v0.9.0 now scopes a relay *on top of* real cryptographic authentication instead of a spoofable id.

The signature is **optional on the wire** by design. Making it mandatory (a fixed 64-byte field on every
envelope) would push even a one-result peer-query response past the modem's 255-byte frame limit, forcing
fragmentation on every control message — and the mesh's receive loop can't reliably reassemble multi-frame
messages from continuous audio. Instead, only the frames that actually pass through the authenticated relay
(relay-data and hop-acks) are signed; the query/route responses that already carry their own inner signatures
stay unsigned at the envelope level and comfortably within one frame.

### Behaviour changes / migration

1. **Relays reject unsigned or forged relay frames.** Enabled by default, with no opt-out. The only software
   that originates relay frames in this project (the mesh daemon) now signs them, so a normal mesh is
   unaffected — but a relay will silently drop relay-data frames from anything that does not sign.
2. **Control-plane wire schema is now v2.** The `OPHF` envelope replaced its fixed 16-byte `auth_tag` with an
   optional 64-byte signature; **v1 and v2 envelopes are not interoperable.** This touches only the mesh,
   relay, peer-query, and route-discovery control plane — all off by default. *Action:* upgrade both ends of
   a mesh/relay link together.

### Resolved from v0.9.0

- The v0.9.0 notes listed **envelope-level authentication of relayed traffic** as future work that would
  "need a mesh reception-model change (burst reception)." That turned out to be avoidable: making the
  signature optional keeps authenticated frames inside a single modem frame, so no reception-model change was
  needed. Resolved in this release.

## v0.9.0 — 2026-07-16

The second release in the security-audit series. A fresh adversarial sweep of the **RX decode path** (the
code that turns untrusted RF-derived bytes into frames) and the **network-facing protocol bridges** (ARDOP
TNC, KISS/AX.25, B2F/Winlink), plus follow-ups. **Anyone running the daemon or a TNC should upgrade** — this
fixes a CRITICAL remote crash. The minor bump reflects the file-offer signature wire-format change; there is
no config break.

### The headline issue

**A crafted transmission could crash the receiver (CRITICAL).** The short-FEC decode path — reached from
the OTA acknowledgement listener (which runs every receive tick) and the short-FEC receive path — handed
attacker-length-controlled demodulator output straight to a Reed–Solomon decoder whose internal buffer is a
fixed 256 bytes. Any input ≥ 256 bytes panics it, killing the receive call. A hostile station could crash any
receiver on frequency simply by transmitting enough audio. The decoder now rejects over-length input before
it can reach that buffer. Two related receive-path panics (32-bit/WASM length-prefix overflow) and an
unbounded SAR reassembly table were fixed in the same pass.

### Also fixed

- The **ARDOP TNC** could be driven out of memory by a client streaming bytes with no newline — the command
  read is now length-bounded.
- The **Winlink gzip decompressor** had no output-size cap (the LZHUF path did) — a decompression bomb from a
  malicious CMS is now rejected.
- The B2F session could be made to **accept and retain an unbounded number of proposals** — now capped.
- A **signed file offer's filename and geometry are now covered by its signature** — previously only the
  content hash was, so an on-path attacker could replay a signed offer with a spoofed filename under a
  valid-signature badge.

### New

- **Relay originator allow-list** (`[relay] allow_list`): an enabled relay can be restricted to forwarding
  only frames from listed originator peer IDs — a defense-in-depth control for a club/mesh relay.
- The mesh can now **carry control responses larger than one modem frame** (SAR fragment/reassemble);
  transparent to current traffic.

### Behaviour changes / migration

1. **The file-offer signature now covers the whole offer.** A signed offer created by v0.8.0 will not verify
   on v0.9.0 (the signature covers more fields). Direct file transfer is off by default and experimental, so
   this only affects operators who enabled it between the two releases. *Action:* none for most; re-send any
   in-flight offer after both ends upgrade.

### Known limitations (tracked, not in this release)

- **Envelope-level authentication of relayed traffic** (the route/query floods a relay forwards) remains
  future work. The modem's 255-byte frame cap means a signed envelope must fragment across frames, which
  needs a mesh reception-model change (burst reception). The allow-list above is the shipped defense-in-depth
  control; see `docs/dev/reviews/2026-07-15-handshake-trust-audit.md` (E1/E3).

## v0.8.0 — 2026-07-15

A **security release**. Three back-to-back adversarial audits — of the signed handshake / trust store, of
session establishment, and of the direct file-transfer + relay subsystems — found two CRITICAL issues, one
SEVERE availability issue, and a set of supporting weaknesses. All are fixed here. This is a minor bump
because several fixes change runtime behaviour (below); there is **no wire-protocol or config-file break**.

Anyone running the daemon or using direct file transfer should upgrade.

### The headline issues

- **Trusted-callsign impersonation (CRITICAL, #896).** The classical signed handshake proved only that the
  sender held *a* key — it verified the signature against the key carried in the frame, then looked up the
  claimed callsign in the trust store, but never checked the two matched. So any station could sign with its
  own key under a trusted operator's callsign and be accepted at full trust. Because the (off-by-default)
  file-transfer accept path trusts that verified identity, an attacker could have their files auto-accepted
  under someone else's callsign. The frame key is now bound to the trusted key, exactly as the post-quantum
  handshake already did.
- **File-transfer disk amplification (CRITICAL, #898).** The offer size, the per-peer quota, and the
  block-geometry check are all evaluated once, at offer time, against the *declared* file size. Nothing then
  constrained the actual bytes each block carried, and each block can decompress to ~64 KB — so a peer could
  offer a 1 KB file, pass the quota, then deliver blocks totalling gigabytes, all written to disk. Blocks are
  now rejected unless their decoded length matches the declared geometry, and a completed file whose total
  size disagrees with the offer is not written.
- **File-transfer send lock-up (SEVERE, #898).** The transfer state machines have offer/stall/verify
  timeouts, but the daemon never called them, and a cancel only ever affected the receive side. A `send` to a
  peer that stayed silent — routine on HF — left the send slot occupied forever, and every subsequent send
  failed with "a file transfer is already active", recoverable only by restarting the daemon. Timeouts now
  fire on every receive tick, and cancelling a transfer cancels an outbound send too.

### Also fixed

- A connection initiator now checks that a CONACK came from the callsign it dialled, not just that it echoed
  the (guessable) session id (#896).
- Autonomous transmit paths refuse to key up without a valid callsign, so a mis-configured station can't
  transmit unidentified (§97.119) (#897).
- The QSY (frequency-move) trust filter now reflects the peer's actual over-air trust instead of a hardcoded
  "unverified", so an allowlist is enforceable (#897).
- A signed file offer is verified against *its own sender's* key rather than whoever handshook most recently
  (#898).
- A malformed peer-query response can't force a multi-megabyte allocation from a tiny frame (#898).
- A received file never overwrites an existing one — the write fails instead of clobbering (#898).
- A trust store that fails to load stops startup rather than silently dropping its revocations (#896).
- A maximum-block-count transfer no longer stalls on its final block (#898).

### Behaviour changes / migration

Read this if you run the daemon:

1. **Trust store load is now fail-closed.** If you set `[trust] store_path` to a file the daemon can't read
   or parse, it now **refuses to start** with a clear error, instead of quietly running with an empty store
   (which silently dropped any revocations). *Action:* ensure the path is correct and readable, or remove the
   setting. An unset/missing path is unaffected.
2. **A callsign is required to transmit.** A daemon with no `[station] callsign` (or `N0CALL`) will still
   receive, but its autonomous responders (handshake reply, QSY, OTA acknowledgement, relay) **no longer key
   the transmitter** — they would otherwise transmit without a station ID. *Action:* set a real callsign to
   enable transmit.
3. **`[discovery] group` is reserved.** It was never wired (the `@OPULSE` group is baked into the beacon
   frame format). Setting a non-default value now logs a warning and has no effect. *Action:* none; remove
   the override if you had one.
4. **Malformed / oversized file offers are now rejected** where some previously produced an oversized or
   quarantined on-disk file. Legitimate transfers are unaffected.

### Known limitations (tracked, not in this release)

- A signed offer's **filename and geometry are not yet covered by the signature** — only the payload hash
  and sender id are. Content integrity is protected, but an on-path attacker can replay a signed offer with a
  spoofed filename while the UI still shows it as signature-valid. Closing this needs a manifest wire-format
  change and is the next planned work.
- With their (default-off) features enabled, **relay forwarding and OTA rate adoption act on unauthenticated
  traffic** — the signed handshake authenticates *identity*, it does not gate those actions. This is
  documented in `docs/dev/reviews/2026-07-15-handshake-trust-audit.md`.

## v0.7.3 — 2026-07-15

The final hardening patch for the **MFSK16 weak-signal sub-floor ARQ rung** — the last open finding from the
adversarial audit. **No breaking changes.** With this release, every finding from the audit is addressed.

- **A weak-rung acknowledgement now reaches a peer that's running a different rate profile.** When a station
  fades onto the MFSK16 sub-floor rung it answers with a robust three-copy acknowledgement — but a peer
  whose configured profile doesn't include that rung couldn't decode it, so its acknowledgement channel went
  silent and its messages stalled. The acknowledgement now **leads with a short standard-waveform copy** that
  any peer can hear, then sends the three-copy weak-signal version for a genuinely deep fade. The receiver
  finds and decodes the leading copy out of the combined transmission automatically. This closes the last
  edge case in the sub-floor rung's acknowledgement path.

## v0.7.2 — 2026-07-15

A hardening patch for the **MFSK16 weak-signal sub-floor ARQ rung** — the robustness findings that the
v0.7.1 adversarial audit had deferred. **No breaking changes.**

- **The station can't get stuck babbling on the acknowledgement channel.** Previously the receiver answered
  *every* burst it couldn't decode with a negative acknowledgement; two adaptive stations, or repetitive
  interference on the frequency, could keep each other transmitting those replies indefinitely — a
  regulatory concern. There's now a budget: after a few consecutive negative acknowledgements with no real
  frame in between, the station goes quiet (and resumes the moment a frame decodes). A genuine retransmission
  still gets through, because the sender retries on its own timeout regardless.
- **An acknowledgement from a different station pair on the same frequency is no longer mistaken for yours.**
  During the weak-rung acknowledgement listen (up to ~9 seconds) another pair's valid acknowledgement could
  be adopted, silently marking your message delivered when your peer never received it. The sender now only
  accepts an acknowledgement addressed to the peer it's talking to.
- If you run the ARDOP TNC with an adaptive profile that includes the MFSK16 sub-floor rung, it now warns at
  startup that the rung is a feature of the background daemon and isn't supported on the ARDOP adaptive path.

One rare mixed-profile edge case (a station on a profile *with* the sub-floor rung talking, without a
completed handshake, to one *without* it) remains documented for a future fix.

## v0.7.1 — 2026-07-15

A correctness patch for the **MFSK16 weak-signal sub-floor ARQ rung** introduced in v0.7.0. **No breaking
changes.**

A focused adversarial audit of the rung found that — as released in v0.7.0 — it **did not actually work on a
real sound card**, even though every v0.7.0 test passed. The tests all shared the same blind spots: an
in-memory loopback that buffers audio perfectly, an end-to-end twin test running at 40 dB with the rung
pinned by hand, and an SNR check that only looked at the slope. Three independent problems hid behind those:

- **The acknowledgement couldn't be captured on real audio.** The sender re-opened its microphone/soundcard
  input for every short read while listening for the reply, throwing away the audio a real device buffers in
  between — so the ~5 second three-copy weak-signal ACK arrived in pieces and never decoded. It now keeps one
  capture stream open for the whole listen.
- **The acknowledgement only decoded for a lucky fraction of turnaround timings.** Finding the three ACK
  copies relied on a loudness test that, at the very weak signal levels this rung exists for, just latched
  onto noise — so the ACK decoded for only about a quarter of the possible sender/receiver turnaround
  timings at the design point. It now locks onto the waveform's own sync pattern, which works down at those
  levels.
- **The rung ejected itself after every frame.** The signal-quality estimate for MFSK16 read about 21 dB too
  high, so the automatic rate control always believed the link had recovered and jumped straight back to a
  faster mode that can't survive the fade — then fell back again on the next frame, over and over. The
  estimate is now on the true channel scale.

Also: a message too large for the sub-floor rung's single small frame now reports clearly and waits for the
link to improve instead of silently burning airtime and dropping; an unsafe soft-combining shortcut for the
rung was removed; and a mistyped `ota_lock_level` warns instead of silently leaving the station adaptive.

If you tried the MFSK16 sub-floor rung on real hardware under v0.7.0, upgrade to v0.7.1. The remaining
known limitations (co-channel acknowledgement disambiguation, and a few opt-in/edge configurations) are
documented for follow-up.

## v0.7.0 — 2026-07-15

The `MFSK16` weak-signal waveform grows from a broadcast/beacon mode (v0.6.0) into a full **adaptive-ARQ
sub-floor rung**. **No breaking changes.**

**A weak-signal rung below BPSK31**

- On the HF profile (`hpx_hf`), the automatic rate ladder now has a bottom rung — **MFSK16 at SL1** — that
  the link drops to when it fades below where BPSK31 works (~3 dB). MFSK16 is a slow but very robust
  constant-envelope 16-tone mode that keeps decoding on deep-fade HF paths where the coherent modes drop
  out. When the path recovers, the ladder climbs back up on its own.
- Because the rung is entered only under a genuinely weak link, it is deliberately slow (a data frame is
  ~17 s) and small (≤ 209 bytes per frame) — it's for getting a short message or acknowledgement through
  when nothing else will, not for throughput.

**A robust acknowledgement for the weak rung**

- The normal fast FSK4 acknowledgement can't survive at the MFSK16 rung's signal levels, so the receiver
  now answers a weak-rung frame with **three spaced copies of a robust MFSK16 acknowledgement**, and the
  sender combines them — recovering the acknowledgement in fade conditions where a single copy would be
  lost (measured to succeed ~99% of the time 3 dB below where the data itself works). The sender listens for
  *either* acknowledgement style at once, so moving in or out of the weak rung never gets the two stations
  out of step.

**Safety and robustness**

- A message too large for the weak rung's single small frame is automatically sent on the next rung that
  can carry it, rather than being quietly dropped.
- Repeated weak-rung transmissions are soft-combined (HARQ), so a frame that fails once can still decode
  once a retransmission arrives.

**Under the hood**

- This shipped as a measured, staged effort. Notably, the belief that the weak-rung acknowledgement was a
  hard blocker (an earlier ~0.6 decode rate) turned out to be a small-sample measurement artifact — at a
  proper trial count it's ~0.9, and the real fix (three combined copies, no frequency hopping) is cheap and
  stays within the same 500 Hz. The whole rung is validated end-to-end across two in-process daemons, and
  the `run-twin-station-audio.sh` rig gained an `OTA_LOCK` knob to exercise it over a real sound card.

## v0.6.0 — 2026-07-15

Post-v0.5.0 improvements: new PTT backends, hotplug-safe audio, a multi-mode monitor, and the `MFSK16`
weak-signal waveform. **No breaking changes.**

**Reliability & safety**

- **The transmitter watchdog can no longer be blocked.** The safety timer that force-releases PTT after the
  maximum key-down duration now runs on its own independent thread, so it still fires even while the daemon
  is busy inside a long operation (a frequency scan, or a message/file send-retry burst) — previously the
  release could be delayed until that operation finished. And if the radio's own release fails (a stuck
  rig), the watchdog keeps retrying rather than telling clients the transmitter is down when it isn't.
- **Faster fail-safe on unexpected errors.** Every automatic transmit now releases PTT the instant its
  scope ends — including if the code hits an unexpected error mid-transmit — instead of possibly holding
  the transmitter keyed until the watchdog timer expires.

**Radio interfaces**

- **CM108 USB-HID PTT.** You can now key transmit through the GPIO on CM108/CM109/CM119 USB sound-card
  interfaces (DMK URI, RepeaterBuilder RA-series, AIOC, homebrew) — `--ptt cm108` on the CLI or
  `[modem] ptt_backend = "cm108"` in the daemon config. It auto-detects a C-Media device (or set the
  `/dev/hidrawN` path and GPIO pin), and needs no extra libraries.
- **GPIO-line PTT.** Key transmit directly from a Linux GPIO line — e.g. a Raspberry Pi header pin —
  with `--ptt gpio` and a `chip:line` spec like `gpiochip0:17` (append `:active_low` for inverting
  interfaces). Built with `--features gpio`.
- **Daemon serial PTT.** The background daemon now drives `rts`/`dtr` serial PTT (built with
  `--features serial`), which previously worked only from the CLI.

**Audio**

- **Your configured audio device survives being renamed or reordered.** Previously, if the OS gave your
  sound card a slightly different name (e.g. a `(2)` suffix) or shuffled its index after a reboot/hotplug,
  the daemon would fail to find it. It now matches by a stable identifier and a fuzzy name fallback, so the
  same `[audio] device` setting keeps working — and it refuses to guess when two devices match.
- **Multi-mode monitor.** The daemon can now watch for several modes at once: list them under `[monitor]`
  and it decodes each from the received audio alongside your active session, reporting what it hears — handy
  for seeing what else is on the frequency. Off by default.
- **Receiver AGC from config.** The receiver automatic-gain-control can now be turned on in the config
  (`[modem] agc_enabled`). It doesn't change whether a signal decodes (that's already level-independent) —
  it steadies the audio level through deep fading and gives a gain/level readout. Off by default.

**Weak-signal mode**

- **New `MFSK16` mode for deep-fade / very weak signals.** A robust, narrow 16-tone waveform that keeps
  decoding on badly-faded HF paths where the coherent BPSK modes drop out — measured to beat BPSK31 by
  several dB on multipath, and to decode on fast-fading paths where BPSK31 fails entirely. Select it with
  `--mode MFSK16` (or in the panel) for robust one-way / beacon traffic. It's a sub-floor mode: very robust
  but slow (~17 s per frame).

**Mesh & TNCs**

- **Multi-hop mesh routes.** A mesh route discovery now records the full path it traverses, so the
  destination replies with the real end-to-end route instead of only the last hop.
- **KISS full-duplex control.** A KISS host can now toggle carrier-sense (CSMA) with the standard
  `FullDuplex` control frame.

**Under the hood**

- A proposed weak-signal **frequency-diversity** mode was measured end to end and **deliberately not
  shipped**: its diversity gain is consumed by the transmit-peak (PAPR) cost of a two-carrier waveform, so
  the net on-air benefit is ~break-even at twice the bandwidth — the existing options (drop to a slower
  mode, or retransmit-and-combine) do better. The measurements and analysis are kept in the repo for any
  future revisit.

## v0.5.0 — 2026-07-14

A hardening release: the deferred tail of a whole-codebase "what isn't nailed down" audit, worked to
completion. It's mostly correctness, regulatory-compliance, and robustness fixes, with a few new
capabilities. **No breaking changes** — existing configs and workflows keep working; the new behaviour is
either automatic (safer defaults) or opt-in.

**New capabilities**

- **Mesh route discovery, end to end.** Mesh nodes can now discover a route to a destination they can't
  reach directly, remember it, and use it to forward relay traffic — including keeping routes fresh (signed
  route updates) and tearing down a route a hop declines to carry (route rejects). Previously only the wire
  format existed; now the whole request → answer → apply → maintain flow works on air.
- **Per-band transmit attenuation.** Setting TX attenuation with a band now remembers a per-band value and
  re-applies it automatically when you retune to that band (like the existing per-band squelch). Setting it
  without a band still sets the global default.
- **Declared transmit power** (`[station] tx_power_watts`) and the operator callsign are now recorded in
  the station's transmit log on every path (daemon, ARDOP/KISS TNCs, mesh) — previously the log showed a
  blank callsign and 0 W outside two CLI commands.
- **PTT resync.** A new `openpulse daemon ptt-state` command (and `GetPttState` control command) lets a
  reconnecting client recover the current transmit state if it missed a change.

**Compliance & safety (§97.119)**

- The **ARDOP** and **KISS** TNCs now refuse to key the transmitter without a valid station identifier —
  ARDOP needs your `MYID`, and KISS requires a real AX.25 source callsign in the frame (no `N0CALL`). The
  mesh daemon already refuses to run as `N0CALL`, and the cross-band repeater now identifies its
  transmitting rig. This prevents an unidentified transmission from a misconfigured station.

**Reliability & robustness**

- A flood of control commands can no longer starve the receiver or the PTT safety watchdog, and a
  CONNECT/DISCONNECT or a long scan no longer stalls other commands the way it could before.
- The control WebSocket now fails closed when authentication is required, the ARDOP data port no longer
  silently drops frames under load, and several signal-processing reliability figures (soft-decision
  calibration for the dense QAM/OFDM/pilot modes, weak-signal JS8 decoding of real off-air transmissions,
  rendezvous timing) were corrected.

Full technical detail and PR links are in `docs/dev/project/changelog.md`.

## v0.4.0 — 2026-07-12

- JS8 station discovery (new, opt-in): when your station is idle you can have it tune to the
  band's JS8 calling frequency and discover other OpenPulse stations there. It uses a native JS8
  waveform that interoperates with JS8Call — no separate JS8Call install — marks itself with an
  `@OPULSE` capability hint, and lists the stations it hears (`openpulse daemon stations` /
  `openpulse daemon peers`, or a new **Discovery** tab in the panel). Enable it under `[discovery]`.
  **Off by default, and receive-only until you explicitly opt into transmitting.**
- JS8 beacon + rendezvous (new, opt-in transmit): with a callsign configured and `[discovery]
  mode = "beacon"` or `"full"`, your station periodically announces itself (a heartbeat and the
  `@OPULSE` hint). In `full` mode it can also negotiate a working frequency with a discovered peer
  over JS8 (the `RendezvousWith` control command), QSY both stations there, and start an
  authenticated OpenPulse session. Every transmit path is off by default and gated behind an
  explicit mode + your callsign + a ±2 s clock-sync check; the automatic-control behaviour is
  documented in `docs/regulatory.md` (FCC §97.221) — you remain the control operator.
- Direct file transfer (new, opt-in): send a file to a connected peer over the air with an
  offer/accept prompt, a progress bar, and optional size-gated auto-accept. Every transfer is
  signed and checksummed and verified against the peer's identity key on the way in — a tampered
  or wrong-sender file is quarantined and flagged UNVERIFIED. Large files are split into blocks and
  can resume after a dropped session. Enable it under `[file_transfer]`; drive it from the panel's
  **Files** tab or `openpulse daemon send-file` / `openpulse daemon files`.
- Faster, more reliable links on good conditions: the adaptive rate ladder now climbs into the
  high-throughput dense modes (it was previously capped mid-ladder by a signal-quality estimator
  that flattened out), the HF ladder switched to OFDM for better performance on fading paths, and
  repeated retransmissions now combine their soft information instead of being retried cold.

## v0.3.0 — 2026-06-29

- Authenticated connections: when you connect to a peer, the daemon now exchanges a
  signed handshake over the air and verifies the peer's identity and Maidenhead grid.
  The verified grid is written to your ADIF logbook. Set your station signing key with
  `[station] identity_key_path` (an Ed25519 seed; auto-generated on first run).
- ARDOP adaptive ARQ (opt-in): set `[ardop] enable_adaptive_arq = true` to let the rate
  ladder adapt over the link and make the host `ARQBW` (bandwidth cap) and `ARQTIMEOUT`
  (idle disconnect) hints take effect. Off by default (fixed-mode, unchanged behaviour).
- Generic serial CAT: drive a transceiver that Hamlib/rigctld doesn't support by setting
  `[radio] cat_backend = "generic"` with a serial port and a rig-definition TOML (build
  with `--features generic-serial`, Unix).
- Automatic ADIF logbook (opt-in, `[logbook]`): one record per contact
  (connect→disconnect), with the worked station's grid taken from the verified handshake
  or a `[logbook.peer_grids]` config map. Runtime `SetLogbook` toggle (CLI + panel).
- Receiver auto-notch productionized into the engine: multicarrier-aware, with persistence
  and user controls, plus automatic QSY on a confirmed in-band interferer a notch can't
  remove.
- Operator panel rework: controls moved to a resizable right side-panel with a full-width
  waterfall and the session status below it; new AGC on/off toggle alongside the
  Notch / CE-SSB / Logbook toggles and the squelch slider. Full control-surface parity
  across daemon / CLI / panel.
- linksim: I/Q constellation views (symbol-spaced crisp dots) flanking a QR-branded info
  band, regrouped Station B views with waterfall/constellation toggles, a CE-SSB toggle,
  an SNR plot, extra FEC modes (LDPC / Turbo / RS-Strong / Concatenated), and a `--serve`
  mode so the operator panel can attach to a live simulation with no radio.
- New CLI command `openpulse daemon set-tx-attenuation <db> [--band <label>]` for
  headless/scripted TX-attenuation control.
- Fix: CE-SSB is now gated off for dense OFDM higher-order modes (8PSK and above), where
  it caused a ~6 dB decode regression.

## v0.2.2 — 2026-06-25

- Live rig-meter polling (ALC / power-out / SWR) over a dedicated rigctld connection,
  surfaced as panel status for drive tuning.
- Guided ALC drive tuning: `openpulse calibrate drive` steps TX attenuation until the
  rig's ALC sits in a target band (keeps CE-SSB on dense OFDM-HOM from over-driving the PA).
- On-air SDR spectral-measurement toolset (scripts) and a one-shot twin-station demo.

## v0.2.1 — 2026-06-24

- CE-SSB transmit envelope conditioning (controlled-envelope SSB, Hershberger W9GR, QEX
  2014): an adaptive, per-mode, default-on TX conditioner for the high-PAPR multicarrier
  modes (OFDM / SC-FDMA) that raises average TX power at fixed PEP. `[modem] cessb_enabled`,
  a `SetCessb` control command, `openpulse daemon set-cessb`, and a panel toggle.
  Channel-sim **+1.6 / +2.7 / +3.8 dB** average power on OFDM52 at zero BER cost; on-air
  confirmed **+1.18 dB** (FT-991A). Believed to be the first open-source HF *data* modem to do this.
- Operator panel: Messages presented as a tab alongside the Event Log.

## v0.2.0 — 2026-06-21

- Two-station link simulator (`openpulse-linksim`, new crate): proves the **effective
  two-way transfer rate** under simulated SNR / noise / fading — real forward data frames
  through a channel, real FSK4 ACKs over a reverse channel, over-the-air rate adaptation
  along a profile ladder, and honest goodput accounting (forward + ACK air time +
  turnaround over retransmissions). CLI sweep → effective-rate table/JSON; GUI with live
  spectra/waterfalls and an SNR slider.
- Signal-path testbench: explicit 2×4 spectrum/waterfall grid (fixes unrendered
  waterfalls), all modes with **measured** per-mode bitrates, and new sources — virtual
  loop, dual-card hardware loop, test-matrix runner, and an adaptive-ladder view.
- Bandplan guardrails now recognize active `-RRC` variants and `SCFDMA52-64QAM-P4` in
  occupied-bandwidth checks; `BandplanPolicy::default()` uses `HamIaruRegion1`; Region 3
  exposes an explicit conservative-proxy warning.
- TX compliance logs reject cross-station frame metadata; session metrics publish
  throughput as an explicit upper-bound proxy with a dedicated note field.
- BL-TP-7 SC-FDMA pilot-density Doppler review coverage (dense vs sparse pilots under
  deterministic Watterson channels).
- `qpsk-plugin` demodulation uses lower-overhead carrier/downmix loops; the `QPSK1000-HF`
  equalizer profile is pinned to `mu=0.015` to match validated Watterson characterization.
- On-air orchestration scripts (`onair-preflight.sh`, `run-onair-tests.sh`,
  `onair-bundle-evidence.sh`) with `--help`, default local preflight, preflight metadata
  in reports, and structured evidence bundles (incl. repo-state traceability).
- Adaptive-rate ACK-UP progression skips unmapped reserved profile rungs; SNR-gated
  admission limited to HPX wideband-HD SL13→SL14.
- Project docs organized under `docs/` with consistent frontmatter; PR docs validation and
  automatic `last_updated` stamping.

## v0.1.0

- First public OpenPulseHF release.
- Introduced plugin-based modem architecture in a Cargo workspace.
- Included BPSK mode support and loopback-based testing path.
