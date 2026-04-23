---
project: openpulse
doc: docs/requirements.md
status: living
last_updated: 2026-04-23
---

# Requirements

## Functional requirements

- Provide a CLI capable of transmit, receive, device listing, and mode listing.
- Support at least one production modulation plugin (BPSK family).
- Preserve a loopback backend for hardware-free development and testing.
- Support cross-platform audio through CPAL-backed implementations.
- Validate frame integrity with versioning, sequence handling, and CRC checks.

## Platform and dependency requirements

- Linux support is the primary target and requires ALSA development headers for CPAL builds.
- macOS support uses CoreAudio through CPAL.
- Windows support uses WASAPI through CPAL.
- Any development environment must support loopback mode for hardware-free testing.
- Rust toolchain must build the full workspace and no-default-features variant.

## Non-functional requirements

- Maintain workspace-level buildability on Linux and macOS CI runners.
- Keep tests runnable without physical audio hardware in default CI workflows.
- Ensure crate boundaries are clear enough for independent testing.
- Keep plugin additions from requiring broad refactors across unrelated crates.

## Compatibility and UX requirements

- CLI usage and docs must stay aligned across releases.
- New user-facing options must be documented in docs/cli-guide.md.
- README usage examples should stay current with implemented behavior.

## Documentation requirements

- Version bumps require updates to docs/changelog.md and docs/releasenotes.md.
- Docs files under docs/ must pass frontmatter validation in CI.
