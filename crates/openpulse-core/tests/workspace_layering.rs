//! Structural constraints on the workspace dependency graph (audit 2026-07-19, finding #14).
//!
//! Nothing mechanically checked the crate layering, so the drift the crate map forbids arrived
//! unnoticed: `mfsk16-plugin` grew a production dependency on `js8-plugin`. A layering rule that
//! lives only in prose is a rule that is already being broken somewhere.
//!
//! Only **production** `[dependencies]` are checked. `[dev-dependencies]` are deliberately exempt:
//! `openpulse-modem` dev-depends on eight plugins to test against real waveforms, and several
//! plugins dev-depend on `openpulse-modem` for loopback harnesses. Those cycles are legal in Cargo
//! (dev-deps do not participate in the library's own build graph) and are the normal shape of
//! integration testing — banning them would delete real coverage to satisfy a diagram.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Production plugin→plugin edges that are permitted, each with the reason it exists and what would
/// retire it. An entry here is a debt with a name on it, not an exemption.
const ALLOWED_PLUGIN_EDGES: &[(&str, &str, &str)] = &[(
    "mfsk16-plugin",
    "js8-plugin",
    "MFSK16 reuses JS8's GFSK modulator and Goertzel energy detector \
     (`modulate_tones`, `GfskParams`, `DEFAULT_BT`, `goertzel_energy`). Both are generic DSP \
     primitives with no JS8-specific behaviour, so the edge is a misplacement rather than a real \
     coupling: they belong in `openpulse-dsp`, which both plugins already depend on. \
     RETIRE BY: extracting those four items into `openpulse-dsp` and dropping this dependency. \
     Recorded 2026-07-19 (audit finding #14) — allow-listed so the gate can land and block NEW \
     drift while the extraction is done separately.",
)];

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is <root>/crates/openpulse-core
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root is two levels above this crate")
        .to_path_buf()
}

struct Member {
    name: String,
    /// Directory group: "crates", "plugins", "apps", "tools", or the crate dir for one-offs.
    group: String,
    /// Names of workspace members this crate production-depends on.
    deps: Vec<String>,
}

/// Parse every workspace member manifest, returning production dependencies restricted to members.
fn members() -> BTreeMap<String, Member> {
    let root = workspace_root();
    let mut found = BTreeMap::new();
    let mut manifests: Vec<(String, PathBuf)> = Vec::new();

    for group in ["crates", "plugins", "apps", "tools"] {
        let dir = root.join(group);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let m = e.path().join("Cargo.toml");
            if m.is_file() {
                manifests.push((group.to_string(), m));
            }
        }
    }
    let pki = root.join("pki-tooling/Cargo.toml");
    if pki.is_file() {
        manifests.push(("pki-tooling".into(), pki));
    }

    assert!(
        manifests.len() > 30,
        "found only {} manifests — the workspace scan is broken, not the workspace. A layering \
         test that silently scans nothing passes forever.",
        manifests.len()
    );

    // First pass: names, so dependency edges can be restricted to workspace members.
    let mut parsed = Vec::new();
    for (group, path) in manifests {
        let text = std::fs::read_to_string(&path).expect("read manifest");
        let doc: toml::Value = toml::from_str(&text).expect("parse manifest");
        let Some(name) = doc
            .get("package")
            .and_then(|p| p.get("name"))
            .and_then(|n| n.as_str())
        else {
            continue; // virtual manifest
        };
        parsed.push((group, name.to_string(), doc));
    }
    let names: Vec<String> = parsed.iter().map(|(_, n, _)| n.clone()).collect();

    for (group, name, doc) in parsed {
        let deps = doc
            .get("dependencies")
            .and_then(|d| d.as_table())
            .map(|t| {
                t.keys()
                    .filter(|k| names.contains(k))
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        found.insert(name.clone(), Member { name, group, deps });
    }
    found
}

/// A plugin must not production-depend on another plugin: plugins are siblings, and an edge between
/// them means shared code sitting in the wrong crate.
#[test]
fn plugins_do_not_depend_on_other_plugins() {
    let members = members();
    let allowed: Vec<(&str, &str)> = ALLOWED_PLUGIN_EDGES
        .iter()
        .map(|(from, to, _)| (*from, *to))
        .collect();

    let mut violations = Vec::new();
    for m in members.values() {
        if m.group != "plugins" {
            continue;
        }
        for dep in &m.deps {
            if members.get(dep).is_none_or(|d| d.group != "plugins") {
                continue;
            }
            if allowed.contains(&(m.name.as_str(), dep.as_str())) {
                continue;
            }
            violations.push(format!("{} -> {}", m.name, dep));
        }
    }

    assert!(
        violations.is_empty(),
        "plugin -> plugin production dependency: {}\n\nPlugins are siblings. Shared code belongs in \
         a lower crate (openpulse-dsp / openpulse-core), not in a peer. If this edge is genuinely \
         unavoidable, add it to ALLOWED_PLUGIN_EDGES in this file WITH a rationale and a retirement \
         condition.",
        violations.join(", ")
    );
}

/// The allow-list must not outlive the edges it covers: a stale entry silently re-permits drift if
/// the same edge is reintroduced later for a different, unexamined reason.
#[test]
fn every_allowed_plugin_edge_still_exists() {
    let members = members();
    for (from, to, _why) in ALLOWED_PLUGIN_EDGES {
        let m = members
            .get(*from)
            .unwrap_or_else(|| panic!("allow-list names unknown crate {from}"));
        assert!(
            m.deps.iter().any(|d| d == to),
            "ALLOWED_PLUGIN_EDGES still lists {from} -> {to}, but that dependency is gone. Delete \
             the entry — a stale allowance re-permits the edge if it ever comes back."
        );
    }
}

/// Library crates must not depend on binaries/apps: an app is a consumer of the stack, never part
/// of it. This direction currently holds; the test keeps it that way.
#[test]
fn libraries_do_not_depend_on_apps_or_tools() {
    let members = members();
    let mut violations = Vec::new();
    for m in members.values() {
        if m.group == "apps" || m.group == "tools" {
            continue;
        }
        for dep in &m.deps {
            if members
                .get(dep)
                .is_some_and(|d| d.group == "apps" || d.group == "tools")
            {
                violations.push(format!("{} -> {}", m.name, dep));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "library crate depends on an app/tool crate: {}",
        violations.join(", ")
    );
}

/// `openpulse-core` and `openpulse-dsp` are the base layer: they must depend on no other workspace
/// member. Everything else is free to depend on them.
#[test]
fn base_layer_crates_have_no_workspace_dependencies() {
    let members = members();
    for base in ["openpulse-core", "openpulse-dsp"] {
        let m = members
            .get(base)
            .unwrap_or_else(|| panic!("{base} not found in the workspace scan"));
        assert!(
            m.deps.is_empty(),
            "{base} is a base-layer crate but production-depends on workspace members: {:?}. \
             Anything it needs must move down into it, not be pulled sideways.",
            m.deps
        );
    }
}
