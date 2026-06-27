//! Minimal ADIF (Amateur Data Interchange Format) writer for the station logbook.
//!
//! One [`AdifRecord`] per completed contact (QSO), appended to an `.adi` file so logs import into
//! standard logging software / LoTW / eQSL. This is per-QSO and distinct from
//! [`crate::tx_metadata::TxSessionLog`] (the per-frame regulatory audit log).

/// One ADIF QSO record. Times/dates are UTC; `qso_date` is `YYYYMMDD`, times are `HHMMSS`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AdifRecord {
    /// Worked station's callsign (`CALL`).
    pub call: String,
    /// `QSO_DATE` — UTC `YYYYMMDD`.
    pub qso_date: String,
    /// `TIME_ON` — UTC `HHMMSS`.
    pub time_on: String,
    /// `TIME_OFF` — UTC `HHMMSS`.
    pub time_off: Option<String>,
    /// `FREQ` in MHz.
    pub freq_mhz: Option<f64>,
    /// `BAND` (e.g. `20m`); derive with [`band_for_mhz`].
    pub band: Option<String>,
    /// `MODE` — the ADIF mode token (OpenPulseHF's adaptive waveform reports `DYNAMIC`).
    pub mode: String,
    /// `SUBMODE` — the concrete HPX mode name (e.g. `QPSK500`).
    pub submode: Option<String>,
    pub rst_sent: Option<String>,
    pub rst_rcvd: Option<String>,
    /// `STATION_CALLSIGN` — our callsign.
    pub station_callsign: Option<String>,
    /// `MY_GRIDSQUARE` — our grid.
    pub my_gridsquare: Option<String>,
    /// `GRIDSQUARE` — the worked station's grid.
    pub gridsquare: Option<String>,
    pub comment: Option<String>,
}

fn field(name: &str, value: &str) -> String {
    format!("<{}:{}>{}", name, value.len(), value)
}

fn opt(name: &str, value: &Option<String>) -> String {
    value.as_deref().map(|v| field(name, v)).unwrap_or_default()
}

impl AdifRecord {
    /// Render this record as an ADIF data-specifier line terminated by `<EOR>`.
    pub fn to_adif(&self) -> String {
        let mut s = String::new();
        s.push_str(&field("CALL", &self.call));
        s.push_str(&field("QSO_DATE", &self.qso_date));
        s.push_str(&field("TIME_ON", &self.time_on));
        s.push_str(&opt("TIME_OFF", &self.time_off));
        if let Some(f) = self.freq_mhz {
            s.push_str(&field("FREQ", &format!("{f:.6}")));
        }
        s.push_str(&opt("BAND", &self.band));
        s.push_str(&field("MODE", &self.mode));
        s.push_str(&opt("SUBMODE", &self.submode));
        s.push_str(&opt("RST_SENT", &self.rst_sent));
        s.push_str(&opt("RST_RCVD", &self.rst_rcvd));
        s.push_str(&opt("STATION_CALLSIGN", &self.station_callsign));
        s.push_str(&opt("MY_GRIDSQUARE", &self.my_gridsquare));
        s.push_str(&opt("GRIDSQUARE", &self.gridsquare));
        s.push_str(&opt("COMMENT", &self.comment));
        s.push_str("<EOR>\n");
        s
    }
}

/// ADIF file header (written once, before the first record).
pub fn adif_header() -> String {
    "OpenPulseHF ADIF logbook\n<ADIF_VER:5>3.1.4<PROGRAMID:11>OpenPulseHF<EOH>\n".to_string()
}

/// Convert a Unix timestamp (milliseconds, UTC) to ADIF `(QSO_DATE "YYYYMMDD", TIME "HHMMSS")`.
/// Pure (no time crate): days→civil via Howard Hinnant's algorithm.
pub fn utc_date_time(unix_ms: u64) -> (String, String) {
    let secs = unix_ms / 1000;
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (h, mi, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let (y, mo, d) = civil_from_days(days);
    (
        format!("{y:04}{mo:02}{d:02}"),
        format!("{h:02}{mi:02}{s:02}"),
    )
}

/// (year, month, day) for a count of days since the Unix epoch (1970-01-01).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Map a frequency (MHz) to its ADIF amateur band token, if it falls in a band.
pub fn band_for_mhz(mhz: f64) -> Option<&'static str> {
    const BANDS: &[(f64, f64, &str)] = &[
        (1.8, 2.0, "160m"),
        (3.5, 4.0, "80m"),
        (5.06, 5.45, "60m"),
        (7.0, 7.3, "40m"),
        (10.1, 10.15, "30m"),
        (14.0, 14.35, "20m"),
        (18.068, 18.168, "17m"),
        (21.0, 21.45, "15m"),
        (24.89, 24.99, "12m"),
        (28.0, 29.7, "10m"),
        (50.0, 54.0, "6m"),
        (144.0, 148.0, "2m"),
        (430.0, 440.0, "70cm"),
    ];
    BANDS
        .iter()
        .find(|(lo, hi, _)| mhz >= *lo && mhz <= *hi)
        .map(|(_, _, b)| *b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_renders_required_and_optional_fields() {
        let r = AdifRecord {
            call: "DL1ABC".into(),
            qso_date: "20260627".into(),
            time_on: "143000".into(),
            time_off: Some("143512".into()),
            freq_mhz: Some(14.070),
            band: band_for_mhz(14.070).map(str::to_string),
            mode: "DYNAMIC".into(),
            submode: Some("QPSK500".into()),
            station_callsign: Some("N0CALL".into()),
            my_gridsquare: Some("AA00aa".into()),
            comment: Some("eff 612 bps, SL6".into()),
            ..Default::default()
        };
        let adif = r.to_adif();
        // Field encoding: <NAME:len>value with the byte length.
        assert!(adif.contains("<CALL:6>DL1ABC"));
        assert!(adif.contains("<QSO_DATE:8>20260627"));
        assert!(adif.contains("<TIME_ON:6>143000"));
        assert!(adif.contains("<TIME_OFF:6>143512"));
        assert!(adif.contains("<BAND:3>20m"));
        assert!(adif.contains("<MODE:7>DYNAMIC"));
        assert!(adif.contains("<SUBMODE:7>QPSK500"));
        assert!(adif.contains("<STATION_CALLSIGN:6>N0CALL"));
        assert!(adif.trim_end().ends_with("<EOR>"));
        // Omitted optionals must not appear.
        assert!(!adif.contains("<RST_SENT"));
    }

    #[test]
    fn band_mapping() {
        assert_eq!(band_for_mhz(14.070), Some("20m"));
        assert_eq!(band_for_mhz(7.040), Some("40m"));
        assert_eq!(band_for_mhz(0.5), None);
    }

    #[test]
    fn utc_date_time_formats() {
        assert_eq!(utc_date_time(0), ("19700101".into(), "000000".into()));
        assert_eq!(
            utc_date_time(86_399_000),
            ("19700101".into(), "235959".into())
        );
        // 1_700_000_000 s = 2023-11-14 22:13:20 UTC.
        assert_eq!(
            utc_date_time(1_700_000_000_000),
            ("20231114".into(), "221320".into())
        );
    }

    #[test]
    fn header_is_well_formed() {
        let h = adif_header();
        assert!(h.contains("<ADIF_VER:5>3.1.4"));
        assert!(h.contains("<PROGRAMID:11>OpenPulseHF"));
        assert!(h.trim_end().ends_with("<EOH>"));
    }
}
