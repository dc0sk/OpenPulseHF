---
project: openpulse
doc: docs/cli-guide.md
status: living
last_updated: 2026-04-23
---

# CLI Guide - openpulse (v0.1.0)

## Build prerequisites

- Linux CPAL builds require ALSA development headers:

```sh
sudo apt-get install libasound2-dev
```

## Build

```sh
cargo build --release
```

## Quick start

1. List available modes: openpulse modes
2. List audio devices: openpulse devices
3. Transmit using loopback: openpulse --backend loopback transmit "CQ CQ" --mode BPSK100
4. Receive from default backend: openpulse receive --mode BPSK31

## Core commands

- openpulse transmit <text> --mode <MODE>
- openpulse receive --mode <MODE>
- openpulse devices
- openpulse modes

## Common options

- --backend <BACKEND>: select loopback or hardware backend where supported.
- --mode <MODE>: select a registered modulation mode.
- --help: show full command and flag reference.

## Operational notes

- Prefer loopback for deterministic testing and debugging.
- Use no-default-features CI-like runs to avoid hardware dependencies in automation.
- Keep command examples aligned with README and release notes.

## Testing commands

```sh
# Run all tests (loopback backend - no audio hardware required)
cargo test --workspace --no-default-features

# Run with full audio support (requires ALSA headers on Linux)
cargo test --workspace
```
