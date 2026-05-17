---
project: openpulsehf
doc: docs/openpulse-manual.md
status: living
last_updated: 2026-05-17
---

# OpenPulseHF Complete Manual

This manual is the operator, integrator, and engineering reference for OpenPulseHF.
It is written as a practical guide first and a technical reference second.

Safety and compliance note:
- OpenPulseHF can run modes and features that are not legal in all jurisdictions or all bands.
- You are responsible for selecting legal frequencies, occupied bandwidth, power, and identification behavior for your country and license class.
- Review [docs/regulatory.md](regulatory.md), [docs/regulatory-compliance-checklist.md](regulatory-compliance-checklist.md), and [Chapter 11](#chapter-11-compliance-notes) before on-air operation.

---

## Table of Contents

1. [Chapter 1: Specification](#chapter-1-specification)
2. [Chapter 2: Quick Start by Use Case](#chapter-2-quick-start-by-use-case)
3. [Chapter 3: Detailed Standard Manual (Core Modem)](#chapter-3-detailed-standard-manual-core-modem)
4. [Chapter 4: Advanced Manual](#chapter-4-advanced-manual)
5. [Chapter 5: Technical Manual (Architecture and Implementation)](#chapter-5-technical-manual-architecture-and-implementation)
6. [Chapter 6: Comparison and Advantages](#chapter-6-comparison-and-advantages)
7. [Chapter 7: Collaboration and Plugins](#chapter-7-collaboration-and-plugins)
8. [Chapter 8: Non-GPLv3 Interfaces](#chapter-8-non-gplv3-interfaces)
9. [Chapter 9: Support and Funding](#chapter-9-support-and-funding)
10. [Chapter 10: Glossary and References](#chapter-10-glossary-and-references)
11. [Chapter 11: Compliance Notes](#chapter-11-compliance-notes)

---

## Chapter 1: Specification

Cross-reference:
- Deployment basics are in [Chapter 2](#chapter-2-quick-start-by-use-case) and [Chapter 3](#chapter-3-detailed-standard-manual-core-modem).
- Advanced operational features are in [Chapter 4](#chapter-4-advanced-manual).
- Deep implementation details are in [Chapter 5](#chapter-5-technical-manual-architecture-and-implementation).
- Regulatory constraints and references are in [Chapter 11](#chapter-11-compliance-notes).

### 1.1 Product Scope

OpenPulseHF is a Rust workspace implementing:
- A plugin-based modem stack (multiple modulation families)
- Adaptive ARQ/session behavior (HPX)
- Multiple FEC and compression strategies
- Signed transfer and trust workflows
- Interop surfaces (ARDOP TCP, KISS TCP, B2F/Winlink gateway)
- Advanced features (QSY, relay, mesh, daemon/panel/TUI)

### 1.2 Mode and Waveform Specification

Current modulation catalog (38 modes across 7 plugin families):

#### BPSK plugin (5 modes)
- `BPSK31`
- `BPSK63`
- `BPSK100`
- `BPSK250`
- `BPSK250-RRC`

#### QPSK plugin (12 modes)
- `QPSK125`
- `QPSK250`
- `QPSK500`
- `QPSK1000`
- `QPSK1000-HF`
- `QPSK1000-HF-RRC`
- `QPSK500-RRC`
- `QPSK1000-RRC`
- `QPSK2000`
- `QPSK2000-RRC`
- `QPSK9600`
- `QPSK9600-RRC`

#### 8PSK plugin (10 modes)
- `8PSK500`
- `8PSK1000`
- `8PSK1000-HF`
- `8PSK1000-HF-RRC`
- `8PSK500-RRC`
- `8PSK1000-RRC`
- `8PSK2000`
- `8PSK2000-RRC`
- `8PSK9600`
- `8PSK9600-RRC`

#### 64QAM plugin (3 modes)
- `64QAM500`
- `64QAM1000`
- `64QAM2000-RRC`

#### FSK4 plugin (1 mode)
- `FSK4-ACK`

#### OFDM plugin (2 modes)
- `OFDM16`
- `OFDM52`

#### SC-FDMA plugin (5 modes)
- `SCFDMA16`
- `SCFDMA52`
- `SCFDMA52-16QAM`
- `SCFDMA52-64QAM`
- `SCFDMA52-64QAM-P4`

### 1.3 FEC Specification

Available FEC/session protection modes include:
- None
- RS (Reed-Solomon)
- RS-Interleaved
- Concatenated
- ShortRS
- RsStrong
- SoftConcatenated
- LDPC iterative decoder path (engine dispatch available)

Operational guidance:
- HF bursty channels usually favor RS/RS-Interleaved/concatenated profiles.
- High-quality links can use lighter overhead profiles.

### 1.4 Compression Specification

Compression algorithms in the core:
- None
- LZ4
- Zstd with embedded HPX dictionary

Behavior:
- Compression can be negotiated in signed handshake context.
- `compress_if_smaller()` selection behavior chooses best practical result for payload.

### 1.5 Session, Security, and Trust Specification

Security and trust layers include:
- Ed25519-signed handshake and signed transfer manifest verification
- Post-quantum handshake support (ML-DSA-44 signatures, ML-KEM-768 key encapsulation)
- Hybrid signing mode (classical + PQ)
- Trust classification and policy profiles

### 1.6 Advanced Feature Specification

- QSY frequency agility (signed frame exchange, candidate scanning and switch coordination)
- Multi-hop relay and query propagation
- Mesh daemon with peer discovery and cache
- Cross-band repeater support with dual-rig model (`rig_a` receive side, `rig_b` transmit side)
- IQ output mode for SDR workflows
- Optional GPU acceleration paths in supported plugin flows

### 1.7 Current Non-Code Gate Status

Code implementation is broad and test-backed.
The major explicit non-code gate still tracked separately is on-air regulatory validation/reporting workflow.

---

## Chapter 2: Quick Start, by Use Case

Cross-reference:
- For complete CLI and architecture details, continue to [Chapter 3](#chapter-3-detailed-standard-manual-core-modem) and [Chapter 5](#chapter-5-technical-manual-architecture-and-implementation).

### 2.1 Basic Local Validation (No RF Hardware)

```bash
cargo build --release
openpulse modes
openpulse devices
openpulse --backend loopback transmit "CQ CQ TEST" --mode BPSK100
openpulse --backend loopback receive --mode BPSK100
```

### 2.2 Mode Recommendation by SNR

```bash
openpulse mode-advisor --snr 12
openpulse mode-advisor --snr 20
```

### 2.3 Session Metrics Export

```bash
openpulse session-metrics --format json
openpulse session-metrics --format text
```

### 2.4 Monitor Engine Events (NDJSON)

```bash
openpulse monitor --mode BPSK100
openpulse monitor --mode QPSK500
```

### 2.5 ARDOP TNC Drop-In Path

```bash
cargo run -p openpulse-ardop -- --cmd-port 8515 --data-port 8516 --mode BPSK250 --backend loopback
```

Use your ARDOP-compatible client against the command/data ports.

### 2.6 KISS TNC Drop-In Path

```bash
cargo run -p openpulse-kiss -- --port 8100 --mode BPSK250 --backend loopback
```

Attach APRS/AX.25 software to KISS-over-TCP.

### 2.7 Winlink/B2F Path

```bash
cargo run -p openpulse-gateway -- --callsign N0CALL send --to W1AW --subject "test" --message "hello"
```

### 2.8 Mesh Daemon Start

```bash
cargo run -p openpulse-mesh -- --mode BPSK250 --max-hops 3
```

Ensure `[mesh] enabled = true` in config, otherwise daemon exits with info message.

### 2.9 Operator UI Path

```bash
cargo run -p openpulse-daemon
cargo run -p openpulse-tui -- --mode BPSK100
cargo run -p openpulse-panel
```

### 2.10 CI-Compatible Validation Path

```bash
cargo fmt --all -- --check
cargo clippy --workspace --no-default-features -- -D warnings
cargo test --workspace --no-default-features
```

---

## Chapter 3: Detailed Standard Manual (Core Modem)

This chapter focuses on standard modem operation and common deployment patterns.

Cross-reference:
- Advanced flows (QSY/relay/mesh/repeater/signed workflows) are in [Chapter 4](#chapter-4-advanced-manual).
- Plugin authoring and validation strategy are in [Chapter 7](#chapter-7-collaboration-and-plugins).

### 3.1 Component Model

Core execution components:
- Core protocol/types: `openpulse-core`
- Audio backends: `openpulse-audio`
- Modem execution pipeline: `openpulse-modem`
- User CLI: `openpulse-cli`
- Modulation plugins: `plugins/*`

Standard execution chain:
1. Payload enters CLI/session path
2. Frame encoding and optional FEC/compression
3. Plugin modulates samples
4. Audio backend emits/captures samples
5. Plugin demodulates and frame validation occurs
6. Decoded payload returned to user/output surface

### 3.2 Standard Use Cases

#### A. Interactive transmit and receive

```bash
openpulse --backend default transmit "status report" --mode BPSK250
openpulse --backend default receive --mode BPSK250
```

#### B. Session lifecycle operations

```bash
openpulse session start --peer W1AW
openpulse session state --diagnostics --format json
openpulse session log --follow --follow-timeout-ms 5000
openpulse session end
```

#### C. Benchmark and regression evaluation

```bash
openpulse benchmark run
openpulse benchmark run --min-pass-rate 1.0 --max-mean-transitions 20.0
```

#### D. Broadcast and beacon patterns

```bash
openpulse broadcast --payload "network status" --mode BPSK250 --ttl 2 --callsign N0CALL
openpulse beacon --mode BPSK250 --interval 600 --callsign N0CALL --ttl 1
```

### 3.3 Identity and Trust Workflows

#### Identity diagnostics

```bash
openpulse identity show N0CALL
openpulse identity verify N0CALL
openpulse identity cache
```

#### Trust operations

```bash
openpulse trust show N0CALL
openpulse trust explain N0CALL
openpulse trust list
openpulse trust policy show
openpulse trust policy set balanced
```

#### Trust store update example

```bash
openpulse trust import --station-id N0CALL --key-id ed25519:abcd1234 --trust full --source out_of_band
openpulse trust revoke --station-or-key N0CALL --reason operator_revoked
```

### 3.4 Diagnostics and Verification

```bash
openpulse diagnose handshake --peer W1AW --format json
openpulse diagnose manifest --session demo-session --format json
openpulse diagnose session --peer W1AW --format text
```

### 3.5 Standard Configuration Baseline

Use template:

```bash
openpulse config init > ~/.config/openpulse/config.toml
```

Recommended baseline:
- Conservative HF mode to start (`BPSK250` or `QPSK500`)
- Explicit backend selection
- Trust policy set to balanced/strict according to operation
- Logging level set to `info` for field work, `debug` for issue triage

### 3.6 Drop-In Replacement Cookbook

This section focuses on replacing existing TNC/modem endpoints with OpenPulseHF while keeping upstream client software unchanged.

#### A. ARDOP-compatible replacement

```bash
cargo run -p openpulse-ardop -- --cmd-port 8515 --data-port 8516 --mode BPSK250 --backend loopback
```

Typical client expectations to validate:
- Command port responds to VERSION/STATE/LISTEN/CONNECT sequence.
- Data port framing remains u16 big-endian length-prefixed binary payloads.
- Disconnect and reconnect handling preserves client workflow.

#### B. KISS/AX.25 TCP replacement

```bash
cargo run -p openpulse-kiss -- --port 8100 --mode BPSK250 --backend loopback
```

Validation checklist:
- KISS FEND/FESC byte-stuffing behavior is preserved.
- AX.25 UI frame payloads round-trip correctly.
- Multi-frame client sessions do not deadlock under reconnect.

#### C. Standard operational script examples

```bash
# station startup
openpulse --backend default receive --mode BPSK250

# second terminal: transmit smoke test
openpulse --backend default transmit "OPHF link check" --mode BPSK250

# diagnostics snapshot
openpulse session state --diagnostics --format json
openpulse trust policy show
openpulse session-metrics --format json
```

Cross-reference:
- For advanced transport behaviors (relay/mesh/QSY/signed workflows), continue to [Chapter 4](#chapter-4-advanced-manual).
- For implementation-level protocol and engine details, continue to [Chapter 5](#chapter-5-technical-manual-architecture-and-implementation).

---

## Chapter 4: Advanced Manual

This chapter covers advanced behaviors and high-leverage workflows.

### 4.1 QSY Mode (Frequency Agility)

Purpose:
- Stations negotiate migration to better frequencies based on candidate scans.

Protocol frames:
- `QSY_REQ`
- `QSY_LIST`
- `QSY_VOTE`
- `QSY_ACK`
- `QSY_REJECT`

CLI examples:

```bash
openpulse qsy status
openpulse qsy init --rig 127.0.0.1:4532
```

Config example:

```toml
[qsy]
enabled = true
allow_trustlevels = ["verified", "psk_verified"]
bandplan_mode = "ham-iaru-r1"
bandplan_awareness_enabled = true
enforce_max_channel_width = true
enforce_segment_conventions = true
candidate_freqs_hz = [7074000, 7076000, 7104000]
scan_dwell_ms = 500
switchover_offset_s = 5
```

### 4.2 Relay Mode

Purpose:
- Forward traffic with hop controls and trust-aware policies.

Key behaviors:
- Hop-limit enforcement
- Duplicate suppression
- Route scoring and best-route selection

Usage examples:

```bash
openpulse relay status
openpulse relay routes --format json
openpulse relay policy --show
```

Use with:
- Mesh daemon for networked propagation
- Policy and trust tuning in config

### 4.3 Mesh Mode

Start:

```bash
cargo run -p openpulse-mesh -- --mode BPSK250 --max-hops 3
```

Config example:

```toml
[mesh]
enabled = true
max_hops = 3
relay_policy = "balanced"
store_forward_ttl_s = 300
beacon_interval_s = 60
peer_cache_capacity = 256
peer_cache_ttl_s = 3600
```

Operational pattern:
- Each node performs periodic step cycles and emits mesh events
- Peer cache updates via beacons and query responses
- Multi-hop relay paths are selected by score and policy constraints

### 4.4 Dual-Transceiver and Cross-Band Repeater

Model:
- `rig_a`: receive/primary side
- `rig_b`: transmit/secondary side

Config example:

```toml
[radio.rig_a]
rigctld_addr = "127.0.0.1:4532"
backend = "rigctld"

[radio.rig_b]
rigctld_addr = "127.0.0.1:4533"
backend = "rigctld"

[repeater]
enabled = true
mode = "BPSK250"
tx_hang_ms = 500
full_duplex = false
```

Behavior highlights:
- Half duplex: assert PTT around each forwarded frame
- Full duplex mode available (`full_duplex = true`) with session-level PTT handling

Usage examples:

```bash
cargo run -p openpulse-repeater -- --mode BPSK250 --max-hops 2
openpulse monitor --mode BPSK250
```

### 4.5 Custom Zstd Dictionary Training

Dictionary trainer is available at `tools/openpulse-dict-trainer`.

Examples:

```bash
cargo run -p openpulse-dict-trainer
cargo run -p openpulse-dict-trainer -- --dict-size 8192 --output ./zstd-hpx-dict.bin
cargo run -p openpulse-dict-trainer -- --corpus-dir ./my-corpus --dict-size 4096 --output ./zstd-hpx-dict.bin
```

Trainer details:
- Uses built-in synthetic HPX/Winlink corpus if no external corpus directory is provided
- Produces a zstd dictionary artifact and prints dictionary ID

Integration note:
- Runtime compression uses embedded dictionary in `openpulse-core`; replacing dictionary in production requires controlled code and test updates.

### 4.6 Signed Transfers and Handshake Modes

Capabilities:
- Classical signed handshake + manifest
- PQ-capable and hybrid handshake variants

Examples:

```bash
openpulse diagnose handshake --peer W1AW --format json
openpulse diagnose manifest --session session-123 --format json
openpulse trust explain W1AW --format json
```

Operational best practices:
- Keep trust store curated and backed up
- Prefer strict or balanced policy for operational paths
- Capture diagnostics logs during field issues for trust and signature events

Cross-reference:
- Trust and signature implementation detail is expanded in [Chapter 5.5](#55-security-and-trust-topics).
- Regulatory and jurisdictional constraints are summarized in [Chapter 11](#chapter-11-compliance-notes).

---

## Chapter 5: Technical Manual (Architecture and Implementation)

Primary technical references now live under [docs/dev/README.md](dev/README.md).

### 5.1 Workspace and Layering

Representative layer map:
- Core and protocol primitives: `openpulse-core`
- DSP and signal utilities: `openpulse-dsp`
- Channel simulation: `openpulse-channel`
- Audio backends: `openpulse-audio`
- Runtime modem engine: `openpulse-modem`
- User and service frontends: CLI/TUI/panel/daemon
- Interop surfaces: ARDOP, KISS, B2F, gateway

### 5.2 Data Flow and Runtime Contracts

Pipeline contracts:
- Plugin boundary takes/returns normalized sample payloads
- Audio backend boundary abstracts loopback vs hardware
- Session/state machinery emits structured events for observability

Event stream:
- Engine events can be consumed via monitor/daemon for NDJSON workflows.

### 5.3 Signal Processing Topics

Covered in implementation and docs:
- Pulse shaping and demod paths
- AFC and correction loops
- Timing/coherence handling
- Equalization and profile-specific tuning
- OFDM and SC-FDMA profile behavior

### 5.4 Reliability and Access Control Topics

- DCD/CSMA shared channel access
- ACK taxonomy and adaptive rate changes
- FEC mode dispatch and comparative behavior
- HARQ/soft-combining and profile-based adaptation

### 5.5 Security and Trust Topics

- Handshake mode selection under policy
- Trust-level classification and transition semantics
- Signed manifest validation
- PQ and hybrid compatibility

### 5.6 Test Strategy and CI Discipline

Primary CI-compatible gate:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --no-default-features -- -D warnings
cargo test --workspace --no-default-features
```

Additional practice:
- Targeted crate tests during iteration
- Channel-model and integration-suite replay after feature changes

### 5.7 Performance and Deployment Topics

- Loopback-first deterministic validation
- Optional hardware backend activation and troubleshooting
- Edge/portable operational profile support

---

## Chapter 6: Comparison and Advantages

This chapter positions OpenPulseHF against commonly used digital-radio software categories.

### 6.1 Positioning

OpenPulseHF differentiators include:
- Fully open Rust workspace with plugin-centric architecture
- Strong in-tree integration testing across protocol and signal layers
- Broad feature surface in one stack (modem + trust + relay + mesh + UI + interop)
- Built-in modern trust/signing path including PQ-capable handshake support

### 6.2 Practical Advantages

Compared with closed or single-focus stacks, OpenPulseHF offers:
- Auditability: protocol, modem, and trust logic are inspectable
- Extensibility: new waveforms and behaviors can be added as plugins/crates
- Repeatability: loopback and channel simulation workflows reduce hardware friction
- Integration range: ARDOP, KISS, B2F, daemon control API, and panel/TUI surfaces

### 6.3 Operator Advantages

- Flexible deployment from local loopback to daemon/panel operations
- Rich diagnostics and event streaming for troubleshooting
- Multiple operating profiles from conservative to high-throughput paths

### 6.4 Engineering Advantages

- Workspace structure supports focused incremental delivery
- Test-backed behavior changes are straightforward to stage and verify
- Non-GPL integration strategies are documented for proprietary interop needs

---

## Chapter 7: Collaboration and Plugins

### 7.1 Contribution Workflow

Recommended path for new plugin work:
1. Read [docs/dev/contributing-plugins.md](dev/contributing-plugins.md)
2. Implement `ModulationPlugin` trait in dedicated plugin crate
3. Add unit tests plus at least one loopback/integration path
4. Register plugin in target runtime(s)
5. Run CI-compatible checks before PR

### 7.2 Plugin Design Checklist

- Clear mode naming and supported mode list
- Deterministic behavior under loopback tests
- Explicit demod error handling and bounds checks
- Soft demod path when applicable for advanced FEC
- Documentation updates in features/roadmap/manual where relevant

### 7.3 Collaboration Norms

- Keep PRs narrow and test-backed
- Avoid unrelated refactors in feature PRs
- Document behavior changes and operator-facing implications

### 7.4 Example New Plugin Bring-Up

```bash
cd plugins
cargo new mymode-plugin --lib
# implement trait + tests
cargo test -p mymode-plugin
cargo clippy -p mymode-plugin -- -D warnings
```

Then wire into runtime registration in the target binary or engine wrapper.

---

## Chapter 8: Non-GPLv3 Interfaces

OpenPulseHF includes documented paths for proprietary or third-party integration without forcing in-tree GPL plugin coupling.

Canonical reference:
- [docs/dev/plugin-commercial-interface.md](dev/plugin-commercial-interface.md)

### 8.1 Approach A: Dynamic C ABI Shim

- Runtime-loaded shared library approach via C ABI boundary
- Suitable where low-latency in-process plugin handling is required

### 8.2 Approach B: Out-of-Process IPC

- Separate process model via local socket/pipe protocol
- Suitable for language-neutral plugins and stronger isolation

### 8.3 Approach C: Proprietary Data Layer Above Open Transport

- Treat OpenPulseHF as transparent bearer
- Keep proprietary logic entirely above ARDOP/KISS/B2F transport boundary

### 8.4 Integration Checklist

- Validate legal and regulatory context for your jurisdiction/band/service
- Define and document ABI/wire compatibility
- Add health metrics and timeout behavior from day one
- Build conformance tests around error and version skew handling

Legal note:
- This is technical guidance, not legal advice.

---

## Chapter 9: Support and Funding

OpenPulseHF advances fastest when operators, testers, and contributors close the loop with real evidence.

### 9.1 How You Can Help (Non-Financial)

High-value contributions:
- Reproducible bug reports with command lines, logs, and environment details
- Benchmark result sets and comparisons to previous baselines
- RF field notes tied to concrete mode/profile/channel context
- Documentation clarity fixes where operators get blocked

Suggested report bundle:
- Config excerpt (sanitized)
- Commands used
- Relevant logs and NDJSON monitor snippets
- Expected vs observed behavior

### 9.2 Testing and Validation Support

You can contribute by running:

```bash
cargo test --workspace --no-default-features
cargo run -p openpulse-testmatrix --no-default-features
```

Attach artifacts and observations to issues/PRs.

### 9.3 Financial Support

If OpenPulseHF helps your operations, consider funding development and maintenance.

Current funding link:
- https://www.paypal.com/donate/?hosted_button_id=WY9U4MQ3ZAQWC

Funding is used to sustain:
- Engineering time for feature and reliability work
- Documentation and validation maintenance
- Infrastructure and release operations

### 9.4 Collaboration Mindset

The most effective support is specific, reproducible, and respectful.
Whether you contribute logs, tests, docs, code, or funding, you are helping build a stronger open stack.

---

## Chapter 10: Glossary and References

### 10.1 Glossary

- ACK: Acknowledgment frame indicating reception outcome.
- AFC: Automatic Frequency Control, tracks/corrects residual frequency offset.
- ARDOP: Amateur Radio Digital Open Protocol, often used via TCP TNC interface.
- ARQ: Automatic Repeat reQuest reliability strategy.
- B2F: Winlink message framing/session protocol family.
- BER: Bit Error Rate.
- CPAL: Cross-platform audio I/O crate used by OpenPulse audio backend.
- CSMA: Carrier Sense Multiple Access channel access strategy.
- DCD: Data Carrier Detect channel busy estimator.
- DFE: Decision Feedback Equalizer.
- FEC: Forward Error Correction.
- FER: Frame Error Rate.
- FSK4: Four-tone FSK mode used for ACK/control path.
- HPX: OpenPulse adaptive session/profile and state-machine framework.
- I/Q: In-phase and quadrature signal representation.
- KISS: Keep It Simple Stupid framing for AX.25/TNC transport.
- LDPC: Low-Density Parity-Check code family.
- LLR: Log-Likelihood Ratio soft decision value.
- LMS: Least Mean Squares adaptive filter/equalizer method.
- ML-DSA-44: Post-quantum signature scheme used in PQ handshake path.
- ML-KEM-768: Post-quantum key encapsulation mechanism used in PQ handshake path.
- NDJSON: Newline-delimited JSON stream format.
- OFDM: Orthogonal Frequency Division Multiplexing.
- PAPR: Peak-to-Average Power Ratio.
- PKI: Public Key Infrastructure.
- PQ: Post-quantum.
- QSY: Frequency shift procedure/negotiation between stations.
- RS: Reed-Solomon code.
- SAR: Segmentation and Reassembly.
- SC-FDMA: Single-Carrier Frequency Division Multiple Access (DFT-spread OFDM style waveform).
- SNR: Signal-to-Noise Ratio.
- TNC: Terminal Node Controller interface/service model.
- TTL: Time-to-live or hop limit field.
- Winlink: Message transport ecosystem commonly accessed through B2F/ARDOP paths.

### 10.2 Public References

General protocol and DSP references:
- RFC 8032 (EdDSA / Ed25519): https://www.rfc-editor.org/rfc/rfc8032
- NIST FIPS 203 (ML-KEM): https://csrc.nist.gov/pubs/fips/203/final
- NIST FIPS 204 (ML-DSA): https://csrc.nist.gov/pubs/fips/204/final
- Zstandard format and dictionary docs: https://facebook.github.io/zstd/
- Reed-Solomon overview: https://en.wikipedia.org/wiki/Reed%E2%80%93Solomon_error_correction
- LDPC overview: https://en.wikipedia.org/wiki/Low-density_parity-check_code
- OFDM overview: https://en.wikipedia.org/wiki/Orthogonal_frequency-division_multiplexing
- IARU HF band plan portal: https://www.iaru.org/reference/band-plans/
- FCC Part 97 eCFR: https://www.ecfr.gov/current/title-47/chapter-I/subchapter-D/part-97

Project-local references:
- Architecture: [docs/dev/architecture.md](dev/architecture.md)
- Features: [docs/features.md](features.md)
- CLI guide: [docs/cli-guide.md](cli-guide.md)
- Roadmap: [docs/dev/roadmap.md](dev/roadmap.md)
- Plugin collaboration: [docs/dev/contributing-plugins.md](dev/contributing-plugins.md)
- Commercial interface: [docs/dev/plugin-commercial-interface.md](dev/plugin-commercial-interface.md)
- Regulatory analysis: [docs/regulatory.md](regulatory.md)
- On-air test plan: [docs/on-air_testplan.md](on-air_testplan.md)

---

## Chapter 11: Compliance Notes

This chapter summarizes regulatory and operational compliance touchpoints for OpenPulseHF operation.

### 11.1 Scope and legal posture

- OpenPulseHF is software; legal operation depends on operator actions, station licensing, regional rules, and band plans.
- Always validate local legal requirements before transmitting.
- Treat this chapter as an engineering aid, not legal advice.

### 11.2 FCC (United States)

- Primary amateur service rule set: 47 CFR Part 97.
- Reference: https://www.ecfr.gov/current/title-47/chapter-I/subchapter-D/part-97
- Practical checks:
	- Emission within authorized band segments.
	- Station identification timing and format.
	- Power and occupied bandwidth constraints for the selected band and mode.

### 11.3 CEPT and IARU region alignment

- CEPT recommendations and national implementations influence HF operating practice across Europe.
- IARU band plans should be followed for practical coexistence and interoperability.
- IARU reference portal: https://www.iaru.org/reference/band-plans/

### 11.4 EU regulatory context

- Verify applicable national administration rules (licensing, permitted emissions, duty cycle, and station obligations).
- For radio equipment/system integration, consult relevant RED compliance obligations where applicable.
- European Commission RED overview: https://single-market-economy.ec.europa.eu/sectors/radio-equipment_en

### 11.5 Operational compliance checklist

- Start with [docs/regulatory.md](regulatory.md) and [docs/regulatory-compliance-checklist.md](regulatory-compliance-checklist.md).
- Use [docs/on-air_testplan.md](on-air_testplan.md) to structure pre-deployment and field validation.
- Keep logs and test artifacts for traceability, especially when introducing new modes, plugins, or policy settings.

### 11.6 Engineering compliance-by-design guidance

- Prefer conservative default modes and narrow bandwidth profiles for first deployment.
- Use loopback and simulated channel paths before any on-air tests.
- Gate field operation behind explicit trust policy configuration and reproducible diagnostics capture.
- Document station-specific SOPs for identification cadence, fallback behavior, and emergency stop procedures.

---

End of manual.
