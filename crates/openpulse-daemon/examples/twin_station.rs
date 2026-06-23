//! Launch two REAL `openpulse-server` daemons bridged through a channel in one
//! process, for live two-panel visualization of both directions.
//!
//! Run it:
//! ```text
//! cargo run -p openpulse-daemon --example twin_station         # AWGN 20 dB
//! TWIN_SNR_DB=6 cargo run -p openpulse-daemon --example twin_station
//! ```
//! Then start two operator panels and Connect each to one station:
//! ```text
//! openpulse-panel    # Connect to 127.0.0.1:9000  (station A)
//! openpulse-panel    # Connect to 127.0.0.1:9002  (station B)
//! ```
//! Drive traffic over the bridged air from the CLI, e.g.:
//! ```text
//! openpulse daemon --addr 127.0.0.1:9000 ...    # or send a message via the panel
//! ```
//! Both daemons run the real stack (RateAdapter/HpxReactor/OTA/QSY), so what you
//! see in the panels is the true on-air behaviour through the simulated channel.

use std::time::Duration;

use openpulse_channel::awgn::AwgnChannel;
use openpulse_channel::AwgnConfig;
use openpulse_config::OpenpulseConfig;
use openpulse_daemon::twin::spawn_bridged_pair;

fn station_cfg(callsign: &str, tcp_port: u16, ws_port: u16) -> OpenpulseConfig {
    let mut c = OpenpulseConfig::default();
    c.station.callsign = callsign.into();
    c.modem.mode = "BPSK250".into();
    c.daemon.tcp_port = tcp_port;
    c.daemon.websocket_port = ws_port;
    c
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let snr_db: f32 = std::env::var("TWIN_SNR_DB")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(20.0);

    let pair = spawn_bridged_pair(
        station_cfg("TWIN-A", 9000, 9001),
        station_cfg("TWIN-B", 9002, 9003),
        Box::new(AwgnChannel::new(AwgnConfig::new(snr_db, Some(1))).unwrap()),
        Box::new(AwgnChannel::new(AwgnConfig::new(snr_db, Some(2))).unwrap()),
        Duration::from_millis(15),
    )
    .await;

    println!("twin-station rig up (forward + reverse channel: AWGN {snr_db} dB):");
    println!(
        "  station A control: {}  → connect a panel here",
        pair.addr_a
    );
    println!(
        "  station B control: {}  → connect a second panel here",
        pair.addr_b
    );
    println!("Both daemons run the real stack. Ctrl+C to stop.");

    // Park forever; Ctrl+C terminates the process (and both daemons + the bridge).
    std::future::pending::<()>().await;
}
