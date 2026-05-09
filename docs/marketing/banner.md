---
project: openpulsehf
doc: docs/marketing/banner.md
status: draft
created: 2026-05-09
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
30+ waveforms.
Post-quantum secure.
100% open source.
```

*Sub-line in lighter weight:*

```
Works with Pat · APRS · Winlink · Your SSB rig
```

---

### Zone 3 — World-first features (next 50 cm)

**Background: white**  
**Two-column list, bold teal accent (#00796B) for the bullet icon:**

Left column:
```
★ Post-quantum handshake
  (ML-DSA-44 + ML-KEM-768)

★ QSY auto channel-hop
  (no operator input required)

★ SC-FDMA waveform
  (3–4 dB lower PAPR than OFDM)

★ K=7 soft-decision Viterbi FEC
```

Right column:
```
★ Memory-ARQ soft combining
  (~3 dB SNR gain per retransmission)

★ Zstd dictionary compression
  (sub-500 byte payloads)

★ GPU-accelerated DSP
  (optional wgpu backend)

★ Runs on Raspberry Pi 4
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
[Pat icon]          [APRS icon]        [Winlink icon]
Pat / ARDOP         KISS / AX.25       Direct CMS gateway
drop-in ready       any APRS client    no extra software
```

---

### Zone 5 — QR code and call to action (bottom 35 cm)

**Background: deep navy (#0D1B2A)**

*Left side — QR code linking to GitHub repo:*

```
[QR CODE — github.com/dc0sk/OpenPulseHF]
```

*Right side — text:*

```
github.com/dc0sk/OpenPulseHF

Free software · GPL v3
Works on any SSB radio
Raspberry Pi 4 ready

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
- Banner grommets at top corners and at 100 cm from top (mid-point)
- Recommended print finish: matte lamination (reduces glare under tradeshow lighting)
