---
project: openpulsehf
doc: docs/mode-fec-ladder.md
status: living
last_updated: 2026-07-17
---

# Mode and FEC ladder — how the modem chooses a waveform and a code

OpenPulseHF picks an operating point along **two independent axes**:

1. **Modulation** — the waveform and constellation. Trades spectral efficiency
   (bits/s/Hz) against the SNR needed to keep symbol errors low.
2. **Forward error correction (FEC)** — trades net throughput (coding overhead)
   against the channel error rate it can clean up.

A link's usable operating point is the highest-throughput combination whose
**post-FEC** error rate is acceptable at the link's SNR and fading profile. The
adaptive rate controller walks the modulation axis automatically; the FEC is
chosen per session/use-case (and, for the dense modes, is not optional).

---

## 1. The modulation ladder (least → most demanding)

Two knobs set how demanding a mode is:

- **Constellation order** — bits per symbol. Each step roughly doubles the data
  but needs more SNR and a tighter phase/timing lock:
  BPSK (1) → QPSK (2) → 8PSK (3) → 16QAM (4) → 32QAM (5) → 64QAM (6).
- **Occupied bandwidth** — baud rate (single-carrier) or subcarrier count
  (SC-FDMA/OFDM). For a *fixed transmit power*, a **narrower** signal puts more
  power per Hz, so it needs less SNR — at the cost of throughput. This is why
  `SCFDMA26-*` (≈1 kHz, 26 SCs) is ~3 dB more robust than `SCFDMA52-*`
  (≈2 kHz, 52 SCs) at the same constellation.

Approximate clean-AWGN SNR each constellation needs to decode (uncoded, before
FEC), measured in sim on the SC-FDMA path:

| Constellation | Bits/sym | ~Uncoded SNR floor | Notes |
|---|---|---|---|
| BPSK / QPSK | 1–2 | ≤ ~10 dB | Wide decision regions; very forgiving |
| 8PSK | 3 | ~13–15 dB | 45° spacing → only ±22.5° of phase margin |
| 16QAM | 4 | ~17–18 dB | |
| 32QAM (cross) | 5 | ~22–23 dB | ~5 dB easier than 64QAM |
| 64QAM | 6 | ~25–26 dB | Needs tight clock + carrier |

**Single-carrier vs multi-carrier.** Single-carrier PSK tolerates clock/timing
offset well (wide eye, carrier loop). SC-FDMA/OFDM pack more constellation into a
slice with low PAPR and pilot-aided equalization, but the DFT de-spread makes them
sensitive to residual per-subcarrier phase — handled by the per-symbol pilot
`deramp_timing` (see §5). RRC variants (`-RRC`, α = 0.35) add ~35 % bandwidth for
a cleaner spectrum and better timing recovery; the plain (Hann/rectangular) 2000-baud
modes close the eye at 4 samples/symbol and are superseded by their `-RRC` siblings.

**Coherent vs differential (`-D`), and why it decides the HF rungs.** A *coherent*,
absolutely-encoded mode carries each symbol's meaning in its absolute phase, so the receiver
must hold a carrier-phase reference for the whole frame. On a fading HF path it cannot: at a
fade null the decision-directed loop slips, and because the encoding is absolute every symbol
after the slip is rotated — the frame tail is lost. A **differential** mode (`-D`, and BPSK's
NRZI decode) encodes each symbol as a phase *increment* from its predecessor, so the fade
rotation is common to both and cancels, and a slip costs one symbol instead of the tail. The
price is ~2 dB of AWGN floor at QPSK (differential detection roughly doubles the effective
noise), and it needs FEC to mop up the symbol each slip still costs.

This is why **phase margin, not baud rate, orders the modes by fade-robustness**: non-coherent
MFSK16 > BPSK (±90°) > QPSK (±45°) > 8PSK (±22.5°). Differential pays for itself at QPSK and
**does not** at 8PSK, where ±22.5° cannot absorb the noise doubling (measured — see §4 and
CLAUDE.md → *Known sharp edges*). `MFSK16` is the non-coherent extreme: it needs no carrier
phase at all, which is why it is the sub-floor deep-fade rung (SL1 of `hpx_hf`).

**Pilot-framed single-carrier (`PILOT-*`).** A third single-carrier family
(`PILOT-{QPSK,8PSK,16QAM,32APSK}<baud>`) carries known in-band pilot symbols at a
fixed cadence and recovers the carrier from them with a data-aided loop, rather
than a decision-directed Costas loop. That makes it immune to the ±90°/±45° cycle
slips that limit dense PSK/QAM through carrier offset, and robust to soundcard
sample-rate offset without a Gardner timing loop — at the cost of the pilot
overhead. `PILOT-32APSK*` uses DVB-S2 32APSK (amplitude-bearing) geometry, with
the demapper normalising by the pilot-referenced amplitude.

> That immunity is specifically to **carrier-offset/SRO** cycle slips — it is **not**
> fade-robustness, and the pilot family is *not* the answer for a fading HF path. Measured on
> Watterson `moderate_f1` (1 Hz Doppler, 1.0 ms delay), `PILOT-QPSK500+Rs` decodes **0% at 40 dB**
> — worse than `QPSK250-D`'s 65% on the same channel — while being perfect (100%) on AWGN down to
> 10 dB. Ablation shows *both* impairments bite independently (delay-only 0.33, Doppler-only 0.21):
> at 500 baud a 1.0 ms delay spread is half the 2 ms symbol, and this family has no equalizer for
> it. Use `PILOT-*` for what it is good at — carrier offset and sample-rate offset — not for fade.

The family spans two pulse shapes and three baud rates:

- **Pulse:** the default **rectangular** pulse (integrate-and-dump; the most
  SRO-tolerant, since it averages over the whole symbol) and the **`-RRC`**
  variants (root-raised-cosine, ~half the occupied bandwidth — measured
  out-of-band power 9.9 % → 0.0 % — but it samples at a point, so slightly less
  SRO-tolerant).
- **Baud:** `500` (~675 Hz RRC), `1000` (2× throughput, 8 samples/symbol), and
  `2000-RRC` (RRC-only — rectangular 2000 baud would alias past Nyquist; ~2700 Hz,
  HF channel edge).

So e.g. `PILOT-16QAM1000-RRC` is 16QAM, 1000 baud, RRC-shaped. See the
[pilot-framed waveform](dev/design/hpx-waveform-design.md#pilot-framed-waveform) design note.

---

## 2. The FEC ladder (least → most powerful)

| FEC mode | Code | Rate | Corrects | Input | Best for |
|---|---|---|---|---|---|
| `None` | — | 1.00 | nothing | — | Clean loopback / very high SNR only |
| `Rs` | RS(255,223), t=16 | 0.875 | ≤ 6.3 % byte errors/block | hard | Light random errors |
| `RsInterleaved` | RS + block interleaver | 0.875 | 6.3 %, **burst-tolerant** | hard | HF burst/fading (Gilbert-Elliott) |
| `RsStrong` | RS(255,191), t=32 | 0.749 | ≤ 12.5 % byte errors/block | hard | Heavier random errors, hard-decision |
| `Concatenated` | Conv(½,K=3) + RS | ~0.44 | high (random) | hard | AWGN-dominant, no soft LLRs |
| `SoftConcatenated` | Soft-Viterbi(K=7) + RS | ~0.44 | **highest practical** | **soft** | Dense modes on real links |
| `Ldpc` | rate-1/2 LDPC (min-sum) | 0.50 | very high | **soft** | Short blocks (≤128 B), soft |
| `LdpcHighRate` | rate-8/9 LDPC (PEG, min-sum) | 0.89 | moderate | **soft** | Dense rungs at high SNR — throughput-first soft code (auto-selected by the HARQ policy on soft-capable modes above ~26 dB) |
| `Turbo` | rate-1/3 PCCC | 0.33 | very high | **soft** | Maximum robustness, low rate |

Two rules of thumb:

- **Soft beats hard by ~3–4 dB** when the modulation emits real LLRs. Most
  OpenPulseHF data plugins provide `demodulate_soft` (BPSK/QPSK/8PSK/64QAM and the
  SC-FDMA/OFDM families), so `SoftConcatenated` / `Ldpc` / `LdpcHighRate` / `Turbo`
  get genuine soft-decision gain on them. The pilot-framed `PILOT-*` family is
  also soft-capable (per-bit max-log-MAP LLRs from the pilot-normalised symbols),
  so the HARQ policy auto-selects high-rate LDPC for its dense rungs too.
- **Interleaving matters on HF.** Watterson/Gilbert-Elliott channels produce
  *bursts*; `RsInterleaved` spreads a burst across many RS codewords so it stays
  within per-block capacity, where bare `Rs` would fail on the same raw rate.

---

## 3. The decision process

```
        ┌─────────────────────────────────────────────────────────┐
  SNR → │ rate controller picks the SpeedLevel (→ modulation mode) │
        │   • below a level's SNR floor   → step DOWN one rung      │
        │   • above its SNR ceiling (+ACK) → step UP one rung       │
        │   • N consecutive NACKs          → step DOWN              │
        └─────────────────────────────────────────────────────────┘
  channel type ─────────────────────────────────────────────► FEC family
   • AWGN / high SNR        → None / Rs
   • HF burst & fading      → RsInterleaved (burst) or SoftConcatenated
   • dense constellation    → soft code REQUIRED (SoftConcatenated/Ldpc/Turbo)
```

1. **Estimate SNR** (from soft-LLR magnitude on RX) and feed it to the rate
   adapter, which maps it to a `SpeedLevel` in the active profile (§4). Each rung
   has an SNR floor (drop below → step down immediately) and ceiling (rise above,
   with a positive ACK → climb).
2. **Pick the FEC** for the channel character, not just the SNR:
   - flat/AWGN, plenty of margin → `None` or `Rs`;
   - HF multipath/fading (bursts) → `RsInterleaved`;
   - any dense mode (16QAM and up, all SC-FDMA HOM, 64QAM) → a **soft** code.
3. **Acceptable** = the post-FEC frame CRC passes reliably. The headline number is
   *net* throughput = `gross_bps × code_rate × (1 − retransmit_fraction)`.

---

## 4. Adaptive profiles (the SpeedLevel ladders)

Each profile is a `SpeedLevel → {mode, SNR floor, SNR ceiling}` map in
`crates/openpulse-core/src/profile.rs`. The controller starts at `initial_level`,
steps **down** when the estimated SNR drops below a rung's floor (or after
`nack_threshold` consecutive NACKs), and steps **up** when SNR clears a rung's
ceiling *and* a positive ACK arrives.

| Profile | Class | Rungs (low → high) | Use |
|---|---|---|---|
| `hpx500` | Narrowband | BPSK31 → BPSK63 → BPSK250 → QPSK250 → QPSK500 | Robust, ≤600 Hz HF |
| **`hpx_hf`** | **HF (≤2700 Hz)** | **MFSK16 → BPSK31/63/100/250 (all coded) → QPSK250-D → OFDM52 → OFDM52-{8PSK,16QAM,32QAM,64QAM} → the same 16/32/64QAM at r≈8/9 LDPC (SL1–14)** | **Primary HF profile — every rung measured to decode on a fade** |
| **`hpx_ofdm_hf`** | **HF multicarrier** | **OFDM16 → OFDM52 → OFDM52-{8PSK,16QAM,32QAM,64QAM} (SL5–10)** | **High-throughput / high-reliability HF — per-SC equalization on fades (§7)** |
| `hpx_pilot` | HF pilot-aided | PILOT-QPSK500 → PILOT-8PSK500 → PILOT-16QAM500 → PILOT-32APSK500 (SL2–5; SNR floors 6/12/17/23 dB) | Carrier-offset / sample-rate-offset-robust single-carrier ladder (cycle-slip-immune **to offset**, not to fade — see §1); soft-capable (auto-selects high-rate LDPC on the dense rungs) |
| `hpx_pilot_rrc` | HF pilot, narrowband | same ladder on the `-RRC` variants | ~half the bandwidth (RRC); same per-symbol floors. Prefer `hpx_pilot` when SRO-heavy |
| `hpx_pilot_fast` | HF pilot, high-throughput | PILOT-{QPSK,8PSK,16QAM,32APSK}**1000** (SL2–5) | 2× bits/s at the same per-symbol floors; ~2× bandwidth |
| `hpx_pilot_fast_rrc` | HF pilot, fast + narrowband | the 1000-baud ladder on `-RRC` | 2× throughput **and** ~half-band (~1350 Hz) |
| `hpx_wideband_hd` | Wideband HD | SCFDMA26-{8PSK,16QAM,32QAM} (SL9–11 fallback) → SCFDMA52-{16QAM,32QAM,64QAM} → 64QAM2000-RRC (SL12–15) | >2700 Hz links; SL9–11 are the graceful-degradation rungs |
| `hpx_wideband` / `hpx_narrowband` / `hpx_narrowband_hd` | Wide / post-1.0 | QPSK/8PSK 1000, 2000-RRC, 9600-RRC | FM / VHF / UHF or wider-than-HF; deferred (§8) |

The four `hpx_pilot*` profiles share one carrier architecture and the same
per-symbol (Es/N0) SNR floors; they trade **bandwidth** (rect vs `-RRC`) against
**throughput** (500 vs 1000 baud). `PILOT-*2000-RRC` rungs exist as selectable
modes but are not yet in an adaptive profile.

For dense multicarrier throughput on HF, **`hpx_ofdm_hf` (OFDM HOM) is preferred over the
SCFDMA52 rungs in `hpx_hf`** — OFDM handles frequency-selective fading better and the
SC-FDMA PAPR advantage that once motivated those rungs did not materialise (§7).

### `hpx_hf` — the primary HF ladder (the full ≤2700 Hz span)

This is the profile for a real HF SSB channel. It spans the **entire** mode set that
fits the 2700 Hz channel — from a non-coherent sub-floor rung up to dense OFDM — so one
adaptive session walks from weak-signal to high-throughput without switching profiles.

> **Every rung here is measured to decode on a fading HF channel** (Watterson `moderate_f1` — 1 Hz
> Doppler, 1.0 ms delay, a routine ITU-R moderate path). That is a deliberate re-seat: the ladder used
> to be calibrated on AWGN and, on a fade, most of it did not work. **Uncoded BPSK31 — the rung every
> session starts on — decoded 0 % of fading frames at every SNR tested**, and the coherent
> single-carrier mid rungs (QPSK250/QPSK500/8PSK500) decoded ~0 % at *any* SNR up to 40 dB. See the
> design points below.

The authoritative rung map is `SessionProfile::hpx_hf` in
`crates/openpulse-core/src/profile.rs`; this table mirrors it. FEC column: `Rs` = Reed-Solomon
RS(255,223), `SC` = `SoftConcatenated`, `LHR` = `LdpcHighRate`
(r≈8/9). "Net bps" is the asymptotic gross × code-rate, before retransmit cost.

> **SNR floors are per-waveform-family by physical necessity — the two scales cannot be unified.**
> The receiver-led ladder compares a single measured SNR (`ModemEngine::rx_snr_db`, which dispatches to
> each plugin's `estimate_snr_db`) against each rung's floor, but the plugins do not — *cannot* — report
> the same quantity:
> - **Single-carrier PSK** (SL2–SL6: BPSK, QPSK250-D) reports ~**true additive channel SNR**. BPSK
>   removes the multiplicative channel with a per-window gain and converts symbol-domain Es/N0 to the
>   channel scale (#934), so a true 20 dB link reads ~20 dB and the floors here are true channel SNR.
> - **Multicarrier** (SL7–SL14: OFDM) reports a **saturation-bounded plugin-domain SNR**. Its
>   zero-forcing equaliser enhances noise on faded subcarriers, so the estimate flattens near ~16 dB and
>   *physically cannot* report the 20–30 dB its top rungs operate at (a true 20 dB link reads ~14.4). The
>   OFDM floors are therefore calibrated in that plugin-domain scale, matching `hpx_ofdm_hf`.
>
> This is **not a bug to unify away**. Forcing OFDM onto a true-SNR scale would put the top rungs' floors
> above anything the estimate can ever read → the SNR path could never climb to them → the exact v0.14.0
> "AWGN-scale floors never clear" stall. What makes two scales safe is the **evidence-based climb**: the
> ladder advances on consecutive clean decodes where the SNR estimate saturates, so it reaches the OFDM
> rungs regardless (see the rate-control notes / #934). The boundary is pinned by
> `tests/snr_scale_boundary.rs` — **if a change makes OFDM's estimate track true SNR, the OFDM floors
> must be re-derived in the same change or that gate fails; the two are one decision.**

| SL | Mode | FEC | ~Net bps | SNR floor | SNR ceiling | Notes |
|---|---|---|---|---|---|---|
| SL1 | MFSK16 | Rs | ~9 | — | 5 | non-coherent sub-floor deep-fade rung (REQ-WSIG-01) |
| SL2 | BPSK31 | Rs | 27 | 3 | 6 | weak-signal floor; `initial_level` |
| SL3 | BPSK63 | Rs | 54 | 4 | 6.5 | |
| SL4 | BPSK100 | Rs | 87 | 4.5 | 7 | breaks the 54→219 bps cliff |
| SL5 | BPSK250 | Rs | 219 | 5 | 9 | differentially decoded → fade-robust |
| SL6 | QPSK250-D | Rs | 437 | 7 | 11 | **differential**; the HF-fade-robust QPSK rung (#923) |
| SL7 | OFDM52 | SC | 1264 | 9 | 12 | fills the old dead zone; first multicarrier rung |
| SL8 | OFDM52-8PSK | SC | 1895 | 10 | 14 | |
| SL9 | OFDM52-16QAM | SC | 2527 | 12 | 16 | |
| SL10 | OFDM52-32QAM | SC | 3159 | 14 | 18 | |
| SL11 | OFDM52-64QAM | SC | 3790 | 16 | 20 | densest constellation at soft-concat FEC |
| SL12 | OFDM52-16QAM | LHR | 5141 | 18 | 21 | code rate is the only lever left above SL11 |
| SL13 | OFDM52-32QAM | LHR | 6426 | 19 | 22 | |
| SL14 | OFDM52-64QAM | LHR | 7710 | 20 | — | ladder top; gated admission |

`initial_level = SL2`, `nack_threshold = 3`.

Design points:

- **The ladder is calibrated for a fade, not for AWGN.** An HF profile whose rungs only work on a
  clean channel is a ladder the adapter falls off. Every rung above is measured on `moderate_f1`;
  the four rungs that could not decode there (QPSK250 uncoded, QPSK500 uncoded, 8PSK500+Rs,
  SCFDMA26-32QAM) were removed rather than left as dead weight the adapter had to climb through.
  Effective throughput (decode × net bps) at 20 dB used to read 346 (SL6) → 0 → 125 → 0 → 395 →
  1816: a four-rung dead zone between the rung that worked and the rungs that worked.
- **Every rung is coded — there is no useful uncoded rung on a fade.** This is #923's law applied
  to the whole ladder: *differential needs FEC*. BPSK is differentially decoded, so it rides the
  fade rotation, but the symbols a carrier slip costs still have to be corrected. Uncoded, these
  rungs decode ~0 % at their own floors (BPSK31 @3 dB **0.00**, BPSK63 @4 dB 0.00, BPSK250 @5 dB
  0.00); with `RsStrong` they work (BPSK31 @3 dB **1.00**, BPSK63 @4 dB 0.83, BPSK250 @8 dB 1.00).
  The floors did not move — they were always fading-appropriate; the rungs simply lacked the code.
- **Why `RsStrong` and not `RsInterleaved`**, despite §2's "burst-tolerant / best for HF fading"
  billing. Measured, `RsInterleaved` is **inert** (BPSK250 on `moderate_f1` @5/8 dB: 0.17/0.58 —
  identical to plain `Rs`): a ≤223-byte payload is *one* RS block, and a single block is
  position-agnostic, so there is nothing to interleave. Code **strength** is the lever, and
  `RsStrong` is **free on the wire** for payloads ≤191 B — RS(255,223) and RS(255,191) both emit a
  255-byte block, so BPSK250+Rs and BPSK250+RsStrong have identical airtime (8.32 s at 64 B).
  Interleaving only earns its place across *multiple* blocks.
- **Above SL6 the ladder is OFDM, because phase margin runs out.** The coherent single-carrier
  rungs that used to sit here are not rescuable: FEC does not help (QPSK250+Rs is also 0.00 — the
  defect is carrier tracking, not errors), and differential does not scale to 8PSK (8PSK500-D
  measured 0.125 at 40 dB for a ~4–6 dB AWGN cost; ±22.5° cannot absorb differential's noise
  doubling). Robustness tracks phase margin: MFSK16 (non-coherent) > BPSK (±90°) > QPSK (±45°) >
  8PSK (±22.5°). OFDM sidesteps the whole question — its cyclic prefix rides the delay spread and
  its per-subcarrier pilots track the fade: OFDM52 decodes 0.58/0.75/0.83 at 8/12/16 dB where
  8PSK500 decodes 0.00 at all three.
- **The dense rungs are OFDM, not SC-FDMA.** At equal gross rate OFDM's CP rides frequency-selective
  fading that SC-FDMA's channel estimator cannot represent — measured, `moderate_f1` @20 dB 16QAM:
  OFDM 0.88 vs SCFDMA 0.35 (§7). The `SCFDMA26-32QAM` narrowband rung was dropped for the same
  reason it never earned its slot: 0.00/0.17/0.17 at 8/12/16 dB. It still lives in
  `hpx_wideband_hd`. (`OFDM16` is the most fade-robust OFDM mode and the narrowest at 625 Hz, but
  its ~401 net bps sits *below* SL6, so it has no monotonic slot here.)
- **High-rate LDPC only at the top.** SL12–SL14 re-use SL9–SL11's modes at r≈8/9. Below the top,
  buying rate with code rate costs +4…+8 dB of floor — a worse trade than climbing one modulation
  order (~2 dB for 1.33×). 64QAM is the densest constellation available, so above SL11 code rate is
  the only remaining lever.
- **The densest rung is gated.** SL14 (OFDM52-64QAM at r≈8/9, 20 dB) is admitted only after a prior
  SNR-upgrade candidate (`ack_up_requires_snr_candidate_at = SL14`), so the controller never jumps
  to the top rung on one lucky ACK.

---

## 5. Which mode/FEC combinations make sense

Robustness (left) vs throughput (right). "Net bps" is gross × code-rate (the
retransmit cost is on top of that). Recommended HF pairings:

| Operating regime | Mode | FEC | ~Net bps | Why |
|---|---|---|---|---|
| Weak signal, NVIS, QRM | BPSK31–250 | `RsInterleaved` | 25–220 | Burst-tolerant; QPSK/BPSK shrug off timing offset |
| Solid HF, ~2 kHz | QPSK500 / SCFDMA52 | `RsInterleaved` | ~900 / ~2 500 | Workhorse; soft optional |
| Good HF, want more | 8PSK500 / SCFDMA52-8PSK | `SoftConcatenated` | ~660 / ~1 900 | 8PSK needs the soft-coding gain |
| Marginal-SNR dense (the `hpx_wideband_hd` SL9–11 fallback) | **SCFDMA26-16QAM / -32QAM** | **`SoftConcatenated`** | ~1 270 / ~1 590 | **+3 dB narrowing + soft FEC — hardware-validated reliable** |
| High SNR, ~2 kHz, max data | SCFDMA52-16QAM/-32QAM | `SoftConcatenated` | ~2 540 / ~3 180 | Soft FEC closes them where hard RS can't |
| Very high SNR (≥25 dB) | SCFDMA52-64QAM / 64QAM2000-RRC | `SoftConcatenated` / `Ldpc` | ~3 800 / ~5 300 | Only on excellent links / on-air with margin |

Combinations that **don't** make sense:

- **Any dense mode (16QAM+) with `None` or bare `Rs`.** Validated on hardware:
  the full-width SCFDMA52-HOM modes fail no-FEC and fail with hard RS; soft FEC
  (`SoftConcatenated`) is what closes 8PSK, and narrowing **plus** soft FEC closes
  16QAM/32QAM reliably. RS(255,223)'s 6.3 % capacity is simply below what these
  modes leave on a realistic channel.
- **64QAM single-carrier on a marginal link.** It needs ~25–26 dB and a tight
  clock; below that no FEC rescues it economically. Use it only when the link
  genuinely supports it (then `SoftConcatenated` for margin).
- **`Turbo`/`Ldpc` on a clean, high-SNR link.** Their low code rate (0.33/0.5)
  throws away throughput you don't need to spend; prefer `Rs`/`RsInterleaved`
  there.

---

## 6. Empirical anchors (hardware loopback, rpi51↔rpi52)

- Single-carrier BPSK/QPSK/8PSK and `SCFDMA16` decode reliably no-FEC.
- `SCFDMA52` (QPSK) required a per-symbol pilot **`deramp_timing`** to survive the
  two-soundcards sample-rate offset (it removes the SFO phase ramp the DFT
  de-spread otherwise amplifies); after that it passes.
- The dense `SCFDMA52-8PSK/16QAM/32QAM` are SNR-bound no-FEC; with
  `SoftConcatenated` 8PSK passes and 32QAM is intermittent, and with the
  half-width `SCFDMA26-*` **+** `SoftConcatenated` all three pass reliably.
- **64QAM single-carrier under sample-rate offset** is marginal (≈2 % byte errors
  at 100 ppm with two-pass carrier tracking) and is SNR-bound on the test cable, so
  it can only be validated in simulation, not on the current hardware. It remains a
  documented, lower-priority item rather than a v1.0 blocker.

---

## 7. Spectral efficiency and PAPR — why OFDM (not SC-FDMA) is the HF high-throughput path

Two numbers decide whether a mode earns a place in a profile: **spectral efficiency**
(gross bps ÷ occupied Hz) at the SNR it needs, and **PAPR** (which sets how much average
power survives a peak-limited transmitter). Representative measurements:

| Mode | Gross bps | Occ. BW | bps/Hz | ~SNR floor | PAPR (dB) |
|---|---|---|---|---|---|
| BPSK250 | 250 | 275 | 0.9 | 5 | 4.2 |
| QPSK500 | 1 000 | 550 | 1.8 | 11 | 4.2 |
| 8PSK500 | 1 500 | 550 | 2.7 | 14 | 4.2 |
| 64QAM500 | 3 000 | 550 | 5.5 | 26 | 6.3 |
| QPSK2000-RRC | 4 000 | 2 700 | 1.5 | 11 | 6.6 |
| 64QAM2000-RRC | 12 000 | 2 700 | 4.4 | ~30 | 7.4 |
| OFDM16 | ~889 | 625 | 1.4 | 8 | 11.7 |
| OFDM52 | ~2 889 | 2 031 | 1.4 | 11 | 12.0 |
| SCFDMA16 | ~889 | 625 | 1.4 | ~9 | 11.3 |
| SCFDMA52 | ~2 889 | 2 031 | 1.4 | 11 | 12.1 |
| SCFDMA52-8PSK | ~4 333 | 2 031 | 2.1 | 15 | 11.1 |
| SCFDMA52-16QAM | ~5 778 | 2 031 | 2.8 | 16 | 12.7 |
| SCFDMA52-64QAM | ~8 667 | 2 031 | 4.3 | 28 | 12.2 |
| SCFDMA26-16QAM | ~2 889 | 1 000 | 2.9 | 13 | 10.9 |

**Findings:**

1. **bps/Hz tracks the constellation order**, as expected — single-carrier 64QAM is the
   most efficient (5.5 bps/Hz) but needs ~26 dB; QPSK sits at ~1.8 and works near 10 dB.
   Efficiency only matters *at the SNR the link can sustain*, so the ladder, not raw
   bps/Hz, is what selects a mode.

2. **OFDM ≡ SC-FDMA in throughput and bandwidth.** OFDM16/52 carry ~889 / ~2 889 bps over
   the *same* subcarriers as SCFDMA16/52 (identical FFT/CP/SC geometry) — earlier docs
   listing OFDM at ~444 / ~1 444 were wrong. So OFDM is **not less efficient**; it is
   **redundant** with SC-FDMA on a flat channel.

3. **The expected SC-FDMA PAPR advantage is currently *not realized*.** SC-FDMA's DFT
   precoding *should* give a single-carrier-like envelope (~4–6 dB PAPR), but the measured
   PAPR (~11–12 dB) equals OFDM's. Root cause: pilots are **frequency-interleaved every
   5th subcarrier**, which breaks the contiguous DFT-spread mapping and restores
   OFDM-like PAPR. At a peak-limited transmitter that ~12 dB PAPR costs ~8 dB of average
   power vs a single-carrier mode — the dominant EVM limiter for the dense modes on the
   hardware rig (the rig is *not* thermal-noise-limited; a chirp probe measured ~39 dB
   SNR available on the 8 kHz path).

**Decisions (the OFDM higher-order ladder, not the SC-FDMA pilot redesign):**

- **OFDM is the high-throughput / high-reliability HF path**, via a higher-order ladder
  (`OFDM52-{8PSK,16QAM,32QAM,64QAM}`, ~4.3–8.7 kbps gross) in `hpx_ofdm_hf` (§4). OFDM's
  CP + per-subcarrier equalization handle frequency-selective HF multipath natively — a
  dead subcarrier costs only its own bits, with **no DFT-despread noise enhancement**.
  This is the industry choice for HF data (VARA HF, Mercury, ARDOP).
- **The SC-FDMA PAPR pilot redesign (old roadmap FF-14) was dropped.** A prototype measured
  the *realized* PAPR gain from contiguous (de-interleaved) pilots at only **~3.8 dB**
  (12.7 → ~8.9 dB), not the ~6–8 dB first assumed: OpenPulseHF's SC-FDMA is a **real-valued
  passband** signal (Hermitian symmetry, 1500 Hz centre), and the ~3 dB real-bandpass
  penalty floors it well above textbook complex-baseband SC-FDMA. Single-carrier RRC already
  beats that (~6.6 dB), and `64QAM2000-RRC` out-throughputs `SCFDMA52-64QAM` — so SC-FDMA is
  dominated (no PAPR edge, *worse* selective-fade handling) and not worth the redesign.
- **SC-FDMA stays as-is** — a working, hardware-validated dense-multicarrier path and the
  source of the shared constellation code (`openpulse_dsp::constellation`) the OFDM HOM
  ladder reuses. Kept, not retired; not invested in further.
- **`SCFDMA52-LP` low-PAPR demonstrator (added later) — and what it actually shows.** A localized
  QPSK variant with one contiguous 61-SC data block + a 4-pilot block (single-tap flat-channel CE).
  Measured **mean PAPR 11.9 → 9.7 dB (~2 dB)** over 16 payloads, decoding on AWGN. **Its ablation
  actually CORRECTS the root-cause story in item 3 above:** ~3/4 of the win is carrying **fewer
  pilot tones** (4 vs 13 — 13 equal-phase pilot cosines peak together), and only ~0.5 dB is the
  localized contiguous mapping; contiguous data *with* 13 pilots recovers ~0 dB. So the dominant
  PAPR lever is pilot **count/power**, not interleaved **placement** — a sparse-interleaved 4-pilot
  SCFDMA52 would reach most of the same PAPR *with* an interpolatable channel estimate. The cost of
  the 4-block-pilot single-tap CE is fragility: it needs a flat, well-timed channel (skips deramp;
  extrapolates one gain ~1.9 kHz down-band), so on selectivity / a ±1-sample timing error / SSB
  tilt it silently mis-decodes. Hence a *demonstrator only* (registered, in no profile). The
  residual ~10 dB (vs a true single carrier's ~6–7 dB) is the real-valued-passband + rectangular-
  LFDMA ceiling; IFDMA or RRC shaping would go lower but need a redesign, still dominated by
  `64QAM2000-RRC` on throughput — consistent with the FF-14 decision. (This ablation supersedes the
  earlier FF-14 "de-interleaving → 8.9 dB" prototype figure below, which conflated the same effect.)
- **`SCFDMA52-P2` (PN-phase pilots) — the clean realization of the pilot-count insight.** Since the
  PAPR driver is the 13 *equal-phase* pilot cosines peaking together (not pilot count per se), giving
  each pilot a known **Zadoff–Chu quadratic phase** decorrelates the comb without dropping any pilot.
  Measured on the PA-relevant **envelope-CCDF@1e-3**: SCFDMA52 8.85 dB → **SCFDMA52-P2 6.70 dB
  (−2.15 dB)** at identical geometry/rate, retaining **full DFT-CE** — so it even undercuts the
  flat-CE `SCFDMA52-LP` (6.90 dB) while staying frequency-selective. This is SC-FDMA's genuine niche
  (power-limited transmitters); it ships as a versioned demonstrator (wire-incompatible with the
  equal-phase modes) and would be the template if PN pilots are rolled into the `hpx_hf` SL8–SL11 rungs.
- No single-carrier mode is dominated; the plain rectangular 2000-baud modes remain
  superseded by their `-RRC` variants (documented in §1).

### Managing OFDM's PAPR — leveling, not clipping

OFDM's ~12 dB PAPR is real, but it is a **leveling** problem, not a blocker: SSB rigs apply
drive backoff for exactly this (VARA HF runs OFDM through them daily). On the ~39 dB-SNR
8 kHz path, even ~12 dB of backoff leaves ~27 dB — enough for 64QAM (~26–28 dB). Two
concrete points from bringing the ladder up on hardware:

- **Clipping is QPSK-only.** Iterative PAPR clipping injects broadband distortion the dense
  constellations cannot absorb — it breaks 64QAM even on a clean channel — so the
  higher-order OFDM modes are left un-clipped.
- **Higher-order frames are peak-normalized to a DAC-safe 0.9.** Un-clipped, OFDM's peaks
  overshoot the DAC, which hard-clips them and shreds the dense constellation (on the rig
  OFDM52-16QAM *acquired but decoded garbage* until this was added). Scaling the frame to
  fit the DAC is the inherent PAPR backoff with no clipping distortion.

With that, `OFDM52-16QAM` (uncoded **and** with soft FEC) and `OFDM52-64QAM` all decode on
the rpi51↔rpi52 hardware loopback, and `OFDM52-16QAM` + soft FEC decodes a Watterson Good-F1
channel through the engine — the high-throughput / high-reliability HF path, realized.

---

## 8. Sample rate vs channel bandwidth (why 8 kHz, and the wide-mode ceiling)

The modem runs its DSP at **8 kHz**. The soundcards run at 48 kHz (or 44.1); ALSA's
`plug` layer resamples 8 ↔ 48 kHz, and cpal opens the device at 8 kHz. This is
deliberate, and it is a common source of confusion, so to be precise:

**Sample rate (Fs) is not channel bandwidth.** 8 kHz Fs gives a usable passband up to
the Nyquist limit of **4 kHz**. An HF SSB channel is ~300–2700 Hz, so 8 kHz covers it
with margin to spare. Two *independent* constraints decide whether a mode runs at 8 kHz:

1. **Occupied bandwidth < Nyquist (4 kHz).** Every HF mode (≤2700 Hz occupied) clears
   this easily — SCFDMA52 tops out near 2.5 kHz.
2. **Enough samples per symbol.** A single-carrier mode needs ≥~4 samples/symbol for
   clean timing recovery, i.e. `Fs ≥ ~4 × baud`. At 8 kHz that caps the baud rate near
   2000 (hence `QPSK2000-RRC` is the fastest single-carrier HF mode).

**Why 8 kHz and not 48 kHz:** matching Fs to the channel keeps CPU and memory low —
every FFT, filter, and equalizer runs on ⅙ the samples — with no loss, because
oversampling a 2.7 kHz channel at 48 kHz buys nothing on air. It is the same reason
VARA, ARDOP, and Mercury use 8 kHz (or 12 kHz) internal rates. The 48 kHz card rate
exists only because consumer sound hardware does not natively clock 8 kHz; the
resampler bridges it, and the chirp probe confirmed the resampler is flat well past
3 kHz (see `docs/dev/virtual-loopback.md`).

**The wide-mode ceiling (why the 9600-baud modes are deferred).** `QPSK9600-RRC` and the
other 9600-baud modes are **physically impossible at 8 kHz** on *both* counts:

- 9600 baud × (1 + 0.35 RRC) ≈ **13 kHz occupied** — over 3× the 4 kHz Nyquist.
- 9600 baud needs `Fs ≥ ~38.4 kHz` just for 4 samples/symbol; at 8 kHz that is 0.83
  samples/symbol, which cannot be demodulated at all.

So they need **two** things the HF path does not provide: a **higher sample rate**
(native 48 kHz, no resample) **and** a **wider channel** than HF SSB allows (13 kHz fits
a VHF/UHF FM data channel or a wideband 10 m segment, not a 2700 Hz HF slot). They are
kept in the registry and in `hpx_narrowband_hd` for a future higher-Fs transport
(post-1.0); the loopback and test-matrix runners **SKIP them with reason** rather than
silently dropping them. They are not a defect — they are simply out of scope for an
8 kHz / 2700 Hz HF modem.
