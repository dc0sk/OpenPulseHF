---
project: openpulsehf
doc: docs/cli-ux-identity-trust-diagnostics.md
status: living
last_updated: 2026-04-26
---

# CLI UX Specification for Identity and Trust Diagnostics

## Purpose

This document defines user-facing CLI behavior for identity and trust diagnostics in OpenPulseHF.

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
- `openpulse trust policy show`
  - prints active policy profile and version
- `openpulse trust policy set <strict|balanced|permissive>`
  - updates local policy profile
- `openpulse trust explain <station-or-record-id>`
  - prints reasoned decision trace for current trust state

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
- `--timeout <seconds>`

## Output model

### Text mode

Text output format:

1. one-line status summary
2. key-value detail lines
3. optional recommendation block

Example:

```text
STATUS: reduced
peer: W1ABC
trust_state: unknown
connection_trust_level: reduced
reason_code: over_air_certificate_without_psk
recommendation: request out-of-band certificate verification before data transfer
```

### JSON mode

JSON output shape:

- `status` (ok, warn, fail)
- `decision` (verified, psk-verified, reduced, unverified, low, rejected)
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

## Decision diagnostics requirements

Every trust decision command must emit:

- policy profile version
- evidence classes used (operator, gpg, tqsl, replication)
- effective revocation state
- certificate acquisition path (out-of-band, over-air, psk-validated)
- final connection trust level

If decision is downgraded, output must include a single primary reason code plus optional secondary notes.

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