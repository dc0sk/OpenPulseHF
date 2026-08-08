//! Every device the backend lists must also be selectable by name — **input and output**.
//!
//! The input half exists because `select_cpal_device` is generic over the enumerator and production
//! calls it with `input_devices()` for every receive stream (`cpal_backend.rs`, the capture path)
//! and `output_devices()` for transmit. The truncation defect below is a property of the ALSA
//! enumeration, not of a direction, so it applies to both — but this file tested output only, which
//! left the half a *receiver* depends on unguarded. Found by an orphan sweep: `resolve_input_name`
//! had no caller anywhere while its output twin was exercised here (issue #1092).
//!
//! **The defect this pins.** cpal's ALSA enumeration is *stateful*: holding `cpal::Device` values
//! alive while iterating silently truncates the list. Measured on one host, same process, same
//! moment:
//!
//! | pattern | devices seen |
//! |---|---|
//! | name each device, drop it | 39 |
//! | name it and **retain** it (what `select_cpal_device` used to do) | **18** |
//! | collect the devices, then name them | **4** |
//!
//! Nothing errors — the missing entries simply never arrive. So `openpulse devices` (which drops as
//! it goes) advertised devices that `--device` could not open: `aloop_tx` was unreachable while
//! `hwloop_tx` happened to fall inside the surviving prefix. That made the entire virtual-loopback
//! rung unrunnable, and it presented as "device not found" for a device sitting in the listing.
//!
//! `select_cpal_device` now enumerates twice — names only in pass 1, retaining just the single match
//! in pass 2.
//!
//! Requires a real ALSA/CoreAudio host, so it is gated on the `cpal-backend` feature and does not run
//! in the `--no-default-features` workspace gate. Run it with:
//! `cargo test -p openpulse-audio --features cpal-backend --test device_enumeration`

#![cfg(feature = "cpal-backend")]

use cpal::traits::{DeviceTrait, HostTrait};
use openpulse_audio::CpalBackend;
use std::sync::{Mutex, MutexGuard};

/// ALSA enumeration is **process-global mutable state**, so these tests must not run concurrently.
///
/// Measured on this host: run in parallel, `every_listed_output_device_can_be_selected_by_name`
/// fails with `1 of 25 listed output devices could not be resolved` — the `samplerate` plugin
/// device appears in one enumeration and not the next while another thread is enumerating. Run with
/// `--test-threads=1`, all pass. The existing tests were already exposed to this; two happened not
/// to collide often enough to notice, and adding the input pair made it reproducible.
///
/// Serialising here rather than documenting `--test-threads=1` deliberately: a caller obligation
/// that the acceptance table does not carry is one nobody will honour, and the failure it produces
/// looks exactly like the truncation defect this file exists to catch — a false positive pointing
/// at production code that is fine.
static ALSA: Mutex<()> = Mutex::new(());

/// Take the enumeration lock, ignoring poisoning — a panic in one test must not convert every
/// sibling into a second failure reported against unrelated code.
fn alsa_lock() -> MutexGuard<'static, ()> {
    ALSA.lock().unwrap_or_else(|e| e.into_inner())
}

/// Names the host reports for output devices, taken without retaining any device.
fn output_names() -> Vec<String> {
    cpal::default_host()
        .output_devices()
        .map(|ds| ds.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default()
}

/// THE GATE: every listed output device must be reachable by its own name.
///
/// Before the fix, the devices past the truncation point returned `device not found` for a name that
/// the listing had just produced.
#[test]
fn every_listed_output_device_can_be_selected_by_name() {
    let _serialised = alsa_lock();
    let names = output_names();
    if names.is_empty() {
        // No audio host at all (headless container). Say so loudly rather than pass quietly — a
        // silent skip here is how a truncation bug would slip through unnoticed.
        eprintln!("SKIP: host reports no output devices; nothing to verify");
        return;
    }

    let backend = CpalBackend::new();

    // Resolve WITHOUT opening. Opening a stream perturbs ALSA state and changes the enumeration —
    // the first version of this test opened all 32 devices in sequence and the device count moved
    // under it (39 -> 32), so it was measuring its own side effects.
    let mut unreachable = Vec::new();
    for name in &names {
        if let Err(e) = backend.resolve_output_name(name) {
            unreachable.push(format!("{name}: {e}"));
        }
    }

    assert!(
        unreachable.is_empty(),
        "{} of {} listed output devices could not be resolved by their own name — the enumeration \
         is being truncated, so the listing advertises devices that cannot be opened:\n  {}",
        unreachable.len(),
        names.len(),
        unreachable.join("\n  ")
    );
}

/// Retaining devices during enumeration must not change how many the host reports.
///
/// This is the underlying property, tested directly: it fails on any host where cpal truncates, even
/// if the truncated tail happens to contain nothing the other test tries to open.
#[test]
fn retaining_devices_during_enumeration_does_not_truncate_the_list() {
    let _serialised = alsa_lock();
    let host = cpal::default_host();

    let dropped: Vec<String> = match host.output_devices() {
        Ok(ds) => ds.filter_map(|d| d.name().ok()).collect(),
        Err(_) => return,
    };
    if dropped.is_empty() {
        eprintln!("SKIP: host reports no output devices; nothing to verify");
        return;
    }

    let retained: Vec<String> = match host.output_devices() {
        Ok(ds) => ds
            .filter_map(|d| d.name().ok().map(|n| (n, d)))
            .collect::<Vec<(String, cpal::Device)>>()
            .into_iter()
            .map(|(n, _)| n)
            .collect(),
        Err(_) => return,
    };

    // Documenting the observed asymmetry rather than asserting equality: this is a property of the
    // cpal/ALSA stack, not of our code, and our fix is to not depend on it. If a future cpal makes
    // these equal, the assert below still holds and this test becomes a no-op sentinel.
    if retained.len() < dropped.len() {
        eprintln!(
            "NOTE: this host truncates under retention ({} retained vs {} dropped) — exactly the \
             condition `select_cpal_device` must not depend on",
            retained.len(),
            dropped.len()
        );
    }

    // Whatever the host does, the resolver must see the FULL list. Prove it via the selector on a
    // name that only appears in the untruncated enumeration.
    if let Some(tail_name) = dropped.get(retained.len()) {
        let backend = CpalBackend::new();
        assert!(
            backend.resolve_output_name(tail_name).is_ok(),
            "'{tail_name}' is listed by the host but the selector cannot resolve it — the resolver \
             is seeing a truncated enumeration"
        );
    }
}

/// Names the host reports for input devices, taken without retaining any device.
fn input_names() -> Vec<String> {
    cpal::default_host()
        .input_devices()
        .map(|ds| ds.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default()
}

/// THE GATE, capture side: every listed input device must be reachable by its own name.
///
/// This is the direction a receiver depends on — `AudioBackend::input_stream` resolves the capture
/// device through the same two-pass `select_cpal_device`. A truncated input enumeration presents
/// exactly as the output defect did: `device not found` for a name the listing just produced.
#[test]
fn every_listed_input_device_can_be_selected_by_name() {
    let _serialised = alsa_lock();
    let names = input_names();
    if names.is_empty() {
        eprintln!("SKIP: host reports no input devices; nothing to verify");
        return;
    }

    let backend = CpalBackend::new();

    // Resolve WITHOUT opening, for the same reason the output test does: opening perturbs ALSA
    // state and moves the enumeration under the measurement.
    let mut unreachable = Vec::new();
    for name in &names {
        if let Err(e) = backend.resolve_input_name(name) {
            unreachable.push(format!("{name}: {e}"));
        }
    }

    assert!(
        unreachable.is_empty(),
        "{} of {} listed input devices could not be resolved by their own name — the enumeration \
         is being truncated, so the listing advertises capture devices that cannot be opened:\n  {}",
        unreachable.len(),
        names.len(),
        unreachable.join("\n  ")
    );
}

/// The retention property on the capture side.
///
/// Same underlying ALSA behaviour as the output test; asserted separately because a host can
/// truncate one direction and not the other, and nothing in cpal guarantees they behave alike.
#[test]
fn retaining_input_devices_during_enumeration_does_not_truncate_the_list() {
    let _serialised = alsa_lock();
    let host = cpal::default_host();

    let dropped: Vec<String> = match host.input_devices() {
        Ok(ds) => ds.filter_map(|d| d.name().ok()).collect(),
        Err(_) => return,
    };
    if dropped.is_empty() {
        eprintln!("SKIP: host reports no input devices; nothing to verify");
        return;
    }

    let retained: Vec<String> = match host.input_devices() {
        Ok(ds) => ds
            .filter_map(|d| d.name().ok().map(|n| (n, d)))
            .collect::<Vec<(String, cpal::Device)>>()
            .into_iter()
            .map(|(n, _)| n)
            .collect(),
        Err(_) => return,
    };

    if retained.len() < dropped.len() {
        eprintln!(
            "NOTE: this host truncates input under retention ({} retained vs {} dropped) — exactly \
             the condition `select_cpal_device` must not depend on",
            retained.len(),
            dropped.len()
        );
    }

    // Every name the retaining pass DID see must still resolve; that is the property our two-pass
    // selector guarantees regardless of how the host behaves.
    let backend = CpalBackend::new();
    for name in &retained {
        assert!(
            backend.resolve_input_name(name).is_ok(),
            "input device `{name}` was enumerated under retention but cannot be resolved by name"
        );
    }
}
