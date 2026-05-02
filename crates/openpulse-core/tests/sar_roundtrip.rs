use openpulse_core::error::SarError;
use openpulse_core::sar::{
    sar_encode, SarReassembler, SAR_MAX_FRAGMENT_DATA, SAR_MAX_SEGMENT_DATA,
};
use std::time::Duration;

#[test]
fn sar_round_trip_256_bytes() {
    let data: Vec<u8> = (0..=255).map(|i| i as u8).collect();
    assert_eq!(data.len(), 256);

    let mut r = SarReassembler::new(Duration::from_secs(60));
    let payloads = sar_encode(1, &data).unwrap();
    assert_eq!(payloads.len(), 2); // 256 > 251 → 2 fragments

    let mut result = None;
    for payload in &payloads {
        result = r.ingest("session-A", payload).unwrap();
    }
    assert_eq!(result.unwrap(), data);
}

#[test]
fn sar_round_trip_64kb() {
    // 64 000 bytes fits within the 64 005-byte SAR maximum.
    let data: Vec<u8> = (0..64_000).map(|i| (i % 251) as u8).collect();
    let expected_fragments = data.len().div_ceil(SAR_MAX_FRAGMENT_DATA);

    let mut r = SarReassembler::new(Duration::from_secs(60));
    let payloads = sar_encode(0xABCD, &data).unwrap();
    assert_eq!(payloads.len(), expected_fragments);

    let mut result = None;
    for payload in &payloads {
        result = r.ingest("session-B", payload).unwrap();
    }
    assert_eq!(result.unwrap(), data);
}

#[test]
fn sar_round_trip_maximum_size() {
    let data = vec![0xFFu8; SAR_MAX_SEGMENT_DATA];
    let mut r = SarReassembler::new(Duration::from_secs(60));
    let payloads = sar_encode(0xFFFF, &data).unwrap();
    assert_eq!(payloads.len(), 255);

    let mut result = None;
    for payload in &payloads {
        result = r.ingest("session-C", payload).unwrap();
    }
    assert_eq!(result.unwrap(), data);
}

#[test]
fn sar_missing_fragment_stays_pending() {
    // Multi-fragment payload; drop the last fragment.
    let data: Vec<u8> = (0..SAR_MAX_FRAGMENT_DATA * 3).map(|i| i as u8).collect();
    let mut r = SarReassembler::new(Duration::from_secs(60));
    let payloads = sar_encode(2, &data).unwrap();
    assert_eq!(payloads.len(), 3);

    // Deliver first two fragments only.
    for payload in &payloads[..2] {
        assert!(r.ingest("session-D", payload).unwrap().is_none());
    }
    assert_eq!(r.pending_count(), 1);

    // Deliver the missing third fragment — now complete.
    let result = r.ingest("session-D", &payloads[2]).unwrap();
    assert_eq!(result.unwrap(), data);
    assert_eq!(r.pending_count(), 0);
}

#[test]
fn sar_reassembly_timeout_discards_incomplete_segment() {
    let data: Vec<u8> = (0..SAR_MAX_FRAGMENT_DATA + 10).map(|i| i as u8).collect();
    let mut r = SarReassembler::new(Duration::from_millis(15));
    let payloads = sar_encode(3, &data).unwrap();

    // Deliver first fragment only — slot opens.
    r.ingest("session-E", &payloads[0]).unwrap();
    assert_eq!(r.pending_count(), 1);

    // Wait for timeout, then expire.
    std::thread::sleep(Duration::from_millis(30));
    r.expire();
    assert_eq!(r.pending_count(), 0);

    // After expiry, the second fragment starts a fresh slot.
    let result = r.ingest("session-E", &payloads[1]).unwrap();
    assert!(result.is_none()); // only one fragment in the fresh slot
    assert_eq!(r.pending_count(), 1);
}

#[test]
fn sar_oversized_data_is_rejected() {
    let oversized = vec![0u8; SAR_MAX_SEGMENT_DATA + 1];
    assert!(matches!(
        sar_encode(0, &oversized),
        Err(SarError::DataTooLarge { .. })
    ));
}

#[test]
fn sar_independent_sessions_do_not_interfere() {
    let data_a: Vec<u8> = vec![0xAA; 300];
    let data_b: Vec<u8> = vec![0xBB; 300];

    let mut r = SarReassembler::new(Duration::from_secs(60));

    let payloads_a = sar_encode(1, &data_a).unwrap();
    let payloads_b = sar_encode(1, &data_b).unwrap(); // same segment_id, different session

    // Interleave fragment delivery.
    r.ingest("sess-A", &payloads_a[0]).unwrap();
    r.ingest("sess-B", &payloads_b[0]).unwrap();
    let res_a = r.ingest("sess-A", &payloads_a[1]).unwrap();
    let res_b = r.ingest("sess-B", &payloads_b[1]).unwrap();

    assert_eq!(res_a.unwrap(), data_a);
    assert_eq!(res_b.unwrap(), data_b);
}
