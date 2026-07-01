---
project: openpulsehf
doc: docs/dev/research/references.md
status: living
last_updated: 2026-06-17
---

# External references and inspirations

Open-source modems and DSP libraries we study for technique and validation. This
is a living index — when a DSP problem stalls (carrier recovery, sync, equalization,
FEC, PAPR), come back here first and check whether one of these has solved it. Add
new sources and new "what we could take" notes over time.

We implement independently (OpenPulseHF is a from-scratch protocol); these inform
*technique*, not code lifted wholesale. Note each project's licence before porting
any code.

> **Source-level scan (2026-06-17):** a full read of these repos' code — a
> prioritized idea catalog (benefit/effort/licence/fit per idea) plus
> recommendations and the SC-FDMA-low-PAPR analysis — is in
> [reference-mining-plan.md](reference-mining-plan.md).

---

## gnuradio/gnuradio — the SDR reference toolkit

<https://github.com/gnuradio/gnuradio> · GPL-3.0

The canonical reference for physical-layer DSP blocks. Especially relevant:

- **FLL Band-Edge** (`gr::digital::fll_band_edge_cc`, <https://wiki.gnuradio.org/index.php/FLL_Band-Edge>)
  — a frequency-locked loop that derives a carrier-frequency error from the signal's
  upper/lower band edges (`e = Re{cc·ss*}`). It is **not** decision-directed (no
  cycle-slip on dense constellations) and uses **no preamble** (no ISI bias), but it
  **requires excess-bandwidth / RRC pulse shaping** (the band-edge filter is the
  derivative of the raised-cosine matched filter). Sits *before* the matched filter
  and Costas loop.
- **Canonical PSK receiver chain**: AGC → **FLL band-edge** (acquire frequency) →
  RRC matched filter → symbol sync (timing) → **Costas loop** (residual phase). The
  two-stage FLL-then-Costas split is the robust pattern.

**Taken / planned:** the FLL-then-Costas two-stage carrier recovery is the fix path
for our 8PSK carrier-offset gap (see `docs/...` / memory `8psk-carrier-offset-gap`).
Our single decision-directed Costas loop + biased data-aided preamble AFC is the
non-standard part.

**Revisit for:** symbol timing recovery (polyphase clock sync), the band-edge FLL
implementation details, LDPC/polar decoders, channel models, equalizer blocks.

---

## daniestevez/qo100-modem — QO-100 narrowband modem (Daniel Estévez)

<https://github.com/daniestevez/qo100-modem>

A high-quality GNU Radio modem for the QO-100 (Es'hail-2) narrowband transponder,
by a well-known SDR/DSP author. **32APSK** waveform in a **2.7 kHz SSB** bandwidth
(directly comparable to our HF channel), plus experiments with **differentially-
encoded 8PSK**.

**Inspirations:**
- Differential encoding to sidestep absolute carrier-*phase* recovery (helps the
  phase loop; does not by itself fix a frequency offset).
- A dense APSK constellation engineered for a 2.7 kHz voice-bandwidth channel —
  relevant to our high-throughput-in-2.7 kHz goal (cf. the OFDM HOM ladder).

**Revisit for:** APSK constellation/throughput design in 2.7 kHz, pilot/sync design
(the `gr-qo100_modem` directory + the Jupyter notebooks hold the DSP detail), and
Doppler/drift handling for satellite-grade carrier tracking.

---

## dj0abr/SSB_HighSpeed_Modem — deployed ham 8PSK/QPSK-over-SSB modem

<https://github.com/dj0abr/SSB_HighSpeed_Modem> · docs at <https://hsmodem.dj0abr.de>

A *fielded* amateur high-speed data modem over a 2.7 kHz SSB audio channel — the
closest analog to our exact use case (PSK between two radios that each have a
carrier offset). Built on **liquid-dsp** (BSD), `libsoundio`, `fftw3`.

**Inspirations:**
- **liquid-dsp `framesync`**: corrects gain/carrier/timing offsets via a known
  preamble — **coarse CFO from preamble correlation, fine CFO refined from the
  payload**. The standard two-stage burst-mode CFO. BSD-licensed C, so it is a
  *portable* reference for a Rust frame synchronizer.
- Proof that robust 8PSK/QPSK over real SSB radios with offsets is achievable with
  RRC shaping + a proper frame synchronizer.

**Revisit for:** burst-frame CFO (coarse+fine), preamble design, the liquid-dsp
modem/framesync primitives generally (it also has FEC, equalizers, resamplers).

---

## Rhizomatica/mercury — deployed HF OFDM data modem + ARQ (HERMES)

<https://github.com/Rhizomatica/mercury> · GPL-3.0 / LGPL-2.1 (vendored FreeDV codec) · C

A *fielded* HF data system — "a Digital Radio OFDM protocol for HF broadcast and
peer-to-peer ARQ connections" for store-and-forward email/file transfer over HF in
rural and emergency scenarios. Part of Rhizomatica's **HERMES** (High-frequency
Emergency and Rural Multimedia Exchange System), funded by ARDC. Unlike the
single-carrier DSP references above, Mercury is a full **OFDM + ARQ + application**
stack — the closest analog to OpenPulseHF's *system* (HPX ARQ + B2F/Winlink), not
just its DSP.

Built on **FreeDV's OFDM modem** (David Rowe): `DATAC13` for signaling, `DATAC4`/
`DATAC3`/`DATAC1` for payload, plus an experimental `FSK_LDPC` mode. We already
interface FreeDV for authenticated voice (`openpulse-freedv-auth`), so the FreeDV
DATAC modes are a shared reference point.

**Inspirations:**
- **Adaptive ARQ "gear-shifting" driven by link quality *and* backlog**, with
  per-direction mode selection — comparable to our `RateAdapter`/HPX rate ladder, but
  the queue-backlog input and asymmetric per-direction rate are ideas we don't use yet.
- A connect/accept handshake with ACK/retry, keepalive, and controlled disconnect
  over HF — a deployed ARQ design to compare against our HPX session state machine.
- The FreeDV DATAC OFDM data modes as a proven HF-OFDM comparison for our OFDM
  higher-order ladder (cyclic-prefix + pilot design rather than RRC + FLL).

**Revisit for:** ARQ rate-adaptation policy (backlog-aware, per-direction), HF
store-and-forward email protocol design (cf. B2F/Winlink), and the FreeDV DATAC OFDM
modem parameters (CP length, pilot scheme).

---

## CE-SSB and polar-SSB transmit conditioning

Sources studied for the TX signal-conditioning path (`openpulse_dsp::cessb`,
`ModemEngine::cessb_benefits`). These informed the *per-mode gate* — not code lifted
wholesale — and one was explicitly weighed and **rejected** for a data modem.

- **David L. Hershberger, W9GR — "Controlled Envelope Single Sideband"**, QEX
  Nov/Dec 2014 (pp. 3–13) + Jan/Feb 2016 external-processing follow-up. The origin of
  CE-SSB: a **baseband RF clipper → band-limit filter → overshoot compensator** chain
  that drives SSB modulator overshoot from ~61 % to ~1.3 % (~2.5× average power at
  fixed PEP). This is the method `openpulse_dsp::cessb` is named after.
  <http://www.arrl.org/files/file/QEX_Next_Issue/2014/Nov-Dec_2014/Hershberger_QEX_11_14.pdf>
- **Ron Economos, W6RZ — `drmpeg/gr-cessb`** (GNU Radio OOT, GPL-3.0). A concrete
  reference impl of the Hershberger chain: `clipper_cc` (memoryless magnitude clip,
  `mag ← min(mag, clip)`, phase preserved) → band-pass filter → `stretcher_cc`
  (overshoot compensator: windowed-max envelope over ±2 samples, then divide by
  `1 + 2·overshoot` where `overshoot = max(0, env·2√2 − 1)`), run at high oversampling
  and typically iterated twice. <https://github.com/drmpeg/gr-cessb>
  - **Considered and REJECTED for our data path.** The Hershberger/gr-cessb loop is
    tuned for *voice* SSB, where a few percent in-band distortion is inaudible and
    average-power/loudness is the objective. Its clip→filter→compensate loop injects
    **more** in-band EVM than our single-stage look-ahead limiter, which is exactly the
    quantity that breaks our dense data constellations (8PSK/QAM: tight decision
    regions). Adopting the aggressive iterative loop would *worsen* the very modes our
    gate already excludes. Our `cessb.rs` therefore stays a **single-pass look-ahead
    peak-stretch** (smooth gain from a windowed-max envelope, applied at passband, no
    hard-clip-then-refilter) — splatter-free by construction and gentler on EVM.
- **Kahn, 1952 — Envelope Elimination and Restoration (EER)**; **K1LI/K1KP — "The
  Polar Explorer"**, QEX Mar/Apr 2017; **PE1NNZ — "Direct SSB generation on a PLL"**
  (2013); **Dave's Hacks, Feb 2025 — polar modulation for uSDX/QMX**. The **polar/EER**
  family: split the signal into `A = √(I²+Q²)` and `φ = atan2(Q, I)`, differentiate φ
  for instantaneous frequency, and drive a switching (Class-E) PA's frequency +
  amplitude directly at RF. <https://www.pe1nnz.nl.eu.org/2013/05/direct-ssb-generation-on-pll.html>
  · <https://daveshacks.blogspot.com/2025/02>
  - **Not applicable to the current soundcard→linear-SSB-rig path** (the rig's linear PA
    already does this). Relevant only if we ever add a **direct-RF backend** for
    Class-E radios (QMX/uSDX) — a new hardware target, not a modem-DSP change.
  - **What we DID take — the theoretical basis for the per-mode gate.** Dave's Hacks
    derives, for a two-tone sum, `A = √(a² + 2ab·cos(Δω·t) + b²)`: as the two amplitudes
    approach equality the envelope passes through zero and the **instantaneous frequency
    goes discontinuous/unbounded**, so faithful reproduction needs the phase/amplitude
    paths to carry ~5× the signal bandwidth. This is the *equal-amplitude singularity*,
    and it is precisely why envelope conditioning helps high-PAPR OFDM-QPSK (a
    near-Gaussian envelope that rarely nulls hard) but hurts single-carrier QAM and
    higher-order OFDM subcarriers (constellations that transit near the origin, where
    the envelope nulls and the phase jumps). It converts our empirically-tuned
    `cessb_benefits` gate into a **principled** one: benefit ⇔ high-PAPR envelope **and**
    loose decision margins. See `ModemEngine::cessb_benefits`.

- **FreeDV 700D symbol diversity** (`drowe67/codec2`) — an idea we do *not* yet use:
  transmit each carrier's symbol twice across the band for a weak-signal mode below the
  current SL floor. Distinct lever from FEC; parked for a future weak-signal rung.

---

## Recurring lesson

The three **single-carrier** references above (gnuradio, qo100-modem,
SSB_HighSpeed_Modem) all use **RRC-shaped pulses** and a **dedicated frequency
acquisition stage** (FLL or coarse preamble-correlation CFO) ahead of phase
recovery. OpenPulseHF's rectangular-pulse PSK modes with a single Costas loop are
the outlier; the carrier-offset robustness gaps (8PSK) trace directly to that.
Mercury takes the other route entirely — **OFDM with cyclic prefix + pilots**
instead of RRC + FLL — which is the architecture of our OFDM higher-order ladder.
