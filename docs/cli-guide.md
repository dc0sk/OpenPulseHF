---
project: openpulsehf
doc: docs/cli-guide.md
status: living
last_updated: 2026-05-02
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

## Session diagnostics and persistence

- openpulse session start --peer <CALLSIGN>
- openpulse session state
- openpulse session state --diagnostics
- openpulse session list
- openpulse session resume
- openpulse session log
- openpulse session log --follow --follow-timeout-ms <ms>
- openpulse session end

Notes:
- `session state --diagnostics` emits structured JSON including transition history, per-event `event_source`, `session_id`, `reason_string`, and pipeline scheduler metrics.
- `session state --diagnostics --format text` emits a concise session summary followed by readable event lines; `--format json` preserves the raw structured diagnostics payload.
- `session resume` restores persisted metadata and policy profile snapshot; runtime handshake state must be re-established.
- `session log --follow` tails the persisted session log for a bounded polling window and is intended for cross-invocation debugging.
- `session start`, `session resume`, and `session end` update the persisted session log so follow mode can observe lifecycle changes across CLI invocations.

## Benchmark

- openpulse benchmark run
- openpulse benchmark run --min-pass-rate <0.0-1.0> --max-mean-transitions <f64>

## Identity commands

- openpulse identity show <STATION_OR_RECORD_ID>
- openpulse identity verify <STATION_OR_RECORD_ID>
- openpulse identity cache

Notes:
- `identity show` resolves by record_id, station_id, or callsign (tries each in order via PKI service).
- `identity verify` confirms no active PKI revocations exist for the identity.
- `identity cache` fetches and summarises the current trust bundle from the PKI service.

## Trust commands

- openpulse trust show <STATION_OR_RECORD_ID>
- openpulse trust explain <STATION_OR_RECORD_ID>
- openpulse trust import --station-id <ID> --key-id <ID> --trust <LEVEL> --source <SOURCE>
- openpulse trust list
- openpulse trust revoke <STATION_OR_KEY>
- openpulse trust policy show
- openpulse trust policy set <strict|balanced|permissive>

Trust levels: `full`, `marginal`, `unknown`, `untrusted`, `revoked`.
Certificate sources: `out_of_band`, `over_air`.

Notes:
- `trust show` and `trust explain` both query PKI; `explain` includes policy recommendation detail in output.
- `trust import` writes to the local trust store (`~/.config/openpulse/trust-store.json`).
- `trust policy set` persists the active policy profile across invocations.

## Diagnose commands

- openpulse diagnose handshake <STATION_OR_RECORD_ID>
- openpulse diagnose manifest
- openpulse diagnose session

## Common options

- --backend <BACKEND>: select loopback or hardware backend where supported.
- --mode <MODE>: select a registered modulation mode.
- --pki-url <URL>: PKI service base URL (default: http://localhost:8080).
- --log <LEVEL>: log level (error, warn, info, debug, trace).
- --ptt <none|rts|dtr|vox|rigctld>: PTT control method (default: none).
  - none: no PTT (loopback, testing)
  - rts / dtr: assert serial RTS or DTR line; --rig specifies the serial port (e.g. /dev/ttyUSB0). Requires the `serial` feature.
  - vox: software-state only (no external line driven; useful for VOX-enabled rigs)
  - rigctld: TCP connection to hamlib rigctld; --rig specifies address:port (default: localhost:4532)
- --rig <path|address:port>: serial port path for rts/dtr PTT, or rigctld address:port.
- --help: show full command and flag reference.

Output format options (available on most commands):
- --format <json|text>: output format (default: text).
- --verbose: include extended detail in output.
- --diagnostics: emit structured JSON diagnostics payload.
- --no-color: suppress terminal colour codes.

Planned options (not yet implemented):
- --signing-key <key-id>: select local signing identity.
- --trust-store <path>: override default trust-store location.
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
