---
project: openpulsehf
doc: docs/dev/test-reports/on-air/a2-mode-corroboration-2026-11-03.md
status: resolved
last_updated: 2026-11-03
---

# A2 corroboration: the transmitted modes, measured from the receive audio (2026-11-03)

`scripts/onair-a2-mode-check.py` measured each logged data frame's baud and BPSK/QPSK from the receiver's
8 kHz recording, without the modem. Command per run:
`scripts/onair-a2-mode-check.py <rx recording> <recording start> <sender's daemon log>`.

| Run | Sender → receiver | Frames measured | Agree | Disagree | Too weak to measure |
|---|---|---|---|---|---|
| a2a | A → B | 57 | 57 | 0 | 7 (the mute and 0.02 steps) |
| a2b | B → A | 79 | 79 | 0 | 12 (the cut) |
| a2c | B → A | 103 | 103 | 0 | 10 (mostly the 0.035 step) |
| a2d | A → B | 73 | 73 | 0 | 0 |

**The climbs** (a2c, 05:07–05:11): 31.25, 62.5, 100 and 250 Bd BPSK, then 250 Bd QPSK. Each matches the
logged BPSK31, BPSK63, BPSK100, BPSK250 and QPSK250-D.

**The demotions:**
- **a2a**, A's frames:
  - 15:51:34.9 and 15:51:48.8: QPSK, 250 Bd;
  - **15:52:03.7: BPSK, 250 Bd**, the SL5 frame after the step-down at 15:51:54.7;
  - 15:52:11.8 on: QPSK again (the re-climb).
- **a2c**, B's frames:
  - 05:14:33.1: QPSK;
  - the next three QPSK and three BPSK250 frames, at the 0.035 drive, are too weak to measure;
  - **05:16:08.7 and 05:16:26.6: BPSK, 250 Bd** (SL5, after the step-down at 05:15:03.1; the drive was
    back at 05:16:05.3);
  - 05:16:35.0 on: QPSK (the re-climb).

The A2 transitions are physically identifiable in the received audio, as the criteria ask. The RSPdx RF
captures of A's transmissions were not analysed; they stay on rpi51.
