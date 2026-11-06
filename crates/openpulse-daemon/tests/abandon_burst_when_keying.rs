//! The daemon drops the burst it was gathering when it keys, on its production path (`server::run`).
//!
//! On air (task 17, 2026-11-02) a burst open when the station keyed was flushed after the transmit,
//! glued to what the reopened stream captured; the piece failed and the station keyed a NACK over the
//! other station's frame. `abandon_burst` is unit-tested in `openpulse-modem`; this proves the
//! daemon's keying path calls it (CLAUDE.md, RX/TX seam rule 3): another station's carrier fills the
//! channel, this station sends a message, and the engine's tripwire log line must appear.

use std::f32::consts::TAU;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use openpulse_config::OpenpulseConfig;
use openpulse_core::audio::{
    AudioBackend, AudioConfig, AudioInputStream, AudioOutputStream, DeviceInfo,
};
use openpulse_core::error::AudioError;
use openpulse_daemon::protocol::ControlCommand;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

#[derive(Clone, Default)]
struct Channel {
    /// Whether another station is transmitting.
    carrier: Arc<AtomicBool>,
    /// Transmits keyed by the daemon (one output stream per transmit).
    transmits: Arc<AtomicUsize>,
}

struct Input {
    carrier: Arc<AtomicBool>,
    phase: f32,
}

impl AudioInputStream for Input {
    fn read(&mut self) -> Result<Vec<f32>, AudioError> {
        std::thread::sleep(Duration::from_millis(10));
        let on = self.carrier.load(Ordering::SeqCst);
        Ok((0..80)
            .map(|_| {
                self.phase = (self.phase + TAU * 1500.0 / 8000.0) % TAU;
                if on {
                    0.3 * self.phase.sin()
                } else {
                    0.0
                }
            })
            .collect())
    }
    fn close(self: Box<Self>) {}
}

struct Sink;
impl AudioOutputStream for Sink {
    fn write(&mut self, _samples: &[f32]) -> Result<(), AudioError> {
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
        Ok(Box::new(Input {
            carrier: Arc::clone(&self.carrier),
            phase: 0.0,
        }))
    }
    fn open_output(
        &self,
        _d: Option<&str>,
        _c: &AudioConfig,
    ) -> Result<Box<dyn AudioOutputStream>, AudioError> {
        self.transmits.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(Sink))
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
        .with_env_filter("openpulse_modem=debug")
        .with_writer(move || sink.clone())
        .with_ansi(false)
        .init();

    let channel = Channel::default();
    let mut c = OpenpulseConfig::default();
    c.station.callsign = "TESTER".into();
    c.modem.mode = "BPSK250".into();
    c.daemon.tcp_port = 19174;
    c.daemon.websocket_port = 19175;
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

    let start = Instant::now();
    let mut abandoned = false;
    while start.elapsed() < Duration::from_secs(10) {
        let text = String::from_utf8_lossy(&log.0.lock().unwrap()).to_string();
        if text.contains("burst abandoned: this station keyed") {
            abandoned = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    channel.carrier.store(false, Ordering::SeqCst);
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
}
