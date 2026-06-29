---
project: openpulsehf
doc: docs/dev/README.md
status: living
last_updated: 2026-06-17
---

# Development and Planning Documentation

This index contains development-oriented, planning, architecture, protocol-spec, research, and validation artifacts moved from docs root.

## Planning and release governance

- [docs/dev/steering/roadmap.md](steering/roadmap.md): phased execution history and roadmap status
- [docs/dev/steering/backlog.md](steering/backlog.md): open and deferred follow-up work
- [docs/dev/archive/backlog-fec-improvements.md](archive/backlog-fec-improvements.md): FEC follow-up backlog (frozen research)
- [docs/dev/archive/backlog-waveforms.md](archive/backlog-waveforms.md): waveform follow-up backlog (frozen research)
- [docs/dev/requirements.md](requirements.md): functional and non-functional requirements
- [docs/dev/steering/changelog.md](steering/changelog.md): internal change history
- [docs/dev/release-checklist.md](release-checklist.md): release and tagging procedure
- [docs/dev/steering.md](steering.md): governance and decision ownership

## Architecture and implementation

- [docs/dev/design/architecture.md](design/architecture.md): system architecture and boundaries
- [docs/dev/design/design.md](design/design.md): product design principles
- [docs/dev/high-performance-mode.md](high-performance-mode.md): HPX mode analysis
- [docs/dev/design/protocol-wire-spec.md](design/protocol-wire-spec.md): byte-level data-plane frame + signed-handshake wire specification (base frame, SAR, CONREQ/CONACK, PQ handshake, ACK, manifest, negotiated params)
- [docs/dev/hpx-session-state-machine.md](hpx-session-state-machine.md): HPX lifecycle and conformance
- [docs/dev/design/hpx-waveform-design.md](design/hpx-waveform-design.md): waveform and profile rationale
- [docs/dev/benchmark-harness.md](benchmark-harness.md): benchmark scenarios and regression gates
- [docs/dev/design/testbench-design.md](design/testbench-design.md): signal-path testbench design
- [docs/dev/implementation-matrix.md](implementation-matrix.md): docs-to-implementation coverage matrix
- [docs/dev/steering/traceability-matrix.md](steering/traceability-matrix.md): full numbered end-to-end traceability matrix (REQ-IDs ↔ CAP-IDs → design → impl → tests → results → assets → PRs)
- [docs/dev/peer-caching-relay.md](peer-caching-relay.md): peer cache and relay behavior
- [docs/dev/peer-query-relay-wire.md](peer-query-relay-wire.md): wire-level query and relay envelope schema

## Plugin and interface development

- [docs/dev/contributing-plugins.md](contributing-plugins.md): plugin contribution workflow
- [docs/dev/plugin-commercial-interface.md](plugin-commercial-interface.md): commercial interface guidance
- [docs/dev/plugin-trait-versioning.md](plugin-trait-versioning.md): plugin trait versioning policy

## Security, policy, and compliance artifacts

- [docs/dev/AGENTS.md](AGENTS.md): agent safeguards and recovery countermeasures
- [docs/dev/sbom.md](sbom.md): software bill of materials policy
- [docs/dev/memories.md](memories.md): distilled lessons and operator notes
- [docs/dev/onair-status.md](onair-status.md): on-air validation status, blockers, and debugging findings (Phase 5.5-reg)
- [docs/dev/onair-signal-chain-verification.md](onair-signal-chain-verification.md): RF signal-chain preflight checklist

## PKI tooling

- [docs/dev/pki/pki-tooling-spec-map.md](pki/pki-tooling-spec-map.md): PKI specification map and reading order
- [docs/dev/pki/pki-tooling-requirements.md](pki/pki-tooling-requirements.md): PKI requirements
- [docs/dev/pki/pki-tooling-architecture.md](pki/pki-tooling-architecture.md): PKI architecture
- [docs/dev/pki/pki-tooling-data-model.md](pki/pki-tooling-data-model.md): PKI data model
- [docs/dev/pki/pki-tooling-api.md](pki/pki-tooling-api.md): PKI API surface
- [docs/dev/pki/pki-tooling-conformance.md](pki/pki-tooling-conformance.md): PKI conformance and release gates
- [docs/dev/pki/pki-tooling-trust-policy.md](pki/pki-tooling-trust-policy.md): PKI moderation and trust policy
- [docs/dev/pki/pki-tooling-operations-runbook.md](pki/pki-tooling-operations-runbook.md): PKI operations runbook
- [docs/dev/pki/pki-tooling-rollout-plan.md](pki/pki-tooling-rollout-plan.md): PKI rollout milestones
- [docs/dev/pki/pki-tooling-implementation-starter.md](pki/pki-tooling-implementation-starter.md): PKI implementation starter blueprint
- [docs/dev/pki/pki-tooling-glossary.md](pki/pki-tooling-glossary.md): PKI glossary

## Research

- [docs/dev/research/vara-research.md](research/vara-research.md): VARA-related technical research notes
- [docs/dev/research/ardop-research.md](research/ardop-research.md): ARDOP research notes
- [docs/dev/research/pactor-research.md](research/pactor-research.md): PACTOR research notes
- [docs/dev/research/wsjtx-analysis.md](research/wsjtx-analysis.md): WSJT-X analysis notes
- [docs/dev/research/js8call-analysis.md](research/js8call-analysis.md): JS8Call analysis notes
- [docs/dev/research/ofdm-research.md](research/ofdm-research.md): OFDM research notes
- [docs/dev/research/freedv-auth-research.md](research/freedv-auth-research.md): authenticated FreeDV research notes
- [docs/dev/research/references.md](research/references.md): external modem/DSP repos studied for technique
- [docs/dev/research/reference-mining-plan.md](research/reference-mining-plan.md): prioritized idea catalog from a source-level scan of those repos (proposal)
- [docs/dev/vara-parity-execution-board.md](vara-parity-execution-board.md): parity and execution board notes

## Historical and review artifacts

- [docs/dev/archive/handoff-2026-05-13.md](archive/handoff-2026-05-13.md): historical AI handoff snapshot
- [docs/dev/archive/project-review-2026-04-29.md](archive/project-review-2026-04-29.md): historical project review snapshot
- [docs/dev/archive/onair-ic9700-ft991a-session-learnings-2026-06-04.md](archive/onair-ic9700-ft991a-session-learnings-2026-06-04.md): on-air session learnings, failure modes, and operator setup notes
- [docs/dev/requests/code-review.md](requests/code-review.md): review request template
- [docs/dev/requests/code-review-output.md](requests/code-review-output.md): review output sample
- [docs/dev/reviews/review-request.md](reviews/review-request.md): review request log
- [docs/dev/reviews/review-260508.md](reviews/review-260508.md): review record
- [docs/dev/reviews/review-260517.md](reviews/review-260517.md): review record
- [docs/dev/reviews/review-260524.md](reviews/review-260524.md): review record
- [docs/dev/reviews/review-260531.md](reviews/review-260531.md): review record
- [docs/dev/test-reports/README.md](test-reports/README.md): validation and benchmark report index
