---
project: openpulsehf
doc: docs/dev/reviews/review-1234-keystore-writer.md
status: resolved
last_updated: 2026-09-19
---

# Design review — give the keystore a writer and a reader (#1234, wiring half)

#1234's retirement half shipped in #1391. What remains is the wiring, which the issue's last comment
recorded as **blocked**: wiring the daemon to READ the keystore while nothing can WRITE to it ships a
config surface an operator cannot populate. Option 1 of that comment — *build the writer in the same
change* — is the only one that leaves the feature usable. This is that design, before implementation.

## Consumer

- `crates/openpulse-daemon/src/server.rs:2083` `load_control_psk()` — reads `OPENPULSE_CONTROL_PSK`
  and nothing else. `:472` emits `inert_psk_key_id_warning`, which exists solely to make
  `psk_key_id`'s inertness audible (#1234).
- `apps/openpulse-panel/src/transport.rs:7` `control_psk_from_env()` — **a second reader**, on the
  same env var. Verified by `grep -rln OPENPULSE_CONTROL_PSK --include=*.rs .`, which returns
  exactly three files (the third is the config template's prose).
- `crates/openpulse-cli` has **no** keystore surface at all — `grep -rn 'keystore\|Keystore'
  crates/openpulse-cli/src/` returns nothing. That absence is the blocker.

## Prior art

- `crates/openpulse-keystore/src/lib.rs:73` `FileKeystore::open(path, master)` and `:97` `get` —
  the store exists and is tested; it has no production caller.
- `crates/openpulse-config/src/secret_file.rs` — `validate_owner_only` / `enforce_owner_only`,
  already used by the keystore itself (`lib.rs:75,141`) and the subject of REQ-CTL-05.
- `KeychainStore` is behind `--features keychain`, which the `--no-default-features` gate never
  compiles (#1234's original blocker, and #1380's class).
- `store.rs:99` `SecretStore` trait with `FileStore` and `KeychainStore`, plus `select_backend` /
  `probe_keychain` from #1391.

## Twins

- **The panel is the sibling front-end.** Wiring only the daemon leaves the panel reading the env
  var — the asymmetry shape this repo has been bitten by repeatedly (ARDOP/KISS vs daemon).
- **`OPENPULSE_CONTROL_PSK` remains a supported path** either way; this adds a second source, it does
  not remove the first. Two sources need a stated precedence.
- **REQ-CTL-04 is `unwired`** ("bound and passing, but nothing consumes the capability yet"). A
  production consumer flips its classification, which is a `trace.py` join outcome, not just prose.

## Prompt

Test this rather than confirm it — especially the security argument in Q1, which I am least sure of.

### Q1 — the one that decides whether this is worth doing: where does the daemon get the master?

`FileKeystore::open(path, master)` requires an operator master password. **An unattended daemon
cannot prompt.** So wiring the file store means the daemon needs the master from somewhere, and the
candidates each have a cost:

1. **A second env var** (`OPENPULSE_KEYSTORE_MASTER`). Simplest. But today the PSK sits in the
   environment; afterwards the *master* sits in the environment and unlocks a file containing the
   PSK **and every other secret**. `/proc/<pid>/environ`, a crash dump or a process listing exposes
   one or the other. **Is that a security improvement or a lateral move that widens the blast
   radius?** I genuinely do not know, and I do not want to ship a "more secure" story that is not.
2. **An owner-only master-password file**, validated by `secret_file::validate_owner_only` — which
   already exists, is already enforced, and is REQ-CTL-05's subject. Unattended-safe, composes with
   machinery that is already tested. Cost: the master is at rest on disk, protected by file
   permissions rather than by a human.
3. **Keychain backend only** — no password at all, which is the genuinely better story. But it is
   feature-gated off in the gate, so the wiring would be untested by CI (#1380's class), and on a
   headless server there is usually no secret service to talk to.

My inclination is (2), because it is unattended-safe and reuses enforcement that is already proven.
**Tell me if (1) is honest enough to prefer for its simplicity, or if the whole thing is a lateral
move and the right answer is option 3 of the issue — defer the wiring and keep today's honest
warning.** That last outcome is acceptable.

### Q2 — scope: does the panel get it too?

The writer (`openpulse-cli keystore set|get|list|delete`) is needed regardless. The reader could be
daemon-only, or daemon + panel. Daemon-only leaves a sibling asymmetry; both doubles the surface and
the panel is an interactive app that *could* prompt, so it may warrant a different answer rather than
the same one. Which?

### Q3 — precedence between two sources

With both `OPENPULSE_CONTROL_PSK` and a keystore-backed `psk_key_id` live, one must win. Env-wins is
the familiar override convention; keystore-wins matches "the config field is what you set". A silent
precedence is how an operator ends up authenticating with a PSK they did not think they were using.
Should a conflict be a **refusal to start** rather than a precedence at all?

### Q4 — what it is worth, stated so it can be argued down

Beyond #1234: a daemon dependency on `openpulse-keystore` makes the package non-dormant, which
flips REQ-CTL-04 out of `unwired` and makes REQ-CTL-01/02's 57 keystore mutants reachable (#1405's
three blocked cases). **That is a side effect and must not become the justification** — wiring a
feature to improve a metric is the shape this week already caught twice. If the security answer to
Q1 is "lateral move", the metric payoff does not rescue it.

## Verdict

Reviewed 2026-09-19. **DEFER — build none of it.** Keep `inert_psk_key_id_warning` as it stands.

**Q1 — all three options rejected.**

- **My preferred option (2) is forbidden by the requirement it would implement.** REQ-CTL-04's bullet
  ends: *"The master password must never be written to disk in plaintext."* That sentence is **one
  line below** the text I quoted in Prior art. `control-channel-security.md:59` says the same: the
  master *"is prompted (or supplied via a one-shot env var for headless automation)"*. Not a
  judgement call — a text conflict, decisive on its own.
- **Option (1) is a lateral move, and the thread already said so.** #1234's second comment:
  *"For a single PSK on a headless daemon a file keystore is indirection without security — the
  Argon2id master password must itself come from an env var or a prompt, so env→env buys nothing.
  The keystore earns its place only via REQ-CTL-03 (an OS keychain…) or multi-secret storage."*
  Neither precondition holds: no keychain on a headless Pi (and it is not compiled into the gated
  build), and this design moves exactly one secret. So "unlocks every other secret" was my own
  hypothetical.
- **The threat-model argument fails.** "Environment" is a delivery mechanism whose backing store is
  already a 0600 file (a systemd `EnvironmentFile`, a start script); `/proc/<pid>/environ` is 0400
  and ptrace-scoped. Same tier as a 0600 master file. And the **Ed25519 station seed — the more
  valuable secret, signing all 13 registered domains — already sits in plaintext at 0600**
  (`config/src/lib.rs:910-955`). AEAD-wrapping the PSK while the identity key lies in the clear
  beside it protects nothing.

**Costs the design failed to list**, each measured by review: it **un-deflates audit finding B4** —
`FileKeystore::save` truncates without temp+rename or fsync, deflated *because nothing consumes the
keystore*; give it a consumer and an interrupted `keystore set` on a Pi's SD card leaves a file
`open` rejects, so the daemon refuses to start with the PSK gone (an env var cannot be half-written).
B3 (no zeroize) un-deflates identically. A **C library enters the release binaries** (`keyring` →
`dbus-secret-service` → `libdbus-sys`), which the same design doc rejected OpenSSL for. Argon2 costs
19 MiB at daemon start, unbudgeted. No prompt dependency exists. And **no deployment has ever set the
PSK**: every on-air rig binds loopback, where it is discarded by policy.

**Q3 — the design had no activation rule at all**, which is worse than a bad precedence.
`psk_key_id` defaults to `"control-psk"`, so it is *always* set and liveness cannot be inferred from
it; and `FileStore::open` **creates an empty in-memory store when the path is absent**
(`store.rs:113-121`), so a mistyped path yields "key missing" rather than an error — fail-open by
construction. If ever built: an explicit `psk_source = "env" | "keystore"`, `FileKeystore::open`
(which errors on absent) never `FileStore::open`, and a present non-selected source **refuses to
start even when the values are equal** — the equal case is the pre-image of the dangerous one
(rotate the keystore, forget a stale `export`, and the old PSK still works while the operator
believes they rotated). Note my "silent" framing was overstated: Noise NNpsk0 fails loudly on a
mismatch; the genuinely silent case is that double-stale rotation.

**Q2 — I re-opened a resolved decision.** `control-channel-security.md:93` already settles the panel:
*"read the OS keychain first, prompt in-UI only as a fallback."* Divergence per front-end is correct
here — the ARDOP/KISS-vs-daemon lesson is about *safety properties* that must hold everywhere, and
provisioning a shared secret is not one.

**Q4 — the design does not lean on the metric**, confirmed. But my parenthetical misstated it, and
the correction cuts against me: a daemon→keystore dependency would help **CTL-01/02 only, not
CTL-05** (which is bound in `openpulse-config`), and even then those 57 mutants would go from
"unreachable by construction" to "reachable and MISSED", because `control_auth.rs` never calls the
keystore. More honest, no better. *(Review's own list of #1405's blocked three was itself inverted —
it named FUN-11/FUN-10, which are the two already fixed — but its substantive correction stands.)*

**What would reopen this, and it is not a keystore.** REQ-CTL-05's text already lists a *"PSK file"*
among secret files. `[control_security] psk_file = "<path>"` — read once at start through
`validate_owner_only`, 64 hex, the same parser as today — is ~20 lines, no crate, no KDF, no writer,
at the same security tier with the moving parts removed. It also composes with the real headless
upgrade on Linux, systemd `LoadCredential=`/`LoadCredentialEncrypted=`. That is a **new proposal**
and needs its own review; it is named here so this deferral is not read as "nothing can improve it".
