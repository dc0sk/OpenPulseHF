---
project: openpulse
doc: docs/high-performance-mode.md
status: living
last_updated: 2026-04-23
---

# High-Performance Mode Analysis

## Scope

This document defines the target characteristics for a new OpenPulse mode family intended to perform as well as, or better than, established amateur-radio data modems in practical operation.

The scope is first-principles design and public-source-driven engineering. It does not assume proprietary protocol internals.

## Problem statement

OpenPulse needs a mode that improves usable throughput, link reliability, and operator confidence under variable HF and VHF channel conditions, while preserving open implementation and reproducible testing.

## Competitive target framing

The target is not byte-level protocol compatibility. The target is outcome parity or better in measurable link outcomes versus strong incumbents.

Primary reference competitors:

- VARA family (publicly observable behavior and published claims)
- PACTOR family (publicly documented operating behavior)
- ARDOP family (open and publicly documented behavior)

## Performance objectives

OpenPulse HPX mode (working name) must prioritize the following metrics:

- Goodput at equal occupied bandwidth under reproducible channel profiles.
- Time to first successful payload delivery from cold link start.
- Retry efficiency and completion rate under fading and impulse noise.
- Stability of operation across low, medium, and strong SNR regimes.
- Recovery speed after short dropouts and tuning offsets.

## Benchmark model

All competitive claims must be backed by repeatable benchmark suites.

### Channel profiles

Minimum profile set:

- HF narrow profile: selective fading, Doppler drift, burst noise.
- HF wide profile: moderate multipath, variable SNR over session.
- VHF FM profile: near-static path, occasional burst interference.
- Stress profile: rapid SNR swings and timing jitter.

### Bandwidth profiles

Minimum occupied bandwidth presets:

- 500 Hz class for weak-signal and crowded spectrum operation.
- 2300-2400 Hz class for higher data rate operation.

### Benchmark outputs

Each run must emit:

- raw throughput and goodput
- transfer success or failure rate
- median and p95 transfer completion time
- retransmission count and ARQ efficiency
- estimated spectral efficiency in bit/s/Hz

## HPX technical feature set

### Adaptive modulation and coding

- Support mode adaptation across modulation/coding combinations during session.
- Include at least one robust low-rate profile and one high-throughput profile.
- Add coding-rate agility and interleaver-depth control with bounded latency.

### Link adaptation and resilience

- Add channel quality estimation with periodic update cadence.
- Add adaptation hysteresis to avoid unstable mode flapping.
- Include ARQ with selective retransmission capability.
- Include configurable burst interleaving for fading resistance.

### Session model

- Define explicit states: discovery, training, active transfer, recovery, teardown.
- Require deterministic state transitions with timeout and retry bounds.
- Log state transitions in machine-readable form for benchmarking.

## Signed transfer and PKI-like trust model

OpenPulse HPX must support cryptographically signed transfers with an operator-manageable trust model.

### Identity and trust

- Each station has a signing identity key pair.
- Public keys are distributed as signed identity records (certificate-like documents).
- Trust anchors are managed locally through a trust store.
- Trust decisions are explicit: trusted, untrusted, revoked, unknown.

### Signature requirements

- Session handshake messages are signed.
- File transfer manifest is signed before payload transfer.
- Per-chunk or per-frame integrity authentication is required.
- Receiver verifies signatures before marking transfer complete.

### Recommended baseline crypto

- Signature algorithm default: Ed25519.
- Hash baseline: SHA-256 or stronger.
- Optional encryption can be layered later and is out of scope for first milestone.

### Revocation and lifecycle

- Trust store supports key revocation markers.
- Identity records include validity windows.
- Rotation procedure is documented for planned key changes.

## Engineering constraints

- Keep HPX mode implementation as a plugin with stable trait boundaries.
- Maintain loopback-test compatibility for baseline CI and local development.
- Keep control-plane and data-plane logic independently testable.

## Exit criteria for first HPX milestone

- Benchmark suite implemented and runnable in CI-friendly reduced mode.
- HPX meets or exceeds defined baseline targets in at least two channel profiles.
- Signed transfer handshake and manifest verification pass conformance tests.
- CLI exposes HPX mode and trust-related options with documented behavior.
