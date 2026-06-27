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
}

/// Per-station ADIF logbook state.
#[derive(Default)]
pub struct Logbook {
    enabled: bool,
    path: String,
    station_callsign: String,
    my_grid: String,
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
    pub fn new(enabled: bool, path: &str, station_callsign: &str, my_grid: &str) -> Self {
        let station_callsign = if station_callsign == "N0CALL" {
            String::new()
        } else {
            station_callsign.to_string()
        };
        Self {
            enabled,
            path: expand_home(path),
            station_callsign,
            my_grid: my_grid.to_string(),
            pending: None,
        }
    }

    /// Record the start of a QSO (a successful connect). Overwrites any prior pending QSO.
    pub fn begin_qso(&mut self, peer: &str, submode: &str, freq_hz: Option<u64>, now_ms: u64) {
        self.pending = Some(Pending {
            peer: peer.to_string(),
            start_ms: now_ms,
            submode: submode.to_string(),
            freq_hz,
        });
    }

    /// Finalize the pending QSO (a disconnect) and append an ADIF record. Returns `Ok(true)` when a
    /// record was written, `Ok(false)` when there was nothing to write or the logbook is disabled.
    pub fn end_qso(&mut self, now_ms: u64) -> std::io::Result<bool> {
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
        let record = AdifRecord {
            call: p.peer,
            qso_date,
            time_on,
            time_off: Some(time_off),
            freq_mhz,
            band,
            mode: "DYNAMIC".into(),
            submode: nonempty(&p.submode),
            station_callsign: nonempty(&self.station_callsign),
            my_gridsquare: nonempty(&self.my_grid),
            ..Default::default()
        };
        append(&self.path, &record.to_adif())?;
        Ok(true)
    }
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

    #[test]
    fn writes_a_record_on_connect_then_disconnect() {
        let dir = std::env::temp_dir().join(format!("opadif-{}", std::process::id()));
        let path = dir.join("log.adi");
        let _ = std::fs::remove_file(&path);
        let mut lb = Logbook::new(true, path.to_str().unwrap(), "DL0XYZ", "AA00aa");

        lb.begin_qso("DL1ABC", "QPSK500", Some(14_070_000), 1_700_000_000_000);
        let wrote = lb.end_qso(1_700_000_300_000).unwrap();
        assert!(wrote);

        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("<ADIF_VER:5>3.1.4")); // header once
        assert!(body.contains("<CALL:6>DL1ABC"));
        assert!(body.contains("<BAND:3>20m"));
        assert!(body.contains("<MODE:7>DYNAMIC"));
        assert!(body.contains("<SUBMODE:7>QPSK500"));
        assert!(body.contains("<STATION_CALLSIGN:6>DL0XYZ"));

        // A second QSO appends (no duplicate header).
        lb.begin_qso("OZ2DEF", "BPSK250", None, 1_700_001_000_000);
        assert!(lb.end_qso(1_700_001_200_000).unwrap());
        let body = std::fs::read_to_string(&path).unwrap();
        assert_eq!(body.matches("<ADIF_VER").count(), 1);
        assert_eq!(body.matches("<EOR>").count(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn disabled_or_no_pending_writes_nothing() {
        let mut off = Logbook::new(false, "/nonexistent/should-not-write.adi", "X", "");
        off.begin_qso("A", "BPSK250", None, 1);
        assert!(!off.end_qso(2).unwrap()); // disabled → no write, no error

        let mut on = Logbook::new(true, "/nonexistent/should-not-write.adi", "X", "");
        assert!(!on.end_qso(2).unwrap()); // no pending → nothing
    }
}
