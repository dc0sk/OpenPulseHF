//! Daemon-side file-transfer glue (FF-16): both directions — policy, file I/O, quota, and driving the
//! [`openpulse_filexfer`] `ReceiverSession`/`SenderSession` state machines from control frames.
//!
//! **Receive:** `process_received_bytes` routes a reassembly here by SAR segment-id (0 → handshake,
//! else → here, §6.2). Control frames (segment `0xFFFF`) are reassembled + decoded; block-data frames
//! (segment `block_index + 1`) feed the receive session's `BlockAssembler`.
//!
//! **Send:** the `SendFile` command builds a `SenderSession` and transmits the offer; the receiver's
//! `FileAccept`/`BlockAck`/`FileComplete` control frames (which arrive on the same seam) drive the next
//! block out. Delivery is event-reactive, so the loopback/twin path needs no separate tick loop;
//! real-radio PTT burst sequencing is a `server::run` refinement layered on this.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::broadcast;

use openpulse_config::FileTransferConfig;
use openpulse_core::manifest::{verify_manifest_with_payload, ManifestError, TransferManifest};
use openpulse_core::sar::sar_encode;
use openpulse_filexfer::{
    decide, encode_block, sanitize_filename, BlockAssembler, BlockEvent, CompleteStatus, FileOffer,
    FxAction, FxFrame, OfferDecision, OfferPolicy, Outcome, Reason, ReceiverSession, SenderSession,
    Timeouts, TransferResult, DEFAULT_BLOCK_SIZE,
};
use openpulse_modem::ModemEngine;

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
    engine: &mut ModemEngine,
) {
    runtime_state.filexfer_frames_routed = runtime_state.filexfer_frames_routed.saturating_add(1);

    if segment_id == FX_CONTROL_SEGMENT_ID {
        let assembled = match runtime_state.filexfer_sar.ingest(FX_CONTROL_SESSION, bytes) {
            Ok(Some(full)) => full,
            _ => return,
        };
        match FxFrame::decode(&assembled) {
            Ok(FxFrame::FileOffer(offer)) => on_offer(offer, runtime_state, event_tx, mode, engine),
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
            ) => on_tx_frame(frame, runtime_state, event_tx, mode, engine),
            Ok(_) => {}
            Err(e) => tracing::debug!(error = %e, "filexfer: control frame decode failed"),
        }
    } else {
        on_block_fragment(bytes, runtime_state, event_tx, mode, engine);
    }
}

/// Handle an inbound `FileOffer`: verify, apply policy, emit `FileOffered`, and either auto-accept
/// (transmit `FileAccept`), prompt (await `AcceptFile`), or reject on air.
fn on_offer(
    offer: FileOffer,
    rs: &mut RuntimeControlState,
    event_tx: &Arc<broadcast::Sender<ControlEvent>>,
    mode: &str,
    engine: &mut ModemEngine,
) {
    // One transfer per link: reject a second offer while one is active.
    if rs.file_rx.is_some() {
        transmit_ctrl(engine, mode, &reject(offer.transfer_id, Reason::Busy));
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

    let (receiver, actions) =
        ReceiverSession::new(&offer, decision, rs.filexfer_policy.timeouts, now_ms());
    let assembler = BlockAssembler::new(offer.transfer_id, offer.block_count);
    let mut fx = FxRxState {
        receiver,
        assembler,
        offer,
        from,
        peer_pubkey,
        file_received_emitted: false,
    };
    drive_rx_actions(&mut fx, actions, rs, event_tx, mode, engine);
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
    engine: &mut ModemEngine,
) {
    if let Some(mut fx) = rs.file_rx.take() {
        if fx.offer.transfer_id == transfer_id {
            let actions = fx.receiver.accept(now_ms());
            drive_rx_actions(&mut fx, actions, rs, event_tx, mode, engine);
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
    engine: &mut ModemEngine,
) {
    if let Some(mut fx) = rs.file_rx.take() {
        if fx.offer.transfer_id == transfer_id {
            let actions = fx.receiver.reject(Reason::OperatorDeclined);
            drive_rx_actions(&mut fx, actions, rs, event_tx, mode, engine);
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
    engine: &mut ModemEngine,
) {
    if rs.file_rx.as_ref().map(|fx| fx.offer.transfer_id) == Some(transfer_id) {
        transmit_ctrl(
            engine,
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
    engine: &mut ModemEngine,
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
    drive_tx_actions(&mut fx, actions, event_tx, mode, engine);
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
    engine: &mut ModemEngine,
) {
    let Some(mut fx) = rs.file_tx.take() else {
        return;
    };
    if frame.transfer_id() == fx.offer.transfer_id {
        let actions = fx.sender.apply(&frame, now_ms());
        drive_tx_actions(&mut fx, actions, event_tx, mode, engine);
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
    event_tx: &Arc<broadcast::Sender<ControlEvent>>,
    mode: &str,
    engine: &mut ModemEngine,
) {
    for action in actions {
        match action {
            FxAction::Transmit(frame) => transmit_ctrl(engine, mode, &frame),
            FxAction::SendBlock {
                block_index,
                missing,
            } => {
                let bs = fx.offer.block_size as usize;
                let start = (block_index as usize).saturating_mul(bs).min(fx.file.len());
                let end = start.saturating_add(bs).min(fx.file.len());
                match encode_block(
                    fx.offer.transfer_id,
                    block_index,
                    &fx.file[start..end],
                    missing.as_deref(),
                ) {
                    Ok(fragments) => {
                        for frag in fragments {
                            if let Err(e) = engine.transmit(&frag, mode, None) {
                                tracing::warn!(error = %e, "filexfer: block fragment transmit failed");
                            }
                        }
                    }
                    Err(e) => tracing::warn!(error = %e, "filexfer: block encode failed"),
                }
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
    engine: &mut ModemEngine,
) {
    let Some(mut fx) = rs.file_rx.take() else {
        return;
    };
    if let BlockEvent::Complete { block_index } = fx.assembler.ingest_fragment(bytes) {
        transmit_ctrl(
            engine,
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
        drive_rx_actions(&mut fx, actions, rs, event_tx, mode, engine);
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
    engine: &mut ModemEngine,
) {
    for action in actions {
        match action {
            FxAction::Transmit(frame) => transmit_ctrl(engine, mode, &frame),
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
                drive_rx_actions(fx, more, rs, event_tx, mode, engine);
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
    rs: &RuntimeControlState,
    event_tx: &Arc<broadcast::Sender<ControlEvent>>,
) -> (CompleteStatus, [u8; 64]) {
    let Some(payload) = fx.assembler.reassemble() else {
        return (CompleteStatus::SizeMismatch, [0u8; 64]);
    };
    let manifest = fx.offer.to_manifest();
    // No verified-peer key ⇒ verification can only fail; the all-zero key never validates a signature.
    let pubkey = fx.peer_pubkey.unwrap_or([0u8; 32]);

    match verify_manifest_with_payload(&manifest, &pubkey, &payload) {
        Ok(()) => match write_file(rs, &fx.from, &fx.offer.name, &payload, true) {
            Ok(path) => {
                fx.file_received_emitted = true;
                let _ = event_tx.send(ControlEvent::FileReceived {
                    transfer_id: fx.offer.transfer_id,
                    from: fx.from.clone(),
                    name: fx.offer.name.clone(),
                    size: payload.len() as u64,
                    path,
                    verified: true,
                });
                let countersig = countersign(&payload, &fx.offer.sender_id, &rs.station_seed);
                (CompleteStatus::VerifiedOk, countersig)
            }
            Err(e) => {
                tracing::warn!(error = %e, "filexfer: verified file write failed");
                (CompleteStatus::SizeMismatch, [0u8; 64])
            }
        },
        Err(ManifestError::PayloadHashMismatch) => {
            // Quarantine — never silently drop; the operator sees an UNVERIFIED file.
            let path =
                write_file(rs, &fx.from, &fx.offer.name, &payload, false).unwrap_or_default();
            fx.file_received_emitted = true;
            let _ = event_tx.send(ControlEvent::FileReceived {
                transfer_id: fx.offer.transfer_id,
                from: fx.from.clone(),
                name: fx.offer.name.clone(),
                size: payload.len() as u64,
                path,
                verified: false,
            });
            (CompleteStatus::HashMismatch, [0u8; 64])
        }
        Err(_) => (CompleteStatus::SignatureInvalid, [0u8; 64]),
    }
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

/// SAR-encode a control frame with the reserved control segment-id and transmit its fragments.
fn transmit_ctrl(engine: &mut ModemEngine, mode: &str, frame: &[u8]) {
    match sar_encode(FX_CONTROL_SEGMENT_ID, frame) {
        Ok(fragments) => {
            for frag in fragments {
                if let Err(e) = engine.transmit(&frag, mode, None) {
                    tracing::warn!(error = %e, "filexfer: control frame transmit failed");
                }
            }
        }
        Err(e) => tracing::warn!(error = %e, "filexfer: control frame SAR encode failed"),
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

/// True if accepting `incoming` bytes for `from` would exceed the per-peer quota (0 = unlimited).
fn quota_would_exceed(policy: &FileTransferPolicy, from: &str, incoming: u64) -> bool {
    if policy.per_peer_quota_bytes == 0 {
        return false;
    }
    let peer = sanitize_filename(if from.is_empty() { "unknown" } else { from });
    let used = dir_size(&policy.download_dir.join(peer));
    used.saturating_add(incoming) > policy.per_peer_quota_bytes
}

fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum()
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
