---
project: openpulsehf
doc: docs/non-gpl-interfacing.md
status: living
last_updated: 2026-10-15
---

# Non-GPL Interfacing Methods

OpenPulseHF is licensed under the **GNU General Public License v3.0 or later**.  The GPL
requires that software which links against GPL code be distributed under a compatible
licence.  However, several well-established legal boundaries allow non-GPL software to
interoperate with OpenPulseHF without being subject to the GPL:

1. **Process boundary** — communicating with an OpenPulseHF binary over standard I/O,
   sockets, or pipes does not make the calling program a derivative work.
2. **Network protocol** — connecting to a service over TCP/IP is not linking.  The FSF
   and courts have consistently held that communicating over a network does not trigger
   GPL copyleft.

The interfaces below all sit on the safe side of one or both of those boundaries.

Release 1 (`v0.17.0`) covers the ARDOP TNC, the daemon control ports and the CLI. The KISS TNC,
the Winlink CMS gateway and `pki-tooling` exist in the code but are experimental and not part of
Release 1.

---

## Interface summary

| Interface | Protocol | Default address | Notes |
|---|---|---|---|
| [ARDOP command port](#ardop-tnc-tcp-interface) | ASCII line (ARDOP) | `127.0.0.1:8515` | Pat, Winlink Express, any ARQ app |
| [ARDOP data port](#ardop-tnc-tcp-interface) | `u16 BE` length-framed binary | `127.0.0.1:8516` | Payload bytes in both directions |
| [KISS/AX.25 TCP TNC](#kissax25-tcp-tnc) | KISS framing over TCP | port `8100` | APRS clients, Direwolf-compatible apps (experimental) |
| [Daemon TCP control port](#daemon-control-port-ndjson--tcp) | NDJSON lines | `127.0.0.1:9000` | Events + commands; operator panel |
| [Daemon WebSocket endpoint](#daemon-websocket-endpoint) | JSON over WebSocket | `127.0.0.1:9001` | Browser / Electron clients |
| [PKI tooling REST API](#pki-tooling-rest-api) | HTTP/JSON | `127.0.0.1:8080` (default; configurable via `PKI_BIND_ADDR`) | Trust-bundle and key management (experimental) |
| [CLI subprocess](#cli-subprocess) | stdin / stdout | — | Pipe-based scripting |
| [Winlink CMS gateway](#winlink-cms-gateway) | B2F over TCP | `cms.winlink.org:8772` | Outbound gateway; no local server (experimental) |

---

## ARDOP TNC TCP interface

`openpulse-tnc` (`crates/openpulse-ardop`) exposes two TCP ports:

- **Command port** (default 8515) — ASCII line protocol.  Compatible with
  [Pat](https://getpat.io/) and other ARDOP-aware applications.
  Commands (`crates/openpulse-ardop/src/command.rs`): `VERSION`, `MYID`, `LISTEN`, `CONNECT`,
  `DISCONNECT`, `ABORT`, `STATE`, `BUFFER`, `PTT`, `GRIDSQUARE`, `ARQBW`, `ARQTIMEOUT`, `CWID`,
  `SENDID`, `FECSEND`, `FECRCV`, `CONNECT_MESH`, `WAVEFORM`, `PING`, `CLOSE`.
  `GRIDSQUARE` is informational (stored and echoed). `ARQBW` (200/500/1000/2000) caps the
  adaptive ladder and `ARQTIMEOUT` (30–600 s) drops an idle connection, but only when adaptive ARQ
  is on (`[ardop] enable_adaptive_arq = true`, default `false`); in fixed-mode operation they are
  accepted and echoed. The adaptive ladder (`[ardop] adaptive_profile`, default `fast`) is floored
  at SL2, so the TNC never uses MFSK16. `CWID TRUE|FALSE` turns the Morse CW ID on or off;
  `SENDID` sends a one-shot ID at the next frame boundary (plus a CW ID when `CWID` is on).
  `CONNECT_MESH` belongs to the experimental mesh feature.
- **Data port** (default 8516) — binary `u16 BE` length-prefixed frames in both
  directions.  Send a 2-byte big-endian length followed by that many payload bytes;
  receive frames in the same format.

Any application that speaks the ARDOP wire protocol can connect without needing any
OpenPulseHF source code or headers.  The command port is unauthenticated and
bind-address–restricted to `127.0.0.1` by default; change `[ardop] bind_addr` in the
config file (or pass `openpulse-tnc --bind`) to expose it on a LAN interface.

---

## KISS/AX.25 TCP TNC

`openpulse-kisstnc` (`crates/openpulse-kiss`) presents a standard KISS TNC over TCP
(default port 8100).  Frames are FEND-delimited with standard KISS byte stuffing
(FEND=`0xC0`, FESC=`0xDB`, TFEND=`0xDC`, TFESC=`0xDD`).  AX.25 UI frames with
Control=`0x03` and PID=`0xF0` are fully supported.

Compatible with virtually all APRS software (Xastir, YAAC, APRX), Linux AX.25 tools,
and any application that can connect to a KISS TNC over TCP.

---

## Daemon control port (NDJSON + TCP)

`openpulse-daemon` (`crates/openpulse-daemon`) exposes a NDJSON-over-TCP control port
(default `127.0.0.1:9000`).  Each line is a UTF-8 JSON object terminated by `\n`.

**Receiving events** — connect and read lines; each is a serialised `ControlEvent`, tagged by
a snake_case `type` field. Modem engine events arrive wrapped in `engine_event`:

```json
{"type":"engine_event","event":{"type":"frame_received","mode":"BPSK250","bytes":64}}
{"type":"ptt_changed","active":true}
```

**Sending commands** — write a JSON-serialised `ControlCommand` followed by `\n`, tagged by a
snake_case `cmd` field. The server replies with a `CommandResponse`:

```json
{"cmd":"set_mode","mode":"BPSK250"}
{"cmd":"send_message","to":"W1AW","subject":"test","body":"hello"}
{"cmd":"start_ota_session","profile":"fast"}
```

The full `ControlCommand` and `ControlEvent` schemas are defined in
`crates/openpulse-daemon/src/protocol.rs`.  Both types carry `#[derive(Serialize, Deserialize)]`
so the JSON schema is stable and machine-readable.

**Spectrum subscription** — send `{"cmd":"subscribe_spectrum","fps":10}` and the daemon
begins interleaving binary power-spectrum frames (4-byte magic `OPSP` + `u16 LE` bin count +
`u32 LE` sample rate + `f32 LE` bins) alongside the NDJSON event stream on the same connection.
A reader tells them apart by the first byte: `{` is JSON, `O` is a spectrum frame.

**Authentication** — on a loopback bind the control port is open by default. On a non-loopback
bind, PSK authentication and encryption (`openpulse-linksec`, Noise) are mandatory; set
`[control_security] require_auth = true` to force them on loopback too. The PSK comes from the
`OPENPULSE_CONTROL_PSK` environment variable.

---

## Daemon WebSocket endpoint

The daemon also exposes an identical interface over WebSocket (default
`127.0.0.1:9001`).  Text frames carry NDJSON events and command responses; binary
frames carry spectrum data.  This is the interface used by the `openpulse-panel` operator
panel (iced; experimental) and is suitable for browser or Electron clients.

---

## PKI tooling REST API

`pki-tooling` exposes an HTTP/JSON REST API. By default it binds to `127.0.0.1:8080`,
and operators can override that with `PKI_BIND_ADDR`. Read-only endpoints require no
authentication. Mutating endpoints require an `Authorization: Bearer <token>` header
whose value matches the non-empty `PKI_API_KEY` environment variable set at server
startup. The service also requires `PKI_SIGNING_KEY` as a base64-encoded 32-byte seed
for persistent trust-bundle signing; `PKI_ALLOW_EPHEMERAL_KEY=true` is an explicit
development-only override and should not be used for persistent deployments.

Exception: `POST /api/v1/submissions` is intentionally public intake for moderation
and signature verification workflows; it does not require the bearer token.

Key endpoints:

| Method | Path | Auth | Description |
|---|---|---|---|
| `GET` | `/api/v1/trust-bundles/current` | None | Fetch the current trust bundle |
| `GET` | `/api/v1/trust-bundles/{bundle_id}` | None | Fetch a bundle by ID |
| `GET` | `/api/v1/identities/{record_id}`, `/api/v1/identities:lookup` | None | Identity lookup |
| `GET` | `/api/v1/revocations` | None | List revocations |
| `GET` | `/api/v1/signing-key` | None | Retrieve the service Ed25519 public key |
| `POST` | `/api/v1/submissions` | None | Public submission intake (validation/moderation pipeline) |
| `POST` | `/api/v1/trust-bundles` | Bearer | Publish a new trust bundle |
| `PATCH` | `/api/v1/trust-bundles/{bundle_id}/promote` | Bearer | Promote a bundle to active |
| `POST` | `/api/v1/revocations` | Bearer | Record a key revocation |
| `GET` | `/api/v1/moderation/queue` | Bearer | Read the moderation queue |
| `POST` | `/api/v1/moderation/{submission_id}/decision` | Bearer | Post a moderation outcome |
| `POST` | `/api/v1/session-audit-events` | Bearer | Submit an audit event |

The route table is `build_router` in `pki-tooling/src/lib.rs`. Any HTTP client library in any language can consume this API.  Response bodies are
`application/json`.

---

## CLI subprocess

`openpulse-cli` (`crates/openpulse-cli`) can be invoked as a subprocess.  Structured
output (e.g. `benchmark run`, `monitor`) is written to stdout as NDJSON; exit codes
follow Unix conventions (0 = success, non-zero = error).

Subprocess invocation crosses the process boundary and does not trigger GPL copyleft in
the calling program.

Example:

```bash
# Stream engine events as NDJSON
openpulse-cli --backend loopback monitor --mode BPSK250 | jq .

# Run the benchmark and capture JSON results
openpulse-cli --backend loopback --log error benchmark run > bench.json
```

---

## Winlink CMS gateway

`openpulse-gateway` (`crates/openpulse-gateway`) connects outbound to
`cms.winlink.org:8772` (or any B2F gateway) over the Winlink B2F protocol.  It is a
client, not a server.  The CMS itself is not open-source; connecting to it is
unaffected by the GPL.

Note: the B2F protocol on port 8772 is **unauthenticated plaintext by specification**.
A startup warning is emitted (`tracing::warn!`) to make this visible in logs.

---

## What still requires GPL compliance

The process-boundary and network-protocol safe harbours do **not** apply to:

- **Plugins compiled against `openpulse-core`** — a Rust crate that statically links
  against `openpulse-core` (via `ModulationPlugin`, `FecCodec`, etc.) is a derivative
  work and must be GPL-compatible.
- **Fork-and-modify** — distributing a modified version of any OpenPulseHF crate
  requires the modified source to be made available under the GPL.

If you need a proprietary modulation plugin or signal-processing backend, the only
supported path today is to run it as a separate process and bridge data through one of
the TCP interfaces listed above.
