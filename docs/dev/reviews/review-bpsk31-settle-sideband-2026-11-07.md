---
project: openpulsehf
doc: docs/dev/reviews/review-bpsk31-settle-sideband-2026-11-07.md
status: resolved
last_updated: 2026-11-07
---

# Review: the BPSK carrier estimate lands one sideband off (2026-11-07)

Design review by Fable before implementing, because the change replaces the coarse stage of the
carrier estimator on every BPSK receive (a DSP-path change with a new constant), and it is the fix
for an on-air A2 failure on the Release 1 entry rung. Design:
`docs/dev/design/bpsk31-settle-sideband.md`.

## Prompt

On-air run f2j (task 30, 2 m, B→A) lost B's first three SL2 frames (`BPSK31|rs`, 64 B, 66.6 s): A's
cold trigger settle read +7.21, −24.86 and −25.31 Hz against a carrier at about −9 Hz, the decode
ran past the ACK deadline, and B abandoned a message. The design attributes this to the coarse
stage's 12.5 Hz Goertzel picket grid picking a structural line of the squared `--++` preamble, and
proposes a dense, zero-padded FFT peak search of the squared spectrum in its place.

The reviewer was asked to falsify:
- **the mechanism**: re-derive the sideband lattice, the line levels and the fine stage's wrap from
  the code, and re-measure on the recordings, not on the note's tables;
- **the prototype-versus-design equivalence**: the measured fix was a 0.25 Hz Goertzel grid, the
  proposed one an FFT; show where they differ;
- **negative controls**: noise windows, CW tones and a second BPSK signal, where a dense scan finds
  what the picket missed by luck;
- **scope**: which paths and rungs the estimator change reaches, and which it was measured on;
- **the gates and sabotages**: whether each can fail, and whether the replay's construction matches
  `server::run`;
- **the lesson** the design draws (a decode does not certify a settle);
- **option 2** (seed the AFC from the previous session), which the note does not recommend;
- **"Does this block Release 1? If not, say so."**

## Verdict

**APPROVE WITH CHANGES.** The mechanism is confirmed, the fix is the right one, and eight changes are
required before implementing.

**Key measurements (review, re-run on the recordings):**
- **Real bursts.** Shipped settles +7.21 / −24.86 / −25.31 Hz on f2j #1–3, against dense −8.45 /
  −9.20 / −9.65.
- **The modem-level replay with the daemon's construction.** Unpatched, f2j #1–3 all decode false,
  with 152 decode attempts past the deadline and 7.64 s. Patched, all true in 1.52–1.67 s, AFC
  −8.51 / −9.25 / −9.68. f2h #1's post-decode AFC moves from +1.09 (a sideband tracking update) to
  −3.98.
- **The synthetic sweep, 492 cells.** Shipped right / sideband / Rs/4 / other = 228 / 165 / 37 / 62;
  dense 492 / 0 / 0 / 0.
- **BPSK63** (SL3, the anti-aligned rung): 24 wrong accepted settles → 124/124 right.
- **Watterson `moderate_f1`**: 240/240 settles right with dense.

**The eight required changes:**
1. **The convergence check changes behaviour; state it and gate it.** A dense peak is stable under
   re-centring, so noise settles now converge, where the picket's rejection of them was an
   accident. No-frame windows, shipped against dense: f2j hiss 2/120 vs 116/120; corpus
   `ic9700-idle-wide-500hz-control` 0/118 vs 113/118; BPSK250 on the same hiss 5/120 vs 99/120.
   Inert on the OTA arm (four foreign-burst / noise-step fixtures identical with and without the
   patch), but NOT shown inert on the CLI `receive_with_timeout` path or on BPSK250's veto counters.
   Required: the measurement in the design; a noise-window gate (idle corpus, settle acceptance
   rate, both estimators); a CLI-path gate on `coded_noise_settle_recovery`'s condemnation counts,
   not only its decode; the BPSK250 veto counters as a gate.
2. **Specify the FFT form by probes, not bins, and guard the unfolded frequency.**
   `goertzel_carrier_scan` folds each probe into [0, fs/2] before its Nyquist guard, so after
   re-centring at fc + c with c ≥ 300 the carrier's mirror sits inside ±400 Hz at the same folded
   bin. An emulated dense path diverged at +300/+350 Hz; a bin-range FFT lost the carrier at +380.
   Required: iterate δf on the bin grid (step binw/m) with the Goertzel's fold and guards per probe,
   lowest δf first, ties to the smallest |δf|; for m = 2 the `fs/2 − 150` guard on the unfolded
   frequency; ±300/±350/±399 Hz sweep cells run through the mini-settle.
3. **The sideband margin is 11.8 dB, not 6 dB.** Squared-domain lines at baseband ±Rs/4 are
   −11.8 dB and at ±Rs/2 −15.6 dB (the `--++` preamble's Fourier coefficients 3/4, 2/(3π), 1/8); on
   the real windows −10.7…−11.2 and −14.5…−16.0 dB.
4. **Fix the mechanism text.** The sidebands are structural lines, and which one the picket hears is
   alignment arithmetic on a 25 Hz squared-domain grid: BPSK250 (Rs = 10 steps) and BPSK100 (4) are
   immune, BPSK63 (2.5) is anti-aligned, BPSK31's lines sit 0.625 and 1.25 steps off. The Rs/4 and
   Rs/2 classes are the same coarse pick, because the fine stage wraps at ±Rs/4.
5. **Burst 4 was luck, not a warm-up.** At 12 of 13 window starts across f2j burst 4's first 0.8 s
   the shipped settle reads −25.37 (a sideband); only the +5632 start reads −9.74.
6. **Gate 5 lists suites that do not run in the default run** (`carrier_offset_matrix`,
   `freq_acquire_accuracy`, `chain_veto_slow_rung`, 10 of 11 `preamble_veto_interference` tests).
   Say where each runs, and pin BPSK100's attempt counts (6/6 at 0 Hz and +50 Hz).
7. **Pin the engine construction in code, and shrink the cuts.** Nine plugins in `server::run`'s
   order, `mode = "BPSK250"` to both calls, the notch as the daemon configures it, CE-SSB on, no
   lock, 40 s of the recording's own hiss ahead. A harness with an SL2 lock,
   `accumulate_capture(Some("BPSK31"))` and five plugins decoded NOTHING, either way. One full f2j
   cut (#2 or #3) for the deadline/attempt gate; short settle cuts (lead + 8448 samples, ~50 KB) for
   f2j #1–3, f2h #1 and f2i #1; the sweep by reference (`bpsk_modulate`). A modem-level gate through
   `ota_decode_burst` with `set_ota_ack_deadline` and `decode_attempts_past_deadline()` is enough.
   Up to about 5.5 MB of captures is acceptable to the maintainer; use the minimum.
8. **Tone and two-signal controls**, measured with the notch off, Δ = −9 Hz, SNR 12: tone +6 dB,
   both estimators lock the tone (not worse); **tone 0 dB, dense converges on the cross-term line
   (+54.6 Hz, accepted) where shipped rejected it**; tone −6 dB, dense right, shipped rejected; two
   BPSK31 frames at +200 Hz, both can lock the cross term at +95.5 Hz, dense reliably. Gate it, with
   the daemon's notch on; if the notch does not remove the 0 dB case, state it as a known limit, not
   worse in kind than today's tone lock at +6 dB.

**Smaller edits:** state gate 6's equivalence numbers (FFT against a 0.25 Hz Goertzel within
0.021 Hz; raw bin within 0.061 Hz); close the Parked note in `ota-scan-onset-major.md` (confirmed,
−16.0 Hz); add to the playbook "a decode does not certify the settle". Park, one line each,
QPSK250-D's coarse stage (data-aided, 42/122 wrong accepts; measure the decode impact at SL6 cold)
and the deferred exhaustive fallback past the deadline (a separate change needing the mandatory
keying review; the bound is a tripwire-counted skip).

**The lesson** stands as written: a decode does not certify the settle. BPSK31's native reach hides
±4 Hz, and the fine stage's residual reads 0 at a sideband, so a settle is judged against the
decoded carrier.

**Option 2** (seed the AFC from the previous session): agreed, not for Release 1. It cannot reach a
first contact, and the value it would seed comes from the same ambiguous estimator.

**Release 1: blocks.** SL2 is the entry rung every session starts on, and A2 is scored on it. Land
before A2 is re-scored on one build and before the HF confirmation.

## Consumer

`ModemEngine::update_afc_estimate` (`crates/openpulse-modem/src/engine.rs` ~9474), the only
production caller of `ModulationPlugin::estimate_afc_hz`. For BPSK it reaches
`plugins/bpsk/src/lib.rs` `estimate_afc_hz` → `demodulate::afc_estimate_hz` →
`estimate_carrier_hz_wide`. The daemon reaches it from `server::run`'s rx tick →
`nack_budget::decode_ota_burst` → `ModemEngine::ota_decode_burst` → `acquire_at_onset` →
`afc_mini_settle`. Found with
`grep -rn "estimate_afc_hz(\|afc_estimate_hz(\|estimate_carrier_hz_wide(" crates plugins --include=*.rs | grep -v /tests/`
and `grep -n "update_afc_estimate(" crates/openpulse-modem/src/engine.rs`.

## Prior art

`docs/dev/design/ota-scan-onset-major.md`, *Parked* (the corpus capture's −12.8 against +3.2 Hz,
"possibly a preamble sideband"; now closed). `docs/dev/research/acquisition-comparison-codec2-ft8.md`
§1.3 and §6 (FT8's exhaustive but cheap grid). The playbook §3 and §5. The QPSK plugin's
`coarse_scan_preamble` comment (4th-power crossfade lines). Found with
`grep -rn "trigger settle\|settle.*sideband\|15.6 Hz" docs/dev --include=*.md`.

## Twins

`plugins/pilot/src/demodulate.rs:208` uses the same `goertzel_carrier_scan(…, 2, 400.0, 12.5)`
picket grid; not on `hpx_hf`, parked post-release. QPSK250-D (SL6) has a different, data-aided coarse
stage with its own ambiguity; parked with a measurement owed. psk8 / 64qam / scfdma do not call
`goertzel_carrier_scan`; UNCHECKED, none is on the `hpx_hf` cold path. Found with
`grep -rn "goertzel_carrier_scan(" plugins crates --include=*.rs`.
