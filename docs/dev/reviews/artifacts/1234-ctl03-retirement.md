---
project: openpulsehf
doc: docs/dev/reviews/artifacts/1234-ctl03-retirement.md
status: review
last_updated: 2026-09-16
---

# #1234 — retiring REQ-CTL-03's id, and gating REQ-CTL-04's fallback clause

Design-class: changes a registered requirement statement and adds a selection seam.

## Prompt

Sent to Fable 5.1 as an adversarial review of a design packet proposing `select_backend(pref, bool)`
bound to a restated REQ-CTL-03, with five questions and three assumptions asked for falsification.
The prompt said explicitly that the maintainer had rejected retiring the id, and asked the reviewer
to say so plainly if the evidence pointed at retirement anyway rather than soften it to be agreeable.

## Verdict

**The proposal was refuted as laundering, and the recommendation was the option the maintainer had
rejected. It went back as a reversal request with reasons, and the maintainer reversed.**

The argument that carried it: `select_backend`'s only non-enum input is a boolean the test
fabricates, and the code joining the selector to the OS store must sit behind
`cfg(feature = "keychain")`, which the gate's `--no-default-features` never compiles. A green
REQ-CTL-03 would therefore rest on a 3×2 truth table over two enums — **strictly less evidence than
the `file_store_get_set_delete_round_trip` binding #1229 already refused for this same id**, on the
ground that it attests a different implementation. `select_backend` attests none.

Three supporting findings, each checked against the source:

- **The #1112 precedent was cited backwards in my own packet.** REQ-PHY-05 is `baseline`,
  grandfathered, has no `// VERIFIES:` binding, and `CLAUDE.md` says in words that the existing test
  is not evidence for it. Its shape is *keep the requirement, bind nothing, name the gap* — the
  opposite of *restate to the verifiable half and bind that*.
- **Restating a requirement to fit a test defeats its own check.** `trace.py:56-63` states that
  germaneness is "a manual norm, unchecked by anything here", so an admissibility rule of the form
  "the restatement will say what the test proves" is satisfied by construction.
- **Two omissions in the packet**: `openpulse-keystore` is workspace-dormant, so a binding there can
  be at most `unwired` until the daemon depends on it (and then `UNWIRED-BUT-REACHED` fires); and
  wiring a reader without a writer — there is no `openpulse-cli keystore set` — leaves the path
  operator-unreachable.

## What I checked afterwards, and what it changed

Review said the fallback property "is REQ-CTL-04's statement verbatim". **That is true of the prose
and false of the registry.** `requirements.md:153-156` carries the fallback clause; the yaml
statement had kept the encryption half and dropped it. Binding a selection test to the registered
statement would have been the same laundering one id over, so the clause was **restored** to the
registry from the requirement's own prose — a restoration, not a narrowing.

Also corrected: the reviewer's `Q2` answer (the `available()` probe is adequate for open-time
selection, because the daemon's operation is `get` and no writer exists) was accepted, and its rule —
any post-selection `get`/`set` error is surfaced, never a second silent fallback — is recorded here
for the wiring PR rather than implemented now.

## Consumer

None yet, and that is the point: `openpulse-keystore` has no dependent crate
(`grep -rn openpulse-keystore` over the workspace `Cargo.toml` files finds the root member entry and
the workspace dependency declaration, nothing else). The items are therefore `pub(crate)` with their
gate as an in-crate unit test, exactly as #1310 PR1b scoped `decode_burst_with_fec`. The intended
consumer is the daemon's `load_control_psk` (`server.rs:2075`), whose companion
`inert_psk_key_id_warning` (`:2059`) currently warns that `psk_key_id` is read by nothing.

## Prior art

`build_audio_backend` (`openpulse-daemon/src/server.rs:2095-2117`) is the shape copied: always
compiled, the feature arm INSIDE, and a warning on the absent-feature path rather than a function
that vanishes with the feature. #1229's retirement of REQ-CTL-06 is the precedent for retiring an id
while keeping its requirement. #1380 is the defect class the `cfg` placement avoids.

## Twins

REQ-CTL-01/02 (linksec, the PSK's consumer) and REQ-CTL-05 (owner-only permissions) are the sibling
control-channel requirements; CTL-05 is `enforced` and unaffected. The other secrets that would want
the same selection — the identity seed (`openpulse-config`'s `load_identity_from`), the trust store
(`openpulse-cli/src/state.rs`), and the panel's env-only PSK (`transport.rs:63-66`) — are **not**
routed through the keystore today, and the client half of REQ-CTL-03's prose ("both the daemon and
the clients") is the part a restatement would have silently dropped.
