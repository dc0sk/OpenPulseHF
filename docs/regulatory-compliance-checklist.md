---
project: openpulsehf
doc: docs/regulatory-compliance-checklist.md
status: living
last_updated: 2026-10-15
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

## What the software does today (checked 2026-10-15)

Several requirements below were written in May 2026 as proposals for the software. This table says
which exist in the code. Items marked **not implemented** are operator actions today.

| Item | Status | Where |
|---|---|---|
| Station callsign | `[station] callsign` in `config.toml` (`~/.config/openpulse/` on Linux). The placeholder `N0CALL` disables auto-ID and blocks keying on the ARDOP TNC and the daemon's autonomous responders | `crates/openpulse-config`, `crates/openpulse-core/src/station_id.rs` |
| Periodic + sign-off station ID | **Implemented** on the daemon and the ARDOP TNC: `DE <callsign>` in the active mode every `auto_id_interval_secs` (default 600) while transmitting, and after `auto_id_signoff_idle_secs` (default 10) of quiet. Optional Morse CW ID on the TNC (`CWID TRUE`) | `station_id.rs`, `cw_id.rs` |
| Per-frame TX record | **Implemented**: every emitted frame appends callsign, UTC timestamp (ms), mode, declared power and frame sequence to `[station] tx_log_path` (default `~/.local/share/openpulse/tx-log.ndjson`), wired in the daemon | `crates/openpulse-core/src/tx_metadata.rs`, `crates/openpulse-daemon/src/server.rs` |
| Declared TX power | `[station] tx_power_watts` (daemon, TNCs) and the CLI's global `--max-power` (default 100) are **recorded only**. The modem cannot measure PA output and does not limit power | `crates/openpulse-modem/src/engine.rs` |
| Callsign or timestamp in the wire frame header | **Not implemented.** The frame is `OPLS` magic, version, sequence, length, payload, CRC-16. "Frame header v2" below is a proposal | `crates/openpulse-core/src/frame.rs` |
| Power validation against band or jurisdiction, `--jurisdiction` flag | **Not implemented** | — |
| Band-edge check on transmit | **Not implemented** in the daemon. The on-air test runners refuse a frequency outside 144.500–144.750 MHz by default and only ever lower rig power (`scripts/lib/rig-power.sh`) | `scripts/run-onair-ic9700-ft991a.sh`, `scripts/run-onair-twin-ota.sh` |
| PTT watchdog, single keying path | **Implemented**: one `SharedPtt`, a second key refused, force-release after 180 s | `crates/openpulse-radio/src/shared_ptt.rs` |

---

## Part I: United States — FCC Part 97

Analysis and band/power tables: see [`docs/regulatory.md` § United States — FCC Part 97](regulatory.md#united-states--fcc-part-97).

### Frequency Allocations

**Compliance Requirement 1.1**: Operator shall verify frequency selection against:
- [  ] FCC database of current allocations (https://www.fcc.gov/oet/spectrum/table)
- [  ] Local repeater listings to avoid conflicts
- [  ] Band plan from local radio club or ARRL

### TX Power Limits

**Compliance Requirement 1.2**: Power is set on the rig by the operator. OpenPulseHF:
- [x] Accepts `--max-power <watts>` on the CLI (default 100) and `[station] tx_power_watts` in config — recorded, not enforced
- [  ] Validate power against band allocation at TX time — not implemented
- [  ] Warn if requested power exceeds 1500 W — not implemented
- [  ] Refuse TX above a jurisdiction maximum — not implemented
- [x] Records the declared TX power for every frame in the TX log (not a measured value)

### Station Identification

**Compliance Requirement 1.3**: OpenPulseHF:
- [  ] Carries the callsign in every transmitted frame header — not implemented; the callsign goes out in the periodic and sign-off ID frames and in the signed handshake
- [x] Logs callsign and a millisecond UTC timestamp for every transmitted frame in the local TX log
- [x] Sends a station ID at least every 10 minutes while transmitting, and a sign-off ID at the end
- [x] Reads the callsign from `config.toml` under `[station]`

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
- [  ] Country-specific power default — not implemented; set power on the rig
- [  ] Validate against national limits at TX time — not implemented
- [x] Declared TX power recorded per frame in the TX log
- [  ] For cross-border mobile operation: use the most restrictive limit (operator action; e.g. roaming in Germany: 750 W)

### Station Identification

**Compliance Requirement 2.3**: EU operation shall:
- [x] Callsign (e.g. `DL1ABC/P`) sent in the station ID and logged per frame in the TX log (not in the frame header)
- [  ] Include `/P` or `/M` suffix if portable or mobile (operator action: put it in `[station] callsign`; country-dependent)
- [x] Millisecond timestamp logged per frame in the TX log
- [  ] Include locator (4-digit grid square) in session metadata if available (e.g., `JO62VC`)

### Regulatory Approval

National regulators vary in requirements for experimental operation:

The "approval required" column that stood here was not sourced and has been removed. Check
with the regulator before operating a new mode:

| Country | Contact |
|---|---|
| Germany (BNetzA) | https://www.bundesnetzagentur.de/amateurfunk |
| France (ANFR) | https://www.anfr.fr/particuliers/radio-amateur |
| UK (Ofcom) | See Part III |

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
- [  ] Hold a valid Ofcom amateur licence (or a foreign licence Ofcom recognises)
- [  ] Confirm frequency against Ofcom UK Band Plan (https://www.ofcom.org.uk/amateur-radio/licensing)
- [  ] Verify power limits per band (all HF bands: 1 kW max)
- [  ] Log callsign with `/M` suffix if mobile or `/P` if portable

### TX Power — Ofcom Rules

Ofcom permits up to 1 kW ERP on HF amateur bands, with no UK-territory reduction (see [`docs/regulatory.md`](regulatory.md#united-kingdom--ofcom-amateur-licence)).

**Compliance Requirement 3.2**: UK TX power shall:
- [  ] Set power on the rig within the licence limit (the software does not set or validate power)
- [  ] For field portable operation: power must match antenna setup (e.g., wire dipole + 100 W tuner)
- [  ] Document power in session metadata and on-air announcements

### Station Identification and Logging

> Identification interval: treat 15 minutes as the conservative default per [`docs/regulatory.md`](regulatory.md#united-kingdom--ofcom-amateur-licence); verify the current Ofcom Amateur Licence terms, which take precedence.

**Compliance Requirement 3.3**: UK operation shall:
- [x] Callsign (e.g. `G4ABC`, `M0XYZ/P`) sent in the station ID and logged per frame in the TX log
- [  ] Include `/M` or `/P` suffix if portable/mobile (operator action, in `[station] callsign`)
- [  ] Set `[station] auto_id_interval_secs` to the interval you need (default 600 s meets a 15-minute rule)
- [x] Millisecond timestamp logged per frame in the TX log
- [  ] Store session metadata with callsign, date, time, band, mode, power for operator records

### Experimentation Approval

**Compliance Requirement 3.4**: UK novel-mode operation:
- [  ] Check Ofcom website (https://www.ofcom.org.uk) for current guidance on experimental digital modes
- [  ] If uncertain, ask Ofcom (or the RSGB for advice) before operating; describe the mode with the published protocol and ladder (`docs/dev/design/protocol-wire-spec.md`, `docs/mode-fec-ladder.md`)

---

## Part IV: Technical Compliance Implementation

### CLI flags (proposal; only `--max-power` exists)

The May 2026 proposal was `openpulse-cli transmit --frequency … --mode … --max-power … --jurisdiction
… --station-id …`. Today only the global `--max-power <watts>` exists (default 100), and it is
recorded in the TX log, not enforced. There is no `--jurisdiction` or `--station-id` flag; the
callsign comes from `[station] callsign`.

### Frame Header Extension

**Frame Header v2** (proposed, not implemented; the v0.17 wire format is
`docs/dev/design/protocol-wire-spec.md`). Changing the frame header is a wire-format change and
needs a design review before implementation:

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

Proposed, not implemented. What exists is the per-frame TX log (`[station] tx_log_path`, one JSON
object per frame: `station_id`, `timestamp_ms`, `mode`, `power_watts`, `frame_sequence`). The
proposal was that every session produce:

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
- [ ] **Power**: set on the rig ≤ jurisdiction limit; `[station] tx_power_watts` set to the declared value for the TX log
- [ ] **Callsign**: `[station] callsign` set (not `N0CALL`); station ID heard on the air
- [ ] **Timestamp**: host clock synchronised with NTP (the TX log uses the host clock)
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
| 1.2 | 2026-10-15 | Agent | Added the status-against-code table; marked unimplemented proposals (power enforcement, `--jurisdiction`, frame header v2, session JSON); recorded implemented auto-ID, CW ID and TX log; removed the unsourced approval-required column and the RSGB-licence wording |
