---
project: openpulsehf
doc: docs/cli-ux-identity-trust-diagnostics.md
status: living
last_updated: 2026-10-15
---

# CLI UX Specification for Identity and Trust Diagnostics

## Purpose

This document defines user-facing CLI behavior for identity and trust diagnostics in OpenPulseHF,
and records where the current `openpulse` CLI (`crates/openpulse-cli`) meets or departs from it.

Release 1 status: the signed classical handshake is part of Release 1. The PKI service these
commands query (`pki-tooling`, default `--pki-url http://127.0.0.1:8787`) is not a Release 1
crate; the `identity`, `trust show`/`explain` and `diagnose` commands need it running.

Goals:

- make trust posture visible before and during session setup
- provide deterministic diagnostic output for automation and operators
- align human-readable and machine-readable output modes

## Scope

In scope:

- command taxonomy for identity and trust diagnostics
- argument and output conventions
- exit-code model for scripting and CI usage
- operator-focused troubleshooting workflows

Out of scope:

- cryptographic implementation details
- PKI service deployment and replication operations
- GUI/TUI behavior

## UX principles

- fail closed for trust-risk operations
- explain decisions with explicit reason codes
- keep default output concise and actionable
- provide JSON output parity for every diagnostic command

## Command taxonomy

### Group: `identity`

- `openpulse identity show <station-or-record-id>`
  - returns current identity summary
- `openpulse identity verify <station-or-record-id>`
  - validates signature chain and key continuity
- `openpulse identity cache`
  - shows local cache state and freshness

### Group: `trust`

- `openpulse trust show <station-or-record-id>`
  - prints trust recommendation and evidence summary
- `openpulse trust explain <station-or-record-id>`
  - prints reasoned decision trace for current trust state
- `openpulse trust policy show`
  - prints active policy profile and version
- `openpulse trust policy set <strict|balanced|permissive>`
  - updates local policy profile (persisted in `trust-policy.json`)
- `openpulse trust import --station-id <id> --key-id <id> [--trust <level>] [--source <source>]`
  - adds or updates a local trust-store record; `--trust` is one of `full`, `marginal`,
    `unknown` (default), `untrusted`, `revoked`; `--source` is `out_of_band` (default) or
    `over_air`
- `openpulse trust list`
  - lists local trust-store records
- `openpulse trust revoke --station-or-key <id> [--reason <text>]`
  - marks a local record revoked (default reason `operator_revoked`)

The local trust store and policy live in the CLI config directory: `$OPENPULSE_CONFIG_DIR`, else
`$XDG_CONFIG_HOME/openpulse`, else `~/.config/openpulse` (`trust-store.json`,
`trust-policy.json`).

### Group: `diagnose`

- `openpulse diagnose handshake --peer <id>`
  - performs dry-run validation of signed handshake prerequisites
- `openpulse diagnose manifest --session <id>`
  - verifies signed manifest structure and signature metadata
- `openpulse diagnose session --peer <id>`
  - composite command combining identity, trust, and handshake checks

## Shared options

All identity/trust diagnostic commands must support:

- `--format text|json` (default: text)
- `--verbose` (adds evidence and validation detail)
- `--no-color` (disable terminal color)
- `--timeout <seconds>` (default 5)

The CLI also accepts `--diagnostics` on these commands (shared option set).

Current implementation: all five options are accepted. The CLI emits no colour, so `--no-color`
changes nothing. `--timeout` is echoed in the output (`timeout_seconds` with `--verbose`) but does
not yet bound the PKI request.

## Output model

### Text mode

Text output format:

1. one-line status summary
2. key-value detail lines
3. optional recommendation block

As implemented, text mode prints `STATUS: <decision>`, then `status:`, `reason_code:`, one line
per `details` key (values JSON-encoded), and `recommendation:`.

Example:

```text
STATUS: reduced
status: warn
reason_code: over_air_certificate_without_psk
peer: "W1ABC"
trust_state: "unknown"
certificate_source: "over_air"
recommendation: Request out-of-band certificate verification before data transfer.
```

### JSON mode

JSON output shape:

- `status` (ok, warn, fail)
- `decision` (for trust decisions: verified, pskverified, reduced, unverified, low, rejected;
  other commands use their own values, e.g. `missing`, `imported`, `unavailable`)
- `reason_code`
- `details` (object)
- `recommendation`

Example:

```json
{
  "status": "warn",
  "decision": "reduced",
  "reason_code": "over_air_certificate_without_psk",
  "details": {
    "peer": "W1ABC",
    "trust_state": "unknown",
    "certificate_source": "over_air"
  },
  "recommendation": "Request out-of-band certificate verification before data transfer."
}
```

## Exit code model

- `0`: checks passed (or non-blocking advisory only)
- `1`: usage/configuration error
- `2`: validation failed (signature/schema/trust policy)
- `3`: transport/backend failure (PKI endpoint unavailable, timeout)

Rules:

- `trust explain` returns `0` for `warn` and `2` for `fail`.
- `diagnose session` returns highest-severity code among sub-checks.

As implemented: `status: fail` exits `2`, `ok` and `warn` exit `0`, an unreachable PKI service
exits `3` (`reason_code: pki_service_unreachable`), and a usage or argument error exits non-zero
through clap or the CLI's error path. `diagnose session` is currently the same trust decision as
`diagnose handshake` plus a PKI audit record, not a roll-up of separate sub-checks.

## Decision diagnostics requirements

Every trust decision command must emit:

- policy profile version
- evidence classes used (operator, gpg, tqsl, replication)
- effective revocation state
- certificate acquisition path (out-of-band, over-air, psk-validated)
- final connection trust level

If decision is downgraded, output must include a single primary reason code plus optional secondary notes.

As implemented, trust decisions carry `trust_state`, `certificate_source`, `policy_profile` and
`policy_profile_version` in `details`, plus `evidence_classes` and `effective_revocation_state`
from the PKI record. `trust explain` (and `diagnose` with `--verbose`) adds `secondary_notes`.

## Command examples

```sh
openpulse identity verify W1ABC
openpulse trust show W1ABC --format json
openpulse trust explain W1ABC --verbose
openpulse diagnose handshake --peer W1ABC
openpulse diagnose manifest --session 0c68d2ea
openpulse diagnose session --peer W1ABC --format json
```

## Error and reason codes

Minimum normalized codes:

- `identity_not_found`
- `signature_verification_failed`
- `revocation_conflict`
- `policy_rejected`
- `over_air_certificate_without_psk`
- `pki_service_unreachable`
- `invalid_manifest_schema`

Text mode must show one primary code. JSON mode must include `reason_code` and optional `sub_reasons[]`.

Implemented codes include all of the above except `signature_verification_failed` and
`invalid_manifest_schema`; `diagnose manifest` reports `no_manifest_for_session`,
`manifest_read_error`, `sender_not_in_trust_store`, `key_id_not_parseable_as_pubkey`,
`signature_valid` and `signature_invalid` instead. `sub_reasons[]` is not emitted; secondary
detail goes in `details.secondary_notes`.

## Conformance expectations

CLI implementation is conformant when:

- every diagnostic command supports `--format text|json`
- JSON output includes `status`, `decision`, and `reason_code`
- exit codes follow the model above
- trust downgrade decisions include explicit reason code
- command help includes at least one example for each command

## Rollout guidance

Recommended implementation order:

1. `identity show` and `trust show`
2. `trust explain` with reason-code mapping
3. `diagnose handshake` and `diagnose manifest`
4. `diagnose session` composite command

## Open questions

- whether `diagnose session` should include optional active network probing by default
- whether `trust policy set` should require an interactive confirmation prompt in non-tty contexts