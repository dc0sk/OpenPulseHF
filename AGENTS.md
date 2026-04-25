# Agent Safety Rules

These rules are mandatory for any coding agent operating in this repository.

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
