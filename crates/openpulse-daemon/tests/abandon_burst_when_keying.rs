//! The daemon drops the burst it was gathering when it keys, on its production path (`server::run`).
//!
//! On air (task 17, 2026-11-02) a burst open when the station keyed was flushed after the transmit,
//! glued to what the reopened stream captured; the piece failed and the station keyed a NACK over the
//! other station's frame. `abandon_burst` is unit-tested in `openpulse-modem`; this proves the
//! daemon's keying path calls it (CLAUDE.md, RX/TX seam rule 3): another station's carrier fills the
//! channel, this station sends a message, and the engine's tripwire log line must appear.
//!
//! **With an OFDM candidate, and its own echo** (gate 12 of `docs/dev/design/ofdm-preamble-detect.md`).
//! The station runs an OTA session at SL7, so the preamble detect P runs, and every frame it sends
//! comes back in its own capture, as on air (rx-a-f1d holds A's own OFDM frames at ρ 0.907–0.942, 98
//! times). P stays off them only because the daemon drops its capture stream after it keys (#1007,
//! task 17), so a regression in that rule shows up here as a P open.

use std::f32::consts::TAU;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use openpulse_config::OpenpulseConfig;
use openpulse_core::audio::{
    AudioBackend, AudioConfig, AudioInputStream, AudioOutputStream, DeviceInfo,
};
use openpulse_core::error::AudioError;
use openpulse_daemon::protocol::ControlCommand;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

/// What one open capture stream has heard of this station's own transmissions, not yet read.
type Echo = Arc<Mutex<Vec<f32>>>;

/// The live capture streams, as weak handles: a dropped stream's unread echo goes with it.
type Inputs = Arc<Mutex<Vec<Weak<Mutex<Vec<f32>>>>>>;

#[derive(Clone, Default)]
struct Channel {
    /// Whether another station is transmitting.
    carrier: Arc<AtomicBool>,
    /// Transmits keyed by the daemon (one output stream per transmit).
    transmits: Arc<AtomicUsize>,
    /// Every capture stream opened; the live ones hear the station's own transmissions, as a sound
    /// card's capture buffer does.
    inputs: Inputs,
}

struct Input {
    carrier: Arc<AtomicBool>,
    phase: f32,
    echo: Echo,
}

impl AudioInputStream for Input {
    fn read(&mut self) -> Result<Vec<f32>, AudioError> {
        std::thread::sleep(Duration::from_millis(10));
        let on = self.carrier.load(Ordering::SeqCst);
        let mut out: Vec<f32> = (0..80)
            .map(|_| {
                self.phase = (self.phase + TAU * 1500.0 / 8000.0) % TAU;
                if on {
                    0.3 * self.phase.sin()
                } else {
                    0.0
                }
            })
            .collect();
        let mut echo = self.echo.lock().expect("echo");
        if !echo.is_empty() {
            let n = echo.len().min(4000);
            out.extend(echo.drain(..n));
        }
        Ok(out)
    }
    fn close(self: Box<Self>) {}
}

/// The station's own output, heard by every live capture stream.
struct Sink {
    inputs: Inputs,
}

impl AudioOutputStream for Sink {
    fn write(&mut self, samples: &[f32]) -> Result<(), AudioError> {
        let mut inputs = self.inputs.lock().expect("inputs");
        inputs.retain(|w| w.strong_count() > 0);
        for w in inputs.iter() {
            if let Some(buf) = w.upgrade() {
                buf.lock().expect("echo").extend_from_slice(samples);
            }
        }
        Ok(())
    }
    fn flush(&mut self) -> Result<(), AudioError> {
        Ok(())
    }
    fn close(self: Box<Self>) {}
}

impl AudioBackend for Channel {
    fn name(&self) -> &str {
        "channel"
    }
    fn list_devices(&self) -> Result<Vec<DeviceInfo>, AudioError> {
        Ok(Vec::new())
    }
    fn open_input(
        &self,
        _d: Option<&str>,
        _c: &AudioConfig,
    ) -> Result<Box<dyn AudioInputStream>, AudioError> {
        let echo: Echo = Arc::default();
        self.inputs
            .lock()
            .expect("inputs")
            .push(Arc::downgrade(&echo));
        Ok(Box::new(Input {
            carrier: Arc::clone(&self.carrier),
            phase: 0.0,
            echo,
        }))
    }
    fn open_output(
        &self,
        _d: Option<&str>,
        _c: &AudioConfig,
    ) -> Result<Box<dyn AudioOutputStream>, AudioError> {
        self.transmits.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(Sink {
            inputs: Arc::clone(&self.inputs),
        }))
    }
}

/// The daemon's log, captured so the test can see the engine's tripwire line.
#[derive(Clone, Default)]
struct Log(Arc<Mutex<Vec<u8>>>);

impl Write for Log {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_daemon_drops_the_burst_it_was_gathering_when_it_keys() {
    let log = Log::default();
    let sink = log.clone();
    tracing_subscriber::fmt()
        .with_env_filter("openpulse_modem=debug,openpulse_daemon=info,ofdm_plugin=debug")
        .with_writer(move || sink.clone())
        .with_ansi(false)
        .init();

    let channel = Channel::default();
    let mut c = OpenpulseConfig::default();
    c.station.callsign = "TESTER".into();
    c.modem.mode = "OFDM52".into();
    c.daemon.tcp_port = 19174;
    c.daemon.websocket_port = 19175;
    // An OTA session held at SL7: its candidate is OFDM52, so the preamble detect runs.
    c.modem.ota_enabled = true;
    c.modem.ota_profile = "fast".into();
    c.modem.ota_min_level = "SL7".into();
    c.modem.ota_max_level = "SL7".into();
    // No station ID, so the only transmission is the message on the command arm.
    c.station.auto_id_interval_secs = 0;
    let backend = channel.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("daemon runtime");
        rt.block_on(async move {
            let _ = openpulse_daemon::server::run(c, Box::new(backend)).await;
        });
    });

    // A quiet second so the floor is warm, then another station transmits for 1.5 s before ours.
    tokio::time::sleep(Duration::from_millis(1000)).await;
    channel.carrier.store(true, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let stream = TcpStream::connect("127.0.0.1:19174")
        .await
        .expect("control port");
    let (_r, mut w) = stream.into_split();
    let mut line = serde_json::to_string(&ControlCommand::SendMessage {
        to: "PEER".into(),
        subject: "x".into(),
        body: "key while a burst is open".into(),
    })
    .unwrap();
    line.push('\n');
    w.write_all(line.as_bytes()).await.unwrap();

    let text = || String::from_utf8_lossy(&log.0.lock().unwrap()).to_string();
    let start = Instant::now();
    let mut abandoned = false;
    while start.elapsed() < Duration::from_secs(30) {
        if text().contains("burst abandoned: this station keyed") {
            abandoned = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    // The own echo is heard only if the stream the daemon keyed over is read again. Give the receive
    // loop time to read whatever it holds, and turn the carrier off so a burst flushes and P logs.
    channel.carrier.store(false, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_secs(4)).await;
    assert!(
        channel.transmits.load(Ordering::SeqCst) >= 1,
        "the message was never transmitted, so nothing was keyed"
    );
    assert!(
        abandoned,
        "the daemon keyed while gathering another station's burst and did not drop it: the burst \
         gathered before the transmit would be flushed after it (on-air task 17). Check the \
         `engine.abandon_burst()` calls where `server.rs` drops `rx_stream` after a transmit"
    );
    let text = text();
    assert!(
        !text.contains("OFDM carrier detect: preamble confirmed"),
        "the preamble detect opened on this station's own OFDM frame: the stream it keyed over was \
         read again (#1007, task 17)"
    );
    assert!(
        text.lines()
            .any(|l| l.contains("preamble carrier detect") && !l.contains(" blocks=0")),
        "the preamble detect never ran with the OFDM candidate, so the check above proves nothing"
    );
}
