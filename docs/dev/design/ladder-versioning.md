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

**A published ladder is a wire contract. Do not mutate it in place.**

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
- **Handshake guard (shipped):** the signed `ConReq`/`ConAck` advertise `(profile_name,
  profile_fingerprint)` in the signature-covered body (skip-serialized when unset, so un-advertised
  frames stay byte-identical to legacy — full signature compatibility). On a completed handshake the
  daemon compares the peer's fingerprint to ours and records `VerifiedPeer.profile_compatible`. On a
  **positive mismatch** (both sides advertised, fingerprints differ) it disables OTA rate-stepping for
  that peer (`RuntimeControlState::ota_suppressed_by_peer` → fixed-mode fallback for both TX and RX) —
  detection instead of silent desync. OTA **without** a handshake, or with a compatible / un-advertised
  peer, is unaffected (the guard fires only on a positive mismatch), preserving today's behaviour.

## Chosen approach (2026-07-01)

Among the options considered — (A) freeze + version + handshake guard, (B) self-describing rate frames
carrying `(mode-id, fec-id)` instead of a level, (C) fingerprint mismatch-guard only — **(A) was
selected**: the standard MCS-table discipline, giving real cross-version interop for shared versions
at moderate cost. (B) remains the most robust option if frequent cross-version links on the same path
ever become a hard requirement; it can be layered on later without discarding (A).
