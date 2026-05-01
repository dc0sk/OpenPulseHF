---
project: openpulsehf
doc: docs/requirements.md
status: living
last_updated: 2026-05-01
---

# Requirements

## Functional requirements

- Provide a CLI capable of transmit, receive, device listing, and mode listing.
- Support at least one production modulation plugin (BPSK family).
- Preserve a loopback backend for hardware-free development and testing.
- Support cross-platform audio through CPAL-backed implementations.
- Validate frame integrity with versioning, sequence handling, and CRC checks.
- Define and implement a high-performance plugin mode (HPX) with adaptive modulation and coding.
- Support occupied bandwidth classes centered on 500 Hz and 2300-2400 Hz operation.
- Provide deterministic session state handling: discovery, training, active transfer, recovery, teardown.
- Support selective retransmission for ARQ-capable sessions.
- Support signed transfer handshake and signed transfer manifests.
- Support trust-store-based verification for station identities.
- Support peer caching of identity, capability, and link-quality metadata.
- Support local and network query interfaces for peer discovery and filtering.
- Support relayed transfers across multiple hops with configurable hop limits.
- Support route selection policies based on trust, reliability, and latency estimates.
- Define versioned wire-level envelopes for peer query, route discovery, and relay transfer control messages.

## Physical layer and radio interface requirements

- Audio backend must support a minimum sample rate of 48 kHz at 16-bit integer or 32-bit float resolution.
- The receive pipeline must apply a high-pass filter (cutoff ≤ 10 Hz) before demodulation to remove DC bias introduced by SSB radio audio paths.
- The demodulator must track station-to-station frequency offsets of up to ±50 Hz without operator intervention (automatic frequency control, AFC).
- The AFC subsystem must handle transmitter drift up to 1 Hz per second for normal SSB radio operation.
- Transmitter release (PTT drop) must occur within 50 ms of the last transmitted sample to preserve turnaround timing budgets.
- The receive path must begin acquiring signal within 150 ms of remote key-down to honour the turnaround timing contract.
- PTT keying must support at minimum: serial port RTS/DTR assertion and software-controlled VOX.
- CAT-based PTT control via Hamlib/rigctld is a recommended integration path that provides access to the majority of amateur transceivers without per-rig code.
- Audio input gain must remain within a range that preserves symbol amplitude stability; the system must document the expected input level range and provide a level indicator.

## Platform and dependency requirements

- Linux support is the primary target and requires ALSA development headers for CPAL builds.
- macOS support uses CoreAudio through CPAL.
- Windows support uses WASAPI through CPAL.
- Raspberry Pi 4 and Raspberry Pi 5 must be supported as first-class Linux deployment targets.
- ARM64 builds for Raspberry Pi 4/5 must be part of regular compatibility testing.
- Any development environment must support loopback mode for hardware-free testing.
- Rust toolchain must build the full workspace and no-default-features variant.

## Non-functional requirements

- Maintain workspace-level buildability on Linux and macOS CI runners.
- Keep tests runnable without physical audio hardware in default CI workflows.
- Ensure crate boundaries are clear enough for independent testing.
- Keep plugin additions from requiring broad refactors across unrelated crates.
- Define objective benchmark suites and publish method and result artifacts.
- Track goodput, completion rate, retry efficiency, and completion latency across channel profiles.
- Require HPX performance claims to be tied to reproducible benchmark runs.
- Benchmark channel profiles must include parameterized Watterson model scenarios (Good/Moderate/Poor path conditions) and Gilbert-Elliott burst error scenarios; AWGN-only benchmarks are insufficient for HF performance claims.
- Maintain deterministic timeout and retry behavior for session-state transitions.
- Use multithreaded execution for modem pipelines where it improves deterministic real-time behavior.
- Support optional GPU acceleration for compute-heavy signal-processing stages when it produces measurable benefit.
- GPU acceleration paths must use open frameworks (for example Vulkan via wgpu, or OpenCL) and provide a CPU fallback.
- Raspberry Pi 4/5 performance targets must be measured and published in benchmark artifacts.
- Peer cache lookup and query operations should remain bounded under large peer tables.
- Multi-hop relay control-plane traffic should include duplicate suppression and loop prevention.

## FEC and interleaving requirements

- All FEC-enabled transfer modes must pair the FEC codec with a block interleaver.
- The interleaver must shuffle symbols across multiple FEC blocks before transmission such that burst errors are dispersed into correctable random-error patterns.
- Default interleaver depth must be at least 5× the expected maximum burst error duration expressed in symbols at the target baud rate.
- Interleaver depth must be a documented parameter in each mode profile definition; it must not be a hidden constant.
- FEC and interleaver parameters must be agreed upon during session handshake and must not be assumed by either party.
- Benchmark scenarios must test FEC+interleaver effectiveness under burst-error conditions (Gilbert-Elliott model) and not only under AWGN.

## Channel access requirements

- Sessions operating in point-to-point mode may assume a dedicated channel and are not required to implement channel sensing.
- Sessions operating in broadcast or relay mode on a shared channel must implement a channel-clear detection (CCD) mechanism before transmitting.
- The reference channel access algorithm for shared-channel operation is 0.3-persistence CSMA: sense the channel, transmit immediately with 30% probability if clear, back off and retry otherwise.
- Data Carrier Detect (DCD) is the mechanism for CCD and must be derived from the demodulated signal energy, not from audio amplitude alone.
- Channel access policy must be documented per mode profile.

## Compression requirements

- Optional lossless payload compression at the session layer is in scope.
- Compression algorithm must be deterministic and produce identical output for identical input across platforms.
- Compression capability must be negotiated during session handshake and must not be assumed.
- If compression is active, compressed size must be compared to uncompressed size before transmission; a compressed frame larger than the uncompressed original must be sent uncompressed.
- Decompression failure must be treated as a frame integrity error.

## Security and trust requirements

- Signed transfers are mandatory for HPX file or object transfer mode.
- Station identities use asymmetric key pairs with operator-managed trust anchors.
- Session handshake messages must be signed and verified.
- Transfer manifests must be signed and verified before final acceptance.
- Trust status must include trusted, untrusted, revoked, and unknown states.
- Key lifecycle must include validity windows and revocation handling.
- Cryptographic defaults should use Ed25519 signatures and SHA-256 or stronger hashing.
- A post-quantum-safe signature method must be supported for identity and transfer-signing workflows.
- The implementation should support a hybrid signature mode (classical + post-quantum) during migration.
- Initial post-quantum-safe default should target ML-DSA (FIPS 204) where available.
- If session key establishment is used, a post-quantum-safe KEM option should be supported, with ML-KEM (FIPS 203) preferred.
- Trust-store metadata must record algorithm type and hybrid-policy requirements per identity.
- Relay path admission must enforce trust policy on each intermediate hop.
- Multi-hop transfers must preserve end-to-end signed integrity and fail closed on trust violations.
- Route metadata should support post-quantum-capable signing under configured policy.
- Relay and query messages must include anti-replay fields and enforce loop-prevention semantics.

### Post-quantum and frame size dependency

- ML-DSA-44 signatures are 2420 bytes. ML-KEM-768 public keys are 1184 bytes. Both exceed the current 255-byte frame payload limit.
- In-band post-quantum handshake messages cannot be carried in the current wire format without a segmentation and reassembly (SAR) sub-layer.
- SAR must be designed and implemented before in-band PQ handshake requirements can be satisfied.
- PQ signature transport requirements are therefore sequentially dependent on SAR delivery; planning must reflect this ordering.
- Out-of-band or application-layer PQ key distribution (for example via the PKI tooling) may proceed independently of SAR.

## Regulatory compliance requirements

Regulatory compliance is a hard requirement for any transmission on amateur radio frequencies. The following rules apply in the primary jurisdictions of interest. See docs/regulatory.md for full analysis and derivations.

### United States — FCC Part 97

- §97.307(f): The maximum symbol rate on any single carrier must not exceed 300 baud below 28 MHz in phone subbands. All currently planned OpenPulseHF single-carrier modes (BPSK31 at 31.25 baud through BPSK250 at 250 baud) satisfy this limit. Any new mode must verify compliance before release.
- §97.309(a)(4): Digital codes whose use is not specifically prohibited elsewhere and whose technical characteristics are publicly documented are permitted. OpenPulseHF must maintain a published technical specification sufficient for any amateur to decode the transmitted signal.
- §97.119(a): Station identification is required every 10 minutes during a transmission and at the end of each transmission series. In digital modes, identification must be in a format decodable by a receiving station.
- §97.221: Automatically controlled digital stations (unattended nodes, relay nodes) require an automatic control point. HPX relay nodes operating without a control operator present are automatically controlled stations and must comply with §97.221 including power limits and frequency restrictions.

### European Union and CEPT

- ECC/REC(05)06 and national implementations: CEPT harmonises amateur radio digital mode permissions across member administrations. Most EU member states permit amateur digital modes across all authorised bands subject to the general licence conditions (power, bandwidth, identification).
- CEPT T/R 61-01: harmonised licensing for portable cross-border operation within CEPT countries. OpenPulseHF documentation must state which modes and bandwidths are intended so that visiting operators can assess compliance with their visiting licence conditions.
- Bandwidth constraint: many EU administrations limit occupied bandwidth by band and mode class. For HF digital modes the typical permitted bandwidth is ≤ 2.7 kHz (matching SSB channel spacing). HPX2300/2400 Hz profiles must be validated against the occupied bandwidth definition used by the relevant national administration.
- Station identification: EU member states typically require identification at least every 10 minutes (consistent with FCC), though interval requirements vary (e.g. UK: every 15 minutes). The identification requirement is the same: it must be decodable by the receiving station in the digital mode in use, or transmitted in supplementary CW or voice.
- Germany (BNetzA): §12 Amateurfunkverordnung (AFuV) requires that technical characteristics of amateur emissions be determinable. Digital modes without a published open specification may be questioned by authorities; OpenPulseHF's open specification satisfies this requirement by design.
- United Kingdom (Ofcom): The UK Full licence permits digital modes on all amateur bands. Station identification every 15 minutes and at end of transmission. The UK left CEPT licensing arrangements post-Brexit; UK operators verify compliance with the current Ofcom amateur licence conditions document directly.

### IARU Region 1 and Region 2 band plans

- IARU band plans are non-binding recommendations but are widely observed to avoid mutual interference.
- Region 2 (Americas) and Region 1 (Europe/Africa/Middle East) both designate sub-bands for HF narrowband digital modes (e.g. 14.070–14.099 MHz on 20 m).
- OpenPulseHF documentation should recommend operating frequencies aligned with IARU band plan digital sub-bands for each supported band.
- Wide-band HPX2300 profiles should operate in segments where wide-band digital modes are plan-consistent (e.g. 14.099–14.112 MHz on 20 m where permitted by national administration).

## Competitive performance requirements

- Primary strategic goal: develop an independent, first-principles OpenPulse protocol stack that competes on reliability, throughput, and usability.
- HPX must target outcome parity or better versus incumbent modems in benchmarked scenarios.
- Comparisons must use equal occupied bandwidth classes and published test conditions.
- Performance evaluation must include at least HF narrow, HF wide, and VHF FM profile families.
- No claim of proprietary protocol compatibility may be made without defensible public evidence.
- Any compatibility mode targeting proprietary systems (including VARA or PACTOR-4) requires explicit legal review and approval before implementation work starts.

## Compatibility and UX requirements

- CLI usage and docs must stay aligned across releases.
- New user-facing options must be documented in docs/cli-guide.md.
- README usage examples should stay current with implemented behavior.

## Documentation requirements

- Version bumps require updates to docs/changelog.md and docs/releasenotes.md.
- Docs files under docs/ must pass frontmatter validation in CI.
- HPX benchmark assumptions and result summaries must be captured in docs/high-performance-mode.md.
