//! Map an OpenPulse-marked station into the shared `PeerCache` (plan §5.2).
//!
//! Plain JS8 stations stay only in the [`StationTable`](crate::station::StationTable) — they are not
//! OpenPulse peers. A station carrying an `@OPULSE` hint becomes a key-less, RF-heard `PeerRecord`
//! (`TrustLevel::Unknown`, `revision` 0), so it passes `TrustFilter::Any`/`TrustedOrUnknown`, is
//! excluded from `TrustedOnly`, and always loses an upsert conflict to an authenticated,
//! descriptor-signed record — exactly the semantics we want for a peer heard only over the air.

use openpulse_core::peer_cache::{PeerRecord, TrustLevel};
use sha2::{Digest, Sha256};

use crate::station::Js8Station;

// Capability-bit registry (plan §5.2; also documented in `docs/dev/peer-query-relay-wire.md`).
/// Speaks OpenPulse HPX sessions.
pub const CAP_HPX: u16 = 1 << 0;
/// Accepts JS8 OPHF rendezvous.
pub const CAP_RENDEZVOUS: u16 = 1 << 1;
/// In-session `openpulse-qsy` protocol.
pub const CAP_QSY: u16 = 1 << 2;
/// Post-quantum handshake.
pub const CAP_PQ: u16 = 1 << 3;
/// Relay forwarding.
pub const CAP_RELAY: u16 = 1 << 4;

/// Map a JS8 SNR (dynamic range −30…+12 dB) onto the 0–252 `route_quality` scale (monotone; plan §5.2).
fn route_quality_from_snr(snr_db: f32) -> u8 {
    ((snr_db + 30.0).clamp(0.0, 42.0) * 6.0) as u8
}

/// Build a `PeerRecord` for an OpenPulse-marked `station`, or `None` if it carries no `@OPULSE` hint
/// (a plain JS8 station is not a peer). Keyed `js8:<callsign>` until an Ed25519 descriptor is learned
/// post-handshake and the record is re-keyed.
pub fn station_to_peer_record(station: &Js8Station) -> Option<PeerRecord> {
    let hint = station.hint.as_ref()?;
    Some(PeerRecord {
        peer_id: format!("js8:{}", station.callsign),
        capability_mask: hint.caps as u32,
        route_quality: route_quality_from_snr(station.snr_db),
        trust_level: TrustLevel::Unknown,
        revision: 0,
        updated_at_ms: station.last_heard_ms,
        callsign_hash: Sha256::digest(station.callsign.as_bytes()).into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::station::{Observation, OphfHint, StationTable};
    use js8_plugin::submode::Submode;

    fn hint(caps: u16) -> OphfHint {
        OphfHint {
            version: 1,
            caps,
            pref_channel: Some(3),
            listen_submode: Submode::Normal,
        }
    }

    fn table_with(call: &str, snr: f32, hint: Option<OphfHint>) -> StationTable {
        let mut t = StationTable::new();
        t.upsert(
            Observation {
                callsign: call.into(),
                grid: Some("EM73".into()),
                snr_db: snr,
                freq_offset_hz: 1500.0,
                dial_freq_hz: 14_078_000,
                hint,
            },
            1000,
        );
        t
    }

    #[test]
    fn plain_js8_station_is_not_a_peer() {
        let t = table_with("KN4CRD", -10.0, None);
        assert!(station_to_peer_record(t.get("KN4CRD").unwrap()).is_none());
    }

    #[test]
    fn marked_station_maps_to_a_keyless_unknown_peer() {
        let t = table_with("DC0SK", -6.0, Some(hint(CAP_HPX | CAP_RENDEZVOUS)));
        let r = station_to_peer_record(t.get("DC0SK").unwrap()).unwrap();
        assert_eq!(r.peer_id, "js8:DC0SK");
        assert_eq!(r.capability_mask, (CAP_HPX | CAP_RENDEZVOUS) as u32);
        assert_eq!(r.trust_level, TrustLevel::Unknown);
        assert_eq!(r.revision, 0);
        assert_eq!(r.updated_at_ms, 1000);
        assert_eq!(r.callsign_hash, <[u8; 32]>::from(Sha256::digest(b"DC0SK")));
    }

    #[test]
    fn route_quality_is_monotone_and_bounded() {
        assert_eq!(route_quality_from_snr(-40.0), 0); // below range → floor
        assert_eq!(route_quality_from_snr(-30.0), 0);
        assert_eq!(route_quality_from_snr(12.0), 252); // top of range → ceiling
        assert_eq!(route_quality_from_snr(100.0), 252); // clamped
        assert!(route_quality_from_snr(-6.0) < route_quality_from_snr(6.0));
    }

    #[test]
    fn record_passes_any_but_not_trusted_only() {
        use openpulse_core::peer_cache::{PeerCache, TrustFilter};
        let t = table_with("W1AW", 0.0, Some(hint(CAP_HPX)));
        let mut cache = PeerCache::new(16, 3_600_000);
        cache.upsert(
            station_to_peer_record(t.get("W1AW").unwrap()).unwrap(),
            1000,
        );
        assert_eq!(
            cache.query(0, 0, TrustFilter::Any, 8, 1000).len(),
            1,
            "TrustFilter::Any includes the RF-heard peer"
        );
        assert_eq!(
            cache.query(0, 0, TrustFilter::TrustedOnly, 8, 1000).len(),
            0,
            "TrustFilter::TrustedOnly excludes an Unknown-trust peer"
        );
    }
}
