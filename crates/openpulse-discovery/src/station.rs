//! Discovered-station table (plan §5.1). Every JS8 decode upserts a [`Js8Station`]; OpenPulse-marked
//! stations additionally map into the shared `PeerCache` (a later unit). This richer table is the
//! discovery panel's source of truth and is swept on a TTL.

use std::collections::BTreeMap;

use js8_plugin::submode::Submode;

use crate::hint::HintPayload;

/// SNR smoothing factor for the per-station EWMA.
const SNR_EWMA_ALPHA: f32 = 0.3;

/// Parsed `@OPULSE` hint attached to a station (plan §5.1). `None` on a plain JS8 station.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OphfHint {
    /// Hint format version.
    pub version: u8,
    /// Capability bits (§5.2 registry).
    pub caps: u16,
    /// Preferred rendezvous channel index, or `None` when the sender advertised none (raw 63).
    pub pref_channel: Option<u8>,
    /// Preferred listen submode.
    pub listen_submode: Submode,
}

impl OphfHint {
    /// Build from a decoded hint payload (`pref_channel` 63 = none; submode per the §5.4 code map).
    pub fn from_payload(version: u8, p: &HintPayload) -> Self {
        Self {
            version,
            caps: p.caps,
            pref_channel: (p.pref_channel != 63).then_some(p.pref_channel),
            listen_submode: submode_from_code(p.listen_submode),
        }
    }
}

/// Map a hint submode code (§5.4: 0=NORMAL,1=SLOW,2=FAST,3=TURBO,4=ULTRA) to a [`Submode`].
fn submode_from_code(code: u8) -> Submode {
    match code {
        1 => Submode::Slow,
        2 => Submode::Fast,
        3 => Submode::Turbo,
        4 => Submode::Ultra,
        _ => Submode::Normal,
    }
}

/// Per-station query backoff (query policy, plan §4.4). Populated in a later TX phase.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueryBackoff {
    /// Number of queries sent to this station.
    pub attempts: u32,
    /// Earliest epoch-ms a further query is allowed.
    pub next_allowed_ms: u64,
}

/// A station heard on the JS8 calling channel.
#[derive(Debug, Clone, PartialEq)]
pub struct Js8Station {
    /// Normalized upper-case callsign.
    pub callsign: String,
    /// Maidenhead grid (4–6 chars) once heard, else `None`.
    pub grid: Option<String>,
    /// EWMA of decode SNRs (dB).
    pub snr_db: f32,
    /// Last audio offset (Hz).
    pub freq_offset_hz: f32,
    /// Dial (calling) frequency we heard them on (Hz).
    pub dial_freq_hz: u64,
    /// Last-heard epoch time (ms).
    pub last_heard_ms: u64,
    /// Times heard.
    pub heard_count: u32,
    /// Parsed `@OPULSE` hint, or `None` for a plain JS8 station.
    pub hint: Option<OphfHint>,
    /// Query backoff state.
    pub query_backoff: QueryBackoff,
}

/// One decode's worth of information about a station (the upsert input).
#[derive(Debug, Clone, PartialEq)]
pub struct Observation {
    /// Sender callsign (will be upper-cased).
    pub callsign: String,
    /// Grid if the frame carried one.
    pub grid: Option<String>,
    /// Decode SNR (dB).
    pub snr_db: f32,
    /// Audio offset (Hz).
    pub freq_offset_hz: f32,
    /// Dial frequency (Hz).
    pub dial_freq_hz: u64,
    /// Parsed hint if this was an `@OPULSE` marker.
    pub hint: Option<OphfHint>,
}

/// The discovered-station table: a callsign-keyed map with a TTL sweep.
#[derive(Debug, Clone, Default)]
pub struct StationTable {
    stations: BTreeMap<String, Js8Station>,
}

impl StationTable {
    /// An empty table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one observation, creating or updating the station. Returns whether the station was newly
    /// created (vs updated).
    pub fn upsert(&mut self, obs: Observation, now_ms: u64) -> bool {
        let key = obs.callsign.trim().to_ascii_uppercase();
        match self.stations.get_mut(&key) {
            Some(s) => {
                s.snr_db = SNR_EWMA_ALPHA * obs.snr_db + (1.0 - SNR_EWMA_ALPHA) * s.snr_db;
                s.freq_offset_hz = obs.freq_offset_hz;
                s.dial_freq_hz = obs.dial_freq_hz;
                s.last_heard_ms = now_ms;
                s.heard_count = s.heard_count.saturating_add(1);
                if obs.grid.is_some() {
                    s.grid = obs.grid;
                }
                if obs.hint.is_some() {
                    s.hint = obs.hint;
                }
                false
            }
            None => {
                self.stations.insert(
                    key.clone(),
                    Js8Station {
                        callsign: key,
                        grid: obs.grid,
                        snr_db: obs.snr_db,
                        freq_offset_hz: obs.freq_offset_hz,
                        dial_freq_hz: obs.dial_freq_hz,
                        last_heard_ms: now_ms,
                        heard_count: 1,
                        hint: obs.hint,
                        query_backoff: QueryBackoff::default(),
                    },
                );
                true
            }
        }
    }

    /// Remove stations not heard within `ttl_ms` of `now_ms`. Returns how many were dropped.
    pub fn sweep(&mut self, now_ms: u64, ttl_ms: u64) -> usize {
        let before = self.stations.len();
        self.stations
            .retain(|_, s| now_ms.saturating_sub(s.last_heard_ms) <= ttl_ms);
        before - self.stations.len()
    }

    /// Look up a station by callsign (case-insensitive).
    pub fn get(&self, callsign: &str) -> Option<&Js8Station> {
        self.stations.get(&callsign.trim().to_ascii_uppercase())
    }

    /// Iterate all stations.
    pub fn iter(&self) -> impl Iterator<Item = &Js8Station> {
        self.stations.values()
    }

    /// Number of stations.
    pub fn len(&self) -> usize {
        self.stations.len()
    }

    /// Whether the table is empty.
    pub fn is_empty(&self) -> bool {
        self.stations.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(call: &str, grid: Option<&str>, snr: f32) -> Observation {
        Observation {
            callsign: call.into(),
            grid: grid.map(Into::into),
            snr_db: snr,
            freq_offset_hz: 1500.0,
            dial_freq_hz: 14_078_000,
            hint: None,
        }
    }

    #[test]
    fn upsert_creates_then_updates() {
        let mut t = StationTable::new();
        assert!(t.upsert(obs("kn4crd", None, -15.0), 1000)); // new
        assert!(!t.upsert(obs("KN4CRD", Some("EM73"), -9.0), 2000)); // update, same call
        let s = t.get("kn4crd").unwrap();
        assert_eq!(s.callsign, "KN4CRD");
        assert_eq!(s.heard_count, 2);
        assert_eq!(s.grid.as_deref(), Some("EM73")); // grid learned on the 2nd hear
        assert_eq!(s.last_heard_ms, 2000);
        // EWMA moved toward the newer, stronger SNR but not all the way.
        assert!(s.snr_db > -15.0 && s.snr_db < -9.0);
    }

    #[test]
    fn grid_and_hint_are_sticky_once_learned() {
        let mut t = StationTable::new();
        t.upsert(obs("W1AW", Some("FN31"), -10.0), 100);
        t.upsert(obs("W1AW", None, -11.0), 200); // no grid this time
        assert_eq!(t.get("W1AW").unwrap().grid.as_deref(), Some("FN31"));
    }

    #[test]
    fn ttl_sweep_drops_stale_stations() {
        let mut t = StationTable::new();
        t.upsert(obs("A1AA", None, -10.0), 0);
        t.upsert(obs("B2BB", None, -10.0), 3_600_000);
        // At now = TTL + 1, A1AA (age TTL+1) is stale; B2BB (age 1) survives.
        assert_eq!(t.sweep(3_600_001, 3_600_000), 1);
        assert!(t.get("A1AA").is_none());
        assert!(t.get("B2BB").is_some());
    }

    #[test]
    fn ophf_hint_maps_pref_channel_and_submode() {
        let none = OphfHint::from_payload(
            1,
            &HintPayload {
                caps: 0x3,
                pref_channel: 63,
                listen_submode: 0,
            },
        );
        assert_eq!(none.pref_channel, None);
        assert_eq!(none.listen_submode, Submode::Normal);
        let some = OphfHint::from_payload(
            1,
            &HintPayload {
                caps: 0x3,
                pref_channel: 9,
                listen_submode: 2,
            },
        );
        assert_eq!(some.pref_channel, Some(9));
        assert_eq!(some.listen_submode, Submode::Fast);
    }
}
