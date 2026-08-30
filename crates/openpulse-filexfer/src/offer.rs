//! `FileOffer`: transfer metadata with the sender's signed manifest embedded inline, plus the pure
//! accept/reject policy the receiver evaluates before a single data byte is accepted.

use ed25519_dalek::VerifyingKey;
use openpulse_core::manifest::{ManifestError, TransferManifest};
use openpulse_core::signing_domain::SigningDomain;

use crate::error::FxError;
use crate::wire::{write_string_truncating, Reader, Reason};
use crate::{MAX_BLOCK_SIZE, MIN_BLOCK_SIZE};

/// Maximum lengths of the offer's string fields (§4.2 wire layout).
/// The identity field must hold anything the handshake can verify, so it is defined BY REFERENCE to
/// the handshake's cap rather than repeating a number (#1201). It was 16 while the handshake was 18,
/// which made a 17-18 byte callsign — legal, verifiable, reachable — truncate here, miss the
/// receiver's verified-peer lookup, and be rejected on air as UntrustedPeer between two stations
/// that had just handshaken successfully. Nothing in that chain named truncation.
///
/// Raising `caps::STATION_ID` therefore changes THIS wire format too. That is the price of the
/// alias, stated here so the next bump is not a silent one.
const SENDER_ID_MAX: usize = openpulse_core::handshake_wire::caps::STATION_ID;

/// The invariant, mechanically. "Keep these in sync" as a comment cannot fail.
const _: () = assert!(SENDER_ID_MAX >= openpulse_core::handshake_wire::caps::STATION_ID);

/// A station identity that is within the wire cap **by construction**.
///
/// The predecessor of this type was a `String` written through a silently-truncating helper, so an
/// over-length callsign produced a validly-signed identity that was not the station's. Making the
/// invalid state unrepresentable is what closes that: there are exactly two doors — this
/// constructor and `Reader::string` on decode, which rejects above the same cap — so `write_to`
/// below is honestly infallible rather than swallowing an error it can never see.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SenderId(String);

impl SenderId {
    /// The only construction door. Refuses rather than truncating.
    pub fn new(id: &str) -> Result<Self, FxError> {
        if id.len() > SENDER_ID_MAX {
            return Err(FxError::FieldTooLong {
                field: "sender_id",
                len: id.len(),
                max: SENDER_ID_MAX,
            });
        }
        Ok(Self(id.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// `len(u8) | UTF-8 bytes`. No `Result`: the type's domain contains no input that fails.
    fn write_to(&self, out: &mut Vec<u8>) {
        let bytes = self.0.as_bytes();
        out.push(bytes.len() as u8);
        out.extend_from_slice(bytes);
    }
}

impl std::fmt::Display for SenderId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
const NAME_MAX: usize = 48;
const MIME_MAX: usize = 24;

/// A file-transfer offer. The Ed25519 `signature` covers the **whole** offer body (content hash plus
/// all metadata — name, mime, block geometry, transfer id), so an on-path attacker cannot replay a
/// signed offer with a spoofed filename or geometry under a valid-signature badge (audit F-2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileOffer {
    pub transfer_id: u32,
    /// Reserved capability/flags bits (0 in v1).
    pub flags: u8,
    /// Original (pre-compression) file size in bytes.
    pub file_size: u64,
    /// SHA-256 of the original file bytes (= manifest `payload_hash`).
    pub sha256: [u8; 32],
    /// Bytes per block (bounded `MIN_BLOCK_SIZE..=MAX_BLOCK_SIZE`).
    pub block_size: u32,
    /// Number of blocks the file splits into.
    pub block_count: u16,
    /// Sender callsign/id (= manifest `sender_id`).
    pub sender_id: SenderId,
    /// Suggested filename (sanitized by the receiver before any disk write).
    pub name: String,
    /// MIME type hint (advisory).
    pub mime: String,
    /// Ed25519 signature over the full offer body (every field above; see [`signing_bytes`]).
    pub signature: [u8; 64],
}

impl FileOffer {
    /// Build an offer from a manifest (source of the content hash / size / sender) plus the transfer
    /// metadata, and sign the **whole** offer body with `signing_key_seed`. The manifest's own
    /// signature is not reused — the offer carries its own signature covering the metadata too.
    ///
    /// Returns `None` if `block_size` is out of range or the file needs more than [`crate::block_count`]
    /// permits.
    pub fn from_manifest(
        transfer_id: u32,
        manifest: &TransferManifest,
        name: &str,
        mime: &str,
        block_size: u32,
        signing_key_seed: &[u8; 32],
    ) -> Option<Self> {
        if !(MIN_BLOCK_SIZE..=MAX_BLOCK_SIZE).contains(&block_size) {
            return None;
        }
        let sha256: [u8; 32] = manifest.payload_hash.as_slice().try_into().ok()?;
        let block_count = crate::block_count(manifest.payload_size, block_size)?;
        let mut offer = Self {
            transfer_id,
            flags: 0,
            file_size: manifest.payload_size,
            sha256,
            block_size,
            block_count,
            sender_id: SenderId::new(&manifest.sender_id).ok()?,
            name: name.to_string(),
            mime: mime.to_string(),
            signature: [0u8; 64],
        };
        offer.signature = openpulse_core::signing::sign_in_domain(
            SigningDomain::FileOffer,
            signing_key_seed,
            &offer.signing_bytes(),
        )
        .ok()?;
        Some(offer)
    }

    /// The canonical bytes the signature covers: every offer field except the signature itself. Binds
    /// the sender to the content hash **and** the metadata (name/mime/geometry/transfer id).
    fn signing_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.encode_signed_fields(&mut out);
        out
    }

    /// Verify the offer signature (over the full body) against the peer's Ed25519 public key.
    pub fn verify_signature(&self, peer_pubkey: &[u8; 32]) -> Result<(), ManifestError> {
        VerifyingKey::from_bytes(peer_pubkey).map_err(|_| ManifestError::InvalidKey)?;
        if openpulse_core::signing::verify_in_domain(
            SigningDomain::FileOffer,
            peer_pubkey,
            &self.signing_bytes(),
            &self.signature,
        ) {
            Ok(())
        } else {
            Err(ManifestError::InvalidSignature)
        }
    }

    /// Every field except the trailing signature — the exact prefix the signature covers, reused by
    /// both [`signing_bytes`] and [`encode_body`] so the signed and wire forms can't drift.
    fn encode_signed_fields(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.transfer_id.to_be_bytes());
        out.push(self.flags);
        out.extend_from_slice(&self.file_size.to_be_bytes());
        out.extend_from_slice(&self.sha256);
        out.extend_from_slice(&self.block_size.to_be_bytes());
        out.extend_from_slice(&self.block_count.to_be_bytes());
        // sender_id is a SenderId, so it cannot be over-cap here — the refusal happened at
        // construction. name/mime go through the TRUNCATING writer, named so the loss is visible at
        // the call site: they are cosmetic, and the signature covers the truncated form
        // consistently. An identity field must never use that writer.
        self.sender_id.write_to(out);
        write_string_truncating(out, &self.name, NAME_MAX);
        write_string_truncating(out, &self.mime, MIME_MAX);
    }

    pub(crate) fn encode_body(&self, out: &mut Vec<u8>) {
        self.encode_signed_fields(out);
        out.extend_from_slice(&self.signature);
    }

    pub(crate) fn decode_body(r: &mut Reader) -> Result<Self, FxError> {
        let transfer_id = r.u32()?;
        let flags = r.u8()?;
        let file_size = r.u64()?;
        let sha256 = r.array::<32>()?;
        let block_size = r.u32()?;
        if !(MIN_BLOCK_SIZE..=MAX_BLOCK_SIZE).contains(&block_size) {
            return Err(FxError::BlockSizeOutOfRange(block_size));
        }
        let block_count = r.u16()?;
        // The reader is the other door: it rejects above the same cap, so the value is in-domain.
        let sender_id = SenderId::new(&r.string("sender_id", SENDER_ID_MAX)?)?;
        let name = r.string("name", NAME_MAX)?;
        let mime = r.string("mime", MIME_MAX)?;
        let signature = r.array::<64>()?;
        Ok(Self {
            transfer_id,
            flags,
            file_size,
            sha256,
            block_size,
            block_count,
            sender_id,
            name,
            mime,
            signature,
        })
    }
}

/// Receiver-side acceptance policy (values supplied by the daemon's `[file_transfer]` config).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfferPolicy {
    /// Master switch. When `false`, all inbound offers are rejected with `feature-disabled`.
    pub enabled: bool,
    /// Hard per-file cap (both directions). Offers above it are rejected `too-large`.
    pub max_file_bytes: u64,
    /// Auto-accept offers at or below this size; `0` = always prompt the operator.
    pub auto_accept_max_bytes: u64,
    /// Require a signature-verified peer before accepting.
    pub require_verified_peer: bool,
}

impl Default for OfferPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            max_file_bytes: 1024 * 1024,
            auto_accept_max_bytes: 0,
            require_verified_peer: true,
        }
    }
}

/// What the receiver should do with an offer after policy evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfferDecision {
    /// Accept immediately (size ≤ auto-accept and all gates passed).
    AutoAccept,
    /// Ask the operator (size above auto-accept but otherwise allowed).
    Prompt,
    /// Decline on air with this reason.
    Reject(Reason),
}

/// Pure accept/reject decision for an offer. `sig_verified` is the result of
/// [`FileOffer::verify_signature`] against the handshake-proven peer key. Quota is checked
/// separately by the daemon (it needs disk accounting) before this is consulted.
pub fn decide(offer: &FileOffer, policy: &OfferPolicy, sig_verified: bool) -> OfferDecision {
    if !policy.enabled {
        return OfferDecision::Reject(Reason::FeatureDisabled);
    }
    if offer.file_size > policy.max_file_bytes {
        return OfferDecision::Reject(Reason::TooLarge);
    }
    if policy.require_verified_peer && !sig_verified {
        return OfferDecision::Reject(Reason::UntrustedPeer);
    }
    if offer.file_size <= policy.auto_accept_max_bytes {
        OfferDecision::AutoAccept
    } else {
        OfferDecision::Prompt
    }
}
