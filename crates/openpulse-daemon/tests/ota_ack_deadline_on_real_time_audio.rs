//! On audio that arrives at the real rate, the daemon sets an ACK deadline on every OTA decode
//! (on-air task 18, run a2a: an ACK keyed past the sender's window landed on its retransmission).
//! `no_ack_past_the_deadline` (openpulse-modem) tests what the engine does with it; this proves the
//! daemon's production path (`server::run`) sets it, and that an in-time decode is still answered.

use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use openpulse_config::OpenpulseConfig;
use openpulse_core::audio::{
    AudioBackend, AudioConfig, AudioInputStream, AudioOutputStream, DeviceInfo,
};
use openpulse_core::error::AudioError;
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;

/// Serves a frame at the audio rate (80 samples per 10 ms read), then quiet; counts transmits.
#[derive(Clone)]
struct RealTime {
    frame: Arc<Vec<f32>>,
    transmits: Arc<AtomicUsize>,
}

struct Input {
    frame: Arc<Vec<f32>>,
    pos: usize,
}

impl AudioInputStream for Input {
    fn read(&mut self) -> Result<Vec<f32>, AudioError> {
        std::thread::sleep(Duration::from_millis(10));
        let end = (self.pos + 80).min(self.frame.len());
        let out = if self.pos < end {
            self.frame[self.pos..end].to_vec()
        } else {
            vec![0.0; 80]
        };
        self.pos = end;
        Ok(out)
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

impl AudioBackend for RealTime {
    fn name(&self) -> &str {
        "real-time"
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
            frame: Arc::clone(&self.frame),
            pos: 0,
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

/// One SL6 frame from a sender pinned there, with a second of quiet in front.
fn sl6_frame() -> Vec<f32> {
    let bk = openpulse_audio::LoopbackBackend::new();
    let mut tx = openpulse_modem::ModemEngine::new(Box::new(bk.clone_shared()));
    tx.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .unwrap();
    tx.register_plugin(Box::new(qpsk_plugin::QpskPlugin::new()))
        .unwrap();
    tx.start_ota_session(SessionProfile::robust());
    tx.ota_lock_level(SpeedLevel::Sl6);
    let mode = tx.ota_tx_mode().unwrap().to_owned();
    let fec = tx.ota_tx_fec();
    tx.transmit_with_fec_mode(b"deadline frame", &mode, fec, None)
        .unwrap();
    let mut audio = vec![0.0f32; 8_000];
    audio.extend(bk.drain_samples());
    audio
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_daemon_sets_an_ack_deadline_on_real_time_audio() {
    let log = Log::default();
    let sink = log.clone();
    tracing_subscriber::fmt()
        .with_env_filter("warn,openpulse_modem::ota_deadline=debug")
        .with_writer(move || sink.clone())
        .with_ansi(false)
        .init();

    let backend = RealTime {
        frame: Arc::new(sl6_frame()),
        transmits: Arc::new(AtomicUsize::new(0)),
    };
    let transmits = Arc::clone(&backend.transmits);
    let mut c = OpenpulseConfig::default();
    c.station.callsign = "DEADLN".into();
    c.modem.mode = "BPSK250".into();
    c.modem.ota_enabled = true;
    c.modem.ota_profile = "robust".into();
    c.modem.ota_lock_level = "SL6".into();
    c.station.auto_id_interval_secs = 0;
    c.daemon.tcp_port = 19180;
    c.daemon.websocket_port = 19181;
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

    let start = Instant::now();
    let seen = |needle: &str| String::from_utf8_lossy(&log.0.lock().unwrap()).contains(needle);
    while start.elapsed() < Duration::from_secs(30)
        && !(seen("OTA decode: ACK deadline set") && transmits.load(Ordering::SeqCst) > 0)
    {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        seen("OTA decode: ACK deadline set"),
        "the daemon decoded an OTA burst of real-time audio without an ACK deadline \
         (server.rs, set_ota_ack_deadline)"
    );
    assert!(
        transmits.load(Ordering::SeqCst) > 0,
        "the in-time decode was not answered: no ACK keyed"
    );
}
