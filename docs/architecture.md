---
project: openpulse
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

1. Input payload is framed into OpenPulse packets with sequence and CRC.
2. A modulation plugin transforms frames into baseband symbols and samples.
3. An audio backend transports samples to and from loopback or hardware I/O.
4. A receive pipeline demodulates, validates frames, and reassembles payload data.
5. Frontend surfaces status and decoded payloads to users and automation.

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

## Frame format

OpenPulse frames follow this logical layout:

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

## Extensibility architecture

- New modulation families are introduced as plugins implementing shared traits.
- Plugin APIs must remain stable enough for out-of-tree experimentation.
- Core crates should remain embeddable for future automation and integrations.

## Documentation process constraints

- Documentation updates flow through pull requests only.
- Frontmatter validation and stamping automation are required quality gates.
