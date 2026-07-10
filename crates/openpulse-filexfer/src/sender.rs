//! Sender-side transfer state machine (see the diagram in `docs/dev/design/file-transfer-plan.md` §3.3).

use crate::offer::FileOffer;
use crate::wire::{FxFrame, Reason};
use crate::{FxAction, Outcome, Timeouts, TransferResult};

/// Retransmissions of a single block before the transfer is failed `stall`.
const DEFAULT_MAX_BLOCK_RETRIES: u8 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Offering,
    Sending { block: u16 },
    AwaitVerify,
    Terminal,
}

/// Drives sending one file to a peer. Feed it `FileAccept`/`BlockAck`/`FileComplete`/`FileCancel`
/// frames and a clock; it emits `SendBlock`/`Transmit`/`Progress`/`Finished` actions.
pub struct SenderSession {
    transfer_id: u32,
    block_count: u16,
    state: State,
    timeouts: Timeouts,
    /// Absolute ms deadline for the current state (offer wait / block stall / verify wait).
    deadline: u64,
    retries: u8,
    max_block_retries: u8,
    /// Blocks the receiver already holds (from `FileAccept.have_bitmap`) — skipped when sending (resume).
    held: Vec<bool>,
}

impl SenderSession {
    /// Start a transfer for an already-signed `offer`. Returns the session and the initial action
    /// (transmit the `FileOffer`).
    pub fn new(offer: FileOffer, timeouts: Timeouts, now_ms: u64) -> (Self, Vec<FxAction>) {
        let transfer_id = offer.transfer_id;
        let block_count = offer.block_count;
        let session = Self {
            transfer_id,
            block_count,
            state: State::Offering,
            timeouts,
            deadline: now_ms.saturating_add(timeouts.offer_ms),
            retries: 0,
            max_block_retries: DEFAULT_MAX_BLOCK_RETRIES,
            held: vec![false; block_count as usize],
        };
        let actions = vec![FxAction::Transmit(FxFrame::FileOffer(offer).encode())];
        (session, actions)
    }

    /// First block index at or after `from` that the receiver doesn't already hold, or `block_count`
    /// when the rest are all held.
    fn next_unheld(&self, from: u16) -> u16 {
        let mut i = from;
        while (i as usize) < self.held.len() && self.held[i as usize] {
            i += 1;
        }
        i
    }

    /// Apply an inbound frame. Frames for another `transfer_id` or unexpected for the current state
    /// are ignored (robust against radio dupes/reorders) rather than erroring.
    pub fn apply(&mut self, frame: &FxFrame, now_ms: u64) -> Vec<FxAction> {
        if frame.transfer_id() != self.transfer_id {
            return Vec::new();
        }
        if let FxFrame::FileCancel { reason, .. } = frame {
            return self.finish(TransferResult::Cancelled { reason: *reason });
        }
        match (self.state, frame) {
            (State::Offering, FxFrame::FileReject { reason, .. }) => {
                self.finish(TransferResult::Rejected { reason: *reason })
            }
            (State::Offering, FxFrame::FileAccept { have_bitmap, .. }) => {
                // Resume: skip blocks the receiver already holds; start at the first it's missing.
                for i in 0..self.block_count {
                    if bit_is_set(have_bitmap, i as usize) {
                        if let Some(slot) = self.held.get_mut(i as usize) {
                            *slot = true;
                        }
                    }
                }
                self.retries = 0;
                let first = self.next_unheld(0);
                if first < self.block_count {
                    self.begin_block(first, None, now_ms)
                } else {
                    // The receiver already has every block — go straight to awaiting verification.
                    self.state = State::AwaitVerify;
                    self.deadline = now_ms.saturating_add(self.timeouts.verify_ms);
                    vec![self.progress(self.block_count)]
                }
            }
            (
                State::Sending { block },
                FxFrame::BlockAck {
                    block_index,
                    complete,
                    missing_frag_bitmap,
                    ..
                },
            ) => {
                if *block_index != block {
                    return Vec::new(); // stale/out-of-order ACK
                }
                if *complete {
                    self.retries = 0;
                    let next = self.next_unheld(block + 1);
                    if next < self.block_count {
                        self.begin_block(next, None, now_ms)
                    } else {
                        self.state = State::AwaitVerify;
                        self.deadline = now_ms.saturating_add(self.timeouts.verify_ms);
                        vec![self.progress(self.block_count)]
                    }
                } else if self.retries < self.max_block_retries {
                    self.retries += 1;
                    self.arm_stall(now_ms);
                    vec![FxAction::SendBlock {
                        block_index: block,
                        missing: Some(missing_frag_bitmap.clone()),
                    }]
                } else {
                    self.finish(TransferResult::Failed {
                        reason: Reason::Stall,
                    })
                }
            }
            (State::AwaitVerify, FxFrame::FileComplete { status, .. }) => {
                self.finish(TransferResult::Sent {
                    peer_verified: status.is_ok(),
                })
            }
            _ => Vec::new(),
        }
    }

    /// Fire time-based transitions (offer/verify wait and block stall). Call each tick.
    pub fn poll_timeout(&mut self, now_ms: u64) -> Vec<FxAction> {
        if self.state == State::Terminal || now_ms < self.deadline {
            return Vec::new();
        }
        match self.state {
            State::Offering => self.finish(TransferResult::Failed {
                reason: Reason::Timeout,
            }),
            State::Sending { .. } => self.finish(TransferResult::Failed {
                reason: Reason::Stall,
            }),
            // No FileComplete arrived; the file is on the peer but unconfirmed.
            State::AwaitVerify => self.finish(TransferResult::Sent {
                peer_verified: false,
            }),
            State::Terminal => Vec::new(),
        }
    }

    /// Operator-initiated cancel: announce it on air and finish.
    pub fn cancel(&mut self) -> Vec<FxAction> {
        if self.state == State::Terminal {
            return Vec::new();
        }
        let frame = FxFrame::FileCancel {
            transfer_id: self.transfer_id,
            reason: Reason::OperatorCancel,
        };
        let mut actions = vec![FxAction::Transmit(frame.encode())];
        actions.extend(self.finish(TransferResult::Cancelled {
            reason: Reason::OperatorCancel,
        }));
        actions
    }

    /// Whether the session has reached a terminal state (the caller may drop it).
    pub fn is_terminal(&self) -> bool {
        self.state == State::Terminal
    }

    fn begin_block(&mut self, block: u16, missing: Option<Vec<u8>>, now_ms: u64) -> Vec<FxAction> {
        self.state = State::Sending { block };
        self.arm_stall(now_ms);
        vec![
            FxAction::SendBlock {
                block_index: block,
                missing,
            },
            self.progress(block),
        ]
    }

    fn arm_stall(&mut self, now_ms: u64) {
        self.deadline = now_ms.saturating_add(self.timeouts.block_stall_ms);
    }

    fn progress(&self, blocks_done: u16) -> FxAction {
        FxAction::Progress {
            transfer_id: self.transfer_id,
            blocks_done,
            blocks_total: self.block_count,
        }
    }

    fn finish(&mut self, result: TransferResult) -> Vec<FxAction> {
        self.state = State::Terminal;
        vec![FxAction::Finished(Outcome {
            transfer_id: self.transfer_id,
            result,
        })]
    }
}

/// Bit `i` of a little-endian-by-byte bitmap (byte `i/8`, bit `i%8`).
fn bit_is_set(bitmap: &[u8], i: usize) -> bool {
    bitmap.get(i / 8).is_some_and(|b| b & (1 << (i % 8)) != 0)
}
