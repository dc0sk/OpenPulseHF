---
project: openpulsehf
doc: docs/regulatory.md
status: living
last_updated: 2026-05-01
---

# Regulatory Compliance

This document analyses the amateur radio regulatory requirements applicable to OpenPulseHF transmissions. Compliance is a hard requirement before any on-air use. The analysis covers the primary jurisdictions of current interest: United States (FCC Part 97), European Union and CEPT member states, and the United Kingdom. It also covers IARU band plan recommendations.

This document is informational — it does not replace consulting the current licence conditions or regulations in force for a specific jurisdiction. Regulations change; operators are responsible for verifying currency.

## United States — FCC Part 97

All transmissions by US-licensed amateur stations on amateur frequencies are governed by 47 CFR Part 97. The following rules are directly relevant to OpenPulseHF.

### §97.307 — Emission standards

**§97.307(f):** Limits the maximum symbol rate per carrier for transmissions below 28 MHz in phone sub-bands and in the 60 m, 30 m, 17 m, and 12 m bands.

- Maximum 300 baud on a single carrier for most HF amateur sub-bands.

All current OpenPulseHF single-carrier modes comply:

| Mode | Baud rate | Compliant |
|------|-----------|-----------|
| BPSK31 | 31.25 | Yes |
| BPSK63 | 62.5 | Yes |
| BPSK100 | 100.0 | Yes |
| BPSK250 | 250.0 | Yes |
| QPSK125 | 125.0 | Yes (2 bits/symbol, not 2 baud) |
| QPSK250 | 250.0 | Yes |
| QPSK500 | 500.0 | Yes — QPSK symbol rate equals baud rate; 500 baud exceeds limit in restricted sub-bands. Verify per-band before use. |

Note: the 300 baud limit applies to the *symbol rate* (baud), not the *bit rate* (bps). QPSK500 transmits 500 symbols per second regardless of the 2 bits/symbol. On bands where the 300-baud limit applies, QPSK500 is non-compliant. Operators must verify the specific band and sub-band before use.

For OFDM modes (future HPX profiles): the 300-baud limit applies *per carrier*. VARA's 42-baud/carrier OFDM complies despite 52 carriers. Any future OFDM or multi-carrier mode in OpenPulseHF must verify per-carrier baud rate.

**§97.307(a):** Occupied bandwidth must not exceed that necessary for the information rate and type of emission in use. For HF narrowband digital modes, the occupied bandwidth should be 500 Hz or less for narrow profiles and should remain within the authorised sub-band.

### §97.309 — AFSK, FSK, Baudot and ASCII

**§97.309(a)(4):** Unspecified digital codes are permitted provided the control operator makes the necessary technical information available to the FCC upon request, and provided the emission is not used to obscure the meaning of the message.

OpenPulseHF transmissions are not encoded to obscure content. The protocol specification is public. This satisfies §97.309(a)(4) by design, provided the technical documentation is kept current and publicly accessible.

### §97.119 — Station identification

**§97.119(a):** Every amateur station must transmit its assigned call sign on its operating frequency at the end of each communication and at least every 10 minutes during a communication.

**§97.119(b)(2):** When transmitting in a digital code, identification must be given in the same digital code used for the communication, in the international Morse code, or in telephony in the English language.

OpenPulseHF requirements:
- The station call sign must be included in session handshake frames and in periodic in-band identification beacons at intervals not exceeding 10 minutes.
- The identification must be decodable by any station running OpenPulseHF (or any open implementation of the protocol) without special decryption.
- Signed identity records in the trust store satisfy this requirement if the callsign is included in plaintext in the signed payload.

### §97.221 — Automatically controlled digital stations

An automatically controlled digital station is one operating without a control operator present at the control point. HPX relay nodes running unattended are automatically controlled stations.

**§97.221 requirements for automatically controlled stations:**
- The station must have an automatic control point capable of terminating transmissions.
- The station must be capable of being turned off by the control operator at any time.
- Power must not exceed 100 W PEP output (unless the station is in a specifically authorised location).
- The station may only communicate with other amateur stations for the purpose of retransmitting third-party communications only when specifically authorised.
- The station must still identify per §97.119.

OpenPulseHF relay node implementations must document the automatic control point interface and must not exceed power limits for automatically controlled operation.

### §97.313 — Transmitter power standards

- Stations must use the minimum transmitter power necessary to carry out the desired communication.
- No specific limit for most HF amateur bands other than the general 1500 W PEP maximum for non-automatically-controlled stations.
- Automatically controlled digital stations: 100 W PEP maximum in most cases.

### Summary table — FCC Part 97

| Rule | Requirement | OpenPulseHF status |
|------|-------------|-------------------|
| §97.307(f) | ≤300 baud/carrier below 28 MHz | Current modes compliant; QPSK500 needs per-band verification |
| §97.309(a)(4) | Unspecified codes: technical info available | Compliant by design (open spec) |
| §97.119 | ID every 10 min and at end of transmission | Must be implemented in session and relay modes |
| §97.221 | Automatic control point for unattended relay | Must be addressed in relay node implementation |
| §97.313 | Minimum necessary power | Operational guidance; not a software constraint |

---

## European Union and CEPT

Amateur radio in EU member states is governed at the national level but harmonised through CEPT (European Conference of Postal and Telecommunications Administrations) recommendations. The key instruments are:

### CEPT T/R 61-01 — Harmonised Amateur Radio Licence

T/R 61-01 establishes the CEPT amateur radio licence harmonisation, enabling cross-border operation for CEPT member country licence holders without a separate visa or permit.

Relevant provisions:
- The CEPT licence covers the amateur radio frequency allocations defined in the ITU Radio Regulations.
- Technical conditions (power, bandwidth, modes) are those of the *visited country*, not the home country licence.
- Operators visiting another CEPT country must therefore verify that the modes they intend to use are permitted in the visited country under the relevant national implementing regulation.

OpenPulseHF implication: documentation must state the occupied bandwidth and modulation characteristics of each mode precisely so that operators can assess compliance in their specific national jurisdiction.

### ECC/REC(05)06 — Amateur radio digital modes

This CEPT recommendation provides harmonised guidance for digital modes in the amateur radio service across member states. Key provisions:

- Digital modes using published, publicly available technical specifications are generally permitted on all amateur frequency allocations where the mode's bandwidth fits within the authorised emission designator.
- Automatic/unattended digital stations (store-and-forward nodes, relay nodes) are permitted subject to the national administration's specific conditions; many EU administrations require notification or coordination.
- Station identification requirements under ECC/REC(05)06 align with ITU Radio Regulations Article 19: identification at least every 10 minutes during a communication and at the end.

### EU/CEPT symbol rate and bandwidth

Unlike FCC Part 97, CEPT does not impose a blanket 300-baud symbol rate limit. Instead, bandwidth is controlled by emission designator and by national frequency allocation tables. For HF:

- Most CEPT member states permit amateur emissions up to 2.7–3 kHz occupied bandwidth in the HF phone sub-bands.
- Narrowband digital sub-bands (analogous to US practice) are defined in the IARU Region 1 band plan and observed by most administrations.
- There is no per-carrier baud rate limit: an OFDM signal with 52 carriers at 42 baud/carrier is equally permitted as a single-carrier signal at 42 baud, provided the total occupied bandwidth is within limits.

OpenPulseHF implication: QPSK500 and future HPX modes are likely compliant with most EU administrations from a symbol rate perspective; the binding constraint is occupied bandwidth.

### Germany — Amateurfunkverordnung (AFuV) and BNetzA

The German amateur radio regulation (AFuV, implementing the Amateurfunkgesetz) and BNetzA (Federal Network Agency) guidance are particularly specific:

- §12 AFuV: Technical characteristics of amateur emissions must be determinable (i.e. reproducible/decodable by a technically competent third party). This is satisfied by open protocol specifications.
- The BNetzA publishes a "Technische Richtlinie" (technical guideline) for amateur radio. Modes that deviate from Appendix 1 standard emission designators must have technical documentation available.
- Unattended automatic operation (Relaisbetrieb) is addressed in §13 AFuV; relay stations require registration with BNetzA in some configurations.

### United Kingdom — Ofcom Amateur Licence

The UK left EU and CEPT licensing arrangements in specific senses post-Brexit; UK operators should verify current conditions directly against the Ofcom Amateur Licence.

Current conditions of particular relevance:
- Digital modes are permitted on all amateur bands under the standard Full (and to a lesser extent Foundation/Intermediate) licence conditions.
- Station identification: every 15 minutes and at the end of each communication (slightly different from the 10-minute US/CEPT requirement). Identification must be in a form receivable and identifiable to other stations.
- Occupied bandwidth: must remain within the band limits; no separate mode-specific bandwidth limit for most HF digital modes.
- Automatic/unattended operation: permitted under the Full licence subject to the station being under the control of the licensee and identifiable.
- The Ofcom licence permits use of the CEPT T/R 61-01 visiting procedure for visiting licensees from most countries, meaning foreign operators with a valid CEPT licence can operate in the UK.

UK-specific OpenPulseHF note: identification interval of 15 minutes (not 10) should be the default when operating from a UK station. The implementation should allow the identification interval to be configured.

### France — ANFR

The French national frequency agency (ANFR) implements EU amateur radio regulations. No specific additional constraints beyond CEPT/ECC for digital HF modes. Automatic stations require notification.

### Netherlands — Agentschap Telecom

Generally permissive for digital modes. Follows ECC/REC(05)06. Automatic stations are permitted subject to non-interference obligations.

---

## IARU Band Plan Recommendations

The IARU (International Amateur Radio Union) publishes band plans for Regions 1 (Europe, Africa, Middle East), 2 (Americas), and 3 (Asia-Pacific). These are not regulations but are widely observed as good practice to minimise interference between amateur stations.

### HF narrowband digital sub-bands (selected)

| Band | Frequency range | IARU usage note |
|------|----------------|-----------------|
| 40 m | 7.040–7.060 MHz | Narrowband digital modes (Region 1) |
| 30 m | 10.140–10.150 MHz | Digital modes (all regions); 10.147–10.150 recommended for automatic stations |
| 20 m | 14.070–14.099 MHz | Narrowband digital modes |
| 20 m | 14.099–14.112 MHz | Beacons and wide-band digital (Region 1 and 2) |
| 17 m | 18.095–18.105 MHz | Digital modes |
| 15 m | 21.070–21.150 MHz | Narrowband digital modes |
| 10 m | 28.050–28.120 MHz | Narrowband digital modes |

### Recommended operating frequencies for OpenPulseHF

OpenPulseHF should publish recommended dial frequencies for each supported bandwidth class aligned with the IARU band plan:

- **HPX500 / BPSK31/63/250 (≤500 Hz occupied BW):** operate in the narrowband digital sub-bands listed above.
- **HPX2300 (≈2300 Hz occupied BW):** operate in sub-bands designated for wider digital modes where available (e.g. 14.099–14.112 MHz on 20 m, subject to national regulations). Verify with national band plans — not all administrations extend wide-band digital permission to the same segments.
- **Relay nodes:** 10.147–10.150 MHz (30 m) is internationally recognised for automatic digital stations. Coordination with other automatic stations on the frequency is expected.

### Non-interference obligation

IARU band plans carry no legal force, but interference complaints between amateurs are handled by national administrations. Operators using OpenPulseHF in segments not aligned with the IARU plan risk complaints. The documentation and CLI should suggest IARU-aligned frequencies as defaults where possible.

---

## Regulatory compliance checklist for releases

Before any release that enables on-air transmission, the following must be confirmed:

- [ ] All transmitted modes have documented symbol rates and occupied bandwidths.
- [ ] Symbol rate per carrier verified ≤ 300 baud for all modes intended for use below 28 MHz (US FCC §97.307(f)).
- [ ] In-band station identification implemented and transmitting callsign in decodable form at ≤ 10-minute intervals (≤ 15-minute UK).
- [ ] Identification interval is user-configurable.
- [ ] Protocol specification is publicly available and technically sufficient for decoding.
- [ ] Relay/automatic control point interface documented and tested.
- [ ] Documentation recommends IARU-aligned operating frequencies.
- [ ] Release notes state applicable jurisdiction limitations (e.g. QPSK500 frequency restrictions).
