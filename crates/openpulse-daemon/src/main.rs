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

    // Refuse to start on a callsign the station cannot actually handshake with, rather than let the
    // operator discover it at the first ConnectPeer — or never (#1199). Three separate cases, each
    // of which reached the daemon before: the placeholder, an EMPTY callsign (which passed this
    // gate, since it only ever checked the sentinel), and one over the wire cap.
    //
    // The cap is taken from `caps::STATION_ID` BY REFERENCE, never transcribed: a cap checked
    // against a copied number stops agreeing with the encoder the moment the encoder changes, which
    // is the class of defect #1191 exists for.
    {
        let call = cfg.station.callsign.trim();
        let reason = if call.is_empty() {
            Some("callsign is empty".to_string())
        } else if call.eq_ignore_ascii_case("N0CALL") {
            Some("callsign is the placeholder N0CALL".to_string())
        } else if call.len() > openpulse_core::handshake_wire::caps::STATION_ID {
            Some(format!(
                "callsign is {} bytes, over the {}-byte handshake limit — this station could not \
                 complete a signed handshake in either direction",
                call.len(),
                openpulse_core::handshake_wire::caps::STATION_ID
            ))
        } else {
            None
        };
        if let Some(reason) = reason {
            tracing::error!("invalid [station].callsign: {reason}; set it before starting daemon");
            std::process::exit(1);
        }
    }

    let backend = build_audio_backend(&cfg.audio.backend);
    if let Err(e) = run(cfg, backend).await {
        tracing::error!(error = %e, "openpulse-server failed to start");
        std::process::exit(1);
    }
}
