---
project: openpulsehf
doc: docs/AGENTS.md
status: living
last_updated: 2026-04-25
---

# Agent Safeguards and Recovery Guide

This document captures the operational safeguards for coding agents in OpenPulseHF,
including root-cause analysis of recent workspace incidents and the countermeasures
required to prevent recurrence.

## Purpose

- Prevent accidental data loss from destructive git cleanup.
- Keep merge and branch maintenance safe when local changes exist.
- Standardize recovery behavior after accidental cleanup.
- Ensure terminal interactions do not hang or become ambiguous.

## Incident Root Causes

### Root cause 1: destructive cleanup executed after merge housekeeping failure

A PR merge succeeded, but local branch deletion/cleanup encountered a dirty worktree.
Instead of stopping for explicit user approval, destructive cleanup commands were run
in the same flow.

Impact:
- Unstaged and untracked local work was removed.
- Recovery effort was required via reflog/fsck and dangling objects.

### Root cause 2: implicit terminal interactivity in file copy operations

Some shell sessions used interactive copy behavior that prompted for overwrite.
Automated command chains became blocked and appeared inconsistent.

Impact:
- Incomplete restore attempts.
- Mixed validation output from partially applied file states.

### Root cause 3: safety policy drift after cleanup

Safeguard artifacts were themselves at risk when cleanup commands were executed.
This removed guardrail files and recovery bundles that were intended to protect
future work.

Impact:
- Temporary loss of prevention and recovery context.
- Additional time spent rebuilding safety artifacts.

## Non-Negotiable Guardrails

1. Never work directly on `main`; all work must happen on a feature branch.
2. Never commit directly to `main`; merge by pull request only.
3. Keep commits small and focused, with one logical change per commit.
4. Never run destructive cleanup commands unless the user explicitly requests the exact command in the current conversation.
5. If merge post-steps fail due to dirty working tree, stop and ask the user which option to use.
6. Recovery must be extraction-first: write snapshots into a separate recovery folder before any overwrite.
7. Never overwrite current files with recovered content automatically; apply only after user confirmation.
8. Prefer non-interactive copy mode for scripted restore actions to avoid hidden prompts.

## Commands Requiring Explicit Approval

- git checkout .
- git checkout -- <path>
- git restore --worktree --staged ...
- git clean -fd
- git clean -fdx
- git reset --hard

## Safe Merge Procedure

1. Validate PR state and checks.
2. Merge PR.
3. Attempt local branch cleanup only if worktree is clean.
4. If cleanup fails because of local changes, stop and offer options:
   - keep local changes and skip branch deletion
   - stash changes
   - commit changes
   - discard changes (only with explicit approval)

## Recovery Procedure

1. Run git reflog and git fsck --lost-found.
2. Identify recoverable commits, trees, and blobs.
3. Export candidate file snapshots into a dedicated folder, for example recovery-rebuilt/.
4. Generate per-file diffs against current workspace state.
5. Present restore options per file.
6. Apply selected restores only after explicit user confirmation.
7. Re-run targeted tests for restored components.

## Terminal Interaction Safeguards

- Avoid commands that can prompt unexpectedly in automation chains.
- For copy operations during recovery, use non-interactive behavior explicitly.
- Treat any overwrite prompt as a hard stop requiring immediate handling.
- Kill stale terminal sessions waiting for input before launching new restore commands.

## Validation Rules After Recovery

- Validate only affected crates first.
- Confirm that key behavior regressions are gone.
- Expand to broader workspace validation after local target passes.
- Do not claim recovery completion until test targets used before failure pass again.

## Documentation and Traceability Rules

- Keep this document current when a new failure mode is discovered.
- Record recovery folders and notes until user approves cleanup.
- Keep root policy file AGENTS.md aligned with this document.

## Quick Checklist

Before merge:
- Worktree inspected.
- No implicit cleanup planned.

If merge cleanup fails:
- Stop.
- Ask user for explicit option.

If cleanup occurred accidentally:
- Run reflog/fsck.
- Extract snapshots to recovery folder.
- Offer per-file restore plan.
- Validate restored targets.

After recovery:
- Re-establish safeguard docs.
- Keep recovery artifacts until user confirms deletion.
