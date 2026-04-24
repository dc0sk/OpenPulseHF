---
project: openpulsehf
doc: docs/release-checklist.md
status: living
last_updated: 2026-04-24
---

# Release Checklist Template

Use this checklist when preparing a new release of OpenPulse. This guide covers version bumping from a current stable release (e.g., `2.3.0`) to a new version (e.g., `2.4.0` for minor, `2.3.1` for patch, or `3.0.0` for major).

## Pre-Release Planning (1 week before)

- [ ] **Decide version number**
  - Patch (`x.y.Z`) for bug fixes only
  - Minor (`x.Y.0`) for backward-compatible features
  - Major (`X.0.0`) for breaking changes
  - Use semantic versioning: [semver.org](https://semver.org/)

- [ ] **Create release milestone** on GitHub (if not already present)
  - Title: `v<version> Release`
  - Description: High-level changes

- [ ] **Triage open issues and PRs**
  - Close or defer non-critical items
  - Add critical fixes to milestone

- [ ] **Communicate with team**
  - Announce release plan in project chat or issue
  - Allow time for final feedback

---

## Code Preparation (3–5 days before)

- [ ] **Create release branch**
  ```bash
  git checkout -b release/v<version>
  ```

- [ ] **Verify all tests pass on main**
  ```bash
  cargo test --all
  cargo test --all --features cpal-backend
  cargo fmt --all -- --check  # Verify formatting
  cargo clippy --all -- -D warnings  # Lint checks
  ```

- [ ] **Generate SBOM** (if applicable)
  ```bash
  # Only when bumping version (not for every patch)
  cargo sbom --output-format spdx-json > SBOM.spdx.json
  git add SBOM.spdx.json
  ```

---

## Version and Documentation Updates

### 1. Update Version Numbers

- [ ] **`Cargo.toml` (workspace root)**
  ```toml
  [workspace]
  resolver = "2"
  members = […]

  [workspace.package]
  version = "2.4.0"  # ← Update here
  ```

- [ ] **Each crate's `Cargo.toml`**
  ```bash
  # Update version field in:
  # - crates/openpulse-core/Cargo.toml
  # - crates/openpulse-audio/Cargo.toml
  # - crates/openpulse-modem/Cargo.toml
  # - crates/openpulse-cli/Cargo.toml
  # - plugins/bpsk/Cargo.toml
  # - ... other crates
  ```

  ```toml
  [package]
  version = "2.4.0"  # ← Update here
  ```

- [ ] **Verify version consistency**
  ```bash
  grep -r "^version = " Cargo.toml* | sort | uniq -c
  # All should show "2.4.0" (or appropriate version)
  ```

### 2. Update README

- [ ] **`README.md`** (project root)
  - [ ] Update version badge: `![version-badge](...)`
  - [ ] Update feature list if features added/removed
  - [ ] Update quick-start example (if command syntax changed)
  - [ ] Update installation instructions if applicable

- [ ] **`docs/cli-guide.md`**
  - [ ] Update version header: `# OpenPulse CLI v2.4.0 Guide`
  - [ ] Update command reference if any new flags/options
  - [ ] Update any version-specific notes

### 3. Update Changelog

- [ ] **`docs/CHANGELOG.md`**
  - [ ] Add new version section at the top
  - [ ] Use format:
    ```markdown
    ## v2.4.0 (2026-04-24)

    ### Features
    - [Brief description of new feature]
    - [Another feature]

    ### Bug fixes
    - [Fixed issue X]
    - [Fixed issue Y]

    ### Breaking changes
    - [If any, list with migration notes]

    ### Known limitations
    - [If any]
    ```
  - [ ] Link all related PRs and issues: `(#123)`, `Fixes #456`
  - [ ] Verify dates are accurate

- [ ] **`docs/releasenotes.md`**
  - [ ] Add release date and version
  - [ ] Summarize major changes (same as CHANGELOG, but with more detail)
  - [ ] If breaking changes: include migration guide

### 4. Update Last-Updated Timestamps

These are auto-managed by CI stamp script, but ensure the script runs:

- [ ] **Run stamp-doc script** (if not auto-running in CI)
  ```bash
  bash scripts/stamp-doc-last-updated.sh
  # Updates all docs/*/md files with current date
  ```

- [ ] **Verify timestamps updated**
  ```bash
  git diff docs/ | grep "last_updated:"
  ```

---

## Testing Before Release

- [ ] **Run full test suite**
  ```bash
  cargo test --all
  ```

- [ ] **Run integration tests**
  ```bash
  cargo test -p openpulse-modem --test hotplug_integration
  # (or other integration test suites)
  ```

- [ ] **Test CLI on multiple platforms** (if you have access)
  - [ ] Linux: `cargo build --features cpal-backend && ./target/debug/openpulse-cli --version`
  - [ ] macOS: `cargo build --features cpal-backend && ./target/debug/openpulse-cli --version`
  - [ ] Windows (if applicable): same

- [ ] **Test a sample workflow end-to-end**
  ```bash
  cargo build --features cpal-backend --release
  ./target/release/openpulse-cli transmit "Hello" BPSK31 --backend loopback
  ./target/release/openpulse-cli receive BPSK31 --backend loopback
  # (Note: --backend flag is illustrative; actual CLI may vary)
  ```

- [ ] **Verify documentation builds** (if using a doc builder like mdBook)
  ```bash
  mdbook build docs/  # If applicable
  # Or just verify Markdown syntax:
  for f in docs/*.md; do markdown-lint "$f" || true; done
  ```

---

## Git and GitHub Operations

### 1. Commit Version and Doc Changes

- [ ] **Stage all updated files**
  ```bash
  git add Cargo.toml Cargo.lock docs/ README.md plugins/ crates/
  ```

- [ ] **Verify staged changes**
  ```bash
  git diff --cached | head -100
  # Review for correctness
  ```

- [ ] **Commit**
  ```bash
  git commit -m "v2.4.0: bump version and update documentation"
  ```

- [ ] **Push release branch**
  ```bash
  git push -u origin release/v<version>
  ```

### 2. Create Pull Request

- [ ] **Open PR on GitHub**
  - Title: `v2.4.0: release preparation`
  - Description:
    ```markdown
    Release version 2.4.0.

    ## Changes
    - Version bumps in Cargo.toml
    - Updated CHANGELOG.md with v2.4.0 entries
    - Updated README.md version badge
    - Updated docs/releasenotes.md with release notes

    ## Testing
    - [x] All tests pass
    - [x] CLI tested on Linux
    - [x] Documentation builds

    Merge with squash before creating GitHub Release.
    ```

- [ ] **Allow CI checks to run**
  - Wait for all checks (tests, lints, docs) to pass
  - Address any failures

### 3. Merge Release PR

- [ ] **Ensure all checks are green**
  ```bash
  gh pr view <PR_NUMBER> --json statusCheckRollup
  ```

- [ ] **Merge PR** (squash merge recommended for clean history)
  ```bash
  gh pr merge <PR_NUMBER> --squash --delete-branch
  ```

- [ ] **Verify main branch is updated locally**
  ```bash
  git checkout main
  git pull origin main
  git log -1 --oneline
  # Should show your commit
  ```

### 4. Create Git Tag

- [ ] **Tag the release**
  ```bash
  git tag -a v2.4.0 -m "Release v2.4.0"
  ```

- [ ] **Verify tag**
  ```bash
  git tag -l v2.4.0 -n1
  ```

- [ ] **Push tag to remote**
  ```bash
  git push origin v2.4.0
  ```

---

## GitHub Release Creation

- [ ] **Create GitHub Release**
  ```bash
  gh release create v2.4.0 \
    --title "OpenPulse v2.4.0" \
    --notes-file docs/releasenotes.md \
    --draft  # Start as draft for review
  ```

  Or use GitHub web UI:
  - Go to [Releases](https://github.com/dc0sk/OpenPulseHF/releases)
  - Click "Create a new release"
  - Tag: `v2.4.0`
  - Title: `OpenPulse v2.4.0`
  - Description: (paste from `docs/releasenotes.md`)
  - [ ] Mark as "Pre-release" if beta/RC (otherwise, leave unchecked)
  - [ ] Click "Save as draft" first for review

- [ ] **Review draft release**
  - [ ] Check formatting is correct
  - [ ] Verify all links work
  - [ ] Ensure changelog is complete

- [ ] **Publish release**
  ```bash
  gh release edit v2.4.0 --draft=false
  # Or click "Publish release" on GitHub web UI
  ```

---

## Build Artifacts (Optional)

If you're distributing pre-built binaries:

- [ ] **Build release binaries**
  ```bash
  cargo build --release --all
  ```

- [ ] **Create distribution archives**
  ```bash
  # Example: Linux
  tar czf openpulse-v2.4.0-linux-x86_64.tar.gz \
    target/release/openpulse-cli \
    README.md \
    LICENSE

  # Example: macOS
  tar czf openpulse-v2.4.0-macos-x86_64.tar.gz \
    target/release/openpulse-cli \
    README.md \
    LICENSE

  # Example: Windows (if applicable)
  zip openpulse-v2.4.0-windows-x86_64.zip \
    target/release/openpulse-cli.exe \
    README.md \
    LICENSE
  ```

- [ ] **Upload artifacts to GitHub Release**
  ```bash
  gh release upload v2.4.0 openpulse-v2.4.0-*.tar.gz openpulse-v2.4.0-*.zip
  ```

- [ ] **Verify downloads are available** on GitHub Releases page

---

## Post-Release (After Publishing)

- [ ] **Announce release**
  - [ ] Post on project homepage / wiki
  - [ ] Email release notes to stakeholders (if applicable)
  - [ ] Announce in chat/forum (if you have one)

- [ ] **Close release milestone** on GitHub
  - Go to [Milestones](https://github.com/dc0sk/OpenPulseHF/milestones)
  - Click the completed version milestone
  - Click "Close milestone"

- [ ] **Update project boards**
  - Move any open issues to next milestone
  - Plan next version

- [ ] **Monitor for issues**
  - Watch GitHub issues for post-release bugs
  - Be prepared to create a patch release (e.g., `v2.4.1`) if critical bugs are found
  - Patch releases skip the RC/draft phase and go straight to release (fast-track)

---

## Patch Release Quick Path (e.g., `v2.4.0` → `v2.4.1`)

For bug-fix-only patch releases, follow the same checklist but:

- [ ] **Skip breaking change discussions** (no API changes allowed)
- [ ] **Update only docs/CHANGELOG.md** (minimal entry)
- [ ] **Use the same release checklist** but fast-track PR review
- [ ] **Merge and tag immediately** after review passes (no draft phase)

---

## Rollback Plan (If Release Has Critical Issues)

- [ ] **Do NOT re-tag the same version**
  - [ ] If `v2.4.0` has critical bugs:
    - Immediately release `v2.4.1` with fixes
    - Do NOT reuse `v2.4.0` tag

- [ ] **Document issue in release notes**
  - [ ] Add note to `v2.4.0` release on GitHub: `⚠️ Do not use; upgrade to v2.4.1`

- [ ] **Revert main if necessary**
  ```bash
  git revert <commit_hash>  # Do NOT force-push main
  git push origin main
  ```

---

## Common Issues During Release

| Issue | Solution |
|---|---|
| Tests fail during release commit | Fix on a feature branch, re-merge to main before release branch |
| Version numbers are inconsistent | Use `grep -r "version"` to find all occurrences, update workspace Cargo.toml first |
| CHANGELOG conflicts | Merge main into release branch before finalizing |
| GitHub CI checks hanging | Cancel, restart, or check GitHub Actions status page |
| Tag already exists locally | `git tag -d v2.4.0` and recreate if needed |
| Cannot push tags | Verify you have push permissions on the repository |

---

## Version Numbering Policy

**OpenPulse semantic versioning**:

- **Major** (`X.0.0`): Breaking changes to user-facing API or file formats
  - Plugin trait changes → Major bump
  - CLI argument removal → Major bump
  - Config file format change → Major bump

- **Minor** (`x.Y.0`): Backward-compatible features, new plugins, improvements
  - New modulation mode (plugin) → Minor bump
  - New CLI flag → Minor bump
  - Performance improvement → Minor bump

- **Patch** (`x.y.Z`): Bug fixes, docs, internal improvements
  - Demodulation bug fix → Patch bump
  - Documentation clarification → Patch bump
  - Internal refactor → Patch bump (only if no user-visible change)

**Example release sequence**:
```
v1.0.0 → initial release
v1.0.1 → patch (bug fix)
v1.1.0 → minor (new feature)
v1.1.1 → patch (bug fix)
v1.2.0 → minor (new features)
v2.0.0 → major (breaking change)
```

---

## Release Cadence

- **Stable releases**: every 4–8 weeks (varies by activity)
- **Patch releases**: as-needed (usually within 1 week of finding critical bugs)
- **RCs (release candidates)**: optional, used for major versions with significant changes

---

## Checklist for Major vs. Minor vs. Patch

### Major Release Checklist

- [ ] Breaking changes are documented in `docs/releasenotes.md` with migration guide
- [ ] `docs/plugin-trait-versioning.md` is updated (if trait changed)
- [ ] All existing plugins tested to ensure incompatibility is clear
- [ ] Documentation examples are updated to reflect breaking changes

### Minor Release Checklist

- [ ] New features are added to `docs/cli-guide.md`
- [ ] New plugins are registered in CLI and documented
- [ ] No breaking changes to public API

### Patch Release Checklist

- [ ] Minimal CHANGELOG entry (e.g., "Bug fixes")
- [ ] No new features or breaking changes
- [ ] All existing tests pass

---

## After-Release (Next Development Cycle)

- [ ] **Bump development version** (optional, can wait)
  ```bash
  # In Cargo.toml: version = "2.5.0-dev"
  # This signals that development is underway for v2.5.0
  ```

- [ ] **Create next milestone** on GitHub for `v2.5.0`

- [ ] **Review and prioritize backlog** for next release

---

## References

- [Semantic Versioning](https://semver.org/)
- [Cargo documentation](https://doc.rust-lang.org/cargo/)
- [GitHub Releases documentation](https://docs.github.com/en/repositories/releasing-projects-on-github)

