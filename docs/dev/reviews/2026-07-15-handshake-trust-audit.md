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

## Enforcement posture (documented, not a per-line fix)

The signed handshake is an **identity label, not an access gate.** `verify_conreq`/`verify_conack`
have exactly two callers, both in the daemon; the only production data path that consults
`verified_peer` is file transfer (off by default, `require_verified_peer = true`). Other
inbound-RF actions are taken on unauthenticated traffic when their (opt-in, default-off)
features are enabled:

- **Relay forward** (`relay.enabled` default false) retransmits envelopes with only a hop-limit,
  replay-suppression, and deny-list check; `RelayTrustPolicy.min_trust_filter` is not read in
  `forward()`, and the 16-byte `auth_tag` in `WireEnvelope` is carried but never verified.
- **QSY responder** (`qsy.enabled` default false) is created with hardcoded `Unverified` peer
  trust, so no config restricts a forced retune to authenticated peers.
- **OTA rate adoption** (opt-in) trusts an unsigned FSK4 ACK's recommended level.
- **ARDOP/KISS TNCs** load a trust store they never consult (write-only field) — misleading.

These match the repo's documented "front-ends don't drive sessions" gap and are architectural
(wiring the handshake into an access gate on each path), not one-line fixes. They are recorded
here so the security posture is stated honestly: with these opt-in features enabled, the daemon
acts on unauthenticated RF. Correctly gated (refuted as non-gaps): the Noise-PSK control-channel
(fail-closed, loopback-scoped, WS-bypass closed) and the filexfer accept path.

## Deferred (tracked, larger change)

- **Replay freshness** — CONREQ/CONACK carry no nonce/timestamp, so a captured valid handshake
  replays. Needs a freshness field in the signed body (protocol change).
- **Per-peer `verified_peer`** — single global slot; a later handshake overwrites an earlier
  peer's verification. Fails safe (filexfer rejects rather than mis-accepts) but mis-attributes.
- **SAR handshake reassembly** uses a single constant key — a poisoned fragment can DoS an
  in-flight handshake reassembly.
