---
project: openpulsehf
doc: docs/dev/design/wide-channel-extension.md
status: living
last_updated: 2026-07-08
---

# Extending OpenPulseHF to 12.5 kHz and 25 kHz channels — design + action list

Assessment and a phased action list for extending the modem from its current ~2.7 kHz HF SSB channel
to wide channels of **12.5 kHz** and **25 kHz** (VHF/UHF-class widths). From a research pass over the
audio/sample-rate path, the OFDM/SC-FDMA plugins, the bandplan guardrails, and the channel-sim stack.
No code was changed.

## Executive summary

**Feasibility is good — much groundwork already exists.** The codebase already has (a) sample-rate-
parameterized single-carrier plugins with 9600-baud modes explicitly designed for "UHF/VHF — 12.5 kHz
HD (requires 48 kHz audio)" and tested at 48 kHz (`plugins/qpsk/src/lib.rs:68`, `plugins/psk8/src/lib.rs:66`);
(b) session profiles for 12.5 kHz channels (`hpx_narrowband`, `hpx_narrowband_hd`); and (c) a TX
I/Q-to-SDR seam (`ModulationPlugin::modulate_iq`, `AudioBackend::open_iq_output`, engine IQ transmit).
The requirements doc already demands a 48 kHz-capable audio backend (`docs/dev/requirements.md:34`).

**What blocks it today:** the engine is pinned to 8 kHz via `AudioConfig::default()` at every audio/DSP
site (`engine.rs:477,673,1237,1357,1468,1779,1843,2318,2440,2583`), with **no `sample_rate` field in the
user config**; the OFDM/SC-FDMA plugins **hard-reject any rate ≠ 8000**; the bandplan guardrail knows
only HF bands (any VHF/UHF frequency fails `FrequencyOutOfBand`); and the channel-sim/calibration stack
defaults to 8 kHz Watterson HF presets.

**The deepest open question is RF architecture, not code:** a 12.5/25 kHz linear waveform cannot pass
an SSB rig's ~2.7 kHz audio path nor an FM rig's nonlinear class-C PA. A **direct-IQ SDR path** (TX seam
exists; RX does not) or a flat-response linear exciter is required. 12.5 kHz is reachable at 48 kHz
audio/IQ; 25 kHz occupied real audio marginally exceeds 48 kHz Nyquist headroom and realistically wants
96 kHz real audio *or* a 48 kHz complex-IQ path.

## Current-state findings (cited)

**Sample rate — 8 kHz is the de-facto hard-coded engine rate.** `AudioConfig::default()` →
`sample_rate: 8000` (`audio.rs:36`), and the engine never reads a configured rate — it calls
`AudioConfig::default()` at ≥8 sites. The `[audio]` config has no `sample_rate` field. Backends are
already rate-agnostic (cpal opens at whatever the config says; loopback advertises 8/16/44.1/48 kHz).
Plugins split into **rate-parameterized** (BPSK/QPSK/8PSK/64QAM/pilot/FSK4 — compute
`samples_per_symbol(fs, baud)`; QPSK9600/8PSK9600 already pass 48 kHz tests) and **rate-pinned** (OFDM,
SC-FDMA — `const SAMPLE_RATE = 8000`, `FFT_SIZE = 256`, spacing 31.25 Hz, with explicit `!= 8000`
rejection). Stray 8 kHz hard-codes: DCD hold-time math, daemon spectrum-frame rate, CLI throughput math.

**Occupied bandwidth.** Single-carrier ≈ `(1+α)·Rs` (RRC) or `2·Rs` (null-to-null), centered on
`center_frequency` (default 1500 Hz). OFDM/SC-FDMA: `total_sc × (fs/FFT)` = `total_sc × 31.25 Hz`
(OFDM52/SCFDMA52 ≈ 2031 Hz). The policy-side estimate is a **hand-maintained static table** in
`occupied_bandwidth_hz()` — QPSK9600/8PSK9600 already listed at 12 000 Hz.

**Bandplan / regulatory.** `BandplanPolicy` enforces band membership + per-segment `max_bw_hz`, but the
validated tables **stop at 10 m** and cap at 2 700 Hz. `band_label_for_hz` knows 6 m/2 m/1.25 m/70 cm
but only for labels/squelch, not validation. Requirements are HF/SSB-framed ("500 Hz and 2300–2400 Hz",
CEPT ≤2.7 kHz).

**Existing "wideband" notions.** `hpx_wideband` (QPSK/8PSK1000, still ≤ ~3 kHz — an FM-voice-channel
waveform, not channel-filling). `hpx_narrowband` = "12.5 kHz channel, 2.7 kHz-wide signal".
**`hpx_narrowband_hd` (QPSK9600-RRC/8PSK9600-RRC, ≈13 kHz, "requires a 48 kHz audio path") cannot
currently run** — the engine opens audio at `AudioConfig::default()` = 8 kHz.

**Channel model.** `WattersonConfig` presets pin `sample_rate: 8000` (the model itself is
parameterized); AWGN is rate-agnostic. There is **no VHF/UHF mobile model** (flat Rayleigh/Rician +
vehicle Doppler). All `profile.rs` floors were calibrated at 8 kHz.

## Action list

### Phase 0 — Decisions and de-risking (do first; mostly analysis)

| # | What | Effort | Risk |
|---|---|---|---|
| 0.1 | **Decide the RF path.** Linear wide waveforms can't pass an FM class-C PA or a ~3 kHz voice path. Options: (a) **direct-IQ SDR** (extend the existing TX-only seam; RX IQ is missing); (b) wide-linear transverter / SSB-class VHF exciter; (c) a **FSK/FM-native mode family** (4FSK à la DMR/C4FM) tolerant of class-C PAs, reusing `plugins/fsk4`. **Recommend (a) primary + (c) constant-envelope fallback.** | M | **High** — everything depends on it |
| 0.2 | **Decide target sample rates:** 48 kHz for 12.5 kHz; for 25 kHz choose 96 kHz real audio **or** 48 kHz complex IQ. Recommend engine supports {8 000, 48 000, 96 000} real + 48 000 IQ. | S | Low |
| 0.3 | **Decide wideband strategy per waveform:** clock-scaling (run OFDM52 at 6×/12× rate — zero DSP change, spacing 187.5/375 Hz, fine for VHF's small delay spread) vs new subcarrier layouts (larger FFT, keeps 23–31 Hz spacing — only if HF-like selectivity matters). **Recommend clock-scaling first.** | S | Low |
| 0.4 | **PA linearity / PAPR assessment** (OFDM52 targets 12 dB PAPR) — else prioritize SC-FDMA/PN-pilot low-PAPR or single-carrier RRC for wide modes. | M | Med |
| 0.5 | **AFC budget at VHF/UHF** — ±1 ppm at 145/435 MHz is ±145/±435 Hz vs the ±50 Hz requirement. Decide the capture range (suggest ±500 Hz for IQ; near-zero for GPS-disciplined SDRs). | S | Med |

### Phase 1 — Sample-rate generalization (enables 12.5 kHz at 48 kHz audio)

| # | What | Effort | Risk |
|---|---|---|---|
| 1.1 | Add `sample_rate` to `[audio]` config (default 8000), validate against {8000, 16000, 48000, 96000}. | S | Low |
| 1.2 | Thread a configured rate through `ModemEngine` (store fs; replace every `AudioConfig::default()` call — CW-ID, CE-SSB, capture, scan, AFC, IQ TX, RX). | M | Med — wide regression surface, but all sites already take fs as a value |
| 1.3 | Fix stray hard-codes: DCD hold-time, daemon spectrum-frame rate (use engine fs so the panel axis is right), CLI on-air-seconds math. | S | Low |
| 1.4 | Wire daemon/CLI/TUI/ARDOP to pass the configured rate into engine + backends. | M | Low |
| 1.5 | Verify frame-geometry / preamble / energy-gate scaling at 48 kHz; add a 48 kHz engine loopback test per mode family. | M | Med |
| 1.6 | **Unblock `hpx_narrowband_hd` end-to-end** (QPSK9600-RRC/8PSK9600-RRC, ~13 kHz, 19.2/28.8 kbps gross) + an ARQ integration test at 48 kHz. | S | Low |
| 1.7 | Guard rate/mode compatibility (reject < 4 samples/symbol with a clear error + mode-advisor hint). | S | Low |

### Phase 2 — Wide modes (fill 12.5 kHz, then 25 kHz)

| # | What | Effort | Risk |
|---|---|---|---|
| 2.1 | **Parameterize OFDM/SC-FDMA on sample rate** — replace the `const SAMPLE_RATE/FFT_SIZE/CP/SPACING`; drop the `!= 8000` rejections (wire-versioning discipline per the `pn_pilots` precedent). | M | Med |
| 2.2 | **12.5 kHz candidates** (≤ ~12.2 kHz): `W12-QPSK9600-RRC`/`W12-8PSK9600-RRC` (exist, 19.2/28.8 kbps gross); `OFDM52@48k` clock-scaled ×6 (65 SC × 187.5 Hz = 12.19 kHz): QPSK 17.3 k / 16QAM 34.7 k / **64QAM 52 kbps gross** (~45 net); plus low-PAPR SC-FDMA variants. | M | Med |
| 2.3 | **25 kHz candidates** (needs 96 kHz audio or complex IQ): `OFDM52@96k` ×12 (65 SC × 375 Hz = 24.4 kHz): QPSK 34.7 k / 16QAM 69 k / **64QAM 104 kbps gross**; `W25-8PSK19200-RRC` at 96 kHz (~26 kHz, 57.6 kbps gross); `OFDM130@48k` (24.4 kHz) — **IQ path only** (exceeds real-audio Nyquist). | M–L | Med–High |
| 2.4 | **RX IQ input path** (if Phase 0 picks SDR): add `AudioBackend::open_iq_input` + complex-baseband demod entry + engine RX wiring. The TX half exists; RX is the **largest new-code item**. | L | High |
| 2.5 | Extend the ladder: `hpx_wide12` / `hpx_wide25` profiles with calibrated floors; keep SCFDMA26/52 as narrowband fallback (mirroring `hpx_wideband_hd`). | M | Low |
| 2.6 | **Channel model for VHF/UHF:** keep AWGN; add rate-parameterized Watterson constructors; add a flat Rayleigh/Rician mobile model (fd = v·f/c, ~13 Hz at 145 MHz / 100 km/h). | M | Low |
| 2.7 | **Recalibrate SNR floors** at the new rates/channels; **define the SNR reference bandwidth explicitly** for wide modes (floors are meaningless across bandwidths without one). | M | Med |

### Phase 3 — Regulatory / bandplan / guardrails

| # | What | Effort | Risk |
|---|---|---|---|
| 3.1 | Add VHF/UHF band tables (6 m/2 m/1.25 m/70 cm) with per-segment `max_bw_hz` of 12 500/25 000 where regionally appropriate + channel-raster alignment for QSY. | M | Med — regional research (IARU R1 VHF/UHF ≠ HF; FCC §97.307 differs above 50 MHz) |
| 3.2 | Route `occupied_bandwidth_hz()` through the plugins' trait hook (kill the dual-maintenance static table). | S | Low |
| 3.3 | Update REQ/regulatory docs — add a wide bandwidth class + VHF/UHF emission designators. | S | Low |
| 3.4 | Clarify profile taxonomy — `hpx_narrowband` means "12.5 kHz channel, 2.7 kHz signal"; the new modes *fill* 12.5 kHz. Avoid operator confusion. | S | Low |

## Open questions for the user (blocking Phase 0)

1. **SSB-on-VHF vs FM-data vs direct-IQ SDR?** The current mono-soundcard → SSB-rig architecture cannot
   carry > ~3 kHz; direct-IQ is the only path that reaches 25 kHz cleanly (TX seam exists, RX does not).
2. **Constant-envelope fallback?** If class-C FM PAs must be supported, a 4FSK wide family (new work) is
   needed alongside the linear modes.
3. **25 kHz via 96 kHz real audio or 48 kHz IQ?** 96 kHz keeps the real-passband architecture but is
   marginal + soundcard/rig-dependent; IQ is cleaner but requires the RX IQ path (2.4).
4. **Does HF-grade subcarrier spacing matter on VHF?** If yes, clock-scaled 187.5 Hz OFDM is wrong and a
   larger-FFT layout is needed — decide before 2.2.
5. **SNR reference-bandwidth convention** for the wide ladder — floors are meaningless across bandwidths
   without one.
