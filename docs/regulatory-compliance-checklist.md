---
project: openpulsehf
doc: docs/regulatory-compliance-checklist.md
status: living
last_updated: 2026-06-17
---

# OpenPulseHF Regulatory Compliance Checklist

This is the **actionable** compliance checklist for operating OpenPulseHF across major jurisdictions: **United States (FCC Part 97)**, **European Union (CEPT/ECC)**, and **United Kingdom (Ofcom)**.

For the jurisdiction-by-jurisdiction regulatory analysis — the rule-by-rule rationale (FCC §97.307/§97.309/§97.119/§97.221/§97.313, CEPT T/R 61-01 and ECC/REC(05)06, Ofcom licence terms, IARU band plans), the bandwidth and symbol-rate derivations, the encryption-versus-authentication discussion, and the full per-jurisdiction band-allocation and power tables — see [`docs/regulatory.md`](regulatory.md). This checklist does not restate that analysis; it lists only the sign-off items, the per-band/per-mode compliance steps, and the operator actions required before and during on-air use.

## Scope

Compliance verification covers:
- **Frequency allocations**: authorised bands per jurisdiction
- **TX power limits**: maximum effective radiated power (ERP) per mode and band
- **Station identification**: callsign logging and transmission requirements
- **Operating modes**: permitted digital modes and constraints
- **Operation plan**: on-air testing and deployment authorisation

> Rationale and derivations for every item below are in [`docs/regulatory.md`](regulatory.md).

---

## Part I: United States — FCC Part 97

Analysis and band/power tables: see [`docs/regulatory.md` § United States — FCC Part 97](regulatory.md#united-states--fcc-part-97).

### Frequency Allocations

**Compliance Requirement 1.1**: Operator shall verify frequency selection against:
- [  ] FCC database of current allocations (https://www.fcc.gov/oet/spectrum/table)
- [  ] Local repeater listings to avoid conflicts
- [  ] Band plan from local radio club or ARRL

### TX Power Limits

**Compliance Requirement 1.2**: OpenPulseHF shall:
- [  ] Accept `--max-power <watts>` CLI flag (default: 100 W)
- [  ] Validate power against band allocation at TX time
- [  ] Warn if requested power exceeds 1500 W (log as ERROR)
- [  ] Enforce hard cap: refuse TX if `--max-power > jurisdiction_max`
- [  ] Include actual TX power in session log

**Implementation Note**: Most amateur stations operate < 100 W. The CLI default of 100 W is conservative and suitable for initial testing.

### Station Identification

**Compliance Requirement 1.3**: OpenPulseHF shall:
- [  ] Log station callsign (`station_id`) in every transmitted frame (header field)
- [  ] Log transmission timestamp (millisecond precision) in every frame
- [  ] Emit identification beacon (callsign only) every 10 minutes if TX is continuous
- [  ] Include callsign in session metadata (public / logged)
- [  ] Configuration: station callsign read from `~/.config/openpulse/config.toml` under `[station]` section

### Operating Modes

**Compliance Requirement 1.4**: OpenPulseHF operation shall:
- [  ] Operate only in modes explicitly listed in FCC Part 97.307
- [  ] Confirm bandwidth ≤ permitted allocation for selected band
- [  ] Document mode in session metadata
- [  ] For HPX modes: confirm ≤ 2.7 kHz occupied BW (HF voice channel standard)

### On-Air Testing Approval

**Compliance Requirement 1.5**: Before on-air deployment:
- [  ] FCC license holder must review operating procedures
- [  ] Create and sign-off on operating plan (see Part V pre-deployment checklist)
- [  ] Document test objectives, duration, and expected band impact
- [  ] Notify local radio club or reflector of testing activity (courtesy)

---

## Part II: European Union — CEPT/ECC Harmonized Bands

Analysis, country-specific power limits, and band tables: see [`docs/regulatory.md` § European Union and CEPT](regulatory.md#european-union-and-cept).

### Frequency Allocations

**Compliance Requirement 2.1**: EU operation shall:
- [  ] Verify frequency against national regulator (e.g., https://www.bundesnetzagentur.de for Germany)
- [  ] Confirm band allocation matches CEPT Rec. T/R 61-02 (current harmonized table)
- [  ] Log national license number and issue country in operating plan
- [  ] Respect country-specific power limits (some nations permit 500 W only on certain bands)

### TX Power Limits — Country-Specific

CEPT harmonises at 1 kW ERP for HF; several countries enforce lower limits (full table in [`docs/regulatory.md`](regulatory.md#eucept-transmitter-power--country-specific-limits)).

**Compliance Requirement 2.2**: EU operation shall:
- [  ] Accept `--max-power <watts>` CLI flag with country-specific default (e.g., `de` → 750 W, `fr` → 1000 W)
- [  ] Validate against national limits at TX time
- [  ] Log actual TX power in session metadata
- [  ] For cross-border mobile operation: use most restrictive limit (e.g., if roaming Germany: 750 W)

### Station Identification

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
| UK (Ofcom) | Yes, for RSGB license holders | See Part III |

**Compliance Requirement 2.4**: Before EU on-air deployment:
- [  ] Obtain written approval from national regulator if testing new digital modes
- [  ] Submit operating plan (band, frequency, duration, power, objectives)
- [  ] Include OpenPulseHF feature summary (e.g., "adaptive HARQ-rate QAM modulation")
- [  ] Wait for authorization letter (typically 2–4 weeks)

---

## Part III: United Kingdom — Ofcom

Analysis, licensing background, and band table: see [`docs/regulatory.md` § United Kingdom — Ofcom Amateur Licence](regulatory.md#united-kingdom--ofcom-amateur-licence).

### Frequency Allocations and Licensing

**Compliance Requirement 3.1**: UK operation shall:
- [  ] Hold valid RSGB full license (or reciprocal foreign license recognized by Ofcom)
- [  ] Confirm frequency against Ofcom UK Band Plan (https://www.ofcom.org.uk/amateur-radio/licensing)
- [  ] Verify power limits per band (all HF bands: 1 kW max)
- [  ] Log callsign with `/M` suffix if mobile or `/P` if portable

### TX Power — Ofcom Rules

Ofcom permits up to 1 kW ERP on HF amateur bands, with no UK-territory reduction (see [`docs/regulatory.md`](regulatory.md#united-kingdom--ofcom-amateur-licence)).

**Compliance Requirement 3.2**: UK TX power shall:
- [  ] Accept `--max-power <watts>` CLI flag (Ofcom default: 1000 W)
- [  ] Validate against 1 kW hard limit
- [  ] For field portable operation: power must match antenna setup (e.g., wire dipole + 100 W tuner)
- [  ] Document power in session metadata and on-air announcements

### Station Identification and Logging

> Identification interval: treat 15 minutes as the conservative default per [`docs/regulatory.md`](regulatory.md#united-kingdom--ofcom-amateur-licence); verify the current Ofcom Amateur Licence terms, which take precedence.

**Compliance Requirement 3.3**: UK operation shall:
- [  ] Log station callsign (e.g., `G4ABC`, `M0XYZ/0`) in every frame header
- [  ] Include `/M` or `/P` suffix if portable/mobile (as registered with Ofcom)
- [  ] Log timestamp (millisecond precision) in every frame
- [  ] Store session metadata with callsign, date, time, band, mode, power for operator records

### Experimentation Approval

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
6. **Jurisdiction analysis and band/power tables**: [`docs/regulatory.md`](regulatory.md)

---

## Document Version History

| Version | Date | Author | Change |
|---|---|---|---|
| 1.0 | 2026-05-14 | Agent | Initial comprehensive checklist; FCC/CEPT/Ofcom sections; CLI/frame header specs |
| 1.1 | 2026-06-17 | Agent | Deduplicated against `docs/regulatory.md`; removed duplicated prose analysis and band/power tables (now sole-sourced in `regulatory.md`); retained all actionable compliance requirements, technical implementation specs, and sign-off items |
