//! A station ID queued behind a data frame counts as sent only if the burst was written: when the
//! output cannot be opened, nothing goes out, so the ID is not logged and rides the next attempt
//! (review 2026-11-03 of the one-stream ID, S1). Before, the ID was marked sent when it was queued:
//! the §97.119 clock was reset and the line logged for an ID that never went out.

use std::f32::consts::TAU;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
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

/// Counts output streams; refuses the next open once `fail_next` is set.
#[derive(Clone, Default)]
struct Counting {
    opened: Arc<Mutex<Vec<Instant>>>,
    fail_next: Arc<AtomicBool>,
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
        if self.fail_next.swap(false, Ordering::SeqCst) {
            return Err(AudioError::DeviceNotFound(
                "refused once by the test".into(),
            ));
        }
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
async fn an_id_whose_burst_was_not_written_is_not_counted_and_rides_the_retry() {
    let log = Log::default();
    let sink = log.clone();
    tracing_subscriber::fmt()
        .with_env_filter("warn,openpulse_daemon=info")
        .with_writer(move || sink.clone())
        .with_ansi(false)
        .init();

    let backend = Counting::default();
    let fail_next = Arc::clone(&backend.fail_next);
    let mut c = OpenpulseConfig::default();
    c.station.callsign = "FAILWR".into();
    c.modem.mode = "BPSK250".into();
    c.modem.ota_enabled = true;
    c.modem.ota_profile = "robust".into();
    c.modem.ota_lock_level = "SL6".into();
    c.station.auto_id_interval_secs = 40;
    c.daemon.tcp_port = 19194;
    c.daemon.websocket_port = 19195;
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
    let (_r, mut w) = TcpStream::connect("127.0.0.1:19194")
        .await
        .expect("control port")
        .into_split();

    send(&mut w, "arm the id timer").await;
    let window_open = start + Duration::from_secs(21);
    tokio::time::sleep(window_open.saturating_duration_since(Instant::now())).await;

    // Message 2's first burst (data + ID) cannot open its output: nothing goes out.
    fail_next.store(true, Ordering::SeqCst);
    send(&mut w, "carries the id").await;
    let text = || String::from_utf8_lossy(&log.0.lock().unwrap()).to_string();
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline && !text().contains("kind=\"with-data\"") {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let t = text();
    let failed_at = t.find("OTA data transmit failed");
    let id_at = t.find("kind=\"with-data\"");
    assert!(
        failed_at.is_some(),
        "the refused output did not fail the attempt: the test did not reach its case"
    );
    assert!(
        id_at.is_some(),
        "the ID was lost: it must ride the retry after a burst that was not written"
    );
    assert!(
        id_at > failed_at,
        "the ID was logged as sent before the failed write: a queued ID was counted as sent"
    );
    assert_eq!(
        t.matches("kind=\"with-data\"").count(),
        1,
        "the ID must be logged once, for the burst that went out"
    );
}
