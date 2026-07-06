---
project: openpulsehf
doc: docs/dev/design/control-channel-security.md
status: living
last_updated: 2026-07-06
---

# Control-channel security (REQ-SEC-CTL)

Design for authenticating and encrypting the daemon ↔ client control channel, plus the key storage
and file-permission requirements that support it. Distinct from the on-air/RF peer link (Ed25519 /
PQ handshake), which is already secured.

## Threat model

The control channel (TCP `:9000`, WebSocket `:9001`) commands the **transmitter**: PTT, frequency,
mode, transmit, messaging, repeater. Today it is plaintext NDJSON with no authentication.

- **Default (loopback bind):** low risk — reachable only from the same host.
- **Non-loopback bind** (the legitimate "operate the panel from another machine" case): the port is
  wide open. Anyone who can reach it can key the transmitter — a safety and regulatory hazard
  (unauthorised emission), not merely a confidentiality one.

The asymmetry to close: the *less-trusted* surface (a network-exposed control port) is the
*unprotected* one, while the RF link is heavily secured.

Reference: K4remote connects to the Elecraft K4 with **TLS-PSK** (OpenSSL), PSK = the radio's remote
password — protocol-mandated. We choose our own scheme since the control protocol is ours.

## Authentication + encryption — options

| Option | Encrypts wire | Authenticates | Effort | Notes |
|---|---|---|---|---|
| 1. PSK token over existing TCP/WS | ✗ | ✓ | low | Client sends a shared secret on connect; daemon rejects unauthenticated clients. Stops unauthorised control but stays plaintext. |
| **2. TLS-PSK (recommended)** | ✓ | ✓ | medium | rustls TLS 1.3 external PSK — one shared key, no certificates. Closest to the K4 model; encrypts + authenticates together. |
| 3. TLS + token / Noise | ✓ | ✓ | high | More machinery than this needs. |

**Recommendation: option 2 (TLS-PSK via rustls).** One PSK, no cert lifecycle, TLS 1.3 confidentiality
+ integrity, and it maps directly onto the K4remote precedent. Gate it: **required on a non-loopback
bind; optional on loopback** (`REQ-SEC-CTL-02`). Transmitter-keying commands fail closed for
unauthenticated clients on a non-loopback bind.

## Key storage

The control-channel PSK (and, over time, the station identity key) needs a home. Two backends:

- **System secret store (preferred, `REQ-SEC-CTL-03`):** the OS keychain via the `keyring` crate —
  Secret Service / GNOME Keyring / KWallet (Linux), Keychain (macOS), Credential Manager (Windows).
  Used by both daemon and clients when a store is available (matches K4remote's `keychain` feature).
- **File-based keystore (fallback, `REQ-SEC-CTL-04`):** for headless hosts with no usable secret
  service. Secrets are encrypted at rest under an operator **master password**: Argon2id KDF →
  key → AEAD (ChaCha20-Poly1305 or AES-256-GCM) over the secret blob. The master password is never
  persisted; it is prompted (or supplied via a one-shot env var for headless automation) and held only
  in memory. Salt + KDF params + nonce are stored alongside the ciphertext.

Selection order: system store if present and unlocked, else the file keystore.

## File permissions (`REQ-SEC-CTL-05`, both server and client)

Every file holding key/secret material — identity key, trust store, the file keystore, any PSK file —
must be **owner-only**: `0600` files, `0700` dirs. On load, **validate and refuse** a group/world-
accessible file; on write, **enforce** owner-only. Generalise the existing precedent in
`crates/openpulse-cli/src/state.rs` (`validate_trust_store_permissions` bails on `mode & 0o077 != 0`;
`enforce_trust_store_permissions` sets `0o600`) into a shared helper used by **both** the daemon
(server) and the panel/clients (client). Non-Unix: no-op (documented).

## Implementation slices (smallest-risk first)

1. **Shared permission helper** — lift the `validate`/`enforce` owner-only logic out of `openpulse-cli`
   into a shared module (e.g. `openpulse-config`); apply it to the existing identity key + trust store
   load paths on both daemon and clients. Small, testable, immediate value.
2. **File keystore + master password** — Argon2id + AEAD keystore type with `open(master)` / `store` /
   `load`, owner-only files via slice 1. Unit-tested round-trip + wrong-password rejection.
3. **System keychain backend** — `keyring`-backed store behind a trait; fall back to slice 2.
4. **TLS-PSK transport** — rustls external-PSK server (daemon) + client (panel); config
   (`[control_security]`): `require_auth`, PSK source (keychain/keystore key id). Enforce the
   non-loopback gate + fail-closed PTT.

Each slice ships behind config, off by default, with the traceability chain (CAP-68).

## Open decisions (confirm before implementing)

- Auth scheme: **TLS-PSK** (recommended) vs PSK-token-only.
- KDF/AEAD primitives for the file keystore: **Argon2id + ChaCha20-Poly1305** (proposed).
- Whether the panel (iced) prompts for the master password in-UI or reads it from the OS keychain only.
