//! File ⇄ blocks ⇄ SAR fragments: split, per-block `pack()`, SAR segment mapping, fragment bitmaps.
//!
//! This is the byte-level layer under the state machines (Phase B). The 64 005-byte SAR-segment cap
//! is cleared by making the **block** the multi-object unit: each block is one SAR segment with
//! `segment_id = block_index + 1` (id 0 stays reserved for handshake frames), so an arbitrarily large
//! file rides across many segments.

use std::collections::HashMap;
use std::time::Duration;

use openpulse_core::compression::{pack, unpack};
use openpulse_core::sar::{sar_encode, SarReassembler, SAR_HEADER_SIZE};

use crate::error::FxError;
use crate::wire::FxFrame;

/// SAR reassembly session key. v1 allows one transfer per link, so a fixed key is sufficient.
const SAR_SESSION: &str = "filexfer";
/// How long a partially-received block is held before its fragments expire.
const BLOCK_SAR_TIMEOUT_SECS: u64 = 300;

/// Split `file` into `block_size`-byte blocks (the last is short). An empty file yields one empty
/// block, matching [`crate::block_count`].
pub fn split_blocks(file: &[u8], block_size: u32) -> Vec<&[u8]> {
    if file.is_empty() {
        return vec![&[]];
    }
    file.chunks(block_size as usize).collect()
}

/// The `segment_id` block `k` rides on (`k + 1`; id 0 is reserved for handshake frames).
fn block_segment_id(block_index: u16) -> u16 {
    block_index.wrapping_add(1)
}

/// Encode one block into transmittable SAR fragments: `pack()` it, wrap in a `FileData` frame, then
/// SAR-fragment. `missing` (a fragment bitmap, bit set = wanted) selects only those fragments for a
/// selective retransmission; `None` sends the whole block.
pub fn encode_block(
    transfer_id: u32,
    block_index: u16,
    block: &[u8],
    missing: Option<&[u8]>,
) -> Result<Vec<Vec<u8>>, FxError> {
    let packed = pack(block);
    let frame = FxFrame::FileData {
        transfer_id,
        block_index,
        packed,
    }
    .encode();
    let fragments = sar_encode(block_segment_id(block_index), &frame)
        .map_err(|_| FxError::BlockTooLarge { block_index })?;
    match missing {
        None => Ok(fragments),
        Some(bitmap) => Ok(fragments
            .into_iter()
            .enumerate()
            .filter(|(i, _)| bit_is_set(bitmap, *i))
            .map(|(_, f)| f)
            .collect()),
    }
}

/// Reassembles received SAR fragments into blocks, tracks per-block fragment arrival for `BlockAck`,
/// and concatenates completed blocks into the original file.
pub struct BlockAssembler {
    transfer_id: u32,
    block_count: u16,
    reasm: SarReassembler,
    seen: HashMap<u16, FragTracker>,
    blocks: HashMap<u16, Vec<u8>>,
}

struct FragTracker {
    total: u8,
    seen: Vec<bool>,
}

/// Outcome of ingesting one fragment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockEvent {
    /// A fragment landed but its block isn't complete yet.
    Progress { block_index: u16 },
    /// The block fully reassembled and unpacked.
    Complete { block_index: u16 },
    /// Malformed, duplicate-completing, unknown-block, or wrong-transfer fragment — dropped.
    Ignored,
}

impl BlockAssembler {
    /// A collector for a `block_count`-block transfer identified by `transfer_id`.
    pub fn new(transfer_id: u32, block_count: u16) -> Self {
        Self {
            transfer_id,
            block_count,
            reasm: SarReassembler::new(Duration::from_secs(BLOCK_SAR_TIMEOUT_SECS)),
            seen: HashMap::new(),
            blocks: HashMap::new(),
        }
    }

    /// Ingest one received SAR fragment: peek its header for the missing-bitmap, then reassemble.
    /// Returns `Complete` when the fragment finished a block (its unpacked bytes are then retained).
    pub fn ingest_fragment(&mut self, fragment: &[u8]) -> BlockEvent {
        if fragment.len() < SAR_HEADER_SIZE {
            return BlockEvent::Ignored;
        }
        let segment_id = ((fragment[0] as u16) << 8) | fragment[1] as u16;
        let frag_index = fragment[2];
        let frag_total = fragment[3];
        if segment_id == 0 || frag_total == 0 || frag_index >= frag_total {
            return BlockEvent::Ignored;
        }
        let block_index = segment_id - 1;
        if block_index >= self.block_count {
            return BlockEvent::Ignored;
        }

        // Track fragment arrival for the missing bitmap (before reassembly consumes it).
        let tracker = self.seen.entry(block_index).or_insert_with(|| FragTracker {
            total: frag_total,
            seen: vec![false; frag_total as usize],
        });
        if tracker.total == frag_total {
            if let Some(slot) = tracker.seen.get_mut(frag_index as usize) {
                *slot = true;
            }
        }

        match self.reasm.ingest(SAR_SESSION, fragment) {
            Ok(Some(frame_bytes)) => match FxFrame::decode(&frame_bytes) {
                Ok(FxFrame::FileData {
                    transfer_id,
                    block_index: bi,
                    packed,
                }) if transfer_id == self.transfer_id && bi == block_index => {
                    let block = unpack(&packed).unwrap_or(packed);
                    self.blocks.insert(block_index, block);
                    BlockEvent::Complete { block_index }
                }
                _ => BlockEvent::Ignored,
            },
            Ok(None) => BlockEvent::Progress { block_index },
            Err(_) => BlockEvent::Ignored,
        }
    }

    /// Seed an already-held block from a resumed transfer's on-disk partial, so it counts complete and
    /// is included in [`reassemble`](Self::reassemble) without any fragment arriving for it.
    pub fn seed_block(&mut self, block_index: u16, bytes: Vec<u8>) {
        if block_index < self.block_count {
            self.blocks.insert(block_index, bytes);
        }
    }

    /// The unpacked bytes of a completed block, for persisting it as a resumable partial.
    pub fn block(&self, block_index: u16) -> Option<&[u8]> {
        self.blocks.get(&block_index).map(Vec::as_slice)
    }

    /// Missing-fragment bitmap for `block_index` (bit set = not yet received) for a `BlockAck`. Empty
    /// when nothing has been seen for that block.
    pub fn missing_bitmap(&self, block_index: u16) -> Vec<u8> {
        match self.seen.get(&block_index) {
            None => Vec::new(),
            Some(t) => {
                let mut bitmap = vec![0u8; (t.total as usize).div_ceil(8)];
                for (i, got) in t.seen.iter().enumerate() {
                    if !*got {
                        bitmap[i / 8] |= 1 << (i % 8);
                    }
                }
                bitmap
            }
        }
    }

    /// True once every block has fully arrived.
    pub fn is_complete(&self) -> bool {
        (0..self.block_count).all(|k| self.blocks.contains_key(&k))
    }

    /// Concatenate all received blocks in order into the original file, or `None` if incomplete.
    pub fn reassemble(&self) -> Option<Vec<u8>> {
        if !self.is_complete() {
            return None;
        }
        let mut out = Vec::new();
        for k in 0..self.block_count {
            out.extend_from_slice(self.blocks.get(&k)?);
        }
        Some(out)
    }
}

/// Bit `i` of a little-endian-by-byte bitmap (byte `i/8`, bit `i%8`).
fn bit_is_set(bitmap: &[u8], i: usize) -> bool {
    bitmap.get(i / 8).is_some_and(|b| b & (1 << (i % 8)) != 0)
}
