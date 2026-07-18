---
doc: docs/dev/reviews/2026-07-15-handshake-trust-audit.md
date: 2026-07-15
status: partial
scope: signed RF handshake (CONREQ/CONACK), session establishment, trust-store evaluation
---

# Handshake / session / trust security audit

Adversarial audit (multi-finder, refute-by-default, source-verified) of the Ed25519 signed
handshake over RF, session establishment, trust-store evaluation, and the OTA
fingerprint/compatibility gate. Findings ordered by severity; each verified against source.

## Fixed in this pass

### F1 — [CRITICAL] Classical handshake never bound the frame key to the trusted key

**Files:** `crates/openpulse-core/src/handshake.rs` (`verify_conreq`, `verify_conack`)

`verify_conreq`/`verify_conack` verified the Ed25519 signature against the frame's **own**
`pubkey`, then consulted `trust_store.trust_level(station_id)` — but never checked that the
frame's key equals the key the trust store holds for that station. A self-signed frame
proves only key possession, so any station could claim any trusted callsign at `Verified`/
`Full` trust using its own key, defeating the file-transfer signature gate (the one production
path that reads `verified_peer`). The PQ path (`pq_handshake.rs`) already had this binding; the
classical path omitted it.

**Fix:** `bind_frame_key()` — when the trust store holds a key for `station_id`, the frame
pubkey must equal it or verification fails with the new `HandshakeError::PublicKeyMismatch`.
Mirrors the PQ path. Tests: `conreq_rejects_impersonation_wrong_key_for_trusted_callsign`,
`conack_rejects_impersonation_wrong_key_for_trusted_callsign` (+20 existing, all pass).

### F2 — [HIGH] Initiator accepted a CONACK from any station, not the dialed one

**File:** `crates/openpulse-daemon/src/lib.rs` (`handle_inbound_conack`)

The initiator gated the inbound CONACK on `session_id` only. The session id is cleartext and
time-based (guessable within the handshake window), so an attacker could race a self-signed
CONACK echoing it under their own callsign and be recorded as the dialed peer. Added a check
that `ack.station_id == pending.peer_callsign` before verifying/recording. Test:
`conack_from_undialed_station_is_ignored`.

### T4 — [LOW-MED] Trust-store load error failed open

**File:** `crates/openpulse-daemon/src/server.rs`

A configured trust store that failed to load (unreadable/malformed) silently started with an
empty store, dropping every revocation it carried. Now fails closed: a load **error** refuses
startup with a clear message; a missing/empty path is still empty-ok (an unconfigured store is
not a downgrade under the Permissive RF profile).

## Enforcement posture (second finder pass)

The signed handshake is an **identity label, not an access gate.** `verify_conreq`/`verify_conack`
have exactly two callers, both in the daemon; the only production data path that consults
`verified_peer` is file transfer (off by default, `require_verified_peer = true`). Other
inbound-RF actions are taken on unauthenticated traffic when their (opt-in, default-off) features
are enabled. The enforcement finder's findings and their disposition:

### Fixed in the follow-up pass

- **E6 — [MEDIUM, §97.119] No-callsign daemon keyed the transmitter with no station ID.** Auto-ID
  is disabled for an empty/`N0CALL` callsign, but the always-on CONREQ→CONACK responder and the
  opt-in OTA-ACK, relay-forward, and auto-QSY paths keyed the transmitter with no per-transmission
  callsign gate — so a daemon left with no callsign that merely *heard* a frame would transmit
  unidentified. **Fix:** `RuntimeControlState::local_callsign_valid()`; every autonomous responder
  (CONACK reply, QSY responder + auto-QSY initiator, relay forward, OTA-ACK) now refuses to key up
  without a valid MYID. RX (decode, peer recording) is unaffected. Tests:
  `responder_without_callsign_does_not_transmit_conack`, `responder_without_callsign_ignores_qsy_req`,
  `callsign_validity_and_rf_peer_trust`.
- **E4 — [MEDIUM] QSY trust gate was inert.** The RF QSY responder was created with a hardcoded
  `Unverified` peer trust, so `qsy.allow_trustlevels` either did nothing (empty) or rejected every
  peer (any non-empty list, since trust never rose above `Unverified`). **Fix:**
  `RuntimeControlState::rf_peer_trust()` classifies the peer verified this session via
  `classify_connection_trust(OverAir)` — `Reduced` for a trust-store key, `Low` for first-seen,
  never `Verified` (which needs an out-of-band cert). `allow_trustlevels = ["reduced"]` is now an
  enforceable gate. Best-effort: the single global `verified_peer` is not bound to the unauthenticated
  QSY requester (see E5), so it means "only after such a handshake this session."
- **E2 — [HIGH-ish, misleading] ARDOP/KISS TNCs load a trust store they never consult.** The field
  is write-only; these bridges run no signed handshake. **Fix:** a loud startup `warn!` at both load
  sites so operators aren't misled into thinking the TNC authenticates peers. (Wiring real
  enforcement is the architectural E1 change below.)

### Deferred (architectural / protocol change)

- **E1 — [HIGH] The handshake gates nothing except off-by-default filexfer.** *Partially addressed.*
  The QSY responder now honours a trust allowlist (E4) and every autonomous responder refuses to key
  without a valid callsign (E6). For **relay**, a study of the wire format showed the envelope
  `auth_tag` has no key-distribution scheme, so the originator (`src_peer_id`) is **not
  cryptographically authenticated** at the relay — a strong access gate is genuinely blocked on
  envelope-authentication infrastructure (future work; also closes E3's `auth_tag` half). As a
  proportionate, honest increment, the relay now supports an operator-configured **originator
  allow-list** (`[relay] allow_list`) alongside the existing deny-list: when set, only listed
  originators are forwarded. It is documented as **defense-in-depth over an unauthenticated (spoofable)
  originator id** — it scopes a club/mesh relay to known stations and raises the bar for casual abuse,
  but is not strong authentication, and gating relay on "verified this session" would be wrong (a mesh
  originator is many hops away and never handshook with us). Wired via `RelayTrustPolicy::set_allow_list`
  → `forward()`'s existing `policy.allows(src)` check.
- **E3 — [MEDIUM] Relay forwards unauthenticated frames.** *(Clarified 2026-07-18: literally true of
  `forward()`, but narrower than it reads — `MeshNode::step` DOES enforce the filter via
  `trust_filter_allows` at `crates/openpulse-mesh/src/lib.rs:499`. The unverified `auth_tag` below
  is the part that remains open.)* `RelayTrustPolicy.min_trust_filter` is
  unread in `forward()` and the 16-byte `WireEnvelope.auth_tag` is never verified. Enforcing either
  needs a trust lookup on `src_peer_id` threaded into the forwarder plus auth-tag key material —
  larger than this pass. (Config already documents `min_trust_filter` as reserved.)
- **E5 — [MEDIUM] `verified_peer` is a single global slot.** A later handshake overwrites an
  earlier peer's verification; filexfer verifies a signed offer against whoever last handshook.
  Fails *safe* (reject/mis-attribute, not bypass). Needs a per-callsign map + filexfer plumbing.
- **E7 — [LOW] OTA rate adopted from unsigned FSK4 ACKs.** Rate-ladder manipulation, not an access
  bypass; ACK signing is a protocol change.

Correctly gated (refuted as non-gaps): the Noise-PSK control-channel (fail-closed, loopback-scoped,
WS-bypass closed) and the filexfer accept path (`require_verified_peer = true` default).

## Deferred from the first pass (larger change)

- **Replay freshness** — CONREQ/CONACK carry no nonce/timestamp, so a captured valid handshake
  replays. Needs a freshness field in the signed body (protocol change).
- **SAR handshake reassembly** uses a single constant key — a poisoned fragment can DoS an
  in-flight handshake reassembly.
