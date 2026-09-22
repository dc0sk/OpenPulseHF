---
project: openpulsehf
doc: docs/dev/reviews/review-1433-gpu-crossfade.md
status: resolved
last_updated: 2026-09-22
---

# Review provenance — the GPU crossfade-cancellation defect (#1433)

This defect was **not found by reading the GPU code**. It was found by an adversarial review of an
unrelated design — the #1428 union — which asked what would have to be true for that design to work
on the shipped binary. The answer was that it would not, and why.

Recording the provenance because the finding's shape is more reusable than the fix.

## Prompt

Not a review of this defect — there was no prompt about the GPU path, which is the point. The
defect surfaced inside a review of the **#1428 union design**, whose prompt asked where to seam the
union and with what scope, and closed with: *"Anything else wrong or unproven in the framing above —
in particular any place I have asserted a property of the code that is not actually there."*

That last clause is what produced it. The framing asserted that the coded receive path takes the
cancelled arm; the reviewer checked the assertion against the shipped build configuration rather
than the source alone, and found it false on any host with a GPU adapter.

## Verdict

Confirmed, and independently re-derived before any action was taken (table above). The union design
as proposed would have been **degenerate on the binary that runs on air**, because both of its arms
are uncancelled there — and the more consequential half is unrelated to the union: an 84-frame swing
at −2 dB AWGN, live since 2026-07-13.

Two of the review's other findings were also confirmed and changed the union's design: the proposed
seam (`receive_from_samples_with_fec_inner`) is not on the path the #1428 A/B measured, and there is
no single hard-decode seam to place it at — `stage_demodulate_payload` has eleven callers across
nine distinct frame-decode chains. Those are recorded against #1428, not here.

## Consumer

- `plugins/bpsk/src/demodulate.rs` `bpsk_demodulate_with_gpu` — the site.
- `crates/openpulse-daemon/src/server.rs:142` — registers `BpskPlugin::with_gpu(ctx)` when an
  adapter exists; `Cargo.toml:28` is `default = ["gpu"]`. This is what makes it a shipped defect
  rather than a latent one.
- #1428's union and #1429's arm question both assume the coded path is the *cancelled* arm. On a GPU
  daemon it was not, so both issues' premises were wrong there.

## Prior art

`gh issue list --state all --search "crossfade GPU"` returned nothing; the control search on
`crossfade` alone returned #1363, #1429, #1361, #1428, #923 — so the filter works and there was no
existing issue. #1080's GPU-divergence sweep found three defects (I-only timing search, psk8's
reversed LLR bit order, modulator drift) and not this one.

## Twins

Swept, with a reason for each rather than an absence: `psk8_demodulate_gpu` returns `None` for
non-RRC modes (`plugins/psk8/src/demodulate.rs:377`) and RRC does not crossfade; `qpsk`'s
`demodulate` never dispatches to the GPU; `64qam` has no crossfade canceller at all. BPSK was the
only affected plugin.

## What the review contributed, and what I verified before acting

The review asserted the defect and the daemon's exposure. Per the standing rule that a subagent's
structural claims are not taken at face value, every step was re-derived against the source before
anything was filed or changed:

| claim | how it was checked |
|---|---|
| GPU branch does not cancel | read `demodulate.rs:522-585`; control — the CPU path at `:114` does |
| daemon defaults to GPU | `Cargo.toml:28`, `server.rs:126/142` |
| `demodulate` dispatches to it | `lib.rs:112-116` |
| the existing gate cannot see it | ran it: 4–20 dB, 27 B, "byte-level disagreements 0" |
| it actually costs frames | measured: 0 dB → CPU 11/16, GPU **0/16** |

The last row is the one that mattered. The review argued from processing gain that the fixture was
too easy; the measurement showed **4 dB is the first SNR at which the difference vanishes**, i.e.
the existing fixture's easiest cell sits exactly on the boundary. That is a sharper statement than
the argument, and it is what the new test's cells are chosen from.

## The lesson worth keeping

An equivalence test between an accelerated path and its reference must run in a cell where **the
property under test decides the outcome**. Comparing decode success where both arms succeed
certifies nothing — and "both arms succeed" is the comfortable place a fixture drifts to, because it
is also where the test is fast and never flakes. The probe: *at what input does this test's
assertion start to depend on the thing it is named after?* If the answer is "outside the swept
range", the test is decorative.

This is the artificially-easy-fixture archetype with an accelerator twist: the reference arm hid the
defect, because the test only ever asked whether the two agreed, never whether either was right.
