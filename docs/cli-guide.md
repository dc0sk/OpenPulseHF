---
project: openpulsehf
doc: docs/cli-guide.md
status: living
last_updated: 2026-04-24
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

Planned HPX and trust commands:

Detailed UX behavior for identity and trust diagnostics:

- docs/cli-ux-identity-trust-diagnostics.md

- openpulse hpx send <file> --mode <HPX500|HPX2300>
- openpulse hpx receive --out <dir>
- openpulse trust init
- openpulse trust import-key <path>
- openpulse trust list
- openpulse trust revoke <key-id>
- openpulse trust policy set --unknown-signer <reject|warn-allow>
- openpulse peers list
- openpulse peers query --mode <MODE> --min-quality <score>
- openpulse relay route --to <peer-id> --max-hops <n>
- openpulse hpx send <file> --relay auto --max-hops <n>
- openpulse relay inspect-route --route-id <id>
- openpulse peers query --trust <trusted|trusted-or-unknown|any> --max-results <n>

## Common options

- --backend <BACKEND>: select loopback or hardware backend where supported.
- --mode <MODE>: select a registered modulation mode.
- --help: show full command and flag reference.

Planned trust-related options:

- --signing-key <key-id>: select local signing identity for handshake and manifest signing.
- --trust-store <path>: select trust-store location.
- --require-signatures: fail transfer if required signatures are missing.
- --allow-unknown-signer: override default reject policy for unknown signers.
- --max-hops <n>: set relay hop limit.
- --relay <off|auto|required>: control relay usage policy.

## Operational notes

- Prefer loopback for deterministic testing and debugging.
- Use no-default-features CI-like runs to avoid hardware dependencies in automation.
- Keep command examples aligned with README and release notes.
- For signed transfers, keep trust-store backups and rotate keys on schedule.
- Treat unknown-signer allowance as temporary troubleshooting, not steady-state policy.

## Testing commands

```sh
# Run all tests (loopback backend - no audio hardware required)
cargo test --workspace --no-default-features

# Run with full audio support (requires ALSA headers on Linux)
cargo test --workspace

# Validate benchmark scaffold files and result artifacts
bash scripts/validate-benchmark-artifacts.sh

# Compare aggregate benchmark results against stored baselines
bash scripts/check-benchmark-regressions.sh benchmark/baselines benchmark/results/aggregate
```
