//! Direct peer-to-peer file transfer protocol (`OPFX`) — pure, no-I/O state machines.
//!
//! The sender and receiver are sans-I/O state machines (the `openpulse-b2f` / `openpulse-qsy`
//! pattern): the caller feeds them decoded [`FxFrame`]s plus a millisecond clock and receives a list
//! of [`FxAction`]s to perform (transmit a frame, materialize a block, prompt the operator, verify a
//! payload, report progress, finish). Nothing here touches the filesystem, the modem, or tokio — the
//! daemon glue (`openpulse-daemon::filexfer`) performs all I/O. This keeps every protocol edge unit
//! testable with an injected clock.
//!
//! Byte-level block splitting / SAR mapping / fragment bitmaps land in `blocks.rs` (Phase B); this
//! crate (Phase A) is the wire codec, the offer + policy, and the control-flow state machines.

mod blocks;
mod error;
mod offer;
mod receiver;
mod sanitize;
mod sender;
mod wire;

pub use blocks::{encode_block, split_blocks, BlockAssembler, BlockEvent};
pub use error::FxError;
pub use offer::{decide, FileOffer, OfferDecision, OfferPolicy};
pub use receiver::ReceiverSession;
pub use sanitize::sanitize_filename;
pub use sender::SenderSession;
pub use wire::{
    is_filexfer_frame, CompleteStatus, FxFrame, Reason, FILEXFER_MAGIC, FILEXFER_VERSION,
};

/// Smallest legal `block_size` (bytes). Below this the SAR/header overhead dominates.
pub const MIN_BLOCK_SIZE: u32 = 1024;
/// Largest legal `block_size` (bytes). Chosen so a packed block + 12-byte `FileData` header can never
/// exceed the 64 005-byte SAR-segment / `MAX_DECOMPRESSED_SIZE` cap, even for incompressible data.
pub const MAX_BLOCK_SIZE: u32 = 49_152;
/// Default `block_size` when the sender doesn't specify one.
pub const DEFAULT_BLOCK_SIZE: u32 = 16_384;

/// Highest permitted block count. The last block's SAR `segment_id` is `block_index + 1`, so a count
/// of `0xFFFF` would make the final block's id `0xFFFF` — the reserved file-transfer control segment
/// id — and route that block into the control channel where it is silently dropped (audit F-6).
pub const MAX_BLOCK_COUNT: u16 = 0xFFFE;

/// Number of blocks a `file_size`-byte file splits into at `block_size`, or `None` if `block_size` is
/// 0 or the count would exceed [`MAX_BLOCK_COUNT`] (≈3 GiB, never the binding limit under the 1 MiB
/// config cap). Always ≥ 1.
pub fn block_count(file_size: u64, block_size: u32) -> Option<u16> {
    if block_size == 0 {
        return None;
    }
    let n = file_size.div_ceil(block_size as u64).max(1);
    match u16::try_from(n) {
        Ok(c) if c <= MAX_BLOCK_COUNT => Some(c),
        _ => None,
    }
}

/// Timeouts driving the session state machines (all milliseconds). Injected so tests are deterministic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timeouts {
    /// How long the sender waits for a `FileAccept`/`FileReject`, and the receiver waits for the
    /// operator to answer a prompt, before giving up.
    pub offer_ms: u64,
    /// How long either side tolerates no forward progress within the transfer before aborting `stall`.
    pub block_stall_ms: u64,
    /// How long the sender waits for the receiver's `FileComplete` after the last block.
    pub verify_ms: u64,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            offer_ms: 60_000,
            block_stall_ms: 120_000,
            verify_ms: 60_000,
        }
    }
}

/// An instruction the caller must carry out. The session state machines never perform I/O themselves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FxAction {
    /// Transmit this already-`OPFX`-encoded frame (the daemon SAR-encodes and sends it).
    Transmit(Vec<u8>),
    /// Sender: materialize and transmit data block `block_index`. `missing = Some(bitmap)` means only
    /// the listed fragments need re-sending (selective retransmit). Fulfilled by `blocks.rs` (Phase B).
    SendBlock {
        block_index: u16,
        missing: Option<Vec<u8>>,
    },
    /// Receiver: ask the operator to accept or reject this offer (size above auto-accept).
    Prompt { transfer_id: u32 },
    /// Receiver: all blocks arrived — reassemble and verify the payload, then call `set_verify_result`.
    Verify { transfer_id: u32 },
    /// Progress update for the UI/logs.
    Progress {
        transfer_id: u32,
        blocks_done: u16,
        blocks_total: u16,
    },
    /// The transfer reached a terminal state.
    Finished(Outcome),
}

/// Terminal result of a transfer session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub transfer_id: u32,
    pub result: TransferResult,
}

/// How a transfer ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferResult {
    /// Sender: the receiver acknowledged the whole file; `peer_verified` reflects its `FileComplete`.
    Sent { peer_verified: bool },
    /// Receiver: the file was reassembled; `verified` is the signed-manifest check result.
    Received { verified: bool },
    /// The offer was declined.
    Rejected { reason: Reason },
    /// The transfer was cancelled by either side.
    Cancelled { reason: Reason },
    /// The transfer failed (timeout, stall, exhausted retries).
    Failed { reason: Reason },
}
