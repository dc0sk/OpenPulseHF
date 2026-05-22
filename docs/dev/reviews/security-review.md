---
doc: docs/dev/reviews/security-review.md
date: 2026-05-22
status: initial
---

# Security Review

## Summary

One medium-severity CVE in a transitive dependency (`rsa` via `sqlx-mysql`). No unsafe
blocks in production code. Cryptographic paths are sound. PTT fail-safe holds on the CLI
transmit path but has a gap in the daemon. Key material is not logged. LZHUF decompression
is correctly bounded. One input-validation gap in the ARDOP length prefix.

---

## Findings

### SEC-01 — CVE in transitive `rsa` dep (RUSTSEC-2023-0071) · Severity: Medium

**File:** `Cargo.lock` (transitive: `rsa 0.9.10` ← `sqlx-mysql` ← `sqlx` ← `pki-tooling`,
`openpulse-cli`)

`rsa 0.9.10` is vulnerable to the Marvin Attack (timing side-channel key recovery,
CVSS 5.9). No fixed upgrade is available upstream. The `rsa` crate is not used directly
by OpenPulseHF; it enters via `sqlx-mysql` which is pulled in for the PKI web service.

**Risk:** Affects `pki-tooling` only (the PKI web service). The HF modem and daemon
do not use RSA. However `openpulse-cli` transitively depends on it via the PKI feature.

**Recommendation:** Pin `sqlx` to the MySQL-free feature set in `openpulse-cli` to
remove the transitive dependency, or accept the risk with a documented exception since
the affected code path (MySQL RSA authentication) is not used in the field deployment.

---

### SEC-02 — PTT transmitter left keyed on panic in daemon · Severity: Low

**File:** `crates/openpulse-daemon/src/main.rs:162–191`

The CLI transmit path (`crates/openpulse-cli/src/commands/transmit.rs:16–21`) correctly
pairs `assert_ptt()` and `release_ptt()`, calling release even on TX error via a
`let rel_result = …` pattern that always executes.

The daemon's `apply_command_to_engine` does not hold a PTT assertion across a transmit
call — PTT is only asserted/released for `PttAssert`/`PttRelease` commands, which the
operator sends explicitly. If the operator sends `PttAssert` and the daemon task panics
before a `PttRelease` command arrives, the transmitter remains keyed until the process
terminates. There is no timeout-based PTT watchdog.

**Recommendation:** Add a PTT watchdog task (e.g. `tokio::time::timeout` guarding the
`PttAssert` state) or a max-transmit-time guard in `RuntimeControlState`.

---

### SEC-03 — ARDOP data port: no per-frame length cap · Severity: Low

**File:** `crates/openpulse-ardop/src/data.rs` (data port framing)

The ARDOP data port uses a `u16 BE` length prefix. A malicious client can send
`length = 65535` followed by a partial body, causing the bridge worker to block
waiting to read up to 64 KB into a heap allocation before any timeout. There is no
per-frame size cap below the `u16::MAX` limit.

**Recommendation:** Reject frames above a reasonable maximum (e.g. 4096 bytes for
modem payloads) at the data port decode layer and close the connection.

---

### SEC-04 — Key material: no logging found · Severity: Pass

**Files:** `crates/openpulse-core/src/handshake.rs`,
`crates/openpulse-core/src/pq_handshake.rs`,
`crates/openpulse-freedv-auth/src/`

No `tracing::` or `println!` calls were found in any cryptographic path. Signing keys,
shared secrets, and LLR session data are not logged. The `SigningKey` type from
`ed25519-dalek` does not implement `Debug`, providing an additional safety net.

---

### SEC-05 — Unsafe code: none in production paths · Severity: Pass

`grep -rn "unsafe {" crates/ plugins/ --include="*.rs"` (excluding tests and cfg guards)
returned no results. All DSP and protocol code is safe Rust.

---

### SEC-06 — LZHUF decompression size cap is present · Severity: Pass

**File:** `crates/openpulse-b2f/src/compress.rs:42`

`decompress_lzhuf` caps `orig_len` at `16 MiB` before allocating the output buffer.
`decompress_lzhuf_winlink` applies the same cap. A malformed 4-byte length field
cannot cause an OOM allocation.

---

### SEC-07 — Config parsing does not panic · Severity: Pass

**File:** `crates/openpulse-config/src/lib.rs`

`load()` returns `Result` and propagates TOML parse errors to the caller. All
daemon/CLI entry points call `unwrap_or_default()` or explicitly handle the error,
so a malformed config file causes a graceful error message rather than a panic.

---

### SEC-08 — `unwrap()` in library test code only · Severity: Pass

All `unwrap()` calls found in library crate source files (`openpulse-core`,
`openpulse-modem`, `openpulse-audio`, `openpulse-dsp`, `openpulse-radio`) are inside
`#[cfg(test)]` modules or inline test functions. One `expect()` in
`crates/openpulse-core/src/sar.rs:164` is in a reassembly path that is provably
reachable only when `received == total`, making it a documented invariant rather than
a panic hazard.

---

## Action Items

| ID | Severity | Action |
|---|---|---|
| SEC-01 | Medium | Audit `sqlx` feature flags in `openpulse-cli` to remove `sqlx-mysql` and the `rsa` dep |
| SEC-02 | Low | Add PTT watchdog / max-TX-time guard to daemon `RuntimeControlState` |
| SEC-03 | Low | Cap ARDOP data port frame size at ≤ 4096 bytes |
