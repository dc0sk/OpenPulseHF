//! Daemon-side file-transfer glue (FF-16): both directions — policy, file I/O, quota, and driving the
//! [`openpulse_filexfer`] `ReceiverSession`/`SenderSession` state machines from control frames.
//!
//! **Receive:** `process_received_bytes` routes a reassembly here by SAR segment-id (0 → handshake,
//! else → here, §6.2). Control frames (segment `0xFFFF`) are reassembled + decoded; block-data frames
//! (segment `block_index + 1`) feed the receive session's `BlockAssembler`.
//!
//! **Send:** the `SendFile` command builds a `SenderSession` and queues the offer; the receiver's
//! `FileAccept`/`BlockAck`/`FileComplete` control frames (which arrive on the same seam) drive the next
//! block out. Delivery is event-reactive, so no separate tick loop is needed.
//!
//! **Transmit path:** this module never touches the modem — it queues fragments onto
//! `RuntimeControlState::filexfer_tx_queue`, and `server::run` drains that queue as one PTT-keyed burst
//! (`drain_filexfer_tx`) after each command / receive tick, so the half-duplex PTT sequencing lives with
//! the controller and works on real radio.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::broadcast;

use openpulse_config::FileTransferConfig;
use openpulse_core::manifest::{verify_manifest, TransferManifest};
use openpulse_core::sar::sar_encode;
use openpulse_filexfer::{
    decide, encode_block, sanitize_filename, BlockAssembler, BlockEvent, CompleteStatus, FileOffer,
    FxAction, FxFrame, OfferDecision, OfferPolicy, Outcome, Reason, ReceiverSession, SenderSession,
    Timeouts, TransferResult, DEFAULT_BLOCK_SIZE,
};

use crate::protocol::ControlEvent;
use crate::RuntimeControlState;

/// SAR segment-id carrying single-fragment `OPFX` control frames. Block-data frames use
/// `block_index + 1` (1..=0xFFFE); handshake frames use 0. The ranges never overlap.
pub const FX_CONTROL_SEGMENT_ID: u16 = 0xFFFF;

/// SAR session key for reassembling `OPFX` control frames.
const FX_CONTROL_SESSION: &str = "filexfer-ctrl";

/// Storage + acceptance policy resolved from `[file_transfer]` config.
#[derive(Debug, Clone)]
pub struct FileTransferPolicy {
    /// Offer accept/reject gates (enabled, size cap, auto-accept, require-verified).
    pub offer: OfferPolicy,
    /// Directory received files are written under (`~` expanded).
    pub download_dir: PathBuf,
    /// Per-peer retained-bytes quota (0 = unlimited).
    pub per_peer_quota_bytes: u64,
    /// Callsign allowlist (upper-cased); empty = any peer passing the trust gate.
    pub allowed_peers: Vec<String>,
    /// Hours a resumable partial is kept before purge (0 = keep indefinitely).
    pub partial_ttl_hours: u64,
    /// Max estimated on-air seconds per keyed TX burst (airtime-bounded PTT sequencing).
    pub burst_max_secs: f64,
    /// Session timeouts.
    pub timeouts: Timeouts,
}

impl FileTransferPolicy {
    /// Build from `[file_transfer]` config.
    pub fn from_config(cfg: &FileTransferConfig) -> Self {
        Self {
            offer: OfferPolicy {
                enabled: cfg.enabled,
                max_file_bytes: cfg.max_file_bytes,
                auto_accept_max_bytes: cfg.auto_accept_max_bytes,
                require_verified_peer: cfg.require_verified_peer,
            },
            download_dir: expand_tilde(&cfg.download_dir),
            per_peer_quota_bytes: cfg.per_peer_quota_bytes,
            allowed_peers: cfg.allowed_peers.iter().map(|s| s.to_uppercase()).collect(),
            partial_ttl_hours: cfg.partial_ttl_hours,
            burst_max_secs: cfg.burst_max_secs,
            timeouts: Timeouts {
                offer_ms: cfg.offer_timeout_secs.saturating_mul(1000),
                ..Timeouts::default()
            },
        }
    }

    fn peer_allowed(&self, callsign: &str) -> bool {
        self.allowed_peers.is_empty() || self.allowed_peers.contains(&callsign.to_uppercase())
    }
}

impl Default for FileTransferPolicy {
    fn default() -> Self {
        Self::from_config(&FileTransferConfig::default())
    }
}

/// Active receive-side session context (one transfer per link in v1).
pub struct FxRxState {
    receiver: ReceiverSession,
    assembler: BlockAssembler,
    offer: FileOffer,
    from: String,
    peer_pubkey: Option<[u8; 32]>,
    file_received_emitted: bool,
    /// Directory holding this transfer's resumable partial blocks (`…/.partial/<sha256>/`).
    partial_dir: PathBuf,
}

/// Active send-side session context (one transfer per link in v1).
pub struct FxTxState {
    sender: SenderSession,
    file: Vec<u8>,
    offer: FileOffer,
    to: String,
}

impl FxTxState {
    /// The transfer's random id.
    pub fn transfer_id(&self) -> u32 {
        self.offer.transfer_id
    }
    /// Number of blocks the file splits into.
    pub fn block_count(&self) -> u16 {
        self.offer.block_count
    }
}

/// Route one inbound SAR fragment on the file-transfer path (segment-id ≠ 0).
pub fn route_inbound_fragment(
    bytes: &[u8],
    segment_id: u16,
    runtime_state: &mut RuntimeControlState,
    event_tx: &Arc<broadcast::Sender<ControlEvent>>,
    mode: &str,
) {
    runtime_state.filexfer_frames_routed = runtime_state.filexfer_frames_routed.saturating_add(1);

    if segment_id == FX_CONTROL_SEGMENT_ID {
        let assembled = match runtime_state.filexfer_sar.ingest(FX_CONTROL_SESSION, bytes) {
            Ok(Some(full)) => full,
            _ => return,
        };
        match FxFrame::decode(&assembled) {
            Ok(FxFrame::FileOffer(offer)) => on_offer(offer, runtime_state, event_tx, mode),
            Ok(FxFrame::FileCancel {
                transfer_id,
                reason,
            }) => {
                // A cancel can target either the inbound (receive) or outbound (send) session.
                on_inbound_cancel(transfer_id, reason, runtime_state, event_tx);
                on_tx_cancel(transfer_id, reason, runtime_state, event_tx);
            }
            // Receiver → sender control drives the active send session.
            Ok(
                frame @ (FxFrame::FileAccept { .. }
                | FxFrame::BlockAck { .. }
                | FxFrame::FileComplete { .. }
                | FxFrame::FileReject { .. }),
            ) => on_tx_frame(frame, runtime_state, event_tx, mode),
            Ok(_) => {}
            Err(e) => tracing::debug!(error = %e, "filexfer: control frame decode failed"),
        }
    } else {
        on_block_fragment(bytes, runtime_state, event_tx, mode);
    }
}

/// Handle an inbound `FileOffer`: verify, apply policy, emit `FileOffered`, and either auto-accept
/// (transmit `FileAccept`), prompt (await `AcceptFile`), or reject on air.
fn on_offer(
    offer: FileOffer,
    rs: &mut RuntimeControlState,
    event_tx: &Arc<broadcast::Sender<ControlEvent>>,
    mode: &str,
) {
    // One transfer per link: reject a second offer while one is active.
    if rs.file_rx.is_some() {
        enqueue_ctrl(rs, mode, &reject(offer.transfer_id, Reason::Busy));
        return;
    }

    let (from, peer_pubkey) = match &rs.verified_peer {
        Some(vp) => (
            vp.callsign.clone(),
            <[u8; 32]>::try_from(vp.pubkey.as_slice()).ok(),
        ),
        None => (String::new(), None),
    };
    let sig_valid = peer_pubkey
        .map(|pk| offer.verify_signature(&pk).is_ok())
        .unwrap_or(false);

    let decision = if !rs.filexfer_policy.peer_allowed(&from) {
        OfferDecision::Reject(Reason::UntrustedPeer)
    } else if !offer_geometry_ok(&offer) {
        // Reject a malformed/hostile geometry (block_size out of range, or block_count inconsistent with
        // file_size) up front — otherwise a crafted `block_count` decouples the size gate/quota (which key
        // on `file_size`) from the bytes actually reassembled and written. Audit #13.
        OfferDecision::Reject(Reason::TooLarge)
    } else if quota_would_exceed(&rs.filexfer_policy, &from, offer.file_size) {
        OfferDecision::Reject(Reason::QuotaExceeded)
    } else {
        decide(&offer, &rs.filexfer_policy.offer, sig_valid)
    };

    let _ = event_tx.send(ControlEvent::FileOffered {
        transfer_id: offer.transfer_id,
        from: from.clone(),
        name: offer.name.clone(),
        size: offer.file_size,
        sha256_hex: hex(&offer.sha256),
        mime: offer.mime.clone(),
        auto_accepted: matches!(decision, OfferDecision::AutoAccept),
        signature_valid: sig_valid,
    });

    // Resume: reclaim any blocks persisted from an earlier interrupted transfer of the *same content*
    // (keyed by the offer's SHA-256), seed them into the assembler, and announce them via `resume` so
    // the sender skips them. TTL-purge stale partials for this peer first.
    purge_stale_partials(&rs.filexfer_policy, &from);
    let partial_dir = partial_dir_for(&rs.filexfer_policy, &from, &offer.sha256);
    let mut assembler = BlockAssembler::new(offer.transfer_id, offer.block_count);
    let held = load_partials(&offer, &partial_dir, &mut assembler);
    let (receiver, actions) = ReceiverSession::resume(
        &offer,
        decision,
        &held,
        rs.filexfer_policy.timeouts,
        now_ms(),
    );
    let mut fx = FxRxState {
        receiver,
        assembler,
        offer,
        from,
        peer_pubkey,
        file_received_emitted: false,
        partial_dir,
    };
    drive_rx_actions(&mut fx, actions, rs, event_tx, mode);
    if !fx.receiver.is_terminal() {
        rs.file_rx = Some(fx);
    }
}

/// Operator accepts a prompted offer (`AcceptFile` command).
pub fn accept_offer(
    transfer_id: u32,
    rs: &mut RuntimeControlState,
    event_tx: &Arc<broadcast::Sender<ControlEvent>>,
    mode: &str,
) {
    if let Some(mut fx) = rs.file_rx.take() {
        if fx.offer.transfer_id == transfer_id {
            let actions = fx.receiver.accept(now_ms());
            drive_rx_actions(&mut fx, actions, rs, event_tx, mode);
        }
        if !fx.receiver.is_terminal() {
            rs.file_rx = Some(fx);
        }
    }
}

/// Operator rejects a prompted offer (`RejectFile` command).
pub fn reject_offer(
    transfer_id: u32,
    rs: &mut RuntimeControlState,
    event_tx: &Arc<broadcast::Sender<ControlEvent>>,
    mode: &str,
) {
    if let Some(mut fx) = rs.file_rx.take() {
        if fx.offer.transfer_id == transfer_id {
            let actions = fx.receiver.reject(Reason::OperatorDeclined);
            drive_rx_actions(&mut fx, actions, rs, event_tx, mode);
        }
        if !fx.receiver.is_terminal() {
            rs.file_rx = Some(fx);
        }
    }
}

/// Operator cancels the active receive (`CancelFile` command): announce on air and drop the session.
pub fn cancel_transfer(
    transfer_id: u32,
    rs: &mut RuntimeControlState,
    event_tx: &Arc<broadcast::Sender<ControlEvent>>,
    mode: &str,
) {
    if rs.file_rx.as_ref().map(|fx| fx.offer.transfer_id) == Some(transfer_id) {
        enqueue_ctrl(
            rs,
            mode,
            &FxFrame::FileCancel {
                transfer_id,
                reason: Reason::OperatorCancel,
            }
            .encode(),
        );
        rs.file_rx = None;
        let _ = event_tx.send(ControlEvent::FileFailed {
            transfer_id,
            direction: "rx".into(),
            reason: "operator-cancel".into(),
        });
    }
}

fn on_inbound_cancel(
    transfer_id: u32,
    reason: Reason,
    rs: &mut RuntimeControlState,
    event_tx: &Arc<broadcast::Sender<ControlEvent>>,
) {
    if rs.file_rx.as_ref().map(|fx| fx.offer.transfer_id) == Some(transfer_id) {
        rs.file_rx = None;
        let _ = event_tx.send(ControlEvent::FileFailed {
            transfer_id,
            direction: "rx".into(),
            reason: format!("{reason:?}"),
        });
    }
}

// ── send side ───────────────────────────────────────────────────────────────

/// Start sending a daemon-host-local file to `to` (the `SendFile` command). Reads + size-checks the
/// file, signs its manifest with the station key, builds the `SenderSession`, and transmits the offer.
pub fn send_file(
    to: &str,
    path: &str,
    rs: &mut RuntimeControlState,
    event_tx: &Arc<broadcast::Sender<ControlEvent>>,
    mode: &str,
) {
    let fail = |reason: String| {
        let _ = event_tx.send(ControlEvent::CommandError {
            command: "send_file".into(),
            reason,
        });
    };
    if rs.file_tx.is_some() || rs.file_rx.is_some() {
        return fail("a file transfer is already active".into());
    }
    let file = match std::fs::read(path) {
        Ok(f) => f,
        Err(e) => return fail(format!("cannot read '{path}': {e}")),
    };
    if file.len() as u64 > rs.filexfer_policy.offer.max_file_bytes {
        return fail(format!(
            "file is {} bytes, over max_file_bytes {}",
            file.len(),
            rs.filexfer_policy.offer.max_file_bytes
        ));
    }
    let manifest = match TransferManifest::sign(&file, &rs.local_callsign, &rs.station_seed) {
        Ok(m) => m,
        Err(_) => return fail("failed to sign the transfer manifest".into()),
    };
    let name = std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file.bin".into());
    let transfer_id = (now_ms() as u32) ^ 0x5f5f_5f5f;
    let offer = match FileOffer::from_manifest(
        transfer_id,
        &manifest,
        &name,
        "application/octet-stream",
        DEFAULT_BLOCK_SIZE,
    ) {
        Some(o) => o,
        None => return fail("could not build the offer (file too large?)".into()),
    };
    let (sender, actions) =
        SenderSession::new(offer.clone(), rs.filexfer_policy.timeouts, now_ms());
    let mut fx = FxTxState {
        sender,
        file,
        offer,
        to: to.to_string(),
    };
    drive_tx_actions(&mut fx, actions, rs, event_tx, mode);
    if !fx.sender.is_terminal() {
        rs.file_tx = Some(fx);
    }
}

/// Drive the active send session with an inbound receiver → sender frame (`FileAccept`/`BlockAck`/
/// `FileComplete`/`FileReject`).
fn on_tx_frame(
    frame: FxFrame,
    rs: &mut RuntimeControlState,
    event_tx: &Arc<broadcast::Sender<ControlEvent>>,
    mode: &str,
) {
    let Some(mut fx) = rs.file_tx.take() else {
        return;
    };
    if frame.transfer_id() == fx.offer.transfer_id {
        let actions = fx.sender.apply(&frame, now_ms());
        drive_tx_actions(&mut fx, actions, rs, event_tx, mode);
    }
    if !fx.sender.is_terminal() {
        rs.file_tx = Some(fx);
    }
}

fn on_tx_cancel(
    transfer_id: u32,
    reason: Reason,
    rs: &mut RuntimeControlState,
    event_tx: &Arc<broadcast::Sender<ControlEvent>>,
) {
    if rs.file_tx.as_ref().map(|fx| fx.offer.transfer_id) == Some(transfer_id) {
        rs.file_tx = None;
        let _ = event_tx.send(ControlEvent::FileFailed {
            transfer_id,
            direction: "tx".into(),
            reason: format!("{reason:?}"),
        });
    }
}

/// Execute the `FxAction`s a send session emits: transmit control frames and data blocks, report
/// progress, and emit the terminal `FileSent`/`FileFailed` event.
fn drive_tx_actions(
    fx: &mut FxTxState,
    actions: Vec<FxAction>,
    rs: &mut RuntimeControlState,
    event_tx: &Arc<broadcast::Sender<ControlEvent>>,
    mode: &str,
) {
    for action in actions {
        match action {
            FxAction::Transmit(frame) => enqueue_ctrl(rs, mode, &frame),
            FxAction::SendBlock {
                block_index,
                missing,
            } => {
                let bs = fx.offer.block_size as usize;
                let start = (block_index as usize).saturating_mul(bs).min(fx.file.len());
                let end = start.saturating_add(bs).min(fx.file.len());
                let transfer_id = fx.offer.transfer_id;
                let block = fx.file[start..end].to_vec();
                enqueue_block(
                    rs,
                    mode,
                    transfer_id,
                    block_index,
                    &block,
                    missing.as_deref(),
                );
            }
            FxAction::Progress {
                transfer_id,
                blocks_done,
                blocks_total,
            } => {
                let _ = event_tx.send(ControlEvent::FileProgress {
                    transfer_id,
                    direction: "tx".into(),
                    name: fx.offer.name.clone(),
                    blocks_done,
                    blocks_total,
                    bytes_done: block_bytes(&fx.offer, blocks_done),
                    bytes_total: fx.offer.file_size,
                });
            }
            FxAction::Finished(Outcome {
                transfer_id,
                result,
            }) => match result {
                TransferResult::Sent { peer_verified } => {
                    let _ = event_tx.send(ControlEvent::FileSent {
                        transfer_id,
                        to: fx.to.clone(),
                        name: fx.offer.name.clone(),
                        receipt_valid: Some(peer_verified),
                    });
                }
                TransferResult::Rejected { reason }
                | TransferResult::Cancelled { reason }
                | TransferResult::Failed { reason } => {
                    let _ = event_tx.send(ControlEvent::FileFailed {
                        transfer_id,
                        direction: "tx".into(),
                        reason: format!("{reason:?}"),
                    });
                }
                TransferResult::Received { .. } => {}
            },
            FxAction::Verify { .. } | FxAction::Prompt { .. } => {}
        }
    }
}

/// Feed one block-data fragment into the active receive session; on a completed block send a
/// `BlockAck` and, once every block is in, reassemble → verify → write → `FileComplete`.
fn on_block_fragment(
    bytes: &[u8],
    rs: &mut RuntimeControlState,
    event_tx: &Arc<broadcast::Sender<ControlEvent>>,
    mode: &str,
) {
    let Some(mut fx) = rs.file_rx.take() else {
        return;
    };
    if let BlockEvent::Complete { block_index } = fx.assembler.ingest_fragment(bytes) {
        // Persist the just-completed block so an interrupted transfer can resume (disjoint field
        // borrows: `assembler` and `partial_dir` are separate fields).
        if let Some(block) = fx.assembler.block(block_index) {
            persist_block(&fx.partial_dir, block_index, block);
        }
        enqueue_ctrl(
            rs,
            mode,
            &FxFrame::BlockAck {
                transfer_id: fx.offer.transfer_id,
                block_index,
                complete: true,
                missing_frag_bitmap: Vec::new(),
            }
            .encode(),
        );
        let actions = fx.receiver.note_block_complete(block_index, now_ms());
        drive_rx_actions(&mut fx, actions, rs, event_tx, mode);
    }
    if !fx.receiver.is_terminal() {
        rs.file_rx = Some(fx);
    }
}

/// Execute the `FxAction`s a receive session emits.
fn drive_rx_actions(
    fx: &mut FxRxState,
    actions: Vec<FxAction>,
    rs: &mut RuntimeControlState,
    event_tx: &Arc<broadcast::Sender<ControlEvent>>,
    mode: &str,
) {
    for action in actions {
        match action {
            FxAction::Transmit(frame) => enqueue_ctrl(rs, mode, &frame),
            FxAction::Progress {
                transfer_id,
                blocks_done,
                blocks_total,
            } => {
                let _ = event_tx.send(ControlEvent::FileProgress {
                    transfer_id,
                    direction: "rx".into(),
                    name: fx.offer.name.clone(),
                    blocks_done,
                    blocks_total,
                    bytes_done: block_bytes(&fx.offer, blocks_done),
                    bytes_total: fx.offer.file_size,
                });
            }
            FxAction::Verify { .. } => {
                let (status, countersig) = reassemble_verify_write(fx, rs, event_tx);
                let more = fx.receiver.set_verify_result(status, countersig);
                drive_rx_actions(fx, more, rs, event_tx, mode);
            }
            FxAction::Finished(Outcome {
                transfer_id,
                result,
            }) => {
                emit_terminal(transfer_id, &result, fx, event_tx);
            }
            // Sender-only / already-prompted actions have no receive-side effect.
            FxAction::SendBlock { .. } | FxAction::Prompt { .. } => {}
        }
    }
}

/// Reassemble the received blocks, verify the signed manifest, and write the file (or quarantine it
/// on a hash mismatch). Returns the `FileComplete` status + the delivery-receipt countersignature.
fn reassemble_verify_write(
    fx: &mut FxRxState,
    rs: &mut RuntimeControlState,
    event_tx: &Arc<broadcast::Sender<ControlEvent>>,
) -> (CompleteStatus, [u8; 64]) {
    let Some(payload) = fx.assembler.reassemble() else {
        return (CompleteStatus::SizeMismatch, [0u8; 64]);
    };
    let manifest = fx.offer.to_manifest();
    let pubkey = fx.peer_pubkey.unwrap_or([0u8; 32]);

    // Evaluate the two integrity axes independently so the file is always written with an accurate
    // badge (never silently dropped): the content is intact iff its hash matches the offer's, and it
    // is *authenticated* iff a verified-peer key also validates the signature over that hash.
    let hash_ok = {
        use sha2::{Digest, Sha256};
        Sha256::digest(&payload).as_slice() == manifest.payload_hash.as_slice()
    };
    let sig_ok = fx.peer_pubkey.is_some() && verify_manifest(&manifest, &pubkey).is_ok();
    let verified = hash_ok && sig_ok;
    let status = if verified {
        CompleteStatus::VerifiedOk
    } else if !hash_ok {
        CompleteStatus::HashMismatch // corrupted content → quarantine
    } else {
        CompleteStatus::SignatureInvalid // intact but unsigned/unknown peer → quarantine
    };

    // The file is fully assembled — the resume partials are no longer needed (a re-offer would start
    // fresh anyway; on a hash mismatch that is the correct outcome).
    clear_partials(&fx.partial_dir);

    match write_file(rs, &fx.from, &fx.offer.name, &payload, verified) {
        Ok(path) => {
            fx.file_received_emitted = true;
            let timestamp_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            // Record it for `ListFiles` so late-connecting clients see completed transfers.
            rs.received_files.push(crate::protocol::FileSummary {
                name: fx.offer.name.clone(),
                from: fx.from.clone(),
                size: payload.len() as u64,
                verified,
                path: path.clone(),
                timestamp_secs,
            });
            let _ = event_tx.send(ControlEvent::FileReceived {
                transfer_id: fx.offer.transfer_id,
                from: fx.from.clone(),
                name: fx.offer.name.clone(),
                size: payload.len() as u64,
                path,
                verified,
            });
        }
        Err(e) => tracing::warn!(error = %e, "filexfer: file write failed"),
    }

    let countersig = if verified {
        countersign(&payload, &fx.offer.sender_id, &rs.station_seed)
    } else {
        [0u8; 64]
    };
    (status, countersig)
}

fn emit_terminal(
    transfer_id: u32,
    result: &TransferResult,
    fx: &FxRxState,
    event_tx: &Arc<broadcast::Sender<ControlEvent>>,
) {
    match result {
        // FileReceived was already emitted at write time with the path + verified flag.
        TransferResult::Received { .. } if fx.file_received_emitted => {}
        TransferResult::Received { verified } => {
            let _ = event_tx.send(ControlEvent::FileReceived {
                transfer_id,
                from: fx.from.clone(),
                name: fx.offer.name.clone(),
                size: fx.offer.file_size,
                path: String::new(),
                verified: *verified,
            });
        }
        TransferResult::Rejected { reason }
        | TransferResult::Cancelled { reason }
        | TransferResult::Failed { reason } => {
            let _ = event_tx.send(ControlEvent::FileFailed {
                transfer_id,
                direction: "rx".into(),
                reason: format!("{reason:?}"),
            });
        }
        TransferResult::Sent { .. } => {}
    }
}

/// SAR-encode a control frame (reserved control segment-id) and queue its fragments for the
/// PTT-sequenced burst drain in `server::run`.
fn enqueue_ctrl(rs: &mut RuntimeControlState, mode: &str, frame: &[u8]) {
    match sar_encode(FX_CONTROL_SEGMENT_ID, frame) {
        Ok(fragments) => {
            for frag in fragments {
                rs.filexfer_tx_queue.push((frag, mode.to_string()));
            }
        }
        Err(e) => tracing::warn!(error = %e, "filexfer: control frame SAR encode failed"),
    }
}

/// Queue a block's fragments (segment-id `block_index + 1`) for the PTT-sequenced burst drain.
fn enqueue_block(
    rs: &mut RuntimeControlState,
    mode: &str,
    transfer_id: u32,
    block_index: u16,
    block: &[u8],
    missing: Option<&[u8]>,
) {
    match encode_block(transfer_id, block_index, block, missing) {
        Ok(fragments) => {
            for frag in fragments {
                rs.filexfer_tx_queue.push((frag, mode.to_string()));
            }
        }
        Err(e) => tracing::warn!(error = %e, "filexfer: block encode failed"),
    }
}

/// Write `payload` under `download_dir/<peer>/`, sanitizing the filename and never overwriting.
/// `verified = false` appends `.unverified` so a quarantined file is visibly distinct.
fn write_file(
    rs: &RuntimeControlState,
    from: &str,
    name: &str,
    payload: &[u8],
    verified: bool,
) -> std::io::Result<String> {
    let peer = sanitize_filename(if from.is_empty() { "unknown" } else { from });
    let dir = rs.filexfer_policy.download_dir.join(peer);
    std::fs::create_dir_all(&dir)?;
    let mut base = sanitize_filename(name);
    if !verified {
        base.push_str(".unverified");
    }
    let path = unique_path(&dir, &base);
    std::fs::write(&path, payload)?;
    Ok(path.to_string_lossy().into_owned())
}

/// A non-existent path in `dir` for `name`, appending ` (n)` before the extension on collision.
fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) => (s.to_string(), format!(".{e}")),
        None => (name.to_string(), String::new()),
    };
    for n in 1..10_000 {
        let c = dir.join(format!("{stem} ({n}){ext}"));
        if !c.exists() {
            return c;
        }
    }
    dir.join(name)
}

/// Whether an offer's block geometry is internally consistent: `block_size` within the protocol window
/// and `block_count` exactly the count `file_size` splits into. A mismatch means the declared `file_size`
/// (which the size gate + quota key on) does not bound the bytes actually reassembled — reject it.
fn offer_geometry_ok(offer: &openpulse_filexfer::FileOffer) -> bool {
    if !(openpulse_filexfer::MIN_BLOCK_SIZE..=openpulse_filexfer::MAX_BLOCK_SIZE)
        .contains(&offer.block_size)
    {
        return false;
    }
    openpulse_filexfer::block_count(offer.file_size, offer.block_size) == Some(offer.block_count)
}

/// True if accepting `incoming` bytes for `from` would exceed the per-peer quota (0 = unlimited).
fn quota_would_exceed(policy: &FileTransferPolicy, from: &str, incoming: u64) -> bool {
    if policy.per_peer_quota_bytes == 0 {
        return false;
    }
    let peer = sanitize_filename(if from.is_empty() { "unknown" } else { from });
    let used = dir_size(&policy.download_dir.join(peer));
    used.saturating_add(incoming) > policy.per_peer_quota_bytes
}

/// Total bytes of all files under `dir`, **recursively**. The quota must count the peer's `.partial/`
/// subtree (in-flight resumable blocks) too, otherwise a peer can accumulate unbounded bytes there
/// while `quota_would_exceed` reports them under quota. `DirEntry::metadata` does not traverse
/// symlinks, so a symlinked directory reports as neither file nor dir and is skipped (no loop risk).
fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut total = 0u64;
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_file() {
            total = total.saturating_add(meta.len());
        } else if meta.is_dir() {
            total = total.saturating_add(dir_size(&entry.path()));
        }
    }
    total
}

// ── resume: persisted partial blocks ─────────────────────────────────────────

/// Subdirectory under a peer's download dir holding resumable partial blocks, keyed by file hash.
const PARTIAL_SUBDIR: &str = ".partial";

/// The partial-blocks dir for one transfer: `download_dir/<peer>/.partial/<sha256hex>/`.
fn partial_dir_for(policy: &FileTransferPolicy, from: &str, sha256: &[u8]) -> PathBuf {
    let peer = sanitize_filename(if from.is_empty() { "unknown" } else { from });
    policy
        .download_dir
        .join(peer)
        .join(PARTIAL_SUBDIR)
        .join(hex(sha256))
}

/// Bytes block `block_index` should contain (the last block is short): `min(block_size, remaining)`.
fn expected_block_len(offer: &FileOffer, block_index: u16) -> usize {
    let bs = offer.block_size as u64;
    let start = (block_index as u64).saturating_mul(bs);
    offer.file_size.saturating_sub(start).min(bs) as usize
}

/// Write one completed block to `<partial_dir>/<block_index>.blk` (best-effort — a persistence
/// failure only forfeits resumability, never the live transfer).
fn persist_block(partial_dir: &Path, block_index: u16, block: &[u8]) {
    if let Err(e) = std::fs::create_dir_all(partial_dir) {
        tracing::debug!(error = %e, "filexfer: cannot create partial dir");
        return;
    }
    let path = partial_dir.join(format!("{block_index}.blk"));
    if let Err(e) = std::fs::write(&path, block) {
        tracing::debug!(error = %e, "filexfer: cannot persist partial block");
    }
}

/// Load persisted blocks for a resumed transfer into `assembler`, returning the per-block held mask.
/// A `.blk` whose length doesn't match the expected block size is skipped (cheap corruption guard).
fn load_partials(
    offer: &FileOffer,
    partial_dir: &Path,
    assembler: &mut BlockAssembler,
) -> Vec<bool> {
    let mut held = vec![false; offer.block_count as usize];
    let Ok(entries) = std::fs::read_dir(partial_dir) else {
        return held;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(idx_str) = name
            .to_string_lossy()
            .strip_suffix(".blk")
            .map(str::to_string)
        else {
            continue;
        };
        let Ok(idx) = idx_str.parse::<u16>() else {
            continue;
        };
        if idx >= offer.block_count {
            continue;
        }
        let Ok(bytes) = std::fs::read(entry.path()) else {
            continue;
        };
        if bytes.len() != expected_block_len(offer, idx) {
            continue; // stale/corrupt partial from a different offer — re-fetch this block
        }
        assembler.seed_block(idx, bytes);
        held[idx as usize] = true;
    }
    held
}

/// Delete a completed transfer's partial-blocks dir.
fn clear_partials(partial_dir: &Path) {
    let _ = std::fs::remove_dir_all(partial_dir);
}

/// Remove partial dirs under `<peer>/.partial` older than the configured TTL (0 = keep indefinitely).
fn purge_stale_partials(policy: &FileTransferPolicy, from: &str) {
    if policy.partial_ttl_hours == 0 {
        return;
    }
    let peer = sanitize_filename(if from.is_empty() { "unknown" } else { from });
    let base = policy.download_dir.join(peer).join(PARTIAL_SUBDIR);
    let Ok(entries) = std::fs::read_dir(&base) else {
        return;
    };
    let ttl = std::time::Duration::from_secs(policy.partial_ttl_hours.saturating_mul(3600));
    let now = std::time::SystemTime::now();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let stale = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| now.duration_since(t).ok())
            .map(|age| age > ttl)
            .unwrap_or(false);
        if stale {
            let _ = std::fs::remove_dir_all(&path);
        }
    }
}

/// Countersign the manifest body (proof of verified receipt) with the local station key.
fn countersign(payload: &[u8], sender_id: &str, seed: &[u8; 32]) -> [u8; 64] {
    TransferManifest::sign(payload, sender_id, seed)
        .ok()
        .and_then(|m| <[u8; 64]>::try_from(m.signature.as_slice()).ok())
        .unwrap_or([0u8; 64])
}

fn block_bytes(offer: &FileOffer, blocks_done: u16) -> u64 {
    (blocks_done as u64)
        .saturating_mul(offer.block_size as u64)
        .min(offer.file_size)
}

fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}

fn reject(transfer_id: u32, reason: Reason) -> Vec<u8> {
    FxFrame::FileReject {
        transfer_id,
        reason,
    }
    .encode()
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use openpulse_core::manifest::TransferManifest;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn tmp_dir() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let d = std::env::temp_dir().join(format!("ophf-fx-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn policy_with(dir: PathBuf, ttl: u64) -> FileTransferPolicy {
        FileTransferPolicy {
            download_dir: dir,
            partial_ttl_hours: ttl,
            ..Default::default()
        }
    }

    fn offer_for(payload: &[u8], block_size: u32) -> FileOffer {
        let mut seed = [0u8; 32];
        seed[0] = 9;
        let manifest = TransferManifest::sign(payload, "W1AW", &seed).unwrap();
        FileOffer::from_manifest(
            1,
            &manifest,
            "f.bin",
            "application/octet-stream",
            block_size,
        )
        .unwrap()
    }

    #[test]
    fn offer_geometry_check_rejects_inconsistent_block_count() {
        // Audit #13: a well-formed offer passes; a tampered one whose block_count decouples from
        // file_size (the size gate + quota key on file_size) is rejected.
        let file: Vec<u8> = (0..2500u32).map(|i| i as u8).collect(); // 3 × 1024-byte blocks
        let good = offer_for(&file, 1024);
        assert!(offer_geometry_ok(&good), "a well-formed offer is accepted");

        // Inflate block_count far beyond what file_size implies (the finding's gigabytes-bypass shape).
        let mut evil = good.clone();
        evil.block_count = 65535;
        assert!(
            !offer_geometry_ok(&evil),
            "an inflated block_count must be rejected"
        );

        // A block_size outside the protocol window is also rejected.
        let mut bad_bs = good.clone();
        bad_bs.block_size = 10; // below MIN_BLOCK_SIZE
        assert!(
            !offer_geometry_ok(&bad_bs),
            "sub-minimum block_size rejected"
        );
    }

    #[test]
    fn dir_size_counts_the_partial_subtree_for_quota() {
        // The per-peer quota must include bytes held in the `.partial/` subtree (in-flight resumable
        // blocks), not just top-level received files — otherwise a peer accumulates unbounded data
        // there while `quota_would_exceed` reports them under quota.
        let peer_dir = tmp_dir();
        std::fs::write(peer_dir.join("received.bin"), vec![0u8; 100]).unwrap();
        let partial = peer_dir.join(PARTIAL_SUBDIR).join("abc123hex");
        std::fs::create_dir_all(&partial).unwrap();
        std::fs::write(partial.join("0.blk"), vec![0u8; 250]).unwrap();
        std::fs::write(partial.join("1.blk"), vec![0u8; 150]).unwrap();

        assert_eq!(
            dir_size(&peer_dir),
            500,
            "dir_size must count the 100-byte received file plus the 400 bytes of .partial blocks"
        );
    }

    #[test]
    fn persisted_blocks_reload_into_held_mask_and_assembler() {
        let root = tmp_dir();
        let file: Vec<u8> = (0..2500u32).map(|i| i as u8).collect(); // 3 × 1024-byte blocks
        let offer = offer_for(&file, 1024);
        assert_eq!(offer.block_count, 3);
        let dir = partial_dir_for(&policy_with(root, 72), "W1AW", &offer.sha256);

        let blocks = openpulse_filexfer::split_blocks(&file, 1024);
        persist_block(&dir, 0, blocks[0]);
        persist_block(&dir, 2, blocks[2]);

        let mut asm = BlockAssembler::new(offer.transfer_id, offer.block_count);
        let held = load_partials(&offer, &dir, &mut asm);
        assert_eq!(held, vec![true, false, true]);
        assert_eq!(asm.block(0), Some(blocks[0]));
        assert_eq!(asm.block(2), Some(blocks[2]));
        assert_eq!(asm.block(1), None);
    }

    #[test]
    fn wrong_length_partial_is_skipped() {
        let root = tmp_dir();
        let file: Vec<u8> = (0..2500u32).map(|i| i as u8).collect();
        let offer = offer_for(&file, 1024);
        let dir = partial_dir_for(&policy_with(root, 72), "W1AW", &offer.sha256);

        // A truncated block-0 file (wrong length) must not be trusted.
        persist_block(&dir, 0, &[1, 2, 3]);
        let mut asm = BlockAssembler::new(offer.transfer_id, offer.block_count);
        let held = load_partials(&offer, &dir, &mut asm);
        assert_eq!(held, vec![false, false, false]);
        assert_eq!(asm.block(0), None);
    }

    #[test]
    fn clear_partials_removes_the_dir() {
        let root = tmp_dir();
        let offer = offer_for(b"hello", 1024);
        let dir = partial_dir_for(&policy_with(root, 72), "W1AW", &offer.sha256);
        persist_block(&dir, 0, b"hello");
        assert!(dir.exists());
        clear_partials(&dir);
        assert!(!dir.exists());
    }

    #[test]
    fn partial_dir_keyed_by_content_hash() {
        let pol = policy_with(tmp_dir(), 72);
        let a = partial_dir_for(&pol, "W1AW", &[0xab; 32]);
        let a2 = partial_dir_for(&pol, "W1AW", &[0xab; 32]);
        let b = partial_dir_for(&pol, "W1AW", &[0xcd; 32]);
        assert_eq!(a, a2, "same content resumes into the same dir");
        assert_ne!(a, b, "different content keys a different dir");
    }

    #[test]
    fn fresh_partial_survives_purge_and_ttl_zero_is_noop() {
        let root = tmp_dir();
        let pol = policy_with(root, 72);
        let offer = offer_for(b"data", 1024);
        let dir = partial_dir_for(&pol, "W1AW", &offer.sha256);
        persist_block(&dir, 0, b"data");

        purge_stale_partials(&pol, "W1AW");
        assert!(dir.exists(), "a just-written partial is not stale");

        let keep_forever = policy_with(pol.download_dir.clone(), 0);
        purge_stale_partials(&keep_forever, "W1AW");
        assert!(dir.exists(), "ttl 0 never purges");
    }

    #[test]
    fn expected_block_len_last_block_is_short() {
        let file = vec![0u8; 2500];
        let offer = offer_for(&file, 1024);
        assert_eq!(expected_block_len(&offer, 0), 1024);
        assert_eq!(expected_block_len(&offer, 1), 1024);
        assert_eq!(expected_block_len(&offer, 2), 452); // 2500 - 2048
    }

    #[test]
    fn resume_offer_announces_held_blocks_in_accept_bitmap() {
        // #830 coverage gap: the daemon resume path is only tested at the helper level (load_partials,
        // persist_block). This drives the whole composition — pre-place `.blk` partials, feed the offer
        // through `on_offer`, and assert the emitted `FileAccept.have_bitmap` marks exactly those blocks
        // so the sender skips them.
        use openpulse_core::sar::SarReassembler;
        use std::time::Duration;

        let root = tmp_dir();
        let file: Vec<u8> = (0..2500u32).map(|i| i as u8).collect(); // 3 × 1024-byte blocks (last is 452)
        let offer = offer_for(&file, 1024);
        assert_eq!(offer.block_count, 3);

        let policy = FileTransferPolicy::from_config(&FileTransferConfig {
            enabled: true,
            download_dir: root.to_string_lossy().into_owned(),
            auto_accept_max_bytes: u64::MAX, // auto-accept so a FileAccept is emitted
            max_file_bytes: 10 * 1024 * 1024,
            per_peer_quota_bytes: 0,
            require_verified_peer: false, // no verified_peer needed; from = "" → "unknown" peer dir
            allowed_peers: vec![],
            offer_timeout_secs: 120,
            partial_ttl_hours: 72,
            burst_max_secs: 20.0,
        });

        // Pre-place blocks 0 and 2 (not 1) into this offer's content-keyed partial dir.
        let partial_dir = partial_dir_for(&policy, "", &offer.sha256);
        let blocks = openpulse_filexfer::split_blocks(&file, 1024);
        persist_block(&partial_dir, 0, blocks[0]);
        persist_block(&partial_dir, 2, blocks[2]);

        let mut rs = RuntimeControlState {
            filexfer_policy: policy,
            ..RuntimeControlState::default()
        };
        let ev_tx = Arc::new(broadcast::channel::<ControlEvent>(16).0);

        on_offer(offer.clone(), &mut rs, &ev_tx, "BPSK250");

        // Two of three blocks held → the transfer is not complete, so the session stays live.
        assert!(
            rs.file_rx.is_some(),
            "a partial resume keeps the receive session open"
        );

        // Reassemble the queued control frames and find the FileAccept the receiver sent back.
        let mut reasm = SarReassembler::new(Duration::from_secs(60));
        let mut have_bitmap = None;
        for (frag, _mode) in &rs.filexfer_tx_queue {
            if let Ok(Some(full)) = reasm.ingest(FX_CONTROL_SESSION, frag) {
                if let Ok(FxFrame::FileAccept {
                    have_bitmap: hb, ..
                }) = FxFrame::decode(&full)
                {
                    have_bitmap = Some(hb);
                }
            }
        }
        // Bits 0 and 2 set (block 1 absent) → 0b0000_0101.
        assert_eq!(
            have_bitmap,
            Some(vec![0b0000_0101]),
            "FileAccept must announce exactly the held blocks (0 and 2) so the sender skips them"
        );
    }
}
