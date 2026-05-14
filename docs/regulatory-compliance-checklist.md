---
project: openpulsehf
doc: docs/regulatory-compliance-checklist.md
status: living
last_updated: 2026-05-14
---

# OpenPulseHF Regulatory Compliance Checklist

This document provides a structured compliance framework for operating OpenPulseHF across major jurisdictions: **United States (FCC Part 97)**, **European Union (CEPT/ECC)**, and **United Kingdom (Ofcom)**.

## Scope

Compliance verification covers:
- **Frequency allocations**: Authorized bands per jurisdiction
- **TX power limits**: Maximum effective radiated power (ERP) per mode and band
- **Station identification**: Callsign logging and transmission requirements
- **Operating modes**: Permitted digital modes and constraints
- **Operation plan**: On-air testing and deployment authorization

---

## Part I: United States — FCC Part 97

### Frequency Allocations

FCC Part 97 allocates Amateur Radio bands across HF (2–30 MHz) and higher. OpenPulseHF targets the following HF allocations:

| Band | Frequency Range | Mode | CW/Data | License Class | Notes |
|---|---|---|---|---|---|
| 160m | 1.800–2.000 MHz | CW, USB | Data allowed | General | Limited use outside US |
| 80m | 3.500–4.000 MHz | CW, USB, LSB | Data allowed | General | Split between CW and phone |
| 40m | 7.000–7.300 MHz | CW, USB, LSB | Data allowed | General | CW-preferred below 7.100 |
| 30m | 10.100–10.150 MHz | CW, USB, Data | Data only | General | No phone; narrowband (200 Hz BW) |
| 20m | 14.000–14.350 MHz | CW, USB | Data allowed | General | CW-preferred region |
| 17m | 18.068–18.168 MHz | CW, USB | Data allowed | General | Narrowband region above 18.110 |
| 15m | 21.000–21.450 MHz | CW, USB | Data allowed | General | CW-preferred below 21.200 |
| 12m | 24.890–24.990 MHz | CW, USB | Data allowed | General | Narrowband region above 24.930 |
| 10m | 28.000–29.700 MHz | CW, USB, SSB | Data allowed | Technician+ | FM and SSB permitted |
| 6m | 50.000–54.000 MHz | CW, USB, FM, SSB | Data allowed | Technician+ | VHF; USB above 50.125 MHz |

**Compliance Requirement 1.1**: Operator shall verify frequency selection against:
- [  ] FCC database of current allocations (https://www.fcc.gov/oet/spectrum/table)
- [  ] Local repeater listings to avoid conflicts
- [  ] Band plan from local radio club or ARRL

### TX Power Limits

FCC Part 97.313 specifies power limits by band:

| Band | Power Limit (PEP) | Notes |
|---|---|---|
| 160m–12m (HF) | 1.5 kW | Standard amateur limit |
| 10m, 6m | 1.5 kW | Standard amateur limit |
| Above 50 MHz | Per allocation | Generally less restrictive |

**Compliance Requirement 1.2**: OpenPulseHF shall:
- [  ] Accept `--max-power <watts>` CLI flag (default: 100 W)
- [  ] Validate power against band allocation at TX time
- [  ] Warn if requested power exceeds 1500 W (log as ERROR)
- [  ] Enforce hard cap: refuse TX if `--max-power > jurisdiction_max`
- [  ] Include actual TX power in session log

**Implementation Note**: Most amateur stations operate < 100 W. The CLI default of 100 W is conservative and suitable for initial testing.

### Station Identification

FCC Part 97.119 requires periodic station identification by callsign:
- Identification required every 10 minutes during continuous transmission
- Identification at end of transmission
- Identification in CW or voice at operator's discretion

**Compliance Requirement 1.3**: OpenPulseHF shall:
- [  ] Log station callsign (`station_id`) in every transmitted frame (header field)
- [  ] Log transmission timestamp (millisecond precision) in every frame
- [  ] Emit identification beacon (callsign only) every 10 minutes if TX is continuous
- [  ] Include callsign in session metadata (public / logged)
- [  ] Configuration: station callsign read from `~/.config/openpulse/config.toml` under `[station]` section

### Operating Modes

FCC Part 97.307 defines permitted modes. For narrowband digital:

| Mode Class | FCC Designation | Bandwidth Limit | OpenPulseHF Support |
|---|---|---|---|
| Data | "data" | See band plan | BPSK31–QPSK1000 |
| RTTY | Radioteletype | 170 Hz nominal | Not implemented |
| Packet | AX.25/KISS | Per band | KISS interface available |

**Compliance Requirement 1.4**: OpenPulseHF operation shall:
- [  ] Operate only in modes explicitly listed in FCC Part 97.307
- [  ] Confirm bandwidth ≤ permitted allocation for selected band
- [  ] Document mode in session metadata
- [  ] For HPX modes: confirm ≤ 2.7 kHz occupied BW (HF voice channel standard)

### On-Air Testing Approval

On-air operation requires:
1. Valid FCC amateur radio license (General class or higher for HF)
2. Authorized equipment (verified by manufacturer or homebrew build)
3. Documented operating plan with testing objectives

**Compliance Requirement 1.5**: Before on-air deployment:
- [  ] FCC license holder must review operating procedures
- [  ] Create and sign-off on operating plan (see Item 8 use-case deployment guide)
- [  ] Document test objectives, duration, and expected band impact
- [  ] Notify local radio club or reflector of testing activity (courtesy)

---

## Part II: European Union — CEPT/ECC Harmonized Bands

### Frequency Allocations

The European Conference of Postal and Telecommunications Administrations (CEPT) and European Communications Committee (ECC) define harmonized amateur radio allocations. EU member states implement these via national regulations (e.g., Germany: BNetzA, UK: Ofcom, France: ANFR).

**Key EU HF Allocations**:

| Band | Frequency Range | Primary Allocation | Max Power (ERP) | Notes |
|---|---|---|---|---|
| 160m | 1.810–2.000 MHz | AM, CW, SSB, Data | 1 kW | Varies by country; some limit to 500 W |
| 80m | 3.500–3.800 MHz | CW, SSB, Data | 1 kW | Phone typically 3.600–3.800 |
| 40m | 7.000–7.100 MHz | CW, Data | 1 kW | 7.100–7.200: CW + SSB split |
| 30m | 10.100–10.150 MHz | Data, CW | 1 kW | No phone; narrowband (200 Hz BW) |
| 20m | 14.000–14.100 MHz | CW, Data | 1 kW | Phone: 14.100–14.350 |
| 17m | 18.068–18.168 MHz | CW, SSB, Data | 1 kW | Narrowband above 18.110 |
| 15m | 21.000–21.110 MHz | CW, Data | 1 kW | Phone: 21.110–21.450 |
| 12m | 24.890–24.990 MHz | CW, SSB, Data | 1 kW | Narrowband above 24.930 |
| 10m | 28.000–29.700 MHz | CW, SSB, Data, FM | 1 kW | Most liberal allocation |

**Compliance Requirement 2.1**: EU operation shall:
- [  ] Verify frequency against national regulator (e.g., https://www.bundesnetzagentur.de for Germany)
- [  ] Confirm band allocation matches CEPT Rec. T/R 61-02 (current harmonized table)
- [  ] Log national license number and issue country in operating plan
- [  ] Respect country-specific power limits (some nations permit 500 W only on certain bands)

### TX Power Limits — Country-Specific

CEPT harmonizes at **1 kW ERP** for HF, but individual countries may enforce lower limits:

| Country | 80–10m HF Limit | Notes |
|---|---|---|
| Germany | 750 W | Most restrictive EU nation |
| France | 1 kW | Standard CEPT |
| UK | 1 kW | See separate Ofcom section |
| Netherlands | 1 kW | Standard |
| Spain | 750 W | Reduced limit |
| Italy | 1 kW | Standard |
| Poland | 1 kW | Standard |

**Compliance Requirement 2.2**: EU operation shall:
- [  ] Accept `--max-power <watts>` CLI flag with country-specific default (e.g., `de` → 750 W, `fr` → 1000 W)
- [  ] Validate against national limits at TX time
- [  ] Log actual TX power in session metadata
- [  ] For cross-border mobile operation: use most restrictive limit (e.g., if roaming Germany: 750 W)

### Station Identification

CEPT T/R 61-02 requires:
- Callsign identification at least every 10 minutes during transmission
- Call sign and/or locator in CW or voice

**Compliance Requirement 2.3**: EU operation shall:
- [  ] Log station callsign (e.g., `DL1ABC/0`) in every frame header
- [  ] Include `/0` or `/m` suffix if mobile or portable (optional, country-dependent)
- [  ] Log timestamp (millisecond precision) in every frame
- [  ] Include locator (4-digit grid square) in session metadata if available (e.g., `JO62VC`)

### Regulatory Approval

National regulators vary in requirements for experimental operation:

| Country | Approval Required | Contact |
|---|---|---|
| Germany (BNetzA) | Yes, for new modes | https://www.bundesnetzagentur.de/amateurfunk |
| France (ANFR) | Case-by-case | https://www.anfr.fr/particuliers/radio-amateur |
| UK (Ofcom) | Yes, for RSGB license holders | See Ofcom section |

**Compliance Requirement 2.4**: Before EU on-air deployment:
- [  ] Obtain written approval from national regulator if testing new digital modes
- [  ] Submit operating plan (band, frequency, duration, power, objectives)
- [  ] Include OpenPulseHF feature summary (e.g., "adaptive HARQ-rate QAM modulation")
- [  ] Wait for authorization letter (typically 2–4 weeks)

---

## Part III: United Kingdom — Ofcom

### Frequency Allocations and Licensing

Ofcom administers amateur radio in the UK under the Wireless Telegraphy Act 2006. Most UK amateurs operate under the **Radio Society of Great Britain (RSGB) full license** (equivalent to FCC General).

**UK HF Allocations**:

| Band | Frequency Range | Max Power (ERP) | Additional Rules | Notes |
|---|---|---|---|---|
| 160m | 1.810–2.000 MHz | 1 kW | QSO record-keeping | Allocation only; many stations inactive |
| 80m | 3.500–3.800 MHz | 1 kW | — | Primary HF band |
| 40m | 7.000–7.100 MHz | 1 kW | — | CW-preferred below 7.030 |
| 30m | 10.100–10.150 MHz | 1 kW | Narrowband (200 Hz BW) | Data mode preferred |
| 20m | 14.000–14.100 MHz | 1 kW | — | CW-preferred below 14.100 |
| 17m | 18.068–18.168 MHz | 1 kW | Narrowband (200 Hz BW) | Coordination encouraged |
| 15m | 21.000–21.110 MHz | 1 kW | — | CW-preferred below 21.110 |
| 12m | 24.890–24.990 MHz | 1 kW | Narrowband (200 Hz BW) | Limited activity |
| 10m | 28.000–29.700 MHz | 1 kW | — | Most active 28.000–28.500 (FM) |

**Compliance Requirement 3.1**: UK operation shall:
- [  ] Hold valid RSGB full license (or reciprocal foreign license recognized by Ofcom)
- [  ] Confirm frequency against Ofcom UK Band Plan (https://www.ofcom.org.uk/amateur-radio/licensing)
- [  ] Verify power limits per band (all HF bands: 1 kW max)
- [  ] Log callsign with `/M` suffix if mobile or `/P` if portable

### TX Power — Ofcom Rules

Ofcom permits up to **1 kW ERP** on HF amateur bands. Unlike EU countries, no reduction applies in UK territory.

**Compliance Requirement 3.2**: UK TX power shall:
- [  ] Accept `--max-power <watts>` CLI flag (Ofcom default: 1000 W)
- [  ] Validate against 1 kW hard limit
- [  ] For field portable operation: power must match antenna setup (e.g., wire dipole + 100 W tuner)
- [  ] Document power in session metadata and on-air announcements

### Station Identification and Logging

Ofcom requires:
- Call sign identification every 20 minutes during continuous transmission (more lenient than CEPT/FCC)
- Locator grid square recommended for direction-finding capability
- No formal logbook requirement for UK amateurs, but operator endorsement records recommended

**Compliance Requirement 3.3**: UK operation shall:
- [  ] Log station callsign (e.g., `G4ABC`, `M0XYZ/0`) in every frame header
- [  ] Include `/M` or `/P` suffix if portable/mobile (as registered with Ofcom)
- [  ] Log timestamp (millisecond precision) in every frame
- [  ] Store session metadata with callsign, date, time, band, mode, power for operator records

### Experimentation Approval

Ofcom rarely requires pre-approval for standard amateur modes. However, novel modes may trigger:

**Compliance Requirement 3.4**: UK novel-mode operation:
- [  ] Check Ofcom website (https://www.ofcom.org.uk) for current guidance on experimental digital modes
- [  ] If new mode approval required: contact RSGB technical committee for endorsement letter
- [  ] Include endorsement letter with Ofcom inquiry
- [  ] Most modern digital modes (PSK31, RTTY, FT8, etc.) are pre-approved; OpenPulseHF should document mode as "Adaptive HARQ-rate QAM over HF" and submit if uncertain

---

## Part IV: Technical Compliance Implementation

### CLI Flags

```bash
openpulse-cli transmit \
  --frequency 14.100 \
  --mode BPSK250 \
  --max-power 100 \
  --jurisdiction us \
  --station-id W5ABC \
  [--message <file>]

# Flags:
# --max-power <watts>       TX power limit (default: 100)
# --jurisdiction {us|de|fr|uk}  Compliance zone (default: us)
# --station-id <callsign>   Station callsign (read from config if absent)
```

### Frame Header Extension

**Frame Header v2** (proposed):

```
Bytes 0-3:   Magic "OPHF"
Byte 4:      Frame type (0x01 = data)
Byte 5:      HPX mode
Bytes 6-7:   Payload length
Bytes 8-12:  Timestamp (u32 ms, little-endian)
Bytes 13-20: Station ID hash (SHA256[:8] of callsign)
   OR
Bytes 13-28: Full 16-byte callsign (UTF-8, null-padded)
Bytes 29-:   Payload + FEC
```

**Compliance Detail**: Callsign is included unencrypted so receiver can identify station without decoding payload.

### Session Metadata JSON

Every session shall produce:

```json
{
  "session_id": "HPX-SESSION-20260514-001",
  "station_id": "W5ABC",
  "station_locator": "EM13",
  "frequency_mhz": 14.1,
  "mode": "BPSK250",
  "tx_power_w": 100,
  "jurisdiction": "us",
  "start_time_utc": "2026-05-14T14:30:00Z",
  "end_time_utc": "2026-05-14T14:32:45Z",
  "frames_transmitted": 45,
  "frames_received": 43,
  "compliance_checked": true,
  "compliance_notes": "FCC Part 97 verified; 100 W within amateur limit"
}
```

---

## Part V: Pre-Deployment Checklist

Before any on-air transmission:

- [ ] **License**: Valid amateur radio license in operating jurisdiction
- [ ] **Frequency**: Confirmed against band allocation and band plan
- [ ] **Power**: `--max-power` set ≤ jurisdiction limit; documented in session
- [ ] **Callsign**: Station ID configured in `config.toml`; tested in frame headers
- [ ] **Timestamp**: Verified that frame timestamps are synchronized with NTP
- [ ] **Operating Plan**: Written plan with test objectives, duration, expected band impact
- [ ] **Regulatory Approval**: Obtained if required (EU new modes; Ofcom experimental use)
- [ ] **Interference Coordination**: Checked with local radio club / reflector for conflicts
- [ ] **Safety**: Antenna installed safely; RF hazard assessment completed
- [ ] **Logging**: Session metadata and compliance notes stored locally

---

## Part VI: Compliance Sign-Off

**To be completed by operator and/or legal contact before production on-air use:**

| Item | Verified By | Date | Notes |
|---|---|---|---|
| FCC Part 97 compliance (US) | [Callsign] | — | |
| CEPT/ECC compliance (EU) | [Callsign/Country] | — | |
| Ofcom compliance (UK) | [Callsign] | — | |
| Technical compliance (power, callsign, timestamp) | [Engineer] | — | |
| Legal review (if applicable) | [Legal Contact] | — | |
| **AUTHORIZED FOR OPERATION** | [Authorization Authority] | — | |

---

## References

1. **FCC Part 97**: https://www.ecfr.gov/current/title-47/part-97
2. **CEPT Rec. T/R 61-02**: https://www.cept.org/documents/59/94
3. **Ofcom Amateur Radio Licensing**: https://www.ofcom.org.uk/amateur-radio/licensing
4. **RSGB Operating Guidelines**: https://www.rsgb.org/
5. **ARRL Band Plan**: https://www.arrl.org/band-plan

---

## Document Version History

| Version | Date | Author | Change |
|---|---|---|---|
| 1.0 | 2026-05-14 | Agent | Initial comprehensive checklist; FCC/CEPT/Ofcom sections; CLI/frame header specs |

