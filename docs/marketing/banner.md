---
project: openpulsehf
doc: docs/marketing/banner.md
status: draft
last_updated: 2026-05-12
---

# OpenPulseHF — HAMRADIO 2026 Banner Text

*Roll-up / pull-up banner: 85 cm × 200 cm portrait format*
*Print resolution: 150 dpi minimum*

---

## Layout (top to bottom)

### Zone 1 — Header (top 25 cm)

**Background: deep navy (#0D1B2A)**  
**Logotype in white, condensed sans-serif**

```
OpenPulseHF
```

*Tagline below, smaller weight:*

```
The Open HF Data Modem
```

---

### Zone 2 — Hero claim (next 30 cm)

**Background: accent blue (#1565C0) with subtle waterfall spectrum texture**  
**Large white text, centred:**

```
33 waveforms.
Post-quantum secure.
See your signal live.
100% open source.
```

*Sub-line in lighter weight:*

```
Works with Pat · APRS · Winlink · Your SSB rig
```

---

### Zone 3 — World-first features (next 60 cm)

**Background: white**  
**Three-column list, bold teal accent (#00796B) for the bullet icon:**

Left column:
```
★ Post-quantum handshake
  (ML-DSA-44 + ML-KEM-768)

★ QSY auto channel-hop
  (no operator input required)

★ SC-FDMA waveform
  (3–4 dB lower PAPR than OFDM)

★ K=7 soft-decision Viterbi FEC
  (+5 dB gain over hard-decision K=3)
```

Centre column:
```
★ Multi-block RS FEC
  (full protection at any payload
  size — no per-frame byte limit)

★ RRC matched filtering
  (Gardner TED + Costas PLL on
  all RRC modes; cleaner spectrum,
  sharper symbol recovery)

★ Automatic frequency correction
  (tracks ±62.5 Hz drift; works
  even on uncalibrated radios)

★ Memory-ARQ soft combining
  (~3 dB SNR gain per retransmission)
```

Right column:
```
★ Built-in signal-path testbench
  (live 4-tap waterfall, IQ scatter,
  BER meter — 7 channel models,
  no radio required)

★ Zstd dictionary compression
  (sub-500 byte payloads)

★ GPU-accelerated DSP
  (optional wgpu backend)

★ 322-case test matrix
  (every mode × FEC × channel,
  all passing — CI-gated)
```

*Footer of this zone:*

```
All features shipping in v1.0 — no roadmap promises
```

---

### Zone 4 — Compatibility strip (next 30 cm)

**Background: light grey (#F5F5F5)**  
**Centred icons + labels:**

```
<!-- layout placeholder: Pat icon -->   <!-- layout placeholder: APRS icon -->   <!-- layout placeholder: Winlink icon -->   <!-- layout placeholder: testbench icon -->
Pat / ARDOP                              KISS / AX.25                             Direct CMS gateway                         Signal testbench
drop-in ready                            any APRS client                          no extra software                           no radio required
```

---

### Zone 5 — QR code and call to action (bottom 35 cm)

**Background: deep navy (#0D1B2A)**

*Left side — QR code linking to GitHub repo:*

```
<!-- layout placeholder: QR code linking to github.com/dc0sk/OpenPulseHF -->
```

*Right side — text:*

```
github.com/dc0sk/OpenPulseHF

Free software · GPL v3
Works on any SSB radio
Raspberry Pi 4 ready

322/322 test cases passing
7 ITU channel models validated

⬛ Scan to get started
```

*Footer strip at very bottom:*

```
HAMRADIO 2026 · Friedrichshafen · OpenPulseHF Contributors · GPL v3
```

---

## Print notes

- All text zones should have ≥ 5 mm bleed margin
- QR code minimum size 6 cm × 6 cm for reliable scanning at 50 cm distance
- Waterfall spectrum in Zone 2: render a 2-second BPSK250 transmission at 8 kHz,
  FFT size 256, plasma colormap — embed as a horizontal band behind the text
- Consider inset screenshot of the testbench GUI in Zone 3 right column (replace
  the ★ testbench bullet with a 10 cm × 8 cm cropped screenshot showing the 4-tap
  waterfall and IQ scatter plot; caption: "Included: live signal visualizer")
- Banner grommets at top corners and at 100 cm from top (mid-point)
- Recommended print finish: matte lamination (reduces glare under tradeshow lighting)
