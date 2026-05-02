//! SAR (Segmentation and Reassembly) sub-layer.
//!
//! Large payloads that exceed the 255-byte frame limit are split into fragments
//! by the encoder and reassembled by the receiver.
//!
//! ## SAR payload layout (within one frame's payload field)
//!
//! ```text
//! ┌───────────────────┬────────────────────┬───────────────────┬──────────┐
//! │ segment_id (u16)  │ fragment_index (u8) │ fragment_total (u8) │ data   │
//! │ 2 B               │ 1 B                 │ 1 B               │ 0–251 B │
//! └───────────────────┴────────────────────┴───────────────────┴──────────┘
//! ```
//!
//! - `segment_id`: caller-assigned identifier for this logical data unit.
//! - `fragment_index`: 0-based index of this fragment within the segment.
//! - `fragment_total`: total number of fragments in this segment (1–255).
//! - Max user data per fragment: 255 − 4 = 251 bytes.
//! - Max transportable data per segment: 255 × 251 = 64 005 bytes.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::error::SarError;

/// SAR header size in bytes.
pub const SAR_HEADER_SIZE: usize = 4;

/// Maximum user data bytes per fragment.
pub const SAR_MAX_FRAGMENT_DATA: usize = 255 - SAR_HEADER_SIZE;

/// Maximum total data bytes encodable in a single SAR segment.
pub const SAR_MAX_SEGMENT_DATA: usize = (u8::MAX as usize) * SAR_MAX_FRAGMENT_DATA;

// ── Encoder ───────────────────────────────────────────────────────────────────

/// Encode `data` into SAR fragment payloads ready to be placed into frames.
///
/// Each returned `Vec<u8>` is a complete frame payload (SAR header + data slice)
/// that fits within the 255-byte frame payload limit.
///
/// # Errors
///
/// Returns `SarError::DataTooLarge` when `data.len() > SAR_MAX_SEGMENT_DATA`.
pub fn sar_encode(segment_id: u16, data: &[u8]) -> Result<Vec<Vec<u8>>, SarError> {
    if data.len() > SAR_MAX_SEGMENT_DATA {
        return Err(SarError::DataTooLarge {
            len: data.len(),
            max: SAR_MAX_SEGMENT_DATA,
        });
    }

    let chunks: Vec<&[u8]> = if data.is_empty() {
        vec![&[]]
    } else {
        data.chunks(SAR_MAX_FRAGMENT_DATA).collect()
    };

    let total = chunks.len() as u8;
    let mut payloads = Vec::with_capacity(chunks.len());

    for (index, chunk) in chunks.iter().enumerate() {
        let mut payload = Vec::with_capacity(SAR_HEADER_SIZE + chunk.len());
        payload.push((segment_id >> 8) as u8);
        payload.push(segment_id as u8);
        payload.push(index as u8);
        payload.push(total);
        payload.extend_from_slice(chunk);
        payloads.push(payload);
    }

    Ok(payloads)
}

// ── Reassembler ───────────────────────────────────────────────────────────────

struct ReassemblySlot {
    total: u8,
    fragments: Vec<Option<Vec<u8>>>,
    received: u8,
    created: Instant,
}

/// Reassembles SAR fragments into complete data units.
///
/// Keyed on `(session_id, segment_id)`.  Call [`SarReassembler::ingest`] for
/// each received fragment payload; it returns `Some(data)` when the segment is
/// complete.  Call [`SarReassembler::expire`] periodically to remove stale
/// slots.
pub struct SarReassembler {
    slots: HashMap<(String, u16), ReassemblySlot>,
    timeout: Duration,
}

impl SarReassembler {
    /// Create a reassembler with the given fragment reassembly timeout.
    pub fn new(timeout: Duration) -> Self {
        Self {
            slots: HashMap::new(),
            timeout,
        }
    }

    /// Ingest a SAR fragment payload for the given session.
    ///
    /// `payload` must begin with the 4-byte SAR header followed by fragment
    /// data.  Returns `Ok(Some(data))` when all fragments for the segment have
    /// arrived and the segment is fully reassembled.
    ///
    /// # Errors
    ///
    /// - `SarError::MalformedHeader` — payload shorter than 4 bytes.
    /// - `SarError::FragmentIndexOutOfRange` — `fragment_index >= fragment_total`.
    /// - `SarError::FragmentCountMismatch` — `fragment_total` differs from an
    ///   earlier fragment for the same `(session_id, segment_id)`.
    pub fn ingest(
        &mut self,
        session_id: &str,
        payload: &[u8],
    ) -> Result<Option<Vec<u8>>, SarError> {
        if payload.len() < SAR_HEADER_SIZE {
            return Err(SarError::MalformedHeader);
        }

        let segment_id = ((payload[0] as u16) << 8) | payload[1] as u16;
        let fragment_index = payload[2];
        let fragment_total = payload[3];
        let data = &payload[SAR_HEADER_SIZE..];

        if fragment_total == 0 || fragment_index >= fragment_total {
            return Err(SarError::FragmentIndexOutOfRange {
                index: fragment_index,
                total: fragment_total,
            });
        }

        let key = (session_id.to_string(), segment_id);
        let slot = self.slots.entry(key).or_insert_with(|| ReassemblySlot {
            total: fragment_total,
            fragments: vec![None; fragment_total as usize],
            received: 0,
            created: Instant::now(),
        });

        if slot.total != fragment_total {
            return Err(SarError::FragmentCountMismatch {
                expected: slot.total,
                got: fragment_total,
            });
        }

        // Accept duplicate fragments (idempotent).
        if slot.fragments[fragment_index as usize].is_none() {
            slot.fragments[fragment_index as usize] = Some(data.to_vec());
            slot.received += 1;
        }

        if slot.received == slot.total {
            let assembled: Vec<u8> = slot
                .fragments
                .iter()
                .flat_map(|f| {
                    f.as_ref()
                        .expect("received == total guarantees all present")
                })
                .copied()
                .collect();
            // Remove completed slot.
            let key = (session_id.to_string(), segment_id);
            self.slots.remove(&key);
            return Ok(Some(assembled));
        }

        Ok(None)
    }

    /// Remove all slots whose age exceeds the configured timeout.
    pub fn expire(&mut self) {
        let timeout = self.timeout;
        self.slots
            .retain(|_, slot| slot.created.elapsed() < timeout);
    }

    /// Return the number of in-progress reassembly slots.
    pub fn pending_count(&self) -> usize {
        self.slots.len()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_single_fragment_for_small_data() {
        let payloads = sar_encode(1, b"hello").unwrap();
        assert_eq!(payloads.len(), 1);
        let p = &payloads[0];
        assert_eq!((p[0] as u16) << 8 | p[1] as u16, 1); // segment_id
        assert_eq!(p[2], 0); // index
        assert_eq!(p[3], 1); // total
        assert_eq!(&p[4..], b"hello");
    }

    #[test]
    fn encode_empty_data_yields_one_fragment() {
        let payloads = sar_encode(0, &[]).unwrap();
        assert_eq!(payloads.len(), 1);
        assert_eq!(payloads[0].len(), SAR_HEADER_SIZE); // header only
    }

    #[test]
    fn encode_splits_at_boundary() {
        let data = vec![0xABu8; SAR_MAX_FRAGMENT_DATA * 2];
        let payloads = sar_encode(42, &data).unwrap();
        assert_eq!(payloads.len(), 2);
        assert_eq!(payloads[0][3], 2); // total
        assert_eq!(payloads[0][4..].len(), SAR_MAX_FRAGMENT_DATA);
        assert_eq!(payloads[1][4..].len(), SAR_MAX_FRAGMENT_DATA);
    }

    #[test]
    fn encode_rejects_data_too_large() {
        let oversized = vec![0u8; SAR_MAX_SEGMENT_DATA + 1];
        assert!(matches!(
            sar_encode(0, &oversized),
            Err(SarError::DataTooLarge { .. })
        ));
    }

    #[test]
    fn reassemble_single_fragment() {
        let mut r = SarReassembler::new(Duration::from_secs(60));
        let payloads = sar_encode(5, b"world").unwrap();
        let result = r.ingest("sess-1", &payloads[0]).unwrap();
        assert_eq!(result, Some(b"world".to_vec()));
    }

    #[test]
    fn reassemble_multiple_fragments_in_order() {
        let data: Vec<u8> = (0..=u8::MAX).collect();
        let mut r = SarReassembler::new(Duration::from_secs(60));
        let payloads = sar_encode(10, &data).unwrap();
        let n = payloads.len();
        for (i, payload) in payloads.iter().enumerate() {
            let result = r.ingest("sess-a", payload).unwrap();
            if i < n - 1 {
                assert!(result.is_none());
            } else {
                assert_eq!(result.unwrap(), data);
            }
        }
    }

    #[test]
    fn reassemble_out_of_order_fragments() {
        let data: Vec<u8> = (0u8..200).collect();
        let mut r = SarReassembler::new(Duration::from_secs(60));
        let mut payloads = sar_encode(3, &data).unwrap();
        payloads.reverse();
        let n = payloads.len();
        for (i, payload) in payloads.iter().enumerate() {
            let result = r.ingest("sess-b", payload).unwrap();
            if i < n - 1 {
                assert!(result.is_none());
            } else {
                assert_eq!(result.unwrap(), data);
            }
        }
    }

    #[test]
    fn duplicate_fragment_is_idempotent() {
        let data = vec![7u8; SAR_MAX_FRAGMENT_DATA + 1];
        let mut r = SarReassembler::new(Duration::from_secs(60));
        let payloads = sar_encode(99, &data).unwrap();
        r.ingest("sess-c", &payloads[0]).unwrap();
        r.ingest("sess-c", &payloads[0]).unwrap(); // duplicate
        let result = r.ingest("sess-c", &payloads[1]).unwrap();
        assert_eq!(result.unwrap(), data);
    }

    #[test]
    fn missing_fragment_returns_none() {
        // Need multiple fragments: use data > SAR_MAX_FRAGMENT_DATA.
        let data: Vec<u8> = (0..SAR_MAX_FRAGMENT_DATA + 50).map(|i| i as u8).collect();
        let mut r = SarReassembler::new(Duration::from_secs(60));
        let payloads = sar_encode(7, &data).unwrap();
        // Deliver all but last
        for payload in &payloads[..payloads.len() - 1] {
            assert!(r.ingest("sess-d", payload).unwrap().is_none());
        }
        assert_eq!(r.pending_count(), 1);
    }

    #[test]
    fn expired_slot_is_removed() {
        let mut r = SarReassembler::new(Duration::from_millis(10));
        // Need multiple fragments so the first ingest leaves the slot open.
        let data: Vec<u8> = (0..SAR_MAX_FRAGMENT_DATA + 10).map(|i| i as u8).collect();
        let payloads = sar_encode(1, &data).unwrap();
        r.ingest("sess-e", &payloads[0]).unwrap();
        assert_eq!(r.pending_count(), 1);
        std::thread::sleep(Duration::from_millis(20));
        r.expire();
        assert_eq!(r.pending_count(), 0);
    }

    #[test]
    fn malformed_header_error() {
        let mut r = SarReassembler::new(Duration::from_secs(60));
        assert_eq!(r.ingest("s", &[0, 1, 0]), Err(SarError::MalformedHeader));
    }

    #[test]
    fn fragment_index_out_of_range_error() {
        let mut r = SarReassembler::new(Duration::from_secs(60));
        // total=2, index=2 → out of range
        let payload = [0, 0, 2, 2, 0xAA];
        assert!(matches!(
            r.ingest("s", &payload),
            Err(SarError::FragmentIndexOutOfRange { .. })
        ));
    }

    #[test]
    fn fragment_count_mismatch_error() {
        let mut r = SarReassembler::new(Duration::from_secs(60));
        let payload_a = [0, 0, 0, 3, 0xAA]; // total=3
        let payload_b = [0, 0, 1, 2, 0xBB]; // total=2 — mismatch
        r.ingest("s", &payload_a).unwrap();
        assert!(matches!(
            r.ingest("s", &payload_b),
            Err(SarError::FragmentCountMismatch { .. })
        ));
    }

    #[test]
    fn different_sessions_are_independent() {
        let data = b"XYZ";
        let mut r = SarReassembler::new(Duration::from_secs(60));
        // Two segments with same segment_id but different session_ids
        let payloads_a = sar_encode(1, data).unwrap();
        let payloads_b = sar_encode(1, data).unwrap();
        let res_a = r.ingest("sess-A", &payloads_a[0]).unwrap();
        let res_b = r.ingest("sess-B", &payloads_b[0]).unwrap();
        assert_eq!(res_a, Some(data.to_vec()));
        assert_eq!(res_b, Some(data.to_vec()));
    }
}
