---
project: openpulsehf
doc: docs/dev/reviews/review-1408-psk-file.md
status: resolved
last_updated: 2026-09-19
---

# Design review — `[control_security] psk_file`, or close #1408 (#1408)

Nothing is built. This asks whether ~20 lines are worth adding to a feature no deployment uses, and
**"close it" is the expected answer unless something below survives.**

## Consumer

- `crates/openpulse-daemon/src/server.rs:2083` `load_control_psk()` — reads `OPENPULSE_CONTROL_PSK`
  (64 hex chars) and nothing else; `:472` emits `inert_psk_key_id_warning` for the keystore knob
  that #1234 left deliberately inert.
- `apps/openpulse-panel/src/transport.rs:7` `control_psk_from_env()` — the second reader, same env
  var, `cfg(not(wasm32))`.
- Nothing else. `grep -rln OPENPULSE_CONTROL_PSK --include=*.rs .` returns exactly three files.

## Prior art

- **#1234's deferral (2026-09-19)** is the governing decision and it rejected a keystore reader for
  this same PSK. Its findings carry over: the master password may not be written to disk in
  plaintext (REQ-CTL-04), env→env buys nothing, and the Ed25519 station seed — the more valuable
  secret — already sits in plaintext at 0600 (`config/src/lib.rs:910-955`).
- `crates/openpulse-config/src/secret_file.rs:15,35` — `validate_owner_only` / `enforce_owner_only`,
  already used by the keystore (`keystore/src/lib.rs:75,141`) and the subject of REQ-CTL-05.
- **REQ-CTL-05's own text already names a "PSK file"** among the files that must be owner-only
  (`requirements.md:160-165`) — so the concept is registered and unimplemented.
- `control-channel-security.md:93` settles the panel: OS keychain first, in-UI prompt as fallback.

## Twins

- **`OPENPULSE_CONTROL_PSK` stays supported**, so this adds a second source and needs a precedence
  rule — the trap #1234 hit.
- **The panel** is the sibling reader; its path is already decided, so this is daemon-only.
- **`load_identity_from`** already reads a 0600 secret from a configured path. If `psk_file` is
  worth having, it is the same shape as something the daemon does today — which is either the
  strongest argument for it or evidence that it adds nothing new.

## Prompt

Test this rather than confirm it, and **recommend closing if that is the honest answer** — #1409 and
#1403 both ended that way this week and neither was a loss.

### A. Is it worth building at all?

Against it: **#1234 established that no deployment has ever set the PSK** — every on-air rig binds
loopback, where it is discarded by policy. So this is a config surface, a parser branch, a
precedence rule and a doc for a feature with zero known users. The "20 lines" figure also excludes
the precedence logic, the refusal path, the config-template entry and the tests.

For it: the only *new* capability it unlocks is systemd `LoadCredential=` /
`LoadCredentialEncrypted=`, which decrypts a secret to `$CREDENTIALS_DIRECTORY/<name>` on a
non-persistent tmpfs, encrypted at rest to the host key or TPM2. That is a **path**, so it needs
exactly this field and nothing else.

**What I most want tested: is that systemd claim real and is it reachable here?** I have not
verified that `LoadCredentialEncrypted` is available on the deployment targets (a Raspberry Pi OS /
Debian systemd, and AerynOS on the dev host), nor that an amateur-radio operator running a daemon
from a shell or a cron line would ever use it. If the credential story does not land, the proposal
has no argument left.

### B. Precedence

With both sources live, one must win. #1234's review concluded that a present non-selected source
should be a **refusal to start even when the values are equal**, because the equal case is the
pre-image of the dangerous one. Does that hold here, where there is no `psk_source` selector and the
field's presence is itself the selection? Or is "file wins, warn on env" enough — noting that Noise
NNpsk0 fails loudly on a mismatch, so the silent case is narrow.

### C. Does it need a requirement id?

It implements a clause of REQ-CTL-05 that nothing implements. CTL-05 is `enforced` and currently
8/303 reachable (#1405), bound in `openpulse-config`. Adding a daemon-side implementation of one of
its clauses may widen that scope without widening what its binding can reach — the #1405 shape
again. Is the right answer a binding, a new id, or neither?

### D. The thing I am most likely wrong about

I claimed in #1408 that this is "the same security tier with the moving parts removed". Check that:
a file read at startup through `validate_owner_only` versus an env var whose backing store is
already a 0600 file. If the tiers are genuinely identical and the systemd story does not hold, then
this is pure churn and should be closed.

## Verdict

Reviewed 2026-09-19. **CLOSE — record the option, do not build.** The proposal's load-bearing
sentence is false, and with it gone nothing left is a capability.

**A — refuted on both halves.**

- **The systemd claim is wrong in the way that matters.** I wrote that `LoadCredential=` delivers a
  *path*, "so it needs exactly this field and nothing else". It does not: a credential composes with
  the **existing** env reader today, with zero code —
  `ExecStart=/bin/sh -c 'OPENPULSE_CONTROL_PSK=$(cat "$CREDENTIALS_DIRECTORY/control-psk") exec …'`.
  So `psk_file` does not *unlock* systemd credentials; it removes a one-line wrapper. The sole
  claimed new capability evaporates.
- **No systemd deployment exists in this repo.** Verified independently: **zero** `.service` /
  `.timer` / `.socket` files and no packaging directory; every launch is `nohup … &` over ssh as an
  interactive user (6 scripts), `deploy-rpi-pair.sh` rsyncs binaries to `~/bin` and stops, and
  control binds `127.0.0.1`. **Nothing in the repo sets `OPENPULSE_CONTROL_PSK`** (control: the same
  filter finds the reader in 3 Rust files), confirming #1234's finding from a second direction.
- Two further qualifiers: the daemon needs the operator's PipeWire session, so the natural unit is a
  **user** unit — and encrypted credentials for per-user managers arrived only in systemd v256
  (trixie yes, the bookworm legacy image no). And a Pi has no TPM2, so "encrypted at rest" means the
  host key on the same SD card: protection against a non-root same-host reader, which `0600` already
  provides, and none against card theft — beside an Ed25519 station seed that is plaintext at 0600.

**D — my "same tier" claim HOLDS, and that is what closes it.** Against every threat that matters
(same-uid reader, root, media theft) `/proc/<pid>/environ` and a `0600` file are the same tier. The
two real deltas are small and neither is a capability: env is **inherited** by children (measured —
the daemon's only spawn is `nvidia-smi` per GPU tick, absent on a Pi), and `validate_owner_only`
would refuse a `0644` file where systemd accepts a `0644` `EnvironmentFile` silently. So "same tier
with the moving parts removed" is true relative to the *keystore*; relative to **env** it is the same
tier with moving parts **added** — a path, a permission check, a precedence rule.

**The "~20 lines" was the parser branch only.** Honest inventory: both config templates' comments
become false, `inert_psk_key_id_warning` says "`OPENPULSE_CONTROL_PSK` only" and a test asserts that
substring, the fail-closed error names only the env var, the parser embeds the variable name in its
error strings, plus both-present refusal, missing-path error, empty-string-unset, docs in four places
in the book, and tests — `load_control_psk` has **zero** tests today (confirmed: no test-file
references), because env is process-global, so the file path needs a seam. Realistically **150–250
lines across ~8 files**.

**If it were ever built** (recorded so the next attempt starts here): refuse on both-present — but
*not* for #1234's reason, which does not transfer. Under **file-wins** a stale `export` is inert, so
the dangerous rotation pre-image exists only under env-wins; refusal is right because it is the
cheapest rule and legible at start, whereas Noise's loudness is on the *client* side. Also: a set
path to a missing file must be `Err`, never `Ok(None)` (the `FileStore::open`-creates-empty
fail-open), and `psk_file = ""` must mean unset, per the `rig_file = ""` precedent.

**C — no new id.** A daemon-side CTL-05 binding would pull CAP-68's linksec files into reach, moving
"reachable" from 8 toward ~246 with the kill count unchanged — the "reachable and MISSED, more
honest, no better" correction #1234 already recorded. CAP-68's `code:` also omits `server.rs`, so the
new loader would sit outside any capability's scope.

**Reopen condition, stated so it is checkable:** a systemd-managed deployment exists in this repo (a
unit file or a packaging target) **and** a non-loopback bind is in use. Even then, try the zero-code
`ExecStart` wrapper first and build `psk_file` only if the env hop is shown to matter.
