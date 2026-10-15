---
project: openpulsehf
doc: docs/cli-guide.md
status: living
last_updated: 2026-10-15
---

# CLI Guide - openpulse

Covers the `openpulse` binary on current `main` (workspace version 0.16.0; Release 1 will be
`v0.17.0`). The flag reference below is taken from the clap definitions in
`crates/openpulse-cli/src/cli.rs`; `openpulse <command> --help` is always authoritative.

## Build prerequisites

- Linux CPAL builds require ALSA development headers:

```sh
sudo apt-get install libasound2-dev
```

## Build

```sh
cargo build --release -p openpulse-cli                        # hardware audio (cpal-backend, the default)
cargo build --release -p openpulse-cli --no-default-features  # loopback only, no ALSA needed
```

Optional features: `serial` (RTS/DTR PTT), `gpio` (Linux GPIO PTT), `generic-serial` (scripted
serial CAT, Unix only).

## Quick start

1. List available modes: `openpulse modes`
2. List audio devices: `openpulse devices`
3. Transmit using loopback: `openpulse --backend loopback transmit "CQ CQ" --mode BPSK100`
4. Receive from the default backend: `openpulse receive --mode BPSK31 --listen-ms 60000`

The loopback backend is an in-memory buffer inside one process. A loopback `transmit` and a
separate loopback `receive` do not hear each other; for a hardware-free round trip use
`openpulse adaptive`, `openpulse --backend loopback benchmark run`, or the workspace tests.

## Core commands

- `openpulse transmit <TEXT> [--mode <MODE>] [--fec <FEC>] [--device <NAME>] [--center-frequency <HZ>]`
  - Default mode `BPSK100`, default `--fec none`, default centre 1500 Hz.
  - `--fec`: `none`, `rs`, `rs-interleaved`, `concatenated`, `rs-strong`, `soft-concatenated`,
    `ldpc`, `turbo`. The receiver must pass the same value.
  - Refuses, before keying, a frame whose airtime (plus the PTT leader) exceeds the 180 s PTT
    watchdog, and says so; nothing is transmitted.
  - With `[qsy] bandplan_awareness_enabled = true` (the default) it checks the mode against the
    bandplan at the rig's current frequency, read from rigctld at `[radio] rigctld_addr`; if rigctld
    is unreachable the check is skipped with a warning.
- `openpulse receive [--mode <MODE>] [--fec <FEC>] [--listen-ms <MS>] [--device <NAME>] [--center-frequency <HZ>] [--no-afc]`
  - `--listen-ms` gives up after that many milliseconds. Timeout reception supports `--fec` `none`,
    `rs`, `rs-interleaved`, `soft-concatenated`, `ldpc`.
  - `--no-afc` disables AFC settling, for a shared audio path with no frequency offset.
- `openpulse devices`
- `openpulse modes [--airtime]` (`--airtime` prints `MODE<TAB>SECONDS`, the airtime of the largest
  frame per mode)
- `openpulse mode-advisor --snr <dB> [--profile <name>]` (see [cli-mode-advisor-guide.md](cli-mode-advisor-guide.md))
- `openpulse adaptive [--profile <name>] [--channel <model>] [--snr <dB>] [--frames <n>] [--payload-len <bytes>] [--min-backlog <bytes>] [--seed <n>] [--json]`
- `openpulse arq send --payload <TEXT> [--mode <MODE>] [--profile <name>] [--retries <n>] [--device <NAME>]`
- `openpulse arq listen [--mode <MODE>] [--profile <name>] [--frames <n>] [--session <ID>] [--device <NAME>]`
- `openpulse session-metrics [--format json|text]`
- `openpulse monitor [--mode <MODE>]`: stream the engine's NDJSON event feed to stdout (default mode `BPSK100`).
- `openpulse calibrate <audio|ptt|afc|drive> [--output <PATH>]`: prints a JSON result (and writes it
  to `--output`). `audio` and `afc` are self-checks on the in-memory loopback, not measurements of
  your sound card; `ptt` keys the `--ptt`/`--rig` backend and measures assert/release latency against
  a 50 ms target. `drive` keys the
  transmitter to find the TX attenuation that puts the rig's ALC in a target band (`--mode`, default
  `OFDM52`; `--target-alc-lo` 0.3, `--target-alc-hi` 0.5); it needs a real radio, rigctld and the
  `cpal-backend` feature. Use a dummy load.
- `openpulse config init`: print a commented `config.toml` template to stdout.
- `openpulse audit-bundle`: see below.

Not part of Release 1 (experimental, no release gate):

- `openpulse broadcast --payload <TEXT> [--mode <MODE>] [--ttl <n>] [--callsign <CALL>]`: one-to-many
  broadcast frame for the relay/mesh path (no ACK, no session; default mode `BPSK250`, TTL 3).
- `openpulse beacon --callsign <CALL> [--interval <secs>] [--mode <MODE>] [--ttl <n>]`: periodic
  beacon frames (default interval 600 s, mode `BPSK250`, TTL 1). Regulatory station ID on a running
  station is done by the daemon's and the ARDOP TNC's own auto-ID (`[station] auto_id_interval_secs`).
- `openpulse qsy init [--rig <addr:port>]` / `openpulse qsy status`: QSY frequency agility.

Notes:
- `mode-advisor`, `adaptive` and `arq` select the ladder with `--profile` (overrides `[modem] profile`).
  There are exactly two profiles: `fast` (default; the full `hpx_hf` ladder SL1–SL14) and `robust`
  (the same ladder capped at SL6, at most 500 Hz, single-carrier). Any other name, including the
  pre-2026-10-01 profile names, is rejected with the list of valid names. Modes those old profiles
  used remain available as fixed modes (`--mode`, `[modem] mode`).
- `adaptive` runs a real rate-control session over a simulated channel (`clean`, `awgn` with
  `--snr`, `watterson-good-f1`, `watterson-poor-f1`) and reports each speed-level transition; no
  audio hardware required. Defaults: 8 frames of 64 bytes. `--json` gives newline-delimited JSON.
- `arq send` (ISS) / `arq listen` (IRS) run a two-way exchange over the modem: data forward, FSK4
  ACK return, retransmit on NACK. Run `listen` on one station and `send` on the other. Keying is per
  transmission, so use a VOX or wired/full-duplex audio path. Defaults: mode `BPSK250`, 3 retries,
  1 frame, session id `openpulse-arq`.

## Session diagnostics and persistence

- `openpulse session start --peer <CALLSIGN>`
- `openpulse session state [--diagnostics]`
- `openpulse session list`
- `openpulse session resume`
- `openpulse session log [--follow] [--follow-timeout-ms <ms>] [--poll-interval-ms <ms>]` (defaults 5000 / 250)
- `openpulse session end`

Notes:
- `session state --diagnostics` emits structured JSON including transition history, per-event `event_source`, `session_id`, `reason_string`, and pipeline scheduler metrics.
- `session state --diagnostics --format text` emits a concise session summary followed by readable event lines; `--format json` preserves the raw structured diagnostics payload.
- `session resume` restores persisted metadata and policy profile snapshot; runtime handshake state must be re-established.
- `session log --follow` tails the persisted session log for a bounded polling window and is intended for cross-invocation debugging.
- `session start`, `session resume`, and `session end` update the persisted session log so follow mode can observe lifecycle changes across CLI invocations.

## Audit bundle (REQ-OBS-03)

- `openpulse audit-bundle`
- `openpulse audit-bundle --archive-dir <DIR> --output <DIR> --label <NAME>`

Notes:
- Packages the audit-mode artifacts into a single `.tar.gz` (with a `metadata.json` manifest) for handoff to a developer, and prints the bundle path.
- Includes everything under the audit `archive_dir` (`events.ndjson`, `snapshot.json`, …) plus the daily-rolled log files that share the configured `[logging] file` prefix (under `logs/`).
- Defaults come from config: `[observability] archive_dir` (default `~/.local/share/openpulse/audit`) and `[logging] file`; `--archive-dir` / `--output` override. Default output is `<archive_dir>/bundles/`.
- Requires the daemon to have run with `[observability] audit_mode = true` (default `false`) first, else the archive dir won't exist.

## Benchmark

- `openpulse benchmark run`
- `openpulse benchmark run --min-pass-rate <0.0-1.0> --max-mean-transitions <f64>` (defaults 1.0 and 20.0)

## Identity commands

These query the PKI service at `--pki-url` (default `http://127.0.0.1:8080`); `pki-tooling` is not
a Release 1 crate. See [cli-ux-identity-trust-diagnostics.md](cli-ux-identity-trust-diagnostics.md).

- `openpulse identity show <STATION_OR_RECORD_ID>`
- `openpulse identity verify <STATION_OR_RECORD_ID>`
- `openpulse identity cache`

Notes:
- `identity show` resolves by record_id, station_id, or callsign (tries each in order via PKI service).
- `identity verify` confirms no active PKI revocations exist for the identity.
- `identity cache` fetches and summarises the current trust bundle from the PKI service.

## Trust commands

- `openpulse trust show <STATION_OR_RECORD_ID>`
- `openpulse trust explain <STATION_OR_RECORD_ID>`
- `openpulse trust import --station-id <ID> --key-id <ID> [--trust <LEVEL>] [--source <SOURCE>]`
- `openpulse trust list`
- `openpulse trust revoke --station-or-key <ID> [--reason <TEXT>]`
- `openpulse trust policy show`
- `openpulse trust policy set <strict|balanced|permissive>`

Trust levels: `full`, `marginal`, `unknown` (default), `untrusted`, `revoked`.
Certificate sources: `out_of_band` (default), `over_air`.

Notes:
- `trust show` and `trust explain` both query PKI; `explain` adds secondary notes to the output.
- `trust import` writes to the local trust store, `trust-store.json` in the CLI config directory
  (`$OPENPULSE_CONFIG_DIR`, else `$XDG_CONFIG_HOME/openpulse`, else `~/.config/openpulse`).
- `trust policy set` persists the active policy profile across invocations (`trust-policy.json`).

## Daemon control CLI

`openpulse daemon [--addr <host:port>] <SUBCOMMAND>` talks to a running `openpulse-server`
(crate `openpulse-daemon`) over its NDJSON control port. `--addr` (default `127.0.0.1:9000`) belongs
to `daemon` and goes before the subcommand.

Connection, PTT, mode, and frequency:

- `openpulse daemon connect-peer <CALLSIGN>` / `disconnect-peer`
- `openpulse daemon set-mode <MODE>`
- `openpulse daemon set-freq [--rig <r>] <FREQ_HZ>` (CAT via rigctld)
- `openpulse daemon ptt-assert` / `ptt-release`
- `openpulse daemon ptt-state`: print `{"active": true|false}`; a resync for a client that missed a `PttChanged` event.

Messaging and OTA rate control:

- `openpulse daemon send-message --to <CALL> [--subject <s>] --body <text>`
- `openpulse daemon list-messages` | `get-message <ID>` | `delete-message <ID>`
- `openpulse daemon ota-start --profile <fast|robust>` | `ota-stop` | `ota-status`
- `openpulse daemon ota-bounds [--min <SLn>] [--max <SLn>]` | `ota-lock --level <SLn>` | `ota-unlock`
- `openpulse daemon ota-hysteresis [--min-backlog <bytes>] [--upgrade-hold-frames <n>]`
- `openpulse daemon ota-aggressiveness <conservative|balanced|aggressive>` (sets both gates)

Runtime settings: `set-dcd-squelch <RMS>` (a floor under the adaptive squelch; 0 = off),
`set-cessb <true|false>`, `set-notch <true|false>`, `set-agc <true|false>`,
`set-logbook <true|false>`, `set-tx-attenuation <dB> [--band <b>]`,
`set-config [--mode <M>] [--tx-attenuation-db <dB>] [--qsy-enabled <bool>] [--bandplan-mode <m>]`,
`get-config`, `subscribe-spectrum [--fps <n>] [--frames <n>]`.

Not part of Release 1 (experimental):

- JS8 station discovery: `enable-discovery`, `disable-discovery`, `stations`, `peers`.
- Direct P2P file transfer (`OPFX`, needs `[file_transfer] enabled = true`): `send-file <TO_CALLSIGN> <PATH>`,
  `accept-file <TRANSFER_ID>`, `reject-file <TRANSFER_ID>`, `cancel-file <TRANSFER_ID>`, `files`.
- Cross-band repeater: `enable-repeater` / `disable-repeater`.
- QSY: `accept-qsy <TOKEN>` / `reject-qsy <TOKEN>`.

Run `openpulse daemon --help` for the full subcommand and flag reference.

## Diagnose commands

- `openpulse diagnose handshake --peer <ID>`
- `openpulse diagnose manifest --session <ID>`
- `openpulse diagnose session --peer <ID>`

## Common options

Global (accepted before or after the subcommand):

- `--backend <default|cpal|loopback>`: audio backend (default `default`: cpal when compiled in, else loopback).
- `--log <LEVEL>`: `error`, `warn`, `info` (default), `debug`, `trace`.
- `--pki-url <URL>`: PKI service base URL (default `http://127.0.0.1:8080`).
- `--ptt <none|rts|dtr|vox|rigctld|cm108|gpio|generic>`: PTT control method (default `none`).
  - `none`: no PTT (loopback, testing).
  - `rts` / `dtr`: assert serial RTS or DTR; `--rig` is the serial port (e.g. `/dev/ttyUSB0`). Needs the `serial` feature.
  - `vox`: software state only (no line driven), for VOX-keyed rigs.
  - `rigctld`: Hamlib rigctld over TCP; `--rig` is `host:port` (empty = `localhost:4532`).
  - `cm108`: CM108/CM109/CM119 sound-chip GPIO over USB HID (DMK URI, RA-series, AIOC, homebrew);
    `--rig` is the `/dev/hidrawN` path (empty = first C-Media device). Keys GPIO 3. In the daemon,
    `[modem] ptt_device` / `ptt_gpio` set the path and pin.
  - `gpio`: Linux GPIO line (e.g. a Raspberry Pi header pin); `--rig` is `chip:line[:active_low]`
    (e.g. `gpiochip0:17`). Needs the `gpio` feature. In the daemon, `[modem] ptt_device` carries the spec.
  - `generic`: keys through the generic serial CAT backend; needs `--rig` (serial port), `--rig-file`
    and the `generic-serial` feature.
- `--rig <path|host:port>`: the PTT target for `--ptt` (see above).
- `--rig-file <PATH>`: TOML rig definition for the generic serial CAT backend.
- `--ptt-leader-ms <ms>`: wait between the PTT edge and the first sample, so the rig's key-up does
  not clip the preamble (default 0). The daemon and the ARDOP TNC read `[modem] ptt_leader_ms`.
  Not applied by `calibrate`. Measure the rig before setting it.
- `--max-power <W>`: declared maximum TX power in watts for the regulatory record (default 100).
- `--help`: full command and flag reference.

PTT behaviour (all front-ends, one `SharedPtt` keying path): a second key while keyed is refused;
release waits for the audio device's reported drain plus 10 ms (never longer than 200 ms); a
watchdog force-releases after 180 s keyed.

Output format options (on `session`, `session-metrics`, `identity`, `trust` and `diagnose` commands):
- `--format <json|text>`: output format (default: text).
- `--verbose`: include extended detail in output.
- `--diagnostics`: emit structured JSON diagnostics payload.
- `--no-color`: accepted; the CLI emits no colour.
- `--timeout <seconds>`: default 5.

Planned options (not implemented; the CLI rejects them):
- `--signing-key <key-id>`: select local signing identity.
- `--trust-store <path>`: override default trust-store location.
- `--require-signatures`: fail transfer if required signatures are missing.
- `--allow-unknown-signer`: override default reject policy for unknown signers.
- `--relay <off|auto|required>`: control relay usage policy.

## Operational notes

- Prefer loopback for deterministic testing and debugging.
- Use `--no-default-features` builds to keep automation free of audio hardware.
- Keep command examples aligned with README and release notes.
- For signed transfers, keep trust-store backups and rotate keys on schedule.

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
