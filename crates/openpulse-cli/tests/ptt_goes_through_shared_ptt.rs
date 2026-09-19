//! The CLI keys ONLY through `SharedPtt` (#1299).
//!
//! `openpulse-cli` was the last front-end calling `assert_ptt`/`release_ptt` by hand, at three
//! sites in two files. Hand-rolled keying has no RAII release on an unwind and no watchdog, and
//! #1257's transmit leader lives in `SharedPtt::key_as`, so a path outside the funnel would also
//! silently ignore it.
//!
//! This is a flat construct ban rather than an allowlist. An allowlist is what rots into the fourth
//! hand-rolled path: it grows an entry at a time, each justified, until the rule means nothing.
//! There is no exception here — even `calibrate ptt`, which MEASURES the assert→release round trip,
//! goes through the funnel — so the ban is unconditional and needs no exception list to maintain.

/// Raw hardware-keying calls that must not appear in CLI production code.
const BANNED: [&str; 4] = [
    ".assert_ptt(",
    ".release_ptt(",
    ".hw_assert(",
    ".hw_release(",
];

fn sources() -> Vec<(&'static str, &'static str)> {
    vec![
        ("main.rs", include_str!("../src/main.rs")),
        ("radio.rs", include_str!("../src/radio.rs")),
        (
            "commands/transmit.rs",
            include_str!("../src/commands/transmit.rs"),
        ),
        (
            "commands/calibrate.rs",
            include_str!("../src/commands/calibrate.rs"),
        ),
        (
            "commands/daemon.rs",
            include_str!("../src/commands/daemon.rs"),
        ),
        (
            "commands/session.rs",
            include_str!("../src/commands/session.rs"),
        ),
    ]
}

/// Everything before the first `#[cfg(test)]`: test modules may key however they like.
fn production_prefix(src: &str) -> &str {
    match src.find("#[cfg(test)]") {
        Some(i) => &src[..i],
        None => src,
    }
}

fn hits(src: &str) -> Vec<String> {
    src.lines()
        .enumerate()
        .filter(|(_, l)| {
            let code = l.split("//").next().unwrap_or(l);
            BANNED.iter().any(|b| code.contains(b))
        })
        .map(|(i, l)| format!(":{} {}", i + 1, l.trim()))
        .collect()
}

// VERIFIES: REQ-PTT-04
//
// The CLI half of the requirement, and the reason it needs its own binding: REQ-PTT-04's other
// three bindings live in ardop/kiss/daemon, none of whose test binaries can link `openpulse-cli`,
// so `cli/radio.rs` and `cli/commands/calibrate.rs` — 82 of the requirement's 363 mutants — were
// unreachable by construction (#1405).
//
// This is the acceptance method the requirement's own text names — a source scan requiring every
// keying site to sit inside the helper, validated against a planted bare call by the sibling test
// below. Note what that does and does not buy: it makes those mutants LINKABLE, not killable. A
// mutation of `calibrate.rs`'s logic does not introduce a bare `.assert_ptt(`, so the scan will not
// catch it. Reachability is necessary for a mutation verdict to mean anything and is not
// sufficient for it to be strong.
#[test]
fn the_cli_never_keys_the_rig_by_hand() {
    let files = sources();

    // Control: the scan detects a planted call. Without this the test passes on a broken filter.
    let planted = "\nfn planted() { ptt.assert_ptt().unwrap(); }\n";
    let base = production_prefix(files[0].1);
    assert_eq!(
        hits(&format!("{base}{planted}")).len(),
        hits(base).len() + 1,
        "the scan does not detect a planted assert_ptt — it proves nothing"
    );

    // Control: a comment mentioning the call is not a hit (the #1192 prose-as-reference shape).
    assert!(
        hits("// we used to call .assert_ptt( here\n").is_empty(),
        "the scan counts a mention in a comment as a keying call"
    );

    let mut bare = Vec::new();
    for (name, src) in &files {
        for line in hits(production_prefix(src)) {
            bare.push(format!("  {name}{line}"));
        }
    }
    assert!(
        bare.is_empty(),
        "these CLI sites key the rig outside `SharedPtt`, so they have no unwind release, no \
         watchdog, and would ignore the transmit leader (#1299, #1257):\n{}",
        bare.join("\n")
    );
}

#[test]
fn the_scan_actually_covers_the_files_that_key() {
    // A scan whose file list has drifted away from the keying code passes vacuously. These two
    // strings are what the migration put there; if they are gone, the list needs revisiting.
    let by_name = |n: &str| -> &'static str {
        sources()
            .into_iter()
            .find(|(name, _)| *name == n)
            .map(|(_, src)| src)
            .expect("file missing from the scan list")
    };
    assert!(
        by_name("commands/transmit.rs").contains("ptt.keyed("),
        "transmit.rs no longer keys through SharedPtt — the ban may be passing because the code moved"
    );
    assert!(
        by_name("commands/calibrate.rs").contains("SharedPtt::new("),
        "calibrate.rs no longer builds a SharedPtt — the ban may be passing because the code moved"
    );
}
