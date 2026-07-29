//! The multi-mode receive monitor (REQ-RX-01) must keep emitting while an OTA session is active.
//!
//! **The defect this pins** (archetype scan 2026-07-29, finding 9). The receive tick dispatches a
//! captured burst on `engine.ota_active()`. The monitor's only `MonitorFrame` emit lived inside the
//! **non-OTA** arm, so it was skipped entirely whenever OTA was running. And OTA is not a transient
//! state: `start_ota_session` runs once at daemon startup under `ota_enabled`, and nothing ever sets
//! `engine.ota` back to `None` — so the monitor was dark for the whole process lifetime under exactly
//! the configuration an on-air station uses.
//!
//! **Why the existing coverage could not catch it.** REQ-RX-01's acceptance test
//! (`cargo test -p openpulse-daemon monitor::`) calls `MonitorRuntime::decode_all` directly. The
//! monitor code was never broken — the *dispatch around it* was — so a test that calls the monitor
//! itself passes no matter which arm the daemon takes. This test drives the real `server::run` tick
//! and asserts on the event actually reaching a control client, which is the only place the bug lives.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use openpulse_config::OpenpulseConfig;
use openpulse_core::audio::{
    AudioBackend, AudioConfig, AudioInputStream, AudioOutputStream, DeviceInfo,
};
use openpulse_core::error::AudioError;
use openpulse_daemon::protocol::ControlEvent;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::TcpStream;

const MODE: &str = "BPSK250";

// ── A backend that replays one prepared burst, then silence ───────────────────

/// Feeds the daemon a prepared frame followed by silence, so the receive tick sees a carrier, then a
/// carrier drop, and flushes exactly one burst — the shape a real capture has.
#[derive(Clone)]
struct ReplayBackend {
    pending: Arc<Mutex<Vec<f32>>>,
}

impl ReplayBackend {
    fn new(frame: Vec<f32>) -> Self {
        Self {
            pending: Arc::new(Mutex::new(frame)),
        }
    }
}

struct ReplayStream {
    pending: Arc<Mutex<Vec<f32>>>,
}

impl AudioInputStream for ReplayStream {
    fn read(&mut self) -> Result<Vec<f32>, AudioError> {
        let mut g = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        if g.is_empty() {
            // Carrier dropped: silence flushes the accumulated burst.
            Ok(vec![0.0; 800])
        } else {
            let take = g.len().min(4096);
            Ok(g.drain(..take).collect())
        }
    }
    fn close(self: Box<Self>) {}
}

struct NullOut;
impl AudioOutputStream for NullOut {
    fn write(&mut self, _samples: &[f32]) -> Result<(), AudioError> {
        Ok(())
    }
    fn flush(&mut self) -> Result<(), AudioError> {
        Ok(())
    }
    fn close(self: Box<Self>) {}
}

impl AudioBackend for ReplayBackend {
    fn name(&self) -> &str {
        "Replay"
    }
    fn list_devices(&self) -> Result<Vec<DeviceInfo>, AudioError> {
        Ok(vec![])
    }
    fn open_input(
        &self,
        _d: Option<&str>,
        _c: &AudioConfig,
    ) -> Result<Box<dyn AudioInputStream>, AudioError> {
        Ok(Box::new(ReplayStream {
            pending: Arc::clone(&self.pending),
        }))
    }
    fn open_output(
        &self,
        _d: Option<&str>,
        _c: &AudioConfig,
    ) -> Result<Box<dyn AudioOutputStream>, AudioError> {
        Ok(Box::new(NullOut))
    }
}

/// Modulate one frame in `MODE` using a throwaway engine, so the replayed audio is a real decodable
/// burst rather than synthetic noise.
fn one_frame() -> Vec<f32> {
    let lb = openpulse_audio::LoopbackBackend::new();
    let mut e = openpulse_modem::ModemEngine::new(Box::new(lb.clone_shared()));
    e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .unwrap();
    e.transmit(b"monitor me", MODE, None).expect("transmit");
    let mut samples = lb.drain_samples();
    assert!(!samples.is_empty(), "fixture frame is empty");
    // A little lead-in silence so the energy gate sees a rising edge rather than starting mid-carrier.
    let mut out = vec![0.0f32; 1600];
    out.append(&mut samples);
    out
}

fn cfg(tcp_port: u16, ws_port: u16) -> OpenpulseConfig {
    let mut c = OpenpulseConfig::default();
    c.station.callsign = "TESTER".into();
    c.modem.mode = MODE.into();
    c.daemon.tcp_port = tcp_port;
    c.daemon.websocket_port = ws_port;
    // The configuration under test: OTA active AND the monitor enabled. Either alone is fine; it is
    // the INTERSECTION that used to go dark.
    c.modem.ota_enabled = true;
    c.monitor.enabled = true;
    c.monitor.modes = vec![MODE.into()];
    c
}

fn spawn_daemon(cfg: OpenpulseConfig, backend: ReplayBackend) {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("daemon runtime");
        rt.block_on(async move {
            let _ = openpulse_daemon::server::run(cfg, Box::new(backend)).await;
        });
    });
}

/// THE GATE: a MonitorFrame must reach a control client while the OTA session is active.
///
/// Before the fix this timed out — the monitor's only emit site sat in the arm the dispatch never
/// took once `start_ota_session` had run at startup.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_monitor_still_emits_while_an_ota_session_is_active() {
    spawn_daemon(cfg(19160, 19161), ReplayBackend::new(one_frame()));
    tokio::time::sleep(Duration::from_millis(400)).await;

    let stream = TcpStream::connect("127.0.0.1:19160")
        .await
        .expect("control port");
    let (r, _w) = stream.into_split();
    let mut reader = BufReader::new(r);

    let saw_monitor_frame = tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
                return false;
            }
            if let Ok(ControlEvent::MonitorFrame { mode, .. }) =
                serde_json::from_str::<ControlEvent>(line.trim())
            {
                assert_eq!(mode, MODE, "MonitorFrame carried an unexpected mode");
                return true;
            }
        }
    })
    .await
    .unwrap_or(false);

    assert!(
        saw_monitor_frame,
        "no MonitorFrame arrived within 20 s while an OTA session was active. The monitor is wired \
         into only one arm of the receive dispatch, and `ota_active()` is permanently true once \
         `start_ota_session` runs at daemon startup — so REQ-RX-01 is dark for the whole process \
         lifetime under the on-air configuration."
    );
}
