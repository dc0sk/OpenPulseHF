//! `openpulse-mesh` must have no route to a sound card (#1251).
//!
//! This binary beacons and relays automatically, and has none of the transmit-safety machinery its
//! siblings have: no `PttController` (so `[modem] ptt_backend` is unread and only VOX could key it),
//! no carrier sense, and no `StationIdTimer` — while its beacon carries no callsign field of any
//! kind. `docs/regulatory.md` lists the repeater and the JS8 beacon as this project's
//! automatically-controlled stations under §97.221 and does **not** list mesh, which is the gate the
//! JS8 beacon was deliberately held for.
//!
//! Rather than hand-write a fourth copy of a keying pattern that has been wrong four times, the
//! capability was removed. These tests keep it removed: a `cpal` feature or a `CpalBackend`
//! reference reintroduces the route silently, because both compile fine.
//!
//! Each check is validated against a planted input, so a scan whose pattern rotted would fail here
//! rather than pass by matching nothing.

const MANIFEST: &str = include_str!("../Cargo.toml");
const MAIN: &str = include_str!("../src/main.rs");

/// No cargo feature may re-enable a real audio backend.
#[test]
fn the_crate_declares_no_cpal_feature() {
    // Control: the check can see a feature line at all.
    let planted = format!("{MANIFEST}\n[features]\ncpal = [\"openpulse-audio/cpal-backend\"]\n");
    assert!(
        declares_cpal_feature(&planted),
        "the check cannot detect a planted cpal feature — it proves nothing"
    );

    assert!(
        !declares_cpal_feature(MANIFEST),
        "openpulse-mesh declares a `cpal` feature again (#1251): that restores a route to real audio \
         for a binary with no PTT, no carrier sense and no station ID"
    );
}

/// No code path may construct a real audio backend.
#[test]
fn main_never_constructs_a_real_backend() {
    // Control: the check can see a constructor at all.
    let planted = format!("{MAIN}\nfn planted() {{ let _ = CpalBackend::new(); }}\n");
    assert!(
        mentions_cpal_backend(&planted),
        "the check cannot detect a planted CpalBackend — it proves nothing"
    );

    assert!(
        !mentions_cpal_backend(MAIN),
        "openpulse-mesh constructs a CpalBackend again (#1251)"
    );
}

/// `backend = "cpal"` must fail loudly rather than degrade to loopback.
///
/// Silent degradation is what made the sibling defects hard to see: the operator's config said one
/// thing and the binary did another with no diagnostic.
#[test]
fn a_cpal_backend_request_is_refused_not_downgraded() {
    let production = MAIN.split("#[cfg(test)]").next().unwrap_or(MAIN);
    assert!(
        production.contains("openpulse-mesh cannot use a real audio backend"),
        "requesting the cpal backend must bail with the reason, not fall back to loopback"
    );
}

fn declares_cpal_feature(manifest: &str) -> bool {
    manifest
        .lines()
        .map(str::trim)
        .any(|l| !l.starts_with('#') && l.starts_with("cpal") && l.contains('='))
}

fn mentions_cpal_backend(src: &str) -> bool {
    src.lines()
        .map(str::trim)
        .any(|l| !l.starts_with("//") && l.contains("CpalBackend"))
}
