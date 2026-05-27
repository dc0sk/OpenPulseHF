---
project: openpulsehf
doc: docs/AGENTS.md
status: living
last_updated: 2026-05-27
---

# Agent Workflow Safeguards

This file complements the root AGENTS.md. Read both before changing branches, merging, or attempting recovery work.

## Scope

Use this document for:

- merge and branch-cleanup decisions
- dirty-worktree handling during PR flow
- recovery steps after accidental checkout, restore, clean, or reset actions

## Merge Safeguards

1. Confirm the PR is in the expected state before merging.
2. Confirm the target branch and local branch match the intended workflow.
3. Check whether the worktree is dirty before branch deletion or cleanup.
4. If deletion or cleanup would affect uncommitted work, stop and ask the user.
5. Prefer fast-forward or platform-mediated merges over manual history rewriting unless explicitly requested.

## Dirty Worktree Rules

- Do not delete a branch if local-only changes still need review or preservation.
- Do not auto-stash unless the user asked for stash-based recovery.
- Do not auto-commit unrelated changes just to unblock cleanup.
- If local changes conflict with the requested operation, present the conflict clearly and stop for confirmation.

## Recovery Protocol

If files are lost or overwritten by mistake:

1. Freeze the current state and avoid further destructive commands.
2. Inspect `git reflog` and `git fsck --lost-found` for recoverable objects.
3. Restore candidate content into a separate recovery location first.
4. Compare recovered content against the current worktree before applying anything.
5. Ask for confirmation before restoring recovered files into tracked paths.

## Reporting Expectations

When a merge, deletion, or recovery step is blocked, report:

- what command was attempted
- what state blocked it
- what data is at risk
- the safe next options available to the user
