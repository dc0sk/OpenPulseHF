//! Every full plugin registrar must register the same set (audit 2026-07-19, finding #13).
//!
//! There is no shared registrar: each front-end hand-lists the plugins it registers. Nothing checked
//! that those lists agree, so they drifted — the test matrix silently omitted `Mfsk16Plugin`, which
//! is `hpx_hf`'s SL1 sub-floor rung, and therefore published fade reports for a ladder missing its
//! weakest rung. The TUI had lost `Mfsk16Plugin` and `PilotPlugin` the same way.
//!
//! The real fix is one shared registrar crate; that is deferred because the daemon registers
//! GPU-capable plugins through a `cfg`-conditional macro (`with_gpu` vs `new`) that a naive shared
//! function would flatten. This gate holds the line until then: it does not prevent the duplication,
//! it prevents the duplication from *diverging silently*.
//!
//! Scanning source text is a blunt instrument, but it is the only thing that can compare registrars
//! living in five different crates without linking all of them together.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Registrars that are expected to register the FULL plugin set. Divergence between these is a bug.
const FULL_REGISTRARS: &[&str] = &[
    "crates/openpulse-cli/src/plugins.rs",
    "crates/openpulse-daemon/src/server.rs",
    "crates/openpulse-tui/src/main.rs",
    "apps/openpulse-linksim/src/lib.rs",
    "apps/openpulse-testmatrix/src/runners/mod.rs",
    "crates/openpulse-daemon/src/monitor.rs",
];

/// Sites that register a deliberately reduced set, with the reason. These are NOT compared against
/// the full set; they are listed so that "is this site partial on purpose?" has a written answer.
const INTENTIONALLY_PARTIAL: &[(&str, &str)] = &[(
    "crates/openpulse-kiss/src/bridge.rs",
    "KISS/AX.25 is a packet interface pinned to one configured mode; it registers only what that \
         mode needs rather than the whole waveform catalogue.",
)];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root is two levels above this crate")
        .to_path_buf()
}

/// Plugin type names registered in a file, covering both the direct `Plugin::new()` form and the
/// daemon's `register_gpu_plugin!(Plugin, ..)` macro form.
///
/// The macro form is the reason this scan exists in this shape: an earlier hand-audit of the same
/// question used a `Plugin::new` grep only, concluded the daemon was missing `Qam64Plugin`, and was
/// wrong. Any change here must keep both forms.
fn registered_in(path: &Path) -> BTreeSet<String> {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read registrar {}: {e}", path.display()));
    let mut out = BTreeSet::new();

    for (idx, _) in text.match_indices("Plugin::new") {
        let head = &text[..idx];
        let start = head
            .rfind(|c: char| !c.is_alphanumeric() && c != '_')
            .map(|i| i + 1)
            .unwrap_or(0);
        let name = format!("{}Plugin", &text[start..idx].trim_end_matches("Plugin"));
        if name.len() > "Plugin".len() {
            out.insert(name);
        }
    }
    for (idx, _) in text.match_indices("register_gpu_plugin!(") {
        let rest = &text[idx + "register_gpu_plugin!(".len()..];
        if let Some(end) = rest.find([',', ')']) {
            let name = rest[..end].trim();
            // Skip the macro *definition* (`($Plugin:ident, ..)`), keep call sites.
            if !name.starts_with('$') && name.ends_with("Plugin") {
                out.insert(name.to_string());
            }
        }
    }
    out
}

/// All full registrars must agree. The reference is the union: a plugin registered by any of them is
/// one every full front-end is expected to offer.
#[test]
fn full_registrars_register_the_same_plugins() {
    let root = workspace_root();
    let sets: Vec<(&str, BTreeSet<String>)> = FULL_REGISTRARS
        .iter()
        .map(|rel| (*rel, registered_in(&root.join(rel))))
        .collect();

    for (rel, set) in &sets {
        assert!(
            set.len() >= 5,
            "{rel} yielded only {} plugin(s) — the scan is broken, not the registrar. A parity test \
             that silently matches nothing passes forever.",
            set.len()
        );
    }

    let union: BTreeSet<String> = sets.iter().flat_map(|(_, s)| s.iter().cloned()).collect();
    let mut problems = Vec::new();
    for (rel, set) in &sets {
        let missing: Vec<&String> = union.difference(set).collect();
        if !missing.is_empty() {
            problems.push(format!("{rel} is missing {missing:?}"));
        }
    }

    assert!(
        problems.is_empty(),
        "plugin registrars have diverged:\n  {}\n\nEvery front-end in FULL_REGISTRARS must register \
         the same plugins; a missing one means that front-end silently cannot use those modes. The \
         test matrix omitting Mfsk16Plugin is what made this a real defect: it published fade \
         reports for hpx_hf without its SL1 rung. If a site should be partial, move it to \
         INTENTIONALLY_PARTIAL with a reason.",
        problems.join("\n  ")
    );
}

/// Every path named in either list must exist, so a rename cannot silently drop a registrar out of
/// the comparison — which would leave the gate green while the coverage disappeared.
#[test]
fn every_named_registrar_path_exists() {
    let root = workspace_root();
    for rel in FULL_REGISTRARS {
        assert!(
            root.join(rel).is_file(),
            "FULL_REGISTRARS names {rel}, which does not exist. Update the list — a stale path \
             removes a registrar from the parity check without failing it."
        );
    }
    for (rel, _why) in INTENTIONALLY_PARTIAL {
        assert!(
            root.join(rel).is_file(),
            "INTENTIONALLY_PARTIAL names {rel}, which does not exist."
        );
    }
}

/// A partial registrar must actually be partial. If one grows to the full set, the exemption is
/// stale and should be removed rather than left to hide a future omission.
#[test]
fn intentionally_partial_registrars_are_still_partial() {
    let root = workspace_root();
    let full: BTreeSet<String> = FULL_REGISTRARS
        .iter()
        .flat_map(|rel| registered_in(&root.join(rel)))
        .collect();

    for (rel, _why) in INTENTIONALLY_PARTIAL {
        let set = registered_in(&root.join(rel));
        assert!(
            set.len() < full.len(),
            "{rel} is listed as intentionally partial but now registers {} of {} plugins. Either \
             move it to FULL_REGISTRARS or delete the exemption.",
            set.len(),
            full.len()
        );
    }
}
