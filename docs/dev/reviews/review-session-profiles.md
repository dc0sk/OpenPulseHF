---
project: openpulsehf
doc: docs/dev/reviews/review-session-profiles.md
status: resolved
last_updated: 2026-10-01
---

# Adversarial review — session profiles design (`fast` / `robust`)

One round by Fable, held before implementation (mandatory: the handshake carries `profile_name`).
The reviewer was read-only, edited no files, and was prompted for falsification. Review target: the
draft `docs/dev/design/session-profiles.md` and work plan decisions 18–19. The maintainer's choices
(two profiles, one ladder capped at SL6, ten deleted, no aliases) were out of scope; only the
mechanics were reviewed.

## Consumer

The design doc's own Consumer section: `[modem] profile` → `server.rs:234`; `[ardop] adaptive_profile`
→ `openpulse-ardop/src/main.rs:113`; CLI `--profile`; `StartOtaSession`; the handshake fields.
Found by `grep -rn "by_name\|profile_name" --include=*.rs crates`.

## Prior art

`docs/dev/design/ladder-versioning.md` (the fingerprint excludes local policy);
`OtaRateController::set_level_bounds`; `RateAdaptationPolicy::set_max_tx_level`. Found by
`grep -rn "max_level\|set_max_tx_level" --include=*.rs crates`.

## Twins

`bandplan::occupied_bandwidth_hz` vs `ModemPlugin::occupied_bandwidth_hz`. Found by
`grep -rn "fn occupied_bandwidth_hz" --include=*.rs`.

## Prompt

Falsify the mechanics, not the maintainer's choices. Check against the code, true/false with
file:line:
1. Does excluding `max_level` from `fingerprint()` keep adaptive OTA for a fast↔robust pair, in both
   directions? Where could a level above SL6 still leak?
2. The ARDOP path: is the cap at `start_session` enough, or does ARQBW raise it?
3. Does SL6 misbehave as the top rung?
4. Is `profile_name` more than logging, and is its value a wire change under the v0.17 declaration?
5. The bandplan twin claim and its consequence.
6. What does the deletion break that the doc misses: CI benchmark, linksim, testmatrix, release-check
   tests, scripts, config examples, acceptance rows, `requirements.yaml`?
7. Is `robust` described honestly?

Every grep names a positive control. Does this block Release 1? If not, say so.

## Verdict

**Fix design first → all six fixes folded in.** Does not block Release 1 (`fast` is byte-identical to
today's `hpx_hf`), but the draft as written would have shipped a `robust` that silently un-caps.

1. **Cap leak (FALSE as designed).** `set_level_bounds` overwrites `max_level` (`ota_rate.rs:201-203`);
   it is called for `ota_min_level` alone (`server.rs:255-257`) and by `OtaSetLevelBounds` with `None`
   (`lib.rs:2934`). → **Fixed:** a separate `profile_cap`, `hi() = min(...)`, with tests that fail on
   the old seeding.
2. **ARDOP raise (FALSE).** `set_max_tx_level` overwrites on every ARQBW change (`rate_policy.rs:59-64`,
   `bridge.rs:300-303`). → **Fixed:** the same split, and `defined_modes()` is capped. The ARDOP TNC also
   has no `mfsk16` plugin, while both profiles map SL1 = MFSK16, so NACK exhaustion would land on a dead
   rung. → **Fixed:** ARDOP floors at SL2.
3. **Phantom climb.** When SL6 is the top, its ceiling makes the decision read `ClimbOnSnr` on every
   frame (`ota_rate.rs:504-511`), which pollutes the A2 audit. → **Fixed:** report `Hold` at the cap.
4. `profile_name` is logging-only in production (`lib.rs:2184`), so its value is not a wire change.
   Tests and the KAT pin the literal → **listed** as migration items.
5. The twin is TRUE. The consequence was stated wrong: `ARQBW 2000` stops at SL5 even with a corrected
   table, and the loss is at ≥ 2032 Hz. → **Corrected** in the design and the work plan.
6. **Misses:** the `roadmap_profile_table` test, the `hpx_wideband` QRM apparatus, the testmatrix
   use-case keys and the scripts. → **Listed.** The benchmark and `requirements.yaml` name no profile.
7. "Tolerant of drifting oscillators" is unmeasured. → **Dropped**, and recorded as UNCHECKED.
