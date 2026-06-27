//! Automatic ADIF logbook: one record per completed contact (a connect→disconnect session).
//!
//! Opt-in via `[logbook]` config. Decoupled from the modem path — appends to the `.adi` file on
//! disconnect; failures are logged, never propagated into the RF loop.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

use openpulse_core::adif::{adif_header, band_for_mhz, utc_date_time, AdifRecord};

/// In-flight QSO captured at connect, finalized at disconnect.
struct Pending {
    peer: String,
    start_ms: u64,
    submode: String,
    freq_hz: Option<u64>,
    gridsquare: Option<String>,
}

/// Per-station ADIF logbook state.
#[derive(Default)]
pub struct Logbook {
    enabled: bool,
    path: String,
    station_callsign: String,
    my_grid: String,
    /// callsign (lowercased) → grid, to fill the worked station's `GRIDSQUARE` from config when
    /// it isn't exchanged on air.
    peer_grids: std::collections::BTreeMap<String, String>,
    pending: Option<Pending>,
}

fn nonempty(s: &str) -> Option<String> {
    (!s.is_empty()).then(|| s.to_string())
}

fn expand_home(path: &str) -> String {
    match path.strip_prefix("~/") {
        Some(rest) => match std::env::var("HOME") {
            Ok(home) => format!("{home}/{rest}"),
            Err(_) => path.to_string(),
        },
        None => path.to_string(),
    }
}

impl Logbook {
    /// Build from config. `station_callsign`/`my_grid` populate the `STATION_CALLSIGN` /
    /// `MY_GRIDSQUARE` fields; a default `N0CALL` callsign is treated as unset.
    pub fn new(
        enabled: bool,
        path: &str,
        station_callsign: &str,
        my_grid: &str,
        peer_grids: &std::collections::BTreeMap<String, String>,
    ) -> Self {
        let station_callsign = if station_callsign == "N0CALL" {
            String::new()
        } else {
            station_callsign.to_string()
        };
        let peer_grids = peer_grids
            .iter()
            .map(|(k, v)| (k.to_lowercase(), v.clone()))
            .collect();
        Self {
            enabled,
            path: expand_home(path),
            station_callsign,
            my_grid: my_grid.to_string(),
            peer_grids,
            pending: None,
        }
    }

    /// Enable/disable the logbook at runtime (control-protocol `SetLogbook`).
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Whether the logbook is currently enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Record the start of a QSO (a successful connect). Overwrites any prior pending QSO. The
    /// worked station's grid is looked up from the configured `peer_grids` map (case-insensitive).
    pub fn begin_qso(&mut self, peer: &str, submode: &str, freq_hz: Option<u64>, now_ms: u64) {
        let gridsquare = self.peer_grids.get(&peer.to_lowercase()).cloned();
        self.pending = Some(Pending {
            peer: peer.to_string(),
            start_ms: now_ms,
            submode: submode.to_string(),
            freq_hz,
            gridsquare,
        });
    }

    /// Finalize the pending QSO (a disconnect) and append an ADIF record. `rx_snr_db` is the
    /// receiver's last SNR estimate, used to fill `RST_RCVD` + a `COMMENT`. Returns `Ok(true)` when
    /// a record was written, `Ok(false)` when there was nothing to write or the logbook is disabled.
    pub fn end_qso(&mut self, now_ms: u64, rx_snr_db: Option<f32>) -> std::io::Result<bool> {
        let Some(p) = self.pending.take() else {
            return Ok(false);
        };
        if !self.enabled {
            return Ok(false);
        }
        let (qso_date, time_on) = utc_date_time(p.start_ms);
        let (_, time_off) = utc_date_time(now_ms);
        let freq_mhz = p.freq_hz.map(|hz| hz as f64 / 1e6);
        let band = freq_mhz.and_then(band_for_mhz).map(|b| b.to_string());
        let rst_rcvd = rx_snr_db.map(rst_from_snr);
        let comment = {
            let mode = if p.submode.is_empty() {
                "adaptive".to_string()
            } else {
                p.submode.clone()
            };
            match rx_snr_db {
                Some(snr) => format!("OpenPulseHF {mode}, RX SNR {snr:.0} dB"),
                None => format!("OpenPulseHF {mode}"),
            }
        };
        let record = AdifRecord {
            call: p.peer,
            qso_date,
            time_on,
            time_off: Some(time_off),
            freq_mhz,
            band,
            mode: "DYNAMIC".into(),
            submode: nonempty(&p.submode),
            rst_rcvd,
            station_callsign: nonempty(&self.station_callsign),
            my_gridsquare: nonempty(&self.my_grid),
            gridsquare: p.gridsquare,
            comment: Some(comment),
            ..Default::default()
        };
        append(&self.path, &record.to_adif())?;
        Ok(true)
    }
}

/// Coarse readability/strength/tone report from an RX SNR (dB) — a sensible ADIF `RST` for an
/// adaptive digital mode where no signal-report exchange happens.
fn rst_from_snr(snr_db: f32) -> String {
    match snr_db {
        s if s >= 20.0 => "599",
        s if s >= 10.0 => "579",
        s if s >= 3.0 => "559",
        s if s >= -3.0 => "539",
        _ => "519",
    }
    .to_string()
}

/// Append one ADIF record to `path`, writing the header first if the file is new/empty.
fn append(path: &str, record: &str) -> std::io::Result<()> {
    if let Some(dir) = Path::new(path).parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)?;
        }
    }
    let fresh = std::fs::metadata(path)
        .map(|m| m.len() == 0)
        .unwrap_or(true);
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    if fresh {
        f.write_all(adif_header().as_bytes())?;
    }
    f.write_all(record.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grids<const N: usize>(
        pairs: [(&str, &str); N],
    ) -> std::collections::BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn writes_a_record_on_connect_then_disconnect() {
        let dir = std::env::temp_dir().join(format!("opadif-{}", std::process::id()));
        let path = dir.join("log.adi");
        let _ = std::fs::remove_file(&path);
        // Case-insensitive peer-grid lookup → the worked station's GRIDSQUARE.
        let mut lb = Logbook::new(
            true,
            path.to_str().unwrap(),
            "DL0XYZ",
            "AA00aa",
            &grids([("dl1abc", "JO31aa")]), // lowercase key; connect uses uppercase
        );

        lb.begin_qso("DL1ABC", "QPSK500", Some(14_070_000), 1_700_000_000_000);
        let wrote = lb.end_qso(1_700_000_300_000, Some(14.0)).unwrap();
        assert!(wrote);

        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("<ADIF_VER:5>3.1.4")); // header once
        assert!(body.contains("<CALL:6>DL1ABC"));
        assert!(body.contains("<BAND:3>20m"));
        assert!(body.contains("<MODE:7>DYNAMIC"));
        assert!(body.contains("<SUBMODE:7>QPSK500"));
        assert!(body.contains("<STATION_CALLSIGN:6>DL0XYZ"));
        // RX SNR 14 dB → RST 579 + a COMMENT carrying the mode and SNR.
        assert!(body.contains("<RST_RCVD:3>579"));
        assert!(body.contains("RX SNR 14 dB"));
        // Worked station's grid from the config lookup (matched case-insensitively).
        assert!(body.contains("<GRIDSQUARE:6>JO31aa"));

        // A second QSO appends (no duplicate header).
        lb.begin_qso("OZ2DEF", "BPSK250", None, 1_700_001_000_000);
        assert!(lb.end_qso(1_700_001_200_000, None).unwrap());
        let body = std::fs::read_to_string(&path).unwrap();
        assert_eq!(body.matches("<ADIF_VER").count(), 1);
        assert_eq!(body.matches("<EOR>").count(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn disabled_or_no_pending_writes_nothing() {
        let mut off = Logbook::new(
            false,
            "/nonexistent/should-not-write.adi",
            "X",
            "",
            &grids([]),
        );
        off.begin_qso("A", "BPSK250", None, 1);
        assert!(!off.end_qso(2, None).unwrap()); // disabled → no write, no error

        let mut on = Logbook::new(
            true,
            "/nonexistent/should-not-write.adi",
            "X",
            "",
            &grids([]),
        );
        assert!(!on.end_qso(2, None).unwrap()); // no pending → nothing
    }

    #[test]
    fn runtime_toggle_controls_writes() {
        let path = std::env::temp_dir().join(format!("opadif-toggle-{}.adi", std::process::id()));
        let _ = std::fs::remove_file(&path);
        // Built disabled (config), then enabled at runtime via SetLogbook.
        let mut lb = Logbook::new(false, path.to_str().unwrap(), "X", "", &grids([]));
        assert!(!lb.is_enabled());
        lb.set_enabled(true);
        assert!(lb.is_enabled());
        lb.begin_qso("DL1ABC", "BPSK250", None, 1_700_000_000_000);
        assert!(lb.end_qso(1_700_000_100_000, Some(25.0)).unwrap());
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("<CALL:6>DL1ABC"));
        assert!(body.contains("<RST_RCVD:3>599")); // 25 dB → 599
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rst_from_snr_buckets() {
        assert_eq!(rst_from_snr(25.0), "599");
        assert_eq!(rst_from_snr(12.0), "579");
        assert_eq!(rst_from_snr(5.0), "559");
        assert_eq!(rst_from_snr(0.0), "539");
        assert_eq!(rst_from_snr(-10.0), "519");
    }
}
