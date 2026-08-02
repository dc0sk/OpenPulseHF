//! RESEARCH HARNESS for #1058 — is the debug-profile failure a deadline, or the retry budget?
//!
//! Reproduces `a_no_template_mode_decodes_through_a_saturating_floor` exactly (QPSK1000 + Rs, the
//! recorded IC-9700 hot floor, lead 40 000) with the receive timeout parameterised and engine
//! tracing on, so the wall-clock-dependent branches in `engine.rs` can be observed rather than
//! inferred. Asserts nothing.
//!
//! ```text
//! ABL_TIMEOUT_MS=40000 ABL_LEAD=40000 cargo test -p openpulse-modem --no-default-features \
//!     --test saturating_floor_ablation -- --ignored --nocapture
//! ```

use std::time::{Duration, Instant};

use openpulse_core::fec::FecMode;
use openpulse_modem::capture_replay::load_corpus;
use openpulse_modem::channel_sim::ChannelSimHarness;

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

#[test]
#[ignore = "research harness"]
fn ablate() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .try_init();

    let timeout_ms = env_usize("ABL_TIMEOUT_MS", 40_000) as u64;
    let lead = env_usize("ABL_LEAD", 40_000);
    let mode = std::env::var("ABL_MODE").unwrap_or_else(|_| "QPSK1000".to_string());

    let hot = load_corpus("ic9700-idle-hot.wav").expect("corpus");
    let mut h = ChannelSimHarness::new();
    for eng in [&mut h.tx_engine, &mut h.rx_engine] {
        eng.register_plugin(Box::new(qpsk_plugin::QpskPlugin::new()))
            .unwrap();
    }
    h.tx_engine
        .transmit_with_fec_mode(b"no template probe", &mode, FecMode::Rs, None)
        .expect("transmit");
    h.route_embedded_in_capture(&hot, lead, 40_000, 0.3);

    let t0 = Instant::now();
    let got = h.rx_engine.receive_with_fec_mode_timeout(
        &mode,
        FecMode::Rs,
        None,
        Duration::from_millis(timeout_ms),
    );
    let wall = t0.elapsed();

    println!(
        "ABL mode={mode} lead={lead} timeout_ms={timeout_ms} wall_s={:.1} outcome={} \
         condemnations={} rho_rejected={}",
        wall.as_secs_f32(),
        match &got {
            Ok(b) => format!("OK({})", String::from_utf8_lossy(b)),
            Err(e) => format!("ERR({e})"),
        },
        h.rx_engine.settle_condemnations(),
        h.rx_engine.rho_rejected_settles(),
    );
}
