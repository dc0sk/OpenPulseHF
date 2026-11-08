//! The twin rig refuses to start on a port something else holds.
//!
//! Twin tests bind fixed ports, and a run in another worktree may hold one. The rig used to start
//! anyway: the daemon whose bind failed exited, `wait_for_port` connected to whatever held the port,
//! and the test drove the other run's daemon. These hold each kind of port with a foreign listener
//! and require the rig to refuse.

use std::net::TcpListener;
use std::time::Duration;

use openpulse_channel::awgn::AwgnChannel;
use openpulse_channel::AwgnConfig;
use openpulse_config::OpenpulseConfig;
use openpulse_daemon::twin::spawn_bridged_pair;

/// A port the kernel just handed out and nothing holds now.
fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .expect("ephemeral port")
        .port()
}

fn cfg(callsign: &str, tcp_port: u16, ws_port: u16) -> OpenpulseConfig {
    let mut c = OpenpulseConfig::default();
    c.station.callsign = callsign.into();
    c.modem.mode = "BPSK250".into();
    c.daemon.tcp_port = tcp_port;
    c.daemon.websocket_port = ws_port;
    c
}

fn clean_awgn(seed: u64) -> Box<AwgnChannel> {
    Box::new(AwgnChannel::new(AwgnConfig::new(40.0, Some(seed))).unwrap())
}

async fn spawn_with_a_held(tcp_held: bool) {
    let foreign = TcpListener::bind("127.0.0.1:0").expect("foreign listener");
    let held = foreign.local_addr().expect("foreign addr").port();
    let (tcp_a, ws_a) = if tcp_held {
        (held, free_port())
    } else {
        (free_port(), held)
    };
    let pair = spawn_bridged_pair(
        cfg("A1AAA", tcp_a, ws_a),
        cfg("B1BBB", free_port(), free_port()),
        clean_awgn(1),
        clean_awgn(2),
        Duration::from_millis(10),
    )
    .await;
    pair.shutdown();
    drop(foreign);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[should_panic(expected = "is not free")]
async fn a_held_control_port_is_refused() {
    spawn_with_a_held(true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[should_panic(expected = "is not free")]
async fn a_held_websocket_port_is_refused() {
    spawn_with_a_held(false).await;
}
