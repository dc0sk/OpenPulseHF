//! `openpulse-server` binary — a thin wrapper over [`openpulse_daemon::server::run`].
//!
//! Initialises tracing, loads the config, builds the config-selected audio
//! backend, and hands off to the extracted daemon run loop. The loop itself lives
//! in `server.rs` so it can also be driven in-process (the twin-station rig).

use openpulse_daemon::server::{build_audio_backend, run};

#[tokio::main]
async fn main() {
    let cfg = match openpulse_config::load() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("fatal: failed to load config: {e}");
            std::process::exit(1);
        }
    };

    // Init tracing from config: stdout plus an optional persistent rolling file log
    // (REQ-OBS-02). Bind the guard for the whole process so buffered file logs flush.
    let _log_guard = openpulse_config::logging::init_tracing(&cfg.logging);

    if cfg.station.callsign.trim().eq_ignore_ascii_case("N0CALL") {
        tracing::error!(
            "invalid callsign N0CALL in configuration; set [station].callsign before starting daemon"
        );
        std::process::exit(1);
    }

    let backend = build_audio_backend(&cfg.audio.backend);
    if let Err(e) = run(cfg, backend).await {
        tracing::error!(error = %e, "openpulse-server failed to start");
        std::process::exit(1);
    }
}
