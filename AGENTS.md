---
project: openpulsehf
doc: AGENTS.md
status: living
last_updated: 2026-06-07
---

# Agent Safety Rules

These rules are mandatory for any coding agent operating in this repository.

## Branch Discipline

- Never work directly on `main`.
- Never commit directly to `main`.
- Use feature branches and pull requests for all changes.

## Commit Hygiene

- Keep commits small and focused.
- Prefer one logical change per commit.
- Avoid bundling unrelated refactors with behavior changes.

## Destructive Git Commands

Never run destructive cleanup commands unless the user explicitly requests that exact command in the current conversation:

- git checkout .
- git checkout -- <path>
- git restore --worktree --staged ...
- git clean -fd
- git clean -fdx
- git reset --hard

If a merge or branch-delete operation fails because the working tree is dirty, stop and ask the user how to proceed. Do not auto-clean the tree.

## Safe Merge Workflow

When merging PRs:

1. Check PR state and CI.
2. Merge PR.
3. If local branch deletion fails due to local changes, report the failure and stop.
4. Offer explicit options:
   - keep local changes and skip branch deletion
   - stash changes
   - commit changes
   - discard changes (only with explicit user approval)

## Recovery Protocol

If accidental cleanup happens:

1. Run git reflog and git fsck --lost-found.
2. Extract recoverable snapshots into a separate recovery folder.
3. Do not overwrite current files automatically.
4. Present a per-file restore plan and ask for confirmation before applying recovered content.

## Agent Workflow Safeguards

This section captures merge and recovery safeguards previously documented in docs/AGENTS.md.

### Scope

Use these safeguards for:

- merge and branch-cleanup decisions
- dirty-worktree handling during PR flow
- recovery steps after accidental checkout, restore, clean, or reset actions

### Merge Safeguards

1. Confirm the PR is in the expected state before merging.
2. Confirm the target branch and local branch match the intended workflow.
3. Check whether the worktree is dirty before branch deletion or cleanup.
4. If deletion or cleanup would affect uncommitted work, stop and ask the user.
5. Prefer fast-forward or platform-mediated merges over manual history rewriting unless explicitly requested.

### Dirty Worktree Rules

- Do not delete a branch if local-only changes still need review or preservation.
- Do not auto-stash unless the user asked for stash-based recovery.
- Do not auto-commit unrelated changes just to unblock cleanup.
- If local changes conflict with the requested operation, present the conflict clearly and stop for confirmation.

### Reporting Expectations

When a merge, deletion, or recovery step is blocked, report:

- what command was attempted
- what state blocked it
- what data is at risk
- the safe next options available to the user

### Temporary Files and Cleanup

- For temporary files, use a temporary directory inside the project, not /tmp.
- Add that temporary directory to .gitignore.
- Remove temporary scripts and helper files when the task is complete.

## OpenPulseHF Copilot Instructions

This section follows a Copilot-instructions style format and applies to coding agents working in this repository.

### Project Overview

OpenPulseHF is a multi-crate Rust workspace for HF modem and protocol tooling, including DSP, modem pipelines, protocol bridges, daemon services, and operator-facing CLI and UI tools.

### Workspace Architecture

- Core crates: crates/openpulse-core, crates/openpulse-dsp, crates/openpulse-modem, crates/openpulse-channel, crates/openpulse-audio, crates/openpulse-radio
- Protocol and service crates: crates/openpulse-ardop, crates/openpulse-kiss, crates/openpulse-b2f, crates/openpulse-b2f-driver, crates/openpulse-gateway, crates/openpulse-qsy, crates/openpulse-daemon, crates/openpulse-mesh, crates/openpulse-repeater
- Tooling and apps: crates/openpulse-cli, crates/openpulse-tui, apps/openpulse-testbench, apps/openpulse-testmatrix, apps/openpulse-panel, pki-tooling
- Modulation plugins: plugins/bpsk, plugins/qpsk, plugins/psk8, plugins/64qam, plugins/fsk4, plugins/ofdm, plugins/scfdma

### Preferred Discovery Workflow

1. Start with targeted file search and semantic search to scope impact.
2. Follow crate boundaries and imports before making edits.
3. Read existing tests in touched crates to preserve expected behavior.
4. Avoid broad refactors in unrelated crates.

### Rust Build and Validation

Use these commands by default unless a task explicitly requires a different scope.

- Format check: cargo fmt --all -- --check
- Lint gate: cargo clippy --workspace --no-default-features -- -D warnings
- Test gate: cargo test --workspace --no-default-features

If full workspace checks are blocked by local constraints, use scoped fallback gates documented in CLAUDE.md.

### Feature Flag and Environment Rules

- Treat --no-default-features as the default for CI-compatible work.
- Do not assume audio hardware availability in tests.
- Keep CPAL paths feature-gated and prefer loopback or simulated channels for reproducibility.

### Rust Coding Guidelines

- Library crates: avoid unwrap and expect in production paths.
- Error handling: use thiserror in libraries and anyhow in CLI or tests.
- Keep modules focused and avoid unrelated API churn.
- Add or update tests for behavior changes.
- Reuse existing utilities and patterns before introducing new abstractions.

### Operational Testing Guidelines

- For on-air or hardware-adjacent scripts, prioritize safety defaults and predictable cleanup.
- Keep CAT and PTT interactions explicit and auditable in logs.
- Prefer deterministic payload generation for matrix-style validation.
- Persist only intentional artifacts; clean temporary outputs after iterative debugging.

### Completion Checklist

Before declaring work complete:

1. Ensure formatting is clean.
2. Ensure clippy warnings are resolved in touched scope.
3. Ensure tests pass for touched scope, then run broader gates when feasible.
4. Verify no unintended file modifications are included.

### Learnings and Maintenance

- Capture recurring pitfalls as concise additions to this file or CLAUDE.md.
- Keep guidance current with repository tooling and CI behavior.
- Prefer updating existing guidance sections over creating fragmented policy files.
