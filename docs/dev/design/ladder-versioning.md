---
project: openpulsehf
doc: docs/dev/design/ladder-versioning.md
status: living
last_updated: 2026-07-01
---

# Rate-ladder versioning & backward compatibility

## The hazard

Over-the-air adaptive rate control is **receiver-led with an absolute level number**: the data
receiver measures SNR, picks a target `SpeedLevel`, and ships that level in
`AckFrame.recommended_level`; the sender adopts it (see `crates/openpulse-core/src/ota_rate.rs` and
[[adaptive-arq-lockstep-next]]).

A level number is only meaningful if **both stations agree what each level means** — i.e. both run the
same `SessionProfile` mapping `SpeedLevel → (mode, FEC)`. If two code versions define the *same
profile name* differently, they silently disagree: the sender transmits *its* SL7, the receiver tries
to decode *its* SL7, and the exchange fails or the ladder thrashes.

This is not hypothetical. PR #611 added per-level FEC to `hpx_hf` (SL7–SL11). A pre-#611 station and a
post-#611 station now map SL7–SL11 to different (mode, FEC) pairs — a wire incompatibility with no
detection today.

## Policy: freeze published ladders; version changes

**Pre-first-release (now):** there is no shipped release, so there are no in-field peers to stay
compatible with. The ladder — including a mode's internal DSP (constellation, pulse shaping) — may be
**reworked in place** as often as needed; the goal is to reach the first release with a *single*
ladder, not a versioned family. Every station rebuilds from source, so no diversity exists. (Example:
PR #616 re-Gray-coded the cross-32QAM constellation in place — a wire change the fingerprint does *not*
detect, since it hashes mode *strings*, not a mode's internal DSP. Fine pre-release; see the caveat
below.)

**At and after the first release, a published ladder is a wire contract. Do not mutate it in place.**

- Changing any `(level → mode)` or `(level → FEC)` entry of a shipped profile, or adding/removing a
  step, is a **breaking wire change**. It must ship as a **new named profile** (e.g. `hpx_hf_v2`),
  leaving the old one byte-identical, so un-upgraded peers keep working and upgraded peers can offer
  both and negotiate the common one.
- **Local-only policy is NOT part of the contract** and may change freely: SNR floors/ceilings,
  `nack_threshold`, aggressiveness gates. These only affect *when* a station recommends a level, not
  *what mode* a level denotes — two peers with the same mapping but different floors interoperate.

## Mechanism: ladder fingerprint

`SessionProfile::fingerprint() -> u64` is an FNV-1a hash over the **wire-relevant** mapping
(`level → mode`, `level → FEC`) and *nothing else* (floors/ceilings excluded by construction). It
changes iff the mapping changes, so it detects ladder drift automatically — a station can't forget to
"bump the version".

- The daemon logs its active ladder fingerprint at OTA startup (`ladder_fingerprint=…`). Operators can
  diff it across two stations to confirm they will interoperate.
- **Caveat — the fingerprint covers mode *strings*, not a mode's internal DSP.** Two builds that map
  `SL10 → "SCFDMA52-32QAM"` fingerprint the same even if the *constellation bit-mapping or pulse
  shaping* of that mode changed on the wire (as in #616). Pre-release that's harmless; **post-release,
  a mode's PHY is also frozen** — a wire-affecting DSP change to a shipped mode must ship under a new
  mode name (so the fingerprint moves), exactly like a ladder change ships under a new profile name.
- **Handshake guard (shipped):** the signed `ConReq`/`ConAck` advertise `(profile_name,
  profile_fingerprint)` in the signature-covered body (skip-serialized when unset, so un-advertised
  frames stay byte-identical to legacy — full signature compatibility). On a completed handshake the
  daemon compares the peer's fingerprint to ours and records `VerifiedPeer.profile_compatible`. On a
  **positive mismatch** (both sides advertised, fingerprints differ) it disables OTA rate-stepping for
  that peer (`RuntimeControlState::ota_suppressed_by_peer` → fixed-mode fallback for both TX and RX) —
  detection instead of silent desync. OTA **without** a handshake, or with a compatible / un-advertised
  peer, is unaffected (the guard fires only on a positive mismatch), preserving today's behaviour.

### Compatibility characteristic (same as the earlier `grid`/`fec`/`compression` fields)

`skip_serializing_if` makes the canonical JSON byte-identical to a legacy frame **only when the field
is unset** (name `""`, fp `0`). When a station *does* advertise a ladder, the signed canonical
includes the new fields, so a **pre-#615 verifier — which can't reconstruct those fields — will fail
the signature** and reject the frame. This is the established trade-off for every additive signed
field here, and it degrades gracefully: the signed handshake is *additive* (the local `ConnectPeer`
trust eval and un-guarded OTA still run), so a new advertising station and an old station simply fall
back to today's un-verified, un-guarded behaviour rather than mis-decoding. Two pre-#615 peers, two
post-#615 peers, and old→new(non-advertising) all verify normally.

## Chosen approach (2026-07-01)

Among the options considered — (A) freeze + version + handshake guard, (B) self-describing rate frames
carrying `(mode-id, fec-id)` instead of a level, (C) fingerprint mismatch-guard only — **(A) was
selected**: the standard MCS-table discipline, giving real cross-version interop for shared versions
at moderate cost. (B) remains the most robust option if frequent cross-version links on the same path
ever become a hard requirement; it can be layered on later without discarding (A).
