---
project: openpulsehf
doc: docs/dev/design/ladder-versioning.md
status: living
last_updated: 2026-10-15
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

This is not hypothetical. PR #611 added per-level FEC to `hpx_hf` (SL7–SL11), so a pre-#611 station
and a post-#611 station mapped SL7–SL11 to different (mode, FEC) pairs. The fingerprint below was
built to detect exactly that.

**Today (2026-10-15)** there is one ladder, `hpx_hf` (SL1–SL14, `crates/openpulse-core/src/profile.rs`),
run as two profiles: `fast` (the whole ladder) and `robust` (the same ladder capped at SL6). The cap is
local policy, not mapping, so `fast` and `robust` have the same fingerprint and interoperate. The ten
earlier profile names were deleted on 2026-10-01 with no aliases.

## Policy: freeze published ladders; version changes

**Before Release 1 (`v0.17.0`, now):** `v0.16.0` is tagged but the v0.17 wire format makes no
compatibility promise to it (`protocol-wire-spec.md`), so there are no in-field peers to stay
compatible with. The ladder — including a mode's internal DSP (constellation, pulse shaping) — may be
**reworked in place** as often as needed; the goal is to reach the first release with a *single*
ladder, not a versioned family. Every station rebuilds from source, so no diversity exists. (Example:
PR #616 re-Gray-coded the cross-32QAM constellation in place, and the #1062 flag day (2026-10-10)
replaced the BPSK63/100/250 preamble with PN-63 — both wire changes the fingerprint does *not* detect,
since it hashes mode *strings*, not a mode's internal DSP. Fine pre-release; see the caveat below.)

**From Release 1 on, a published ladder is a wire contract. Do not mutate it in place.**

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

- The daemon logs its active ladder fingerprint at OTA startup (`ladder_fingerprint=…`, `server.rs`). Operators can
  diff it across two stations to confirm they will interoperate.
- **Caveat — the fingerprint covers mode *strings*, not a mode's internal DSP.** Two builds that map
  `SL10 → "OFDM52-32QAM"` fingerprint the same even if the *constellation bit-mapping or pulse
  shaping* of that mode changed on the wire (as in #616). Pre-release that's harmless; **post-release,
  a mode's PHY is also frozen** — a wire-affecting DSP change to a shipped mode must ship under a new
  mode name (so the fingerprint moves), exactly like a ladder change ships under a new profile name.
- **Handshake guard (shipped):** the signed `ConReq`/`ConAck` advertise `(profile_name,
  profile_fingerprint)` in the signature-covered body (empty name and fingerprint `0` mean "not
  advertised"; `protocol-wire-spec.md` §3.1–3.2). On a completed handshake the
  daemon compares the peer's fingerprint to ours and records `VerifiedPeer.profile_compatible`. On a
  **positive mismatch** (both sides advertised, fingerprints differ) it disables OTA rate-stepping for
  that peer (`RuntimeControlState::ota_suppressed_by_peer` → fixed-mode fallback for both TX and RX) —
  detection instead of silent desync. OTA **without** a handshake, or with a compatible / un-advertised
  peer, is unaffected (the guard fires only on a positive mismatch), preserving today's behaviour.

### Compatibility characteristic

Since the binary handshake (#1147, #1189) the two fields are always present in the signed body; there
is no JSON form and no skip-serialization, so the earlier "byte-identical to legacy when unset" property
no longer applies. A station built before the binary handshake cannot decode a current handshake at
all (`WIRE_VERSION` `0x01`, frozen until 1.0). Rebuild both ends from the same commit.

## Chosen approach (2026-07-01)

Among the options considered — (A) freeze + version + handshake guard, (B) self-describing rate frames
carrying `(mode-id, fec-id)` instead of a level, (C) fingerprint mismatch-guard only — **(A) was
selected**: the standard MCS-table discipline, giving real cross-version interop for shared versions
at moderate cost. (B) remains the most robust option if frequent cross-version links on the same path
ever become a hard requirement; it can be layered on later without discarding (A).
