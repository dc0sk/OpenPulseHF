//! Twin-station rig: two real `openpulse-server` daemons bridged through a
//! channel in one process, for full-stack validation and investigation.
//!
//! Both daemons run the REAL [`crate::server::run`] stack — `RateAdapter`,
//! `HpxReactor`, OTA rate-stepping, QSY, repeater — unlike `openpulse-linksim`,
//! which reimplements the policy layers. The bridge drains daemon A's modem TX
//! (loopback playback) through a forward [`ChannelModel`] into daemon B's RX
//! (loopback capture), and B's TX through a reverse model into A's RX, so bugs in
//! the real on-air paths surface here against a deterministic seeded channel.
//!
//! Each daemon binds its own control port (from `[daemon]` config), so two real
//! `openpulse-panel` instances can attach — one per station — to watch both
//! directions live. See `examples/twin_station.rs`.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use openpulse_audio::LoopbackBackend;
use openpulse_channel::ChannelModel;
use openpulse_config::OpenpulseConfig;
use openpulse_modem::channel_sim::bridge_through;

use crate::server::run;

/// A running bridged daemon pair. Call [`shutdown`](Self::shutdown) to stop it.
pub struct BridgedPair {
    /// TCP control address of daemon A (attach a panel here).
    pub addr_a: SocketAddr,
    /// TCP control address of daemon B (attach a second panel here).
    pub addr_b: SocketAddr,
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
}

impl BridgedPair {
    /// Stop the bridge and both daemons, joining their threads.
    pub fn shutdown(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for h in self.threads.drain(..) {
            let _ = h.join();
        }
    }
}

/// Spawn two real daemons and bridge A→B through `fwd` and B→A through `rev`.
///
/// Returns once both control ports accept connections. `bridge_tick` paces the
/// sample-moving loop (10–20 ms is finer than the daemons' receive tick, so a
/// whole transmitted frame crosses in one step). Both daemons run the full
/// `server::run` stack, so the caller drives them entirely via the real control
/// protocol on `addr_a`/`addr_b`.
///
/// Must run on a multi-thread Tokio runtime: the daemon receive tick uses
/// `block_in_place`, which panics on a current-thread runtime.
pub async fn spawn_bridged_pair(
    cfg_a: OpenpulseConfig,
    cfg_b: OpenpulseConfig,
    mut fwd: Box<dyn ChannelModel>,
    mut rev: Box<dyn ChannelModel>,
    bridge_tick: Duration,
) -> BridgedPair {
    let addr_a = control_addr(&cfg_a);
    let addr_b = control_addr(&cfg_b);

    // Split-buffer loopbacks: a daemon must NOT receive its own transmissions, so
    // TX and RX are separate queues and the bridge moves TX→peer-RX. The harness
    // keeps a shared handle (`clone_shared`) to drain TX / fill RX.
    let a_lb = LoopbackBackend::new_split();
    let b_lb = LoopbackBackend::new_split();
    let a_run = a_lb.clone_shared();
    let b_run = b_lb.clone_shared();

    let stop = Arc::new(AtomicBool::new(false));

    // Each daemon runs on its own thread with a dedicated multi-thread runtime:
    // `server::run`'s future is `!Send` (the engine holds an mpsc receiver) so it
    // can't be `tokio::spawn`ed, and the daemon receive tick uses `block_in_place`
    // which needs a multi-thread runtime. `block_on` runs the `!Send` future on the
    // thread while the runtime stays multi-threaded — the same shape as the
    // `#[tokio::main]` binary. A stop flag races the run loop so we can join cleanly.
    let daemon_a = spawn_daemon_thread("A", cfg_a, Box::new(a_run), stop.clone());
    let daemon_b = spawn_daemon_thread("B", cfg_b, Box::new(b_run), stop.clone());

    let stop_bridge = stop.clone();
    let bridge = std::thread::spawn(move || {
        while !stop_bridge.load(Ordering::Relaxed) {
            bridge_through(&a_lb, &b_lb, fwd.as_mut()); // A TX (playback) → B RX (capture)
            bridge_through(&b_lb, &a_lb, rev.as_mut()); // B TX (playback) → A RX (capture)
            std::thread::sleep(bridge_tick);
        }
    });

    wait_for_port(addr_a).await;
    wait_for_port(addr_b).await;

    BridgedPair {
        addr_a,
        addr_b,
        stop,
        threads: vec![daemon_a, daemon_b, bridge],
    }
}

/// Spawn one daemon on a dedicated thread + multi-thread runtime, racing
/// `server::run` against the shared stop flag so `shutdown` can join it.
fn spawn_daemon_thread(
    label: &'static str,
    cfg: OpenpulseConfig,
    backend: Box<dyn openpulse_core::audio::AudioBackend>,
    stop: Arc<AtomicBool>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                tracing::error!(label, error = %e, "twin daemon runtime build failed");
                return;
            }
        };
        rt.block_on(async move {
            tokio::select! {
                r = run(cfg, backend) => {
                    if let Err(e) = r {
                        tracing::error!(label, error = %e, "twin daemon exited during startup");
                    }
                }
                _ = poll_stop(stop) => {}
            }
        });
    })
}

/// Resolve once the shared stop flag is set (polled, so it works across runtimes).
async fn poll_stop(stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::Relaxed) {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn control_addr(cfg: &OpenpulseConfig) -> SocketAddr {
    format!("{}:{}", cfg.daemon.tcp_bind_addr, cfg.daemon.tcp_port)
        .parse()
        .expect("valid daemon.tcp_bind_addr/tcp_port")
}

/// Poll until the control port accepts a connection (or give up after ~4 s).
async fn wait_for_port(addr: SocketAddr) {
    for _ in 0..200 {
        if tokio::net::TcpStream::connect(addr).await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    tracing::warn!(%addr, "twin-station control port did not come up in time");
}
