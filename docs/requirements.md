---
project: openpulsehf
doc: docs/requirements.md
status: living
last_updated: 2026-04-23
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
- Maintain deterministic timeout and retry behavior for session-state transitions.
- Use multithreaded execution for modem pipelines where it improves deterministic real-time behavior.
- Support optional GPU acceleration for compute-heavy signal-processing stages when it produces measurable benefit.
- GPU acceleration paths must use open frameworks (for example Vulkan via wgpu, or OpenCL) and provide a CPU fallback.
- Raspberry Pi 4/5 performance targets must be measured and published in benchmark artifacts.
- Peer cache lookup and query operations should remain bounded under large peer tables.
- Multi-hop relay control-plane traffic should include duplicate suppression and loop prevention.

## Compression requirements

- All wire payloads eligible for compression must be compressed before encryption and signing.
- Compression ratio is the primary optimisation target; CPU and memory cost are acceptable tradeoffs.
- The default compression algorithm must be Brotli at quality level 11 (maximum ratio).
- LZMA2 (xz) must be supported as a high-ratio alternative for long-block transfer chunks.
- Zstandard may be used in contexts where streaming decompression is required, configured at the highest compression level.
- Algorithm selection must be declared in the wire envelope so receivers can decompress without out-of-band negotiation.
- Uncompressed payloads must remain valid when compression yields no size reduction (for example short frames or pre-compressed data).
- Compression boundaries and algorithm selection must be included in signed manifests so integrity covers the pre-compression content.
- Relay nodes must not decompress and recompress payload; compression is applied end-to-end.

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

## Competitive performance requirements

- HPX must target outcome parity or better versus incumbent modems in benchmarked scenarios.
- Comparisons must use equal occupied bandwidth classes and published test conditions.
- Performance evaluation must include at least HF narrow, HF wide, and VHF FM profile families.
- No claim of proprietary protocol compatibility may be made without defensible public evidence.

## Compatibility and UX requirements

- CLI usage and docs must stay aligned across releases.
- New user-facing options must be documented in docs/cli-guide.md.
- README usage examples should stay current with implemented behavior.

## Documentation requirements

- Version bumps require updates to docs/changelog.md and docs/releasenotes.md.
- Docs files under docs/ must pass frontmatter validation in CI.
- HPX benchmark assumptions and result summaries must be captured in docs/high-performance-mode.md.
