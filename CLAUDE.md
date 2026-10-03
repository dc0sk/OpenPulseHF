---
project: openpulsehf
doc: CLAUDE.md
status: living
last_updated: 2026-09-30
---

# CLAUDE.md — OpenPulseHF Agent Contract

This file is the authoritative guide for any coding agent working in this repository. Read it before touching code. Mandatory agent safety rules are in `AGENTS.md` (root) and `docs/dev/AGENTS.md`.

---

## Build and test commands

```bash
# Toolchain preflight (required: rustc >= 1.97.1)
./scripts/check-toolchain.sh

# Full workspace build (requires libasound2-dev on Linux)
cargo build --workspace

# Full test suite (no audio hardware required)
cargo test --workspace --no-default-features

# The acceptance suites held OUT of that run, for runtime (~83 min of the ~2 h gate for the first
# two). `scripts/gate.sh` does NOT run them; run before any change to the receiver notch, the OTA
# rate controller, the carrier detect, or the acquisition chain, and before a release.
scripts/slow-tests.sh              # all       (notch REQ-QRM-01 ~35 min, OTA CAP-33 ~48 min,
                                   #            spectral-busy decode counts #1454 ~5 min, release)
scripts/slow-tests.sh notch        # one suite

# Run a specific test file
cargo test --package openpulse-modem --no-default-features --test fec_loopback

# Clippy (treat warnings as errors). --all-targets lints tests/benches too — without it,
# test code is never linted and rots (that is how an unused binding sat in session_key.rs).
cargo clippy --workspace --no-default-features --all-targets -- -D warnings

# Format check
cargo fmt --all -- --check

# Cross-compile check for Raspberry Pi (requires `cross` installed)
cross check --workspace --target aarch64-unknown-linux-gnu --no-default-features

# Run the benchmark and capture JSON output
cargo run -p openpulse-cli --no-default-features -- --backend loopback --log error benchmark run

# CI benchmark regression gate (run locally to verify before PR)
cargo run -p openpulse-cli --no-default-features -- --backend loopback --log error benchmark run >/tmp/bench.json
jq '.passed == .total and .mean_transitions <= 20.0' /tmp/bench.json  # must print true

# Run the quick-tier test matrix (virtual channels, no hardware) — outputs to docs/test-reports/
cargo run -p openpulse-testmatrix --no-default-features

# Run the full test matrix (all propagation channels and payload sizes)
cargo run -p openpulse-testmatrix --no-default-features -- --full --output docs/test-reports

# Fallback core gates when full workspace checks are blocked by local toolchain constraints
cargo clippy --workspace --exclude pki-tooling --no-default-features --all-targets -- -D warnings
cargo test --workspace --exclude pki-tooling --no-default-features
```

The `--no-default-features` flag disables the CPAL audio backend and is required for CI. All tests must pass with this flag. Never add tests that require real audio hardware.

---

## Where things are

The full text of everything below lives in `docs/`; this file keeps what every session needs.
**Code comments that cite a `CLAUDE.md` section by name refer to the file in the right column.**

| Section (as cited in comments) | Now in |
|---|---|
| **Release 1 work plan** — milestones, scope, triage rules, decision log | [`docs/dev/project/workplan.md`](docs/dev/project/workplan.md) |
| Acceptance criteria (requirement → test command, one row each) | [`docs/dev/project/acceptance-criteria.md`](docs/dev/project/acceptance-criteria.md) |
| Known sharp edges (FEC boundaries, ladder, SNR/LLR, acquisition, capture level, RX/TX seams) | [`docs/dev/sharp-edges.md`](docs/dev/sharp-edges.md) |
| DSP acquisition & carrier-recovery playbook | [`docs/dev/dsp-acquisition-playbook.md`](docs/dev/dsp-acquisition-playbook.md) |
| Verification mechanics — full wording, "Known hole", gate history | [`docs/dev/verification-mechanics.md`](docs/dev/verification-mechanics.md) |
| Adversarial review — the 2026-08-02 rule and the reasons behind each routing item | [`docs/dev/adversarial-review.md`](docs/dev/adversarial-review.md) |
| Full crate map and feature-track status (FF-15, FF-16, deferred items) | [`docs/dev/crate-map.md`](docs/dev/crate-map.md) |
| Completed-phase history | [`docs/dev/project/claude-md-completed-phase-history.md`](docs/dev/project/claude-md-completed-phase-history.md) |

**Read before touching:** acquisition, AFC, timing or carrier recovery → the playbook and the
acquisition edges; FEC modes or frame sizes → the FEC and ladder edges; SNR estimators or LLRs → the
SNR/LLR edges; anything that must run on every receive or transmit → the RX/TX seam checklist below.

---

## Current focus

**Release 1** = the `hpx_hf` ladder, FEC, session compression, daemon + CLI and the ARDOP TNC,
gated on A1 + A2 on **2 m**, then confirmed on HF with the release candidate (test stages: virtual
audio → hardware loopback → 2 m → HF). Version `v0.17.0`. Everything else ships disabled. The plan, milestones and triage rules are in
[`docs/dev/project/workplan.md`](docs/dev/project/workplan.md). **Work that serves no milestone there
is `post-release`**; a PR says which milestone it serves and updates that milestone's status line.

---

## Crate map (short)

Full table: [`docs/dev/crate-map.md`](docs/dev/crate-map.md). **Release 1 crates in bold.**

- **Core:** **`openpulse-core`** (frame, FEC, SAR, ACK, rate adaptation, handshake, compression),
  **`openpulse-modem`** (`ModemEngine`, acquisition, DCD), **`openpulse-dsp`**, **`openpulse-audio`**,
  **`openpulse-radio`** (PTT/CAT), **`openpulse-config`**, **`openpulse-channel`** (simulation),
  `openpulse-gpu`, `openpulse-keystore`, `openpulse-linksec`.
- **Protocol:** **`openpulse-daemon`**, **`openpulse-ardop`** (TNC, Pat-compatible), `openpulse-kiss`,
  `openpulse-b2f`, `openpulse-b2f-driver`, `openpulse-gateway`, `openpulse-qsy`,
  `openpulse-discovery`, `openpulse-mesh`, `openpulse-repeater`, `openpulse-freedv-auth`,
  `openpulse-filexfer`.
- **UI/tools:** **`openpulse-cli`**, **`openpulse-linksim`** (goodput gate), `openpulse-tui`,
  `openpulse-panel`, `openpulse-testbench`, `openpulse-testmatrix`, `openpulse-twinview`,
  `openpulse-dict-trainer`, `pki-tooling`.
- **Plugins on `hpx_hf`:** **`mfsk16`** (SL1 + K=3 return channel), **`bpsk`** (SL2–5), **`qpsk`**
  (SL6, `QPSK250-D`), **`ofdm`** (SL7–14), **`fsk4`** (ACK). Not on `hpx_hf`: `psk8`, `64qam`,
  `scfdma`, `pilot`, `js8`.

---

## Coding conventions

### Rust style
- `thiserror` for error types in library crates; `anyhow` in CLI and test code
- No `unwrap()` or `expect()` in library crate production paths (`openpulse-core`, `openpulse-audio`, `openpulse-modem`, `openpulse-channel`, `openpulse-radio`). `expect()` is acceptable in tests and CLI.
- Derive `Debug`, `Clone`, `PartialEq` on config and result types
- Derive `serde::Serialize, Deserialize` on any type that crosses an API boundary or is emitted as JSON
- Use `tracing::{debug, info, warn, error}` for structured logging; no `println!` in library code
- Integer field sizes: use the smallest type that covers the domain (`u8` for counts ≤ 255, `u16` for sequence numbers, `f32` for audio samples and DSP)
- `Arc<RwLock<T>>` for shared state read by multiple threads; `crossbeam_channel` for inter-thread messaging

### Module organisation
- One concept per file; prefer small focused modules over large files
- Traits defined in `mod.rs` or a dedicated `traits.rs`; implementations in separate files named after the implementation
- Test modules inline for unit tests (`#[cfg(test)] mod tests { ... }`); integration tests in `tests/` directory

### Documentation
- All public types and functions get a one-line doc comment
- No multi-paragraph docstrings
- No comments explaining what the code does; only comments explaining why when the reason is non-obvious

### Commit style
- One logical change per commit
- Prefix: `feat:`, `fix:`, `refactor:`, `test:`, `docs:`, `ci:`
- Imperative mood: "add block interleaver" not "added block interleaver"

### PR hygiene
- Every PR must pass `cargo test --workspace --no-default-features` locally before opening
- Every PR that adds a feature must include at least one test
- Link the roadmap task in the PR description

### Traceability (required for substantive changes)
Carry the full chain in the commit message and PR body, and append an entry to
`docs/dev/project/traceability.md`:

**requirement/change → architecture/design decision (+ rationale) → implementation (files/functions) → tests → test results (actually run).**

- The "tests → results" link must be a real run (pass/fail counts), never "covered" asserted from a callers-grep.
- Keep the acceptance-criteria table in `docs/dev/project/acceptance-criteria.md` current (requirement ↔ acceptance test).
- Don't build a separate heavyweight matrix that rots — bake the chain into the artifacts that already travel with the change (commits, PRs, the `traceability.md` ledger, the acceptance table).

---

## Rules that are cheap and load-bearing

- **Rebuild BOTH ends for any loopback test** — preamble and frame geometry are shared protocol; a
  one-sided rebuild fails silently with "invalid magic".
- **Test FEC-protected modes WITH their FEC**; there is no useful uncoded rung on a fade.
- **RX/TX seam checklist** — for a feature that must run on every receive or transmit: (1) trace
  top-down from `server::run`'s `rx_ticker`/tx path, not the engine API; (2) put the transform at the
  shared seam (`route_audio_stage(InputCapture)` for RX), never in one caller; (3) prove "all paths"
  with a test that fails without the wiring, never with a callers-grep; (4) add a processed-block
  tripwire and assert it moves on the production path; (5) test through the production entry
  (`accumulate_capture` or the `twin` harness), not only `ChannelSimHarness`/`receive()`.
- **Delete the mechanism first**: if removing the suspected impairment does not move the number, it
  was never the mechanism. A modem that fails at *every* SNR has a bug, not a limitation.

---

## Verification mechanics (mandatory — these ban CONSTRUCTS, not virtues)

Short form; full wording and history in [`docs/dev/verification-mechanics.md`](docs/dev/verification-mechanics.md).

1. **Pipelines never carry verdicts.** A pass/fail claim, exit status or count comes only from
   `scripts/gate.sh` or from `cmd > log 2>&1; rc=$?`. Never `$?` after a pipeline; never
   `${PIPESTATUS[…]}`/`$pipestatus` (zsh dialect trap). A pipe may shape what you read, never what you
   conclude.
2. **The workspace gate is `scripts/gate.sh`**; quote results only from its `GATE:` line. It uses
   `--no-fail-fast` (without it the failure count is a lower bound). `GATE: INVALID` means the tree
   moved during the run — rerun on a quiet checkout, use a `git worktree` for concurrent work. It does
   **not** run the `#[ignore]`d suites (`scripts/slow-tests.sh` does), so `GATE: PASS` does not
   re-prove REQ-QRM-01 or the spectral decode counts.
3. **After editing `gate.sh`, sabotage-verify it**: `scripts/gate.sh --self-test` (failure detection
   only; prints no `GATE:` line), `python3 scripts/lib/trace.py evidence-self-test` (verdict path),
   `scripts/trace.sh --self-test` (graph + yaml probes).
4. **A zero from a filter is a claim about the filter.** Show the same filter matching a known-present
   instance, or write "my filter found nothing".
5. **Reproduction harnesses share their inputs by reference — parameters AND sequences** — or assert
   equality against the product's generator in the DEFAULT test run. A doc-comment fidelity claim is
   banned (a comment cannot fail). Check the regime the fixture runs under, not just its inputs.
6. **Before opening a PR**, print `git log --oneline origin/main..HEAD` and
   `git diff --stat origin/main...HEAD` and confirm both match the PR description.

**What enforces the gate today:** nothing blocks a merge on `gate.sh`. It runs post-merge on `main`
(detection, not prevention), on `release/**` and on manual dispatch; per work plan decision 9, run it
yourself once a day and before every tag. The pre-push hook tests the crates with changed files and,
when `openpulse-core`, `-dsp` or `-modem` changed, all their reverse dependents (#1074's failure mode);
for a leaf crate it only names them. It refuses to run as a stale copy of `.cargo-husky/hooks/pre-push`
(#1448), and the gate checks the same. Required checks on `main`: traceability and the two benchmark
jobs.

---

## Adversarial review (standing rule, set 2026-09-30 — work plan decision 8)

A second model (**Fable**) reviews work before it is trusted. It exists because the expensive
failures here are confident wrong beliefs, not bad code. Background and the reasons behind each item:
[`docs/dev/adversarial-review.md`](docs/dev/adversarial-review.md).

**Mandatory, before it lands:**
- **Wire-format and trait changes**, reviewed as a design **before implementing**.
- **Anything that keys a transmitter** or changes when/how PTT is asserted or released.
- **Eliminations** — any "do not re-attempt" or "ruled out" conclusion before it is written down,
  because a wrong one closes a door silently.

**Recommended, not mandatory:** any new constant in a DSP path, with the question *what inventory
was it fitted to, and what would falsify it?* (several shipped constants were fitted to too few
fixtures — see the reasons doc).

**Optional:** bug fixes with a sabotage-verified test, PR bodies, write-ups, docs.

**How:** one round per PR. Prompt for falsification, never for agreement, and send the apparatus with
the result. **Every prompt also asks: "Does this block Release 1? If not, say so."** Review follow-ups
default to one-line parking entries in the work plan, not new issues (triage rule 3).

**Design artifacts** (`docs/dev/design/**`, new modules, new scripts, the requirements registry) carry
`## Consumer`, `## Prior art` and `## Twins`, each answered with the command that produced it or the
word `UNCHECKED`; `scripts/check-review.sh` enforces this, and every PR body carries a `Review:` line
(`Review: <artifact>` or `Review: none — <why>`).

Review is not the gate: run `scripts/gate.sh` regardless of how the review went.

---

## Key documents by topic

| Topic | Document |
|---|---|
| Release 1 plan, milestones, decisions | `docs/dev/project/workplan.md` |
| What 1.0 requires | `docs/dev/project/release-1.0-criteria.md` |
| Mode/FEC ladder (`hpx_hf` rung table) | `docs/mode-fec-ladder.md` |
| Protocol & handshake wire format | `docs/dev/design/protocol-wire-spec.md` |
| HPX state machine / waveform design | `docs/dev/hpx-session-state-machine.md`, `docs/dev/design/hpx-waveform-design.md` |
| On-air plan and signal-chain gates | `docs/dev/onair-execution-plan.md`, `docs/dev/onair-signal-chain-verification.md` |
| Regulatory compliance | `docs/regulatory.md` |
| Requirements and traceability ledger | `docs/dev/requirements.md`, `docs/dev/project/traceability.md` |
| Architecture | `docs/dev/design/architecture.md` |
| External modem/DSP references | `docs/dev/research/references.md` |
| Lessons and archetypes | `docs/dev/lessons-and-archetypes.md` |
| ARDOP / VARA / PACTOR / WSJT-X / JS8Call research | `docs/dev/research/` |
| CLI usage | `docs/cli-guide.md` |
| Roadmap history | `docs/dev/project/roadmap.md` |
| Agent safety rules | `AGENTS.md`, `docs/dev/AGENTS.md` |
