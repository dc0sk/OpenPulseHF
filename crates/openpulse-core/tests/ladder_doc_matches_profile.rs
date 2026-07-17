//! `docs/mode-fec-ladder.md`'s `hpx_hf` table must match `SessionProfile::hpx_hf`.
//!
//! That table silently drifted across several releases — it still described a pre-OFDM, pre-MFSK16
//! ladder (SL5=QPSK250, SL8=SCFDMA52-8PSK) long after the code had moved on, and nothing caught it
//! because a doc has no gate. This is the same failure the modem keeps re-learning: a signal that is
//! *recorded* but not *enforced* is not a signal. The rung map is operator-facing (it is how someone
//! picks a profile and reads a level report), so it is worth a gate rather than good intentions.

use openpulse_core::fec::FecMode;
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;

const ALL_LEVELS: &[SpeedLevel] = &[
    SpeedLevel::Sl1,
    SpeedLevel::Sl2,
    SpeedLevel::Sl3,
    SpeedLevel::Sl4,
    SpeedLevel::Sl5,
    SpeedLevel::Sl6,
    SpeedLevel::Sl7,
    SpeedLevel::Sl8,
    SpeedLevel::Sl9,
    SpeedLevel::Sl10,
    SpeedLevel::Sl11,
    SpeedLevel::Sl12,
    SpeedLevel::Sl13,
    SpeedLevel::Sl14,
    SpeedLevel::Sl15,
    SpeedLevel::Sl16,
    SpeedLevel::Sl17,
    SpeedLevel::Sl18,
    SpeedLevel::Sl19,
    SpeedLevel::Sl20,
];

/// One parsed `| SL<n> | mode | fec | bps | floor | ceiling | notes |` row.
struct DocRow {
    level: usize,
    mode: String,
    fec: FecMode,
    floor: Option<f32>,
    ceiling: Option<f32>,
}

fn doc_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/mode-fec-ladder.md")
}

/// `—` (em dash) means "none" in the doc's FEC / SNR columns.
fn parse_fec(cell: &str) -> FecMode {
    match cell {
        "—" => FecMode::None,
        "Rs" => FecMode::Rs,
        "SC" => FecMode::SoftConcatenated,
        "LHR" => FecMode::LdpcHighRate,
        other => panic!("unknown FEC abbreviation {other:?} in the ladder doc table"),
    }
}

fn parse_snr(cell: &str) -> Option<f32> {
    if cell == "—" {
        return None;
    }
    Some(
        cell.parse::<f32>()
            .unwrap_or_else(|_| panic!("unparseable SNR cell {cell:?} in the ladder doc table")),
    )
}

fn parse_doc_rows(text: &str) -> Vec<DocRow> {
    let mut rows = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("| SL") else {
            continue;
        };
        let cells: Vec<&str> = rest.split('|').map(str::trim).collect();
        // cells[0] is the level number; then mode, fec, bps, floor, ceiling, notes.
        if cells.len() < 6 {
            continue;
        }
        let Ok(level) = cells[0].parse::<usize>() else {
            continue;
        };
        rows.push(DocRow {
            level,
            mode: cells[1].replace("**", ""),
            fec: parse_fec(cells[2]),
            floor: parse_snr(cells[4]),
            ceiling: parse_snr(cells[5]),
        });
    }
    rows
}

#[test]
fn mode_fec_ladder_doc_table_matches_hpx_hf_profile() {
    let text = std::fs::read_to_string(doc_path())
        .expect("docs/mode-fec-ladder.md must be readable from the workspace");
    let rows = parse_doc_rows(&text);
    let p = SessionProfile::hpx_hf();

    // The doc must not silently describe a shorter ladder than the code ships.
    let coded: Vec<usize> = ALL_LEVELS
        .iter()
        .enumerate()
        .filter(|(_, l)| p.mode_for(**l).is_some())
        .map(|(i, _)| i + 1)
        .collect();
    let documented: Vec<usize> = rows.iter().map(|r| r.level).collect();
    assert_eq!(
        documented, coded,
        "the ladder doc table must document exactly the rungs hpx_hf defines \
         (doc={documented:?}, code={coded:?})"
    );

    for row in &rows {
        let level = ALL_LEVELS[row.level - 1];
        assert_eq!(
            Some(row.mode.as_str()),
            p.mode_for(level),
            "SL{} mode: doc says {:?}, hpx_hf says {:?}",
            row.level,
            row.mode,
            p.mode_for(level)
        );
        assert_eq!(
            row.fec,
            p.fec_for(level),
            "SL{} FEC: doc says {:?}, hpx_hf says {:?}",
            row.level,
            row.fec,
            p.fec_for(level)
        );
        assert_eq!(
            row.floor,
            p.snr_floor_for_level(level),
            "SL{} SNR floor: doc says {:?}, hpx_hf says {:?}",
            row.level,
            row.floor,
            p.snr_floor_for_level(level)
        );
        assert_eq!(
            row.ceiling,
            p.snr_ceiling_for_level(level),
            "SL{} SNR ceiling: doc says {:?}, hpx_hf says {:?}",
            row.level,
            row.ceiling,
            p.snr_ceiling_for_level(level)
        );
    }
}
