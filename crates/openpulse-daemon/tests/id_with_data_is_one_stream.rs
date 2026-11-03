//! The sender's station ID behind its data frame goes out in the SAME audio stream as the frame.
//!
//! On air (task 20) the data frame and the ID were two output streams inside one PTT hold. The audio
//! gap between them (0.12 s) dropped the SSB carrier, the receiver took it as the frame's end, decoded,
//! and keyed its ACK over the ID while the sender could not hear: 5 of 6 such frames lost their ACK.
//!
//! This drives the real `server::run` with a backend that counts output streams. The input carries a
//! steady carrier, so no quiet-channel ID can take the slot. Message 1 arms the ID timer; message 2,
//! sent once the ride-along window is open, carries the ID. Its keying must open ONE output stream.

use std::f32::consts::TAU;
use std::io::Write;
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

/// Counts output streams, with the time each opened.
#[derive(Clone, Default)]
struct Counting {
    opened: Arc<Mutex<Vec<Instant>>>,
}

struct Carrier {
    phase: f32,
}

impl AudioInputStream for Carrier {
    fn read(&mut self) -> Result<Vec<f32>, AudioError> {
        std::thread::sleep(Duration::from_millis(10));
        Ok((0..80)
            .map(|_| {
                self.phase = (self.phase + TAU * 1000.0 / 8000.0) % TAU;
                0.3 * self.phase.sin()
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

impl AudioBackend for Counting {
    fn name(&self) -> &str {
        "counting"
    }
    fn list_devices(&self) -> Result<Vec<DeviceInfo>, AudioError> {
        Ok(Vec::new())
    }
    fn open_input(
        &self,
        _d: Option<&str>,
        _c: &AudioConfig,
    ) -> Result<Box<dyn AudioInputStream>, AudioError> {
        Ok(Box::new(Carrier { phase: 0.0 }))
    }
    fn open_output(
        &self,
        _d: Option<&str>,
        _c: &AudioConfig,
    ) -> Result<Box<dyn AudioOutputStream>, AudioError> {
        self.opened.lock().unwrap().push(Instant::now());
        Ok(Box::new(Sink))
    }
}

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

async fn send(w: &mut tokio::net::tcp::OwnedWriteHalf, body: &str) {
    let line = serde_json::to_string(&ControlCommand::SendMessage {
        to: "PEER".into(),
        subject: "s".into(),
        body: body.into(),
    })
    .unwrap()
        + "\n";
    w.write_all(line.as_bytes()).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_id_behind_the_data_frame_shares_its_audio_stream() {
    let log = Log::default();
    let sink = log.clone();
    tracing_subscriber::fmt()
        .with_env_filter("warn,openpulse_daemon=info")
        .with_writer(move || sink.clone())
        .with_ansi(false)
        .init();

    let backend = Counting::default();
    let opened = Arc::clone(&backend.opened);
    let mut c = OpenpulseConfig::default();
    c.station.callsign = "ONESTR".into();
    c.modem.mode = "BPSK250".into();
    c.modem.ota_enabled = true;
    c.modem.ota_profile = "robust".into();
    c.modem.ota_lock_level = "SL6".into();
    // A 40 s interval: the ride-along window (half of it) opens 20 s after start.
    c.station.auto_id_interval_secs = 40;
    c.daemon.tcp_port = 19192;
    c.daemon.websocket_port = 19193;
    let start = Instant::now();
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
    tokio::time::sleep(Duration::from_millis(500)).await;
    let (_r, mut w) = TcpStream::connect("127.0.0.1:19192")
        .await
        .expect("control port")
        .into_split();

    // Message 1 arms the ID timer (a transmission since the last ID); no peer answers, so it ends
    // after two silent 9 s ACK windows.
    send(&mut w, "arm the id timer").await;
    let window_open = start + Duration::from_secs(21);
    tokio::time::sleep(window_open.saturating_duration_since(Instant::now())).await;

    // Message 2: the ride-along window is open, so its first keying carries the ID.
    let before = opened.lock().unwrap().len();
    let sent_at = Instant::now();
    send(&mut w, "carries the id").await;
    let seen = |needle: &str| String::from_utf8_lossy(&log.0.lock().unwrap()).contains(needle);
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline && !seen("kind=\"with-data\"") {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        seen("kind=\"with-data\""),
        "no station ID rode behind message 2's data frame: the test did not reach its case"
    );
    // The first keying of message 2: the streams opened within 4 s of the send (its ACK window, 9 s,
    // separates it from any retransmission).
    let in_keying = opened.lock().unwrap()[before..]
        .iter()
        .filter(|t| t.duration_since(sent_at) < Duration::from_secs(4))
        .count();
    assert_eq!(
        in_keying, 1,
        "the data frame and its station ID opened {in_keying} output streams in one keying: the gap \
         between them drops the carrier and the receiver's ACK lands on the ID (on-air task 20). \
         See ModemEngine::transmit_burst"
    );
}
