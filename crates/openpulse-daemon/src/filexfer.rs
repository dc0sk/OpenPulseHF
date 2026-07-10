//! Daemon-side file-transfer glue (FF-16).
//!
//! Phase C-2a: the inbound `OPFX` routing seam + tripwire. `process_received_bytes` dispatches a
//! reassembly by SAR segment-id — 0 → handshake (unchanged), non-zero → here. Control frames
//! (segment `0xFFFF`) are reassembled and decoded; block-data frames (segment `block_index + 1`) are
//! recognized. The receive handler (offer → verify → policy → accept → data → write → verify →
//! `FileComplete`) and the send loop land in the following steps; this step proves the seam without
//! disturbing the handshake path.

use std::sync::Arc;

use tokio::sync::broadcast;

use openpulse_filexfer::FxFrame;
use openpulse_modem::ModemEngine;

use crate::protocol::ControlEvent;
use crate::RuntimeControlState;

/// SAR segment-id carrying single-fragment `OPFX` control frames (offer/accept/reject/ack/complete/
/// cancel). Block-data frames use `block_index + 1` (1..=0xFFFE); handshake frames use 0. The three
/// ranges never overlap, so a handshake fragment and a file fragment can't share a reassembly slot.
pub const FX_CONTROL_SEGMENT_ID: u16 = 0xFFFF;

/// SAR session key for reassembling `OPFX` control frames.
const FX_CONTROL_SESSION: &str = "filexfer-ctrl";

/// Route one inbound SAR fragment on the file-transfer path (segment-id ≠ 0). Control frames are
/// reassembled and decoded; block-data fragments are recognized for the active receive session.
/// Bumps the routing tripwire so a test can prove file frames reach this seam on the production path.
pub fn route_inbound_fragment(
    bytes: &[u8],
    segment_id: u16,
    runtime_state: &mut RuntimeControlState,
    _event_tx: &Arc<broadcast::Sender<ControlEvent>>,
    _mode: &str,
    _engine: &mut ModemEngine,
) {
    runtime_state.filexfer_frames_routed = runtime_state.filexfer_frames_routed.saturating_add(1);

    if segment_id == FX_CONTROL_SEGMENT_ID {
        match runtime_state.filexfer_sar.ingest(FX_CONTROL_SESSION, bytes) {
            Ok(Some(full)) => match FxFrame::decode(&full) {
                Ok(frame) => {
                    // Phase C-2b dispatches offer/accept/data/ack/complete/cancel to the session here.
                    tracing::debug!(
                        frame_type = frame.frame_type(),
                        transfer_id = frame.transfer_id(),
                        "filexfer: control frame received (session handler pending)"
                    );
                }
                Err(e) => tracing::debug!(error = %e, "filexfer: control frame decode failed"),
            },
            Ok(None) => {} // more fragments needed (not expected for v1 single-fragment control frames)
            Err(_) => {}   // malformed SAR fragment — ignore
        }
    } else {
        // Block-data fragment (segment_id = block_index + 1). Phase C-2b feeds the receive
        // `BlockAssembler` and emits `BlockAck` from here.
        tracing::debug!(
            block_index = segment_id.saturating_sub(1),
            "filexfer: block fragment received (session handler pending)"
        );
    }
}
