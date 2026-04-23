---
project: openpulsehf
doc: docs/architecture.md
status: living
last_updated: 2026-04-23
---

# Architecture

## System goals

- Provide a Rust-native, plugin-based software modem for amateur radio data links.
- Keep a reusable workspace split into core, audio, modem engine, and frontend crates.
- Maintain reliable loopback testing that works without external audio hardware.
- Keep frontend behavior consistent by making CLI the reference execution path.
- Support incremental protocol growth through plugin-based modulation modes.

## Core architecture

1. Input payload is framed into OpenPulseHF packets with sequence and CRC.
2. A modulation plugin transforms frames into baseband symbols and samples.
3. An audio backend transports samples to and from loopback or hardware I/O.
4. A receive pipeline demodulates, validates frames, and reassembles payload data.
5. Frontend surfaces status and decoded payloads to users and automation.

For HPX, the pipeline also includes signed session handshake validation and signed transfer manifest verification before delivery completion is acknowledged.
For relayed operation, the control plane includes peer discovery cache, query handling, and route selection across one or more relay hops.

## Workspace architecture

| Crate/Path | Role |
|-----------|------|
| crates/openpulse-core | Core traits (ModulationPlugin and AudioBackend), frame format, CRC-16, and plugin registry |
| crates/openpulse-audio | Audio backend implementations: in-process loopback (testing) and CPAL-based backends |
| crates/openpulse-modem | Modem engine wiring plugins and audio together |
| crates/openpulse-cli | openpulse binary and user-facing CLI options |
| plugins/bpsk | BPSK modulation plugin with NRZI and raised-cosine pulse shaping |

Future plugins (for example QPSK or ARDOP-compatible modes) should implement ModulationPlugin and register at startup.

## Supported modulation modes

| Mode | Baud rate | Notes |
|------|-----------|-------|
| BPSK31 | 31.25 | Narrow-band HF, inspired by PSK31 |
| BPSK63 | 62.5 | Higher throughput than BPSK31 |
| BPSK100 | 100 | Useful for loopback testing |
| BPSK250 | 250 | Wide-band and faster data paths |

Planned:

- HPX500: adaptive high-resilience profile in 500 Hz class.
- HPX2300: adaptive higher-throughput profile in 2300-2400 Hz class.

## Frame format

OpenPulseHF frames follow this logical layout:

```text
magic("OPLS") | version(0x01) | sequence(u16, big-endian) | length(u8) | payload | crc16(ccitt)
```

The payload length range is 0-255 bytes.

## Frontend architecture

- CLI is production-first and defines expected behavior.
- Additional frontends may be added, but must call stable core APIs.
- Frontends must not duplicate modem logic that belongs in shared crates.

## Platform support

| Platform | Audio backend |
|----------|---------------|
| Linux | ALSA, including PipeWire through ALSA compatibility |
| macOS | CoreAudio |
| Windows | WASAPI |
| Any | In-process loopback for hardware-free testing |

## Performance architecture

- Real-time behavior depends on bounded buffering and deterministic frame timing.
- Loopback and no-default-features test paths remain fast and stable in CI.
- Optional optimization work should preserve functional parity with baseline paths.
- Modem execution should separate I/O, framing, and DSP stages so they can run on dedicated worker threads.
- Thread scheduling strategy should avoid unbounded queues and preserve deterministic latency under load.
- GPU offload should target compute-intensive DSP components only when benchmarks show net gain.
- GPU path should use open acceleration stacks (preferred: Vulkan via wgpu; optional: OpenCL) with an always-available CPU path.

## Edge platform support

- Raspberry Pi 4 and Raspberry Pi 5 are supported edge targets for HPX operation.
- ARM64 builds must preserve feature parity for signed-transfer and trust workflows.
- Resource-aware execution profiles should be available for Pi-class CPU and memory budgets.

## Extensibility architecture

- New modulation families are introduced as plugins implementing shared traits.
- Plugin APIs must remain stable enough for out-of-tree experimentation.
- Core crates should remain embeddable for future automation and integrations.

For HPX, keep signal path adaptation logic and trust/signature logic as separate internal components so they can be tested independently.

## Security architecture

- Identity management and trust evaluation are control-plane concerns.
- Transfer signing and verification are data-plane admission checks.
- Verification failures must surface clear failure reasons to frontends and logs.
- Session-state behavior for security and recovery is defined in docs/hpx-session-state-machine.md.
- Relay path trust and end-to-end signer trust are evaluated independently.

## Routing and relay architecture

- Peer cache stores signed identity and capability descriptors with aging policy.
- Query engine supports local filter queries and bounded network query propagation.
- Route planner selects direct or multi-hop path using trust and link-quality scoring.
- Relay layer enforces loop prevention, replay protection, and hop-limited forwarding.

## Documentation process constraints

- Documentation updates flow through pull requests only.
- Frontmatter validation and stamping automation are required quality gates.
