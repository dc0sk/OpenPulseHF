//! The sign-off station ID waits until the channel is quiet; it is not keyed over another station.
//!
//! On air (task 17, 2026-11-02) the receiving station keyed its sign-off ID ten seconds after its own
//! ACK, in the middle of the sender's next 66.6 s frame, every ~17 s until that frame ended. The ID
//! timer measured only the station's own transmissions. Each ID cut the burst being gathered; the
//! piece failed, and the station keyed a NACK over the frame too.
//!
//! This drives the real `server::run`: a message transmit arms the timer (sign-off idle 1 s), the
//! channel carries a signal for 4 s, then goes quiet. No ID may be keyed while the signal is on, and
//! the ID must go out once it is off. A transmit is counted by the output stream it opens.

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
    /// When each transmit opened its output stream.
    keyed_at: Arc<Mutex<Vec<Instant>>>,
}

/// The daemon's log, each line stamped when it was written.
#[derive(Clone, Default)]
struct Log(Arc<Mutex<Vec<(Instant, String)>>>);

impl Write for Log {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let text = String::from_utf8_lossy(buf).to_string();
        let mut g = self.0.lock().unwrap();
        for line in text.lines() {
            g.push((Instant::now(), line.to_string()));
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
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
        self.keyed_at.lock().unwrap().push(Instant::now());
        self.transmits.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(Sink))
    }
}

async fn wait_for(cond: impl Fn() -> bool, limit: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < limit {
        if cond() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    cond()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_signoff_id_is_not_keyed_while_another_station_transmits() {
    let log = Log::default();
    let sink = log.clone();
    tracing_subscriber::fmt()
        .with_env_filter("openpulse_daemon=debug,openpulse_modem=debug")
        .with_writer(move || sink.clone())
        .with_ansi(false)
        .init();
    let channel = Channel::default();
    let mut c = OpenpulseConfig::default();
    c.station.callsign = "TESTER".into();
    c.modem.mode = "BPSK250".into();
    c.daemon.tcp_port = 19172;
    c.daemon.websocket_port = 19173;
    c.station.auto_id_interval_secs = 600;
    c.station.auto_id_signoff_idle_secs = 1;
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

    // A quiet second first, so the noise floor is warm before the other station starts.
    tokio::time::sleep(Duration::from_millis(1000)).await;
    let stream = TcpStream::connect("127.0.0.1:19172")
        .await
        .expect("control port");
    let (_r, mut w) = stream.into_split();

    // The other station starts transmitting; our own message then arms the ID timer.
    channel.carrier.store(true, Ordering::SeqCst);
    let mut line = serde_json::to_string(&ControlCommand::SendMessage {
        to: "PEER".into(),
        subject: "x".into(),
        body: "arm the id timer".into(),
    })
    .unwrap();
    line.push('\n');
    w.write_all(line.as_bytes()).await.unwrap();
    let t = Arc::clone(&channel.transmits);
    assert!(
        wait_for(|| t.load(Ordering::SeqCst) >= 1, Duration::from_secs(10)).await,
        "the message was never transmitted, so the ID timer was never armed"
    );
    let after_message = channel.transmits.load(Ordering::SeqCst);

    // Four seconds of the other station's signal: the sign-off (idle 1 s) falls due inside it.
    tokio::time::sleep(Duration::from_secs(4)).await;
    let during_carrier = channel.transmits.load(Ordering::SeqCst) - after_message;
    channel.carrier.store(false, Ordering::SeqCst);
    let quiet_from = Instant::now();

    let t = Arc::clone(&channel.transmits);
    let id_after_quiet = wait_for(
        || t.load(Ordering::SeqCst) > after_message,
        // The ID needs `ID_MIN_QUIET_MS` (3 s) of quiet after the burst has been flushed and decoded.
        Duration::from_secs(15),
    )
    .await;
    let _ = quiet_from;

    assert_eq!(
        during_carrier, 0,
        "the station keyed {during_carrier} transmission(s) while another station was transmitting: \
         the sign-off ID went out over its frame (on-air task 17). See `StationIdTimer::due_reason`"
    );
    assert!(
        id_after_quiet,
        "no station ID once the channel went quiet: deferring it must not lose it (§97.119)"
    );
    // The channel is quiet from the first capture read after the carrier's burst was flushed and
    // decoded (the receive loop reads nothing while it decodes); the ID must wait ID_MIN_QUIET_MS
    // from there, or it can land in the turnaround before the other station's next frame (task 18).
    let keyed_id = *channel.keyed_at.lock().unwrap().last().unwrap();
    let lines = log.0.lock().unwrap().clone();
    let flushed = lines
        .iter()
        .rposition(|(t, l)| *t < keyed_id && l.contains("burst flushed"))
        .expect("the carrier's burst was flushed");
    let quiet_from = lines[flushed..]
        .iter()
        .find(|(_, l)| l.contains("capture read"))
        .map(|(t, _)| *t)
        .expect("a capture read after the flush");
    let waited = keyed_id.saturating_duration_since(quiet_from);
    assert!(
        waited >= Duration::from_millis(openpulse_core::station_id::ID_MIN_QUIET_MS),
        "the ID was keyed {waited:?} after the channel went quiet; it must wait ID_MIN_QUIET_MS"
    );
}
