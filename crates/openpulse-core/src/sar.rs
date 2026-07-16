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

/// Maximum number of concurrently-pending (incomplete) reassembly candidates a [`SarReassembler`] holds
/// before rejecting new ones — bounds memory against a sender that floods distinct, never-completed
/// segment ids (audit RX-4). Well above any legitimate in-flight transfer.
pub const MAX_PENDING_SLOTS: usize = 4096;

/// Maximum number of concurrent reassembly *candidates* under a single `(session_id, segment_id)` key.
/// Callers that reuse a constant key for every logical message (e.g. the handshake path, keyed
/// `("handshake", 0)`) would otherwise let one crafted or stray fragment poison the single in-flight
/// reassembly. Keeping several consistent candidates per key isolates conflicting fragment streams;
/// this caps that set (oldest evicted) so a flood of conflicting fragments can't exhaust memory.
pub const MAX_CANDIDATES_PER_KEY: usize = 8;

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
/// Keyed on `(session_id, segment_id)`, each key holding up to
/// [`MAX_CANDIDATES_PER_KEY`] concurrent *candidate* reassemblies so that a
/// conflicting fragment stream (a poisoned or stray fragment, or a second
/// message reusing the same key) cannot corrupt an in-flight reassembly. Call
/// [`SarReassembler::ingest`] for each received fragment payload; it returns the
/// reassembled data of every candidate that fragment completed. Call
/// [`SarReassembler::expire`] periodically to remove stale candidates.
pub struct SarReassembler {
    slots: HashMap<(String, u16), Vec<ReassemblySlot>>,
    /// Running total of candidates across all keys (bounds memory; RX-4).
    total_candidates: usize,
    timeout: Duration,
}

impl SarReassembler {
    /// Create a reassembler with the given fragment reassembly timeout.
    pub fn new(timeout: Duration) -> Self {
        Self {
            slots: HashMap::new(),
            total_candidates: 0,
            timeout,
        }
    }

    /// Ingest a SAR fragment payload for the given session.
    ///
    /// `payload` must begin with the 4-byte SAR header followed by fragment data. Returns every
    /// candidate reassembly this fragment *completed* (usually zero or one; more only when conflicting
    /// streams share a key and an ambiguous fragment finishes several at once). The caller verifies each
    /// returned frame and drops the invalid ones.
    ///
    /// A fragment is added to **every** existing candidate it is *consistent* with — same
    /// `fragment_total`, and its index is either empty or already holds identical bytes. If it is
    /// consistent with none (a different total, or a different payload for an already-filled index) it
    /// starts a **new** candidate instead of corrupting an existing one, so a poisoned or interleaved
    /// fragment cannot block a legitimate reassembly; the bad candidate reassembles to a frame that fails
    /// downstream verification while the good one completes. Ambiguous fragments (which could belong to
    /// more than one candidate) are added to all of them, which is why several may complete together.
    ///
    /// # Errors
    ///
    /// - `SarError::MalformedHeader` — payload shorter than 4 bytes.
    /// - `SarError::FragmentIndexOutOfRange` — `fragment_index >= fragment_total`.
    /// - `SarError::TooManyPendingSegments` — the global candidate cap is reached.
    pub fn ingest(&mut self, session_id: &str, payload: &[u8]) -> Result<Vec<Vec<u8>>, SarError> {
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
        let candidates = self.slots.entry(key.clone()).or_default();
        let idx = fragment_index as usize;

        // Every candidate this fragment is consistent with: same total, and the target index is empty
        // or already holds identical bytes (an idempotent duplicate).
        let consistent: Vec<usize> = candidates
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                c.total == fragment_total
                    && match &c.fragments[idx] {
                        None => true,
                        Some(existing) => existing.as_slice() == data,
                    }
            })
            .map(|(i, _)| i)
            .collect();

        if consistent.is_empty() {
            // A new candidate is needed. Enforce the per-key cap first (evicting the oldest so a flood of
            // conflicting fragments can't lock out a legitimate stream), then the global cap.
            if candidates.len() >= MAX_CANDIDATES_PER_KEY {
                if let Some(oldest) = candidates
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, c)| c.created)
                    .map(|(i, _)| i)
                {
                    candidates.remove(oldest);
                    self.total_candidates -= 1;
                }
            } else if self.total_candidates >= MAX_PENDING_SLOTS {
                if candidates.is_empty() {
                    self.slots.remove(&key);
                }
                return Err(SarError::TooManyPendingSegments {
                    max: MAX_PENDING_SLOTS,
                });
            }
            let mut fragments = vec![None; fragment_total as usize];
            fragments[idx] = Some(data.to_vec());
            candidates.push(ReassemblySlot {
                total: fragment_total,
                fragments,
                received: 1,
                created: Instant::now(),
            });
            self.total_candidates += 1;
        } else {
            for &i in &consistent {
                let slot = &mut candidates[i];
                if slot.fragments[idx].is_none() {
                    slot.fragments[idx] = Some(data.to_vec());
                    slot.received += 1;
                }
            }
        }

        // Extract completed candidates (all fragments present); keep the rest.
        let mut completed = Vec::new();
        let mut kept = Vec::with_capacity(candidates.len());
        for slot in candidates.drain(..) {
            if slot.received == slot.total {
                completed.push(slot.fragments.into_iter().flatten().flatten().collect());
            } else {
                kept.push(slot);
            }
        }
        self.total_candidates -= completed.len();
        *candidates = kept;
        if candidates.is_empty() {
            self.slots.remove(&key);
        }

        Ok(completed)
    }

    /// Remove all candidates whose age exceeds the configured timeout.
    pub fn expire(&mut self) {
        let timeout = self.timeout;
        let mut removed = 0;
        self.slots.retain(|_, candidates| {
            let before = candidates.len();
            candidates.retain(|slot| slot.created.elapsed() < timeout);
            removed += before - candidates.len();
            !candidates.is_empty()
        });
        self.total_candidates -= removed;
    }

    /// Return the number of in-progress reassembly candidates (across all keys).
    pub fn pending_count(&self) -> usize {
        self.total_candidates
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// For the non-conflicting tests a fragment completes at most one candidate; unwrap that.
    fn single(v: Vec<Vec<u8>>) -> Option<Vec<u8>> {
        assert!(v.len() <= 1, "expected ≤1 completion, got {}", v.len());
        v.into_iter().next()
    }

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
    fn ingest_caps_pending_incomplete_segments() {
        // Audit RX-4: a sender flooding distinct segment ids, each one fragment short (so the slot never
        // completes), must be bounded. Each fragment is [seg_hi, seg_lo, index=0, total=2, data].
        let mut r = SarReassembler::new(Duration::from_secs(60));
        for seg in 0..MAX_PENDING_SLOTS as u16 {
            let frag = [(seg >> 8) as u8, seg as u8, 0, 2, 0xAA];
            assert!(single(r.ingest("flood", &frag).unwrap()).is_none());
        }
        assert_eq!(r.pending_count(), MAX_PENDING_SLOTS);
        // One more distinct, incomplete segment is rejected rather than growing the table further.
        let over = MAX_PENDING_SLOTS as u16; // a segment id not yet seen
        let frag = [(over >> 8) as u8, over as u8, 0, 2, 0xAA];
        assert!(matches!(
            r.ingest("flood", &frag),
            Err(SarError::TooManyPendingSegments { .. })
        ));
        // A fragment for an *existing* pending slot is still accepted (completes it, freeing the slot).
        let complete = [0, 0, 1, 2, 0xBB]; // segment 0, fragment index 1 of 2
        assert!(single(r.ingest("flood", &complete).unwrap()).is_some());
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
        let result = single(r.ingest("sess-1", &payloads[0]).unwrap());
        assert_eq!(result, Some(b"world".to_vec()));
    }

    #[test]
    fn reassemble_multiple_fragments_in_order() {
        let data: Vec<u8> = (0..=u8::MAX).collect();
        let mut r = SarReassembler::new(Duration::from_secs(60));
        let payloads = sar_encode(10, &data).unwrap();
        let n = payloads.len();
        for (i, payload) in payloads.iter().enumerate() {
            let result = single(r.ingest("sess-a", payload).unwrap());
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
            let result = single(r.ingest("sess-b", payload).unwrap());
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
        single(r.ingest("sess-c", &payloads[0]).unwrap());
        single(r.ingest("sess-c", &payloads[0]).unwrap()); // duplicate
        let result = single(r.ingest("sess-c", &payloads[1]).unwrap());
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
            assert!(single(r.ingest("sess-d", payload).unwrap()).is_none());
        }
        assert_eq!(r.pending_count(), 1);
    }

    #[test]
    fn expired_slot_is_removed() {
        let mut r = SarReassembler::new(Duration::from_millis(10));
        // Need multiple fragments so the first ingest leaves the slot open.
        let data: Vec<u8> = (0..SAR_MAX_FRAGMENT_DATA + 10).map(|i| i as u8).collect();
        let payloads = sar_encode(1, &data).unwrap();
        single(r.ingest("sess-e", &payloads[0]).unwrap());
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
    fn mismatched_total_forms_an_independent_candidate() {
        // Two fragment streams under the same key with different totals no longer collide: each forms
        // its own candidate, and both can complete independently.
        let mut r = SarReassembler::new(Duration::from_secs(60));
        // Stream A: total=1 (completes immediately).
        assert_eq!(
            single(r.ingest("s", &[0, 0, 0, 1, 0xAA]).unwrap()),
            Some(vec![0xAA])
        );
        // Stream B: total=2, both fragments.
        assert!(single(r.ingest("s", &[0, 0, 0, 2, 0xBB]).unwrap()).is_none());
        assert_eq!(
            single(r.ingest("s", &[0, 0, 1, 2, 0xCC]).unwrap()),
            Some(vec![0xBB, 0xCC])
        );
    }

    #[test]
    fn poison_fragment_does_not_block_legit_reassembly() {
        // A crafted fragment sharing the constant handshake key (same segment_id, same total) but with
        // different bytes for an index must not corrupt the legitimate reassembly (audit: SAR poison).
        let mut r = SarReassembler::new(Duration::from_secs(60));
        let legit_data: Vec<u8> = (0..(SAR_MAX_FRAGMENT_DATA + 60)).map(|i| i as u8).collect();
        let legit = sar_encode(0, &legit_data).unwrap();
        assert_eq!(legit.len(), 2, "test frame must span two fragments");
        // Attacker seeds index 0 with garbage under the same key before the legit fragments arrive.
        let mut poison = vec![0u8, 0, 0, 2]; // seg 0, index 0, total 2
        poison.extend(vec![0xDEu8; SAR_MAX_FRAGMENT_DATA]);
        assert!(r.ingest("handshake", &poison).unwrap().is_empty());
        // The legit fragments reassemble to the original frame as a separate candidate. The poison
        // candidate also completes (with a garbage index 0) but is discarded by downstream verification;
        // here we only assert the good frame is among the completions.
        let mut completed = Vec::new();
        for frag in &legit {
            completed.extend(r.ingest("handshake", frag).unwrap());
        }
        assert!(
            completed.iter().any(|f| f == &legit_data),
            "legit frame must reassemble despite the poison candidate"
        );
    }

    #[test]
    fn wrong_total_seed_does_not_block_legit_reassembly() {
        // Attacker seeds the key with a fragment claiming a *different* total; the legit stream (its own
        // total) still reassembles rather than bouncing off a poisoned single slot.
        let mut r = SarReassembler::new(Duration::from_secs(60));
        let legit_data: Vec<u8> = (0..(SAR_MAX_FRAGMENT_DATA + 40))
            .map(|i| (i ^ 0x5A) as u8)
            .collect();
        let legit = sar_encode(0, &legit_data).unwrap();
        assert_eq!(legit.len(), 2, "test frame must span two fragments");
        // Poison: claims total=5 (a distinct candidate that never completes).
        assert!(r
            .ingest("handshake", &[0, 0, 0, 5, 0x00])
            .unwrap()
            .is_empty());
        let mut completed = Vec::new();
        for frag in &legit {
            completed.extend(r.ingest("handshake", frag).unwrap());
        }
        // Only the legit candidate (total 2) completes; the total-5 poison never does.
        assert_eq!(completed, vec![legit_data]);
    }

    #[test]
    fn per_key_candidate_flood_is_capped_and_evicts_oldest() {
        // A flood of conflicting single-fragment candidates under one key is capped; a legitimate stream
        // can still make progress because eviction targets the oldest candidate.
        let mut r = SarReassembler::new(Duration::from_secs(60));
        // Fill the per-key cap with distinct 2-fragment candidates (each holds index 0 with unique data).
        for i in 0..MAX_CANDIDATES_PER_KEY as u8 {
            assert!(single(r.ingest("handshake", &[0, 0, 0, 2, i]).unwrap()).is_none());
        }
        assert_eq!(r.pending_count(), MAX_CANDIDATES_PER_KEY);
        // One more conflicting fragment evicts the oldest rather than growing without bound.
        assert!(single(r.ingest("handshake", &[0, 0, 0, 2, 0xFF]).unwrap()).is_none());
        assert_eq!(r.pending_count(), MAX_CANDIDATES_PER_KEY);
    }

    #[test]
    fn different_sessions_are_independent() {
        let data = b"XYZ";
        let mut r = SarReassembler::new(Duration::from_secs(60));
        // Two segments with same segment_id but different session_ids
        let payloads_a = sar_encode(1, data).unwrap();
        let payloads_b = sar_encode(1, data).unwrap();
        let res_a = single(r.ingest("sess-A", &payloads_a[0]).unwrap());
        let res_b = single(r.ingest("sess-B", &payloads_b[0]).unwrap());
        assert_eq!(res_a, Some(data.to_vec()));
        assert_eq!(res_b, Some(data.to_vec()));
    }
}
