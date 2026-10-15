---
project: openpulsehf
doc: docs/dev/design/design.md
status: living
last_updated: 2026-10-15
---

# Design

## Product design direction

- Keep setup and operation simple for operators who want reliable TX/RX quickly.
- Prioritize predictable command behavior over hidden magic.
- Ensure all beginner-friendly defaults can be overridden for advanced workflows.
- Design features so loopback-first validation is always available.

## Interaction model

- CLI is the canonical interaction surface and baseline for correctness.
- Release 1 (`v0.17.0`) ships three operator surfaces: the CLI, the station daemon it controls
  (`openpulse-daemon`), and the ARDOP-compatible TNC (`openpulse-ardop`) for Pat. The TUI and panel
  exist but are experimental and not part of Release 1.
- Commands should support both one-shot operation and scriptable automation.
- Other frontends should preserve CLI terminology and behavior.

## Output design

- Output should be explicit about mode, backend, and operation outcome.
- Human-readable console output is primary; commands that feed automation also emit JSON (for
  example `benchmark run`).
- Error output should name actionable remediation steps when possible.

## Incremental design strategy

- Start with robust BPSK workflows and strong diagnostics.
- Add new modes, coding, and adaptive rate strategies in isolated increments.
- Expand frontend surfaces only after core modem behavior is stable.
- Where this stands for Release 1: one adaptive ladder (`hpx_hf`, SL1–SL14) run as two profiles,
  `fast` and `robust`; everything outside that scope ships disabled or experimental
  (`docs/dev/project/workplan.md`, decision 1).

## Extensibility design

- Plugin contracts should be small, explicit, and testable.
- New plugin capabilities must include compatibility and lifecycle notes.
- Breaking changes to plugin traits require migration guidance in release notes.

## Documentation design constraints

- Docs files keep standard frontmatter and pull-request update flow.
- last_updated remains CI-managed via stamping workflow.
