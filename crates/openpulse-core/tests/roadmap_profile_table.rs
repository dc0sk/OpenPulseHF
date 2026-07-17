//! The roadmap's SessionProfile summary table must match the actual profiles.
//!
//! `docs/dev/project/roadmap.md` carries a "Profile | SL range | Initial | Top mode" table that had
//! silently drifted — it listed `hpx_hf` as `SL2–SL11 / SCFDMA52-64QAM` long after the fade-aware
//! re-seat made it `SL1–SL14 / OFDM52-64QAM`, and it omitted a profile entirely. A hand-maintained
//! table of twelve profiles rots; this gate makes it self-correcting the same way
//! `ladder_doc_matches_profile.rs` does for the mode/FEC ladder.
//!
//! To regenerate the table after a profile change, run the printer and paste its output:
//! `cargo test -p openpulse-core --test roadmap_profile_table print_roadmap_profile_table -- --ignored --nocapture`

use openpulse_core::profile::SessionProfile;

fn roadmap_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/dev/project/roadmap.md")
}

/// `(name, sl_range, initial, top_mode)` for every profile, in `PROFILE_NAMES` order.
fn expected_rows() -> Vec<(String, String, String, String)> {
    SessionProfile::PROFILE_NAMES
        .iter()
        .map(|&name| {
            let p = SessionProfile::by_name(name).expect("PROFILE_NAMES entry constructs");
            let levels = p.defined_levels();
            let first = *levels.first().expect("profile has ≥1 mapped level");
            let last = *levels.last().expect("profile has ≥1 mapped level");
            let range = if first == last {
                format!("SL{}", first as usize)
            } else {
                format!("SL{}–SL{}", first as usize, last as usize)
            };
            let initial = format!("SL{}", p.initial_level as usize);
            let top = p.mode_for(last).expect("top level has a mode").to_string();
            (name.to_string(), range, initial, top)
        })
        .collect()
}

/// Pull the `| \`name\` | range | initial | top |` rows out of the roadmap's SessionProfile table.
/// Only rows whose first cell is a backticked profile name are considered, so surrounding prose and
/// the header/separator lines are ignored.
fn doc_rows(text: &str) -> Vec<(String, String, String, String)> {
    let mut rows = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if !line.starts_with("| `hpx") {
            continue;
        }
        let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
        if cells.len() != 4 {
            continue;
        }
        let name = cells[0].trim_matches('`').to_string();
        rows.push((
            name,
            cells[1].to_string(),
            cells[2].to_string(),
            cells[3].to_string(),
        ));
    }
    rows
}

#[test]
fn roadmap_profile_table_matches_profiles() {
    let text = std::fs::read_to_string(roadmap_path()).expect("roadmap.md readable");
    let expected = expected_rows();
    let actual = doc_rows(&text);

    let want_names: Vec<&String> = expected.iter().map(|r| &r.0).collect();
    let got_names: Vec<&String> = actual.iter().map(|r| &r.0).collect();
    assert_eq!(
        got_names, want_names,
        "the roadmap SessionProfile table must list exactly the profiles in PROFILE_NAMES, in order \
         (doc={got_names:?}, code={want_names:?})"
    );

    for (want, got) in expected.iter().zip(actual.iter()) {
        assert_eq!(
            got, want,
            "roadmap row for `{}` is stale: doc={:?}, profile={:?}",
            want.0, got, want
        );
    }
}

/// Every mode string used by any profile, across all of `PROFILE_NAMES`.
fn all_profile_modes() -> std::collections::BTreeSet<String> {
    let mut modes = std::collections::BTreeSet::new();
    for &name in SessionProfile::PROFILE_NAMES {
        let p = SessionProfile::by_name(name).expect("constructs");
        for lvl in p.defined_levels() {
            if let Some(m) = p.mode_for(lvl) {
                modes.insert(m.to_string());
            }
        }
    }
    modes
}

/// The roadmap's "Modes in plugins but not in any profile (manual-select only)" table makes a
/// falsifiable claim about every mode it lists. BPSK100 sat there asserting "in no profile" long after
/// the RF-6 re-seat made it `hpx_hf` SL4 — a doc that actively misleads. This checks the claim.
#[test]
fn manual_select_modes_are_in_no_profile() {
    let text = std::fs::read_to_string(roadmap_path()).expect("roadmap.md readable");
    let profile_modes = all_profile_modes();

    // Scope to the table under the "manual-select only" heading.
    let start = text
        .find("### Modes in plugins but not in any profile")
        .expect("manual-select section present");
    let section = &text[start..];
    let end = section[1..]
        .find("\n### ")
        .map(|i| i + 1)
        .unwrap_or(section.len());
    let section = &section[..end];

    let mut checked = 0;
    for line in section.lines() {
        let line = line.trim();
        // Data rows only: skip the header (`| Mode |`) and separator (`|---|`).
        if !line.starts_with('|') || line.starts_with("| Mode") || line.starts_with("|--") {
            continue;
        }
        let first = line
            .trim_matches('|')
            .split('|')
            .next()
            .unwrap_or("")
            .trim();
        // The mode cell may list several with `/`, e.g. "64QAM500 / 64QAM1000".
        for mode in first.split('/').map(str::trim).filter(|m| !m.is_empty()) {
            assert!(
                !profile_modes.contains(mode),
                "`{mode}` is listed as manual-select-only ('in no profile'), but a profile uses it — \
                 move it out of that table (see BPSK100 / RF-6)"
            );
            checked += 1;
        }
    }
    assert!(
        checked > 0,
        "parsed no modes from the manual-select table — parser or heading drifted"
    );
}

#[test]
#[ignore = "printer: regenerate the roadmap SessionProfile table from the profiles"]
fn print_roadmap_profile_table() {
    println!("\n| Profile | SL range | Initial | Top mode |");
    println!("|---|---|---|---|");
    for (name, range, initial, top) in expected_rows() {
        println!("| `{name}` | {range} | {initial} | {top} |");
    }
}
