---
project: openpulsehf
doc: docs/hpx-session-state-machine.md
status: living
last_updated: 2026-04-23
---

# HPX Session State Machine

## Purpose

This document defines the normative HPX session lifecycle used by discovery, adaptive training, transfer, recovery, and teardown flows.

## States

- idle: no active link context.
- discovery: peer capability discovery and initial link establishment.
- training: channel estimation and initial profile selection.
- active_transfer: payload transfer with adaptation and ARQ.
- recovery: bounded recovery after quality drop or sync loss.
- relay_route_discovery: optional route discovery when direct path is not viable.
- relay_active: transfer over one or more relay hops.
- teardown: orderly link close and final result emission.
- failed: terminal failure state for unrecoverable error or policy rejection.

## Events

- start_session
- discovery_ok
- discovery_timeout
- training_ok
- training_timeout
- signature_verification_failed
- transfer_complete
- transfer_error
- quality_drop
- relay_route_found
- relay_route_failed
- relay_policy_failed
- recovery_ok
- recovery_timeout
- local_cancel
- remote_teardown

## Transition table

| From | Event | To | Required actions |
|------|-------|----|------------------|
| idle | start_session | discovery | initialize session context, assign session_id |
| discovery | discovery_ok | training | verify peer identity record, collect capabilities |
| discovery | discovery_timeout | failed | emit timeout diagnostic |
| discovery | signature_verification_failed | failed | emit trust diagnostic and reason |
| training | training_ok | active_transfer | select initial HPX profile and ARQ params |
| training | relay_route_found | relay_active | bind selected route_id and hop metadata |
| training | training_timeout | failed | emit timeout diagnostic |
| training | signature_verification_failed | failed | emit trust diagnostic and reason |
| active_transfer | transfer_complete | teardown | finalize manifest verification and stats |
| active_transfer | relay_route_found | relay_active | switch to relay route with continuity marker |
| active_transfer | quality_drop | recovery | freeze profile changes and start recovery timer |
| active_transfer | transfer_error | recovery | increment retry counters |
| active_transfer | signature_verification_failed | failed | reject data-path admission |
| recovery | relay_route_found | relay_active | resume transfer via selected relay route |
| recovery | recovery_ok | active_transfer | resume transfer with adapted profile |
| recovery | recovery_timeout | failed | emit recovery exhausted diagnostic |
| relay_route_discovery | relay_route_found | relay_active | activate relay route and clear discovery timer |
| relay_route_discovery | relay_route_failed | failed | emit route discovery failure diagnostic |
| relay_active | relay_policy_failed | failed | emit relay trust policy rejection |
| relay_active | transfer_error | recovery | increment retry counters and re-evaluate route |
| relay_active | transfer_complete | teardown | finalize relay and end-to-end verification stats |
| any non-terminal | local_cancel | teardown | emit operator-cancel reason |
| any non-terminal | remote_teardown | teardown | emit peer-close reason |
| teardown | transfer_complete | idle | emit final success summary |
| teardown | transfer_error | failed | emit final failure summary |

## Deterministic timing and retry bounds

Default bounds (subject to implementation tuning):

- discovery timeout: 6 s
- training timeout: 10 s
- recovery window: 8 s
- max recovery attempts per transfer: 4
- max consecutive ARQ retries per chunk: 6

When any bound is exceeded, the session transitions to failed with explicit reason code.

## Security gates

Mandatory security checks:

- discovery to training requires signature verification of peer capability envelope.
- active_transfer completion requires signed manifest verification.
- chunk admission requires per-chunk integrity authentication.
- relay path activation requires trust-policy checks for each selected hop.

Trust decisions:

- trusted: allow normal progression.
- unknown: policy-controlled (allow with warning or block).
- untrusted/revoked: fail immediately with signature_verification_failed.

## Observability requirements

Each transition must emit a machine-readable event containing:

- session_id
- previous_state
- next_state
- triggering_event
- reason_code
- monotonic_timestamp_ms
- profile_id (if applicable)
- trust_decision (if applicable)

## Conformance tests

Minimum conformance coverage:

- valid flow: idle -> discovery -> training -> active_transfer -> teardown -> idle
- timeout flow for discovery and training
- recovery success and recovery exhaustion
- signature rejection path in discovery and active_transfer
- local_cancel and remote_teardown handling
