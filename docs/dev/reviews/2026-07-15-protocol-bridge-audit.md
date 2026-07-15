---
doc: docs/dev/reviews/2026-07-15-protocol-bridge-audit.md
date: 2026-07-15
status: resolved
scope: ARDOP TCP TNC, KISS/AX.25 TNC, B2F/Winlink protocol + CMS gateway (untrusted-input surface)
---

# Protocol-bridge / untrusted-input audit

Three-finder adversarial sweep (refute-by-default, source-verified) of the network-facing protocol
bridges — the code that parses untrusted bytes from TCP clients, on-air peers, and the Winlink CMS. The
lens throughout: can a malicious peer cause a panic, an unbounded allocation, a wedge, or an
unauthorized action? Three findings fixed; the KISS/AX.25 path came back clean.

## Fixed

### A-1 — [HIGH] ARDOP command reader grew without bound

**File:** `crates/openpulse-ardop/src/command.rs` (`handle_client`)

The `MAX_CMD_LINE` (4096) guard was checked *after* `read_line` returned, but tokio's `read_line` has
no internal cap — it appends to the destination `String` until it sees `\n` or EOF. A client that
connected to the command port and streamed data with no newline (never closing) grew the buffer
without limit → OOM. Because the command and data ports share one process, this could take down all
clients on the instance.

**Fix:** the read is now bounded by a `Take` at `MAX_CMD_LINE + 1` bytes (`read_capped_line`), so the
buffer can't exceed the cap; an over-long line EOFs at the cap with no newline and is rejected by the
existing `n > MAX_CMD_LINE` check. Test: `oversized_command_line_drops_the_connection`.

### B-1 — [HIGH] gzip decompression had no output-size cap

**File:** `crates/openpulse-b2f/src/compress.rs` (`decompress_gzip`)

The LZHUF (Type C) path caps decompressed output at 16 MiB, but the gzip (Type D) path — the format a
real Winlink CMS actually sends — used `read_to_end` with no bound. It was reachable from the B2F
session driver and the CMS gateway on attacker-supplied bytes; only an unrelated u16 transport frame
incidentally limited the blast radius. A decompression bomb would otherwise allocate without limit.

**Fix:** `decompress_gzip` wraps the decoder in a `Take` at `MAX_UNCOMPRESSED + 1` (the constant, now
shared with the LZHUF path) and rejects any stream that expands past the cap. Test:
`gzip_decompression_bomb_over_the_cap_is_rejected`.

### B-2 — [MEDIUM] unbounded proposal count per B2F session

**File:** `crates/openpulse-b2f/src/session.rs` (IRS `Fc`/`Ff` handling)

The IRS auto-accepted every `FC` proposal with no count limit, and the `Ff` handler hardcoded `Accept`
for all of them (ignoring the per-proposal answer). A hostile peer/CMS could offer N cheap proposal
lines, then send N decompressed blobs (each up to the B-1 cap), all retained in memory — an unbounded
aggregate DoS compounding B-1.

**Fix:** accepts are capped at `MAX_PROPOSALS = 32`; proposals beyond the cap are recorded and answered
`Reject`, and the `Ff` handler now emits each proposal's recorded answer instead of a blanket accept, so
the sender never transmits the rejected blobs. Test: `irs_caps_the_number_of_accepted_proposals`.

## Clean / refuted

- **KISS / AX.25 TNC — no findings.** Every candidate refuted by a specific guard: the `FESC`-at-end
  case falls through to `InvalidEscape`; `Ax25UiFrame::decode` gates on `data.len() < 16` before any
  slicing and `from_wire` is only ever called with a full 7-byte slice; the reader caps an
  unterminated frame at `MAX_FRAME_BODY = 600`; decoded payloads are dropped over `MAX_PAYLOAD_BYTES`;
  the channels are non-blocking (drop, not grow); the `tx_pending` counter uses `min()` before
  `fetch_sub` (no underflow).
- **ARDOP binary data framing — refuted.** The `u16` length prefix is validated against
  `MAX_FRAME_BYTES` *before* `vec![0u8; len]`, so a crafted length can't force a large allocation.
- **ARDOP command state machine / authorization — by design, not a new finding.** `CONNECT`/`PTT`/etc.
  have no session gating — any client reaching the command port can drive them. This mirrors a real
  ARDOP hardware TNC, the default bind is `127.0.0.1`, and it is already documented in `main.rs` (the
  TNC "does not run the signed CONREQ/CONACK handshake … it is not consulted").
- **LZHUF (Type C) 16 MiB cap — verified sound.** Enforced against the 4-byte length prefix *before*
  decode, in both the BE and LE variants (the compat dual-attempt can't bypass it), and re-bounded
  inside the `oxiarc-lzhuf` decoder's own loop (`bytes_decoded < uncompressed_size`).
- **B2F/Winlink frame/header/banner parsing — no reachable panics.** All slice indexing is guarded by
  a length/`starts_with` check; all `.parse()` use `map_err`.
- **CMS gateway framing (u16-prefixed vs. real Winlink port 8772) — interop bug, not a vulnerability**
  (documented in the file-transfer plan); noted only as context for why B-1's blast radius is limited
  today.
