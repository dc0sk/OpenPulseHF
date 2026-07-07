---
project: openpulsehf
doc: docs/dev/design/control-channel-security.md
status: living
last_updated: 2026-07-07
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
| 2. TLS-PSK (rustls) | ✓ | ✓ | — | **Not viable:** rustls has no external/raw TLS-PSK support (it is certificate-focused). |
| 3. TLS-PSK (OpenSSL) | ✓ | ✓ | high | K4remote's choice, but adds a C dependency (OpenSSL) to an otherwise pure-Rust workspace + a packaging burden (K4remote needs a `vendored-tls` feature). |
| **4. Noise NNpsk0 (chosen)** | ✓ | ✓ | medium | Pure-Rust (`snow`); a PSK-only handshake gives mutual auth + forward secrecy + AEAD from one 32-byte key, no certificates. |

**Decision: option 4 — Noise `Noise_NNpsk0_25519_ChaChaPoly_BLAKE2s` via `snow`.** The original plan
was TLS-PSK-via-rustls, but rustls does not implement external PSK; OpenSSL would drag in a C dep. The
Noise NNpsk0 pattern delivers exactly what this channel needs — both endpoints prove knowledge of the
shared PSK during the handshake (mutual authentication), then exchange ChaCha20-Poly1305-encrypted
messages with forward secrecy — with no certificate lifecycle and no C dependency. Implemented
transport-agnostically in `crates/openpulse-linksec` (`NoiseHandshake` / `NoiseTransport`) so the same
primitive drives the sync CLI and the async daemon/panel. Gate it: **auth required on a non-loopback
bind; optional on loopback** (`auth_required()` / `REQ-SEC-CTL-02`); transmitter-keying commands fail
closed for unauthenticated clients on a non-loopback bind.

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
4. **Noise-PSK transport** — `crates/openpulse-linksec` (`NoiseHandshake`/`NoiseTransport`, NNpsk0)
   + the `auth_required` gate + `[control_security]` config (`require_auth`, `psk_key_id`) shipped;
   **remaining:** wire the handshake+framing into the daemon TCP/WS server and the panel client, and
   enforce fail-closed PTT for unauthenticated clients. This transport integration touches the live
   networking and is done as a separate, live-validated step.

Each slice ships behind config, off by default, with the traceability chain (CAP-68).

## Resolved decisions

- Auth scheme: **Noise NNpsk0** (rustls has no external PSK; OpenSSL would add a C dep). *Shipped.*
- Keystore KDF/AEAD: **Argon2id + ChaCha20-Poly1305**. *Shipped (slice 2).*
- Panel master password: read the OS keychain first, prompt in-UI only as a fallback. *For the
  transport-wiring step.*
