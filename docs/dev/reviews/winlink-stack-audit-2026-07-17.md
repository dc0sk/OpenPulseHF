---
project: openpulsehf
doc: docs/dev/reviews/winlink-stack-audit-2026-07-17.md
status: living
last_updated: 2026-07-17
---

# Winlink network stack loose-ends audit (2026-07-17)

Multi-agent audit (7 finders → per-finding adversarial verification → synthesis), scoped to
`openpulse-b2f`, `openpulse-b2f-driver`, `openpulse-gateway`. 33 agents, 23 confirmed survivors.
The one high-severity live DoS (unbounded proposal accumulation + FC-flood hang) was fixed in the
same session; the remainder is a tracked hardening backlog.

# Winlink Network Stack — Security & Robustness Audit Report

**Scope:** `openpulse-b2f`, `openpulse-b2f-driver`, `openpulse-gateway`
**Threat model:** CMS server and peer TNC are untrusted; every byte from the TcpStream is attacker-controlled.

---

## (a) Executive summary

**The stack is broadly solid — no confirmed panics, no memory-safety bugs, no auth bypass, no RCE.** The core hardening a prior audit pass installed (16 MiB gzip decompress-bomb cap, u16-bounded `DataPort` framing, EOF/error propagation instead of `unwrap`) genuinely holds. Every parser reviewed (`banner::decode`, `frame::decode`, `header::decode`) is panic-safe on adversarial input by inspection.

There is exactly **one real, live, unfixed DoS**: the prior "unbounded B2F proposal accumulation" fix (`MAX_PROPOSALS`) was applied to the wrong variable. It caps whether a proposal is *accepted*, not whether it's *retained* — so an untrusted CMS/peer that streams `FC` frames without ever sending `FF` drives an infinite read loop that grows memory without bound. This is a single one-line root cause that 4 independent audit passes rediscovered from different angles; it should be fixed once, at the `session.rs:231` seam.

Below that, there's a cluster of genuine but lower-impact **hang/timeout gaps** (per-syscall timeouts don't bound a slow-drip peer; one code path clears a timeout and never restores it; the client-side command port lacks the line-length cap its server-side ARDOP twin already got) and one **memory-amplification** gap (no aggregate cap across a session's decompressed messages, 32 × 16 MiB ≈ 512 MiB worst case). Everything else — dead Type C code, a discarded banner field, an unproduced error variant, and a long tail of missing adversarial tests on already-correct code — is low-severity hygiene, not exploitable today.

**Verdict: fix the proposal-Vec bug (top finding) before anything else touches this stack; the rest is a reasonable hardening backlog, not an emergency.**

---

## (b) Ranked top findings

| # | Finding | Severity | Fix sketch |
|---|---|---|---|
| 1 | Unbounded `proposals` Vec — `MAX_PROPOSALS` gates the Accept/Reject answer, not the `push` | **High** | Gate the push itself: `if self.proposals.len() < MAX_PROPOSALS { self.proposals.push(...) }` |
| 2 | `CmdPort::read_line` has no length cap (client-side twin of an already-fixed server-side bug) | Medium | Wrap the reader in `.take(MAX_CMD_LINE)` — mirror `openpulse-ardop`'s `read_capped_line` |
| 3 | No session-aggregate decompression cap (32 × 16 MiB ≈ 512 MB transient) | Medium | Track a running decompressed-byte total in `receive_data`, cap it at the seam |
| 4 | `run_irs` clears the cmd-port timeout for `wait_for("CONNECTED")` and never restores it | Medium | Restore `set_timeout(Some(...))` before the closing `wait_for("DISCONNECTED")` |
| 5 | Read timeout is per-syscall (`SO_RCVTIMEO`), not per-operation — a 1-byte-per-29s drip never times out | Medium | Track a wall-clock operation deadline, shrink the per-read timeout as it elapses |
| 6 | `header::decode` accumulates unbounded `To:`/`File:` lines (~4–7× amplification within the 16 MiB cap) | Low | Cap the number of `To:`/`File:` lines accepted before push |
| 7 | `DataPort::new`/`B2fDriver::new` construction seam has no default timeout (latent public-API foot-gun; no live caller hits it) | Low | Require/default a timeout at `DataPort::new`, not only in `connect()` |
| 8 | Type C (LZHUF) send path is dead code with self-documented unverified external-Winlink compatibility | Low | Wire + validate against a real RMS Express capture, or delete and correct the "shipped" claim in docs |
| 9 | Misc. dead/inert code: banner `version`/`session_key` parsed-and-discarded; `DriverError::Aborted` never constructed; driver `run_iss` missing the gateway's "all proposals rejected" guard | Low | Cosmetic / correctness cleanups, no security impact |
| 10 | Test-coverage gaps on already-correct code: no LZHUF bomb test, no silent-peer timeout test, no `DataPort` framing robustness tests, no malformed-banner/frame/header tests, no tamper test, gateway tested only against a cooperative mock CMS | Low | Add the named adversarial regression tests; nothing to fix in the guarded code itself |

---

## (c) Full detail by area

### 1. Unbounded proposal accumulation — [confirmed] — **the one real live DoS**

**File:** `crates/openpulse-b2f/src/session.rs:226-231`

```rust
let answer = if self.proposals.len() < MAX_PROPOSALS { Accept } else { Reject };
...
self.proposals.push(Proposal { ... });   // ← unconditional, every FC
```

`MAX_PROPOSALS` (32) only picks the `FsAnswer`; the `push` runs regardless. The doc-comment above it explicitly (and now incorrectly) claims the cap bounds what a peer can "receive, decompress, **and retain**."

Consuming loops never terminate on an `FC`-only stream: `handle_line` returns `Ok(vec![])` for `FC` (only `FF`/`FQ` produce a response or set `Done`), and both `run_irs` (`crates/openpulse-b2f-driver/src/lib.rs:135-146`) and `irs_receive` (`crates/openpulse-gateway/src/main.rs:192-218`) loop until a non-empty response or `is_done()`. A malicious/buggy CMS or peer that streams `FC C <mid> <size> <date>\r` forever, without an `FF`, hangs the session **and** grows `self.proposals` (holding attacker-sized `mid`/`date` strings, up to ~64 KB per `DataPort` frame) without bound.

The existing B-2 regression test (`crates/openpulse-b2f/tests/b2f_integration.rs:358-380`) only asserts `accepted_count() == 32`, never `proposals.len()`, so this was invisible to CI. Reachable from the fast TCP gateway path (`cms.winlink.org:8772`), not just RF-rate.

**Fix:** one-line — gate the push behind the same cap. Add a test streaming `N ≫ 32` FC lines and assert `proposals.len()` stays bounded and the loop terminates (or errors) rather than hanging.

---

### 2. Command-port read has no line-length cap — [confirmed]

**File:** `crates/openpulse-b2f-driver/src/cmd.rs:34-47` (used by `wait_for`, `cmd.rs:50-57`)

`CmdPort::read_line` calls `BufReader::read_line` with no cap; `SO_RCVTIMEO` bounds each individual `read()` syscall, not the whole line, so a newline-starved byte drip grows the `String` unbounded → OOM. `wait_for` inherits this and is on the hot path of both `run_iss` and `run_irs` (`lib.rs:79,81,107,120,122,126,158-159`), mostly called with **no** timeout set at all, before any data-channel framing exists to fall back on.

Notably, the identical class was already found and fixed on the **server** side: `crates/openpulse-ardop/src/command.rs:16-27` uses `read_capped_line` with `.take(MAX_CMD_LINE=4096)` specifically because "`read_line` alone grows its destination without limit." The client-side `b2f-driver` twin never got the same fix.

**Fix:** mirror `read_capped_line` — a real ARDOP status line is well under 100 bytes, so a 4 KiB cap is safe.

---

### 3. No session-aggregate decompression cap — [confirmed]

**File:** `crates/openpulse-b2f/src/session.rs:35` (`MAX_PROPOSALS`), `crates/openpulse-b2f/src/compress.rs:62` (`MAX_UNCOMPRESSED = 16 MiB`), `session.rs:278` (`receive_data`)

Each individual decompression is correctly capped at 16 MiB, and each accepted proposal is correctly capped in count at 32 — but nothing caps the **product**. `run_irs` (`b2f-driver/src/lib.rs:150`) and the gateway (`gateway/src/main.rs:224`) retain every decompressed message in one `Vec` and return them all together. A CMS that gets 32 highly-compressible ≤64 KB Type-D/C blobs accepted (each inflating to just under the 16 MiB per-message ceiling, without tripping the reject guard) drives ~512 MB of transient allocation from ~2 MB of wire traffic — a plausible OOM on the Raspberry Pi target the repo explicitly cares about.

**Fix:** track a running decompressed-byte total in `receive_data` (the single shared seam both driver and gateway already call through), cap it there so both callers inherit the fix by construction.

---

### 4. `run_irs` disables the command-port timeout and never restores it — [confirmed]

**File:** `crates/openpulse-b2f-driver/src/lib.rs:125,127,158-159`

```rust
self.cmd.set_timeout(Some(timeout))?;  // for wait_for("CONNECTED")
...
self.cmd.set_timeout(None)?;           // cleared, never restored
...
self.cmd.send("DISCONNECT")?;
self.cmd.wait_for("DISCONNECTED")?;    // now runs with NO timeout
```

A peer/TNC that finishes the data phase but holds the command connection open without ever emitting `DISCONNECTED` hangs `run_irs` permanently at teardown. (`run_iss` doesn't have this bug — it never clears the timeout.)

**Fix:** restore `set_timeout(Some(timeout))` immediately after the `CONNECTED` wait, before the closing handshake.

---

### 5. Read timeout is per-syscall, not per-operation — [confirmed]

**File:** `crates/openpulse-b2f-driver/src/data.rs:39` (`recv_frame`'s `read_exact`), `cmd.rs:36` (`read_line`)

`SO_RCVTIMEO` resets on every successful partial read. A peer that drips one byte every ~29 s across a 64 KB `DataPort` payload (or a long command line) keeps every individual `read()` succeeding forever, pinning the session in a single `recv_frame`/`read_line` call for potentially days. A peer that stops entirely *is* handled correctly (timeout fires); only a steady drip evades it. Reachable directly through the gateway's connection to `cms.winlink.org` (or any `--host`), which has only this 30 s per-read timeout.

**Fix:** replace per-syscall timeouts with a cumulative wall-clock deadline for the whole read operation.

---

### 6. `header::decode` unbounded `To:`/`File:` accumulation — [confirmed]

**File:** `crates/openpulse-b2f/src/header.rs:59-94` (push at lines 70, 83)

No per-field count cap on repeated `To:`/`File:` lines. Bounded by the 16 MiB decompress cap upstream, but a message body of minimal 6-byte `To:a\r\n` lines yields ~2.8M `String` entries — roughly a 4–7× memory amplification over the nominal 16 MiB cap, compounding across up to 32 retained proposals in `run_irs`'s `decoded` Vec.

**Fix:** cap the number of `To:`/`File:` lines accepted (a few hundred is generous) before pushing.

---

### 7–10. Lower-severity items — [confirmed], all low

- **`DataPort::new`/`B2fDriver::new` construction seam has no default timeout** (`data.rs:14-17`, `lib.rs:46-51`) — a latent public-API foot-gun (a caller building from a pre-connected socket via the public `new()` inherits an unbounded blocking read), but every shipped production caller (`connect()`, the gateway) sets a timeout before constructing, so nothing live is exposed today.
- **Type C (LZHUF) is dead code with unverified external compatibility** — `queue_message_type_c` (`session.rs:91`) and the BE-prefix `compress_lzhuf` (`compress.rs:66`) have no production caller; both production senders use gzip Type D. The code's own doc comment already flags the LH5-vs-Okumura bitstream as unverified — this is doc-drift in `CLAUDE.md`/traceability calling it "shipped," not a hidden risk in the code.
- **WL2K banner `version`/`session_key` parsed then discarded** (`session.rs:160,172`) — cosmetic; nothing downstream consumes it, and `session_key` isn't wired to any actual secure-login mechanism in this codebase, so nothing load-bearing is lost.
- **`DriverError::Aborted` never constructed** (`lib.rs:35`) — dead error-taxonomy variant; a remote abort surfaces as `Timeout` or `Ardop("command port closed")` instead.
- **Driver `run_iss` lacks the gateway's "all proposals rejected" guard** (`lib.rs:101-104` vs `gateway/main.rs:172-175`) — a correctness/observability parity gap (silent false-success on full rejection), not a security issue.
- **Test-coverage gaps on code that is already correct**: no LZHUF decompress-bomb test (gzip has one, `b2f_integration.rs:342`; the LZHUF cap at `compress.rs:88/125` is real but unguarded by CI), no silent-peer timeout test, no `DataPort` framing robustness tests (truncated/zero-length/oversized frames), no malformed-banner/frame/header negative tests, no tamper test on an accepted compressed blob, and the gateway's only test (`main.rs:333`) uses a fully cooperative mock CMS. None of these hide a live bug — they're regression insurance against future edits to code that is currently panic-safe and correctly bounded.

---

### What's genuinely solid (confirmed by this audit, not just asserted)

- `DataPort` framing is correctly u16-bounded (≤65535 B/frame) with clean EOF/truncation error propagation — no panic, no unbounded allocation from a single frame.
- The gzip decompress-bomb cap (16 MiB, `compress.rs`) and the LZHUF equivalent are both correctly implemented and reject oversized claimed-lengths before allocating.
- `banner::decode`, `frame::decode`, `header::decode` are all panic-safe on adversarial byte sequences — every slice is guarded, no unchecked indexing.
- No `unwrap()`/`expect()`/panic-capable indexing was found on any attacker-facing production path in `openpulse-b2f` or `openpulse-b2f-driver`.
