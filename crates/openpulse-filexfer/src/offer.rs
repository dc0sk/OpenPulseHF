//! `FileOffer`: transfer metadata with the sender's signed manifest embedded inline, plus the pure
//! accept/reject policy the receiver evaluates before a single data byte is accepted.

use openpulse_core::manifest::{verify_manifest, ManifestError, TransferManifest};

use crate::error::FxError;
use crate::wire::{write_string, Reader, Reason};
use crate::{MAX_BLOCK_SIZE, MIN_BLOCK_SIZE};

/// Maximum lengths of the offer's string fields (§4.2 wire layout).
const SENDER_ID_MAX: usize = 16;
const NAME_MAX: usize = 48;
const MIME_MAX: usize = 24;

/// A file-transfer offer. `sha256`/`file_size`/`sender_id`/`signature` are exactly the four
/// [`TransferManifest`] fields, so the receiver reconstructs the manifest and verifies it with the
/// existing crypto — no new signature code.
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
    pub sender_id: String,
    /// Suggested filename (sanitized by the receiver before any disk write).
    pub name: String,
    /// MIME type hint (advisory).
    pub mime: String,
    /// Ed25519 signature over the manifest body (= manifest `signature`).
    pub signature: [u8; 64],
}

impl FileOffer {
    /// Build a signed offer from a manifest the sender already produced with `TransferManifest::sign`.
    ///
    /// Returns `None` if the manifest signature isn't the expected 64 bytes or `block_size` is out of
    /// range — both caller bugs, surfaced rather than panicked.
    pub fn from_manifest(
        transfer_id: u32,
        manifest: &TransferManifest,
        name: &str,
        mime: &str,
        block_size: u32,
    ) -> Option<Self> {
        if !(MIN_BLOCK_SIZE..=MAX_BLOCK_SIZE).contains(&block_size) {
            return None;
        }
        let sha256: [u8; 32] = manifest.payload_hash.as_slice().try_into().ok()?;
        let signature: [u8; 64] = manifest.signature.as_slice().try_into().ok()?;
        let block_count = crate::block_count(manifest.payload_size, block_size)?;
        Some(Self {
            transfer_id,
            flags: 0,
            file_size: manifest.payload_size,
            sha256,
            block_size,
            block_count,
            sender_id: manifest.sender_id.clone(),
            name: name.to_string(),
            mime: mime.to_string(),
            signature,
        })
    }

    /// Reconstruct the [`TransferManifest`] the offer fields encode.
    pub fn to_manifest(&self) -> TransferManifest {
        TransferManifest {
            payload_hash: self.sha256.to_vec(),
            payload_size: self.file_size,
            sender_id: self.sender_id.clone(),
            signature: self.signature.to_vec(),
        }
    }

    /// Verify the embedded manifest signature against the peer's Ed25519 public key.
    pub fn verify_signature(&self, peer_pubkey: &[u8; 32]) -> Result<(), ManifestError> {
        verify_manifest(&self.to_manifest(), peer_pubkey)
    }

    pub(crate) fn encode_body(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.transfer_id.to_be_bytes());
        out.push(self.flags);
        out.extend_from_slice(&self.file_size.to_be_bytes());
        out.extend_from_slice(&self.sha256);
        out.extend_from_slice(&self.block_size.to_be_bytes());
        out.extend_from_slice(&self.block_count.to_be_bytes());
        write_string(out, &self.sender_id, SENDER_ID_MAX);
        write_string(out, &self.name, NAME_MAX);
        write_string(out, &self.mime, MIME_MAX);
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
        let sender_id = r.string("sender_id", SENDER_ID_MAX)?;
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
