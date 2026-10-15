---
project: openpulsehf
doc: docs/README.md
status: living
last_updated: 2026-10-15
---

# Documentation Index

User-facing documentation for OpenPulseHF. Developer, design and planning documents are indexed
separately in [dev/README.md](dev/README.md). Release 1 (`v0.17.0`) scope and status are summarised
in the [top-level README](../README.md#status).

## Start here

- [overview.md](overview.md): what OpenPulseHF is, Release 1 scope and status, in one page
- [openpulse-abstract.md](openpulse-abstract.md): short abstract for different audiences
- [openpulse-manual.md](openpulse-manual.md): operator, integration and technical manual
- [openpulse-book.md](openpulse-book.md): complete technical account (also as [PDF](openpulse-book.pdf))

## Modes, features and the protocol stack

- [mode-fec-ladder.md](mode-fec-ladder.md): the `hpx_hf` ladder, the `fast`/`robust` profiles, and how a mode and FEC are chosen
- [features.md](features.md): feature reference with the mathematics behind non-obvious algorithms
- [osi-layer-map.md](osi-layer-map.md): where each crate sits in the OSI model
- [non-gpl-interfacing.md](non-gpl-interfacing.md): TCP, WebSocket and subprocess interfaces for non-GPL software

## CLI and configuration

- [cli-guide.md](cli-guide.md): CLI usage and options
- [cli-mode-advisor-guide.md](cli-mode-advisor-guide.md): mode advisor workflow
- [cli-ux-identity-trust-diagnostics.md](cli-ux-identity-trust-diagnostics.md): identity/trust diagnostics and exit codes
- [config/README.md](config/README.md): example configuration files (`openpulse.config.toml`, rig definitions, on-air station scripts, PKI environment)
- [openpulse-manual.md#212-pki-tooling-service-quick-start](openpulse-manual.md#212-pki-tooling-service-quick-start): PKI tooling quick start (not part of Release 1)
- [use-case-deployment-guide.md](use-case-deployment-guide.md): deployment patterns and operator scenarios

## Security, trust and regulatory

- [trust-store-file-format.md](trust-store-file-format.md): trust-store serialisation format
- [regulatory.md](regulatory.md): regulatory analysis by jurisdiction
- [regulatory-compliance-checklist.md](regulatory-compliance-checklist.md): compliance checklist and implementation notes

## On air, operations and troubleshooting

- [on-air_testplan.md](on-air_testplan.md): on-air validation plan and evidence checklist
- [troubleshooting-alsa-cpal.md](troubleshooting-alsa-cpal.md): audio backend troubleshooting
- [raspberry-pi-4-5-tuning-and-benchmarks.md](raspberry-pi-4-5-tuning-and-benchmarks.md): Raspberry Pi tuning

## Releases

- [releasenotes.md](releasenotes.md): user-facing release notes, including the unreleased changes toward `v0.17.0`
- [../CHANGELOG.md](../CHANGELOG.md): pointer to the canonical changelogs, plus the 0.1.0–0.2.0 history

## Presentations

- [presentations/](presentations/): overview slide decks (`OpenPulseHF-Overview.odp`, German `OpenPulseHF-Overview-DE.odp`) and their generator sources

## Developer and planning docs

- [dev/README.md](dev/README.md): development, architecture, planning and validation index
- [dev/project/workplan.md](dev/project/workplan.md): the Release 1 work plan, milestones and decision log
