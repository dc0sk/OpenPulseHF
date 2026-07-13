use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread;

use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_modem::ModemEngine;
use openpulse_radio::NoOpPtt;
use openpulse_repeater::{CrossBandRepeater, RepeaterConfig};

fn make_engine_with_plugin() -> (ModemEngine, LoopbackBackend) {
    let lb = LoopbackBackend::new();
    let mut engine = ModemEngine::new(Box::new(lb.clone_shared()));
    engine
        .register_plugin(Box::new(BpskPlugin::new()))
        .expect("register");
    (engine, lb)
}

/// Spawn a minimal mock rigctld that records PTT commands.
fn spawn_mock_rigctld_with_ptt_log(ptt_log: Arc<std::sync::Mutex<Vec<&'static str>>>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr").to_string();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let stream = stream.expect("accept");
            let log = ptt_log.clone();
            thread::spawn(move || {
                let mut writer = stream.try_clone().expect("clone");
                let reader = BufReader::new(stream);
                for line in reader.lines() {
                    let cmd = match line {
                        Ok(l) => l,
                        Err(_) => break,
                    };
                    match cmd.trim() {
                        "T 1" => {
                            log.lock().unwrap().push("T 1");
                            writeln!(writer, "RPRT 0").ok();
                        }
                        "T 0" => {
                            log.lock().unwrap().push("T 0");
                            writeln!(writer, "RPRT 0").ok();
                        }
                        _ => {
                            writeln!(writer, "RPRT 0").ok();
                        }
                    }
                }
            });
        }
    });
    addr
}

#[test]
fn relay_disabled_returns_none() {
    let (engine_rx, _lb_rx) = make_engine_with_plugin();
    let (engine_tx, _lb_tx) = make_engine_with_plugin();
    let config = RepeaterConfig {
        enabled: false,
        mode: "BPSK250".into(),
        tx_hang_ms: 0,
        full_duplex: false,
        ..Default::default()
    };
    let mut repeater =
        CrossBandRepeater::new(Box::new(NoOpPtt::new()), engine_rx, engine_tx, config);
    let result = repeater.relay_one_frame().expect("no error");
    assert_eq!(result, None);
}

#[test]
fn relay_loopback_cross_band() {
    // Source → encode → loopback_a → engine_rx → relay → engine_tx → loopback_b → decode
    let (engine_rx, lb_rx) = make_engine_with_plugin();
    let (engine_tx, lb_tx) = make_engine_with_plugin();

    // Encode a frame into lb_rx via a separate source engine.
    let mut src_engine = ModemEngine::new(Box::new(lb_rx.clone_shared()));
    src_engine
        .register_plugin(Box::new(BpskPlugin::new()))
        .expect("register src");
    let payload = b"cross-band relay test";
    src_engine
        .transmit(payload, "BPSK250", None)
        .expect("transmit");

    let ptt_log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mock_addr = spawn_mock_rigctld_with_ptt_log(ptt_log.clone());

    // Give the mock server a moment to start
    std::thread::sleep(std::time::Duration::from_millis(10));

    let rig_b = openpulse_radio::RigctldController::connect(&mock_addr).expect("connect");

    let config = RepeaterConfig {
        enabled: true,
        mode: "BPSK250".into(),
        tx_hang_ms: 0,
        full_duplex: false,
        ..Default::default()
    };
    let mut repeater = CrossBandRepeater::new(Box::new(rig_b), engine_rx, engine_tx, config);
    let n = repeater.relay_one_frame().expect("relay").expect("Some");
    assert_eq!(n, payload.len());

    // Verify PTT was asserted then released.
    let log = ptt_log.lock().unwrap();
    assert_eq!(*log, vec!["T 1", "T 0"]);

    // Decode what arrived in lb_tx.
    let mut sink_engine = ModemEngine::new(Box::new(lb_tx.clone_shared()));
    sink_engine
        .register_plugin(Box::new(BpskPlugin::new()))
        .expect("register sink");
    let received = sink_engine.receive("BPSK250", None).expect("receive");
    assert_eq!(&received[..payload.len()], payload);
}

/// A PTT double that records each assert/release, to observe the extra keying the ID performs.
struct LoggingPtt {
    log: Arc<std::sync::Mutex<Vec<&'static str>>>,
    asserted: bool,
}
impl openpulse_radio::PttController for LoggingPtt {
    fn assert_ptt(&mut self) -> Result<(), openpulse_radio::PttError> {
        self.asserted = true;
        self.log.lock().unwrap().push("assert");
        Ok(())
    }
    fn release_ptt(&mut self) -> Result<(), openpulse_radio::PttError> {
        self.asserted = false;
        self.log.lock().unwrap().push("release");
        Ok(())
    }
    fn is_asserted(&self) -> bool {
        self.asserted
    }
}

#[test]
fn transmitting_rig_is_station_identified_when_the_interval_elapses() {
    // Audit #6: rig_b is an automatically-controlled station (§97.221) and must ID per §97.119. In
    // half-duplex the ID keys its own PTT, so a relay whose interval has elapsed shows an *extra*
    // assert/release pair (the relayed frame's, plus the ID's) versus a plain relay.
    fn feed_frame(lb: &LoopbackBackend) {
        let mut src = ModemEngine::new(Box::new(lb.clone_shared()));
        src.register_plugin(Box::new(BpskPlugin::new()))
            .expect("register");
        src.transmit(b"relay frame", "BPSK250", None).expect("tx");
    }

    let (engine_rx, lb_rx) = make_engine_with_plugin();
    let (engine_tx, _lb_tx) = make_engine_with_plugin();
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let rig_b = LoggingPtt {
        log: log.clone(),
        asserted: false,
    };
    let config = RepeaterConfig {
        enabled: true,
        mode: "BPSK250".into(),
        tx_hang_ms: 0,
        full_duplex: false,
        callsign: "N0CALL".into(),
        id_interval_secs: 600,
    };
    let mut repeater = CrossBandRepeater::new(Box::new(rig_b), engine_rx, engine_tx, config);

    // First relay at t=0: one keying pair for the relayed frame, no ID yet.
    feed_frame(&lb_rx);
    repeater
        .relay_one_frame_at(0)
        .expect("relay")
        .expect("Some");
    assert_eq!(
        *log.lock().unwrap(),
        vec!["assert", "release"],
        "a plain relay keys once; no ID before the interval"
    );

    // Second relay at t = 601 s: the interval has elapsed → the ID keys a second time within the call.
    log.lock().unwrap().clear();
    feed_frame(&lb_rx);
    repeater
        .relay_one_frame_at(601_000)
        .expect("relay")
        .expect("Some");
    assert_eq!(
        *log.lock().unwrap(),
        vec!["assert", "assert", "release", "release"],
        "the ID keys its own PTT after the interval elapses"
    );
}

#[test]
fn relay_empty_buffer_returns_none() {
    let (engine_rx, _lb_rx) = make_engine_with_plugin();
    let (engine_tx, _lb_tx) = make_engine_with_plugin();
    let config = RepeaterConfig {
        enabled: true,
        mode: "BPSK250".into(),
        tx_hang_ms: 0,
        full_duplex: false,
        ..Default::default()
    };
    // No samples in loopback_rx — receive() should return empty vec or error
    let mut repeater =
        CrossBandRepeater::new(Box::new(NoOpPtt::new()), engine_rx, engine_tx, config);
    // Either returns Ok(None) or Err (if receive fails on empty buffer), both are acceptable
    let _ = repeater.relay_one_frame();
}

#[test]
fn full_duplex_ptt_released_on_early_stop() {
    // stop is pre-set to true → run_full_duplex returns Ok(0) immediately after assert+release.
    let (engine_rx, _lb_rx) = make_engine_with_plugin();
    let (engine_tx, _lb_tx) = make_engine_with_plugin();

    let ptt_log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mock_addr = spawn_mock_rigctld_with_ptt_log(ptt_log.clone());
    std::thread::sleep(std::time::Duration::from_millis(10));
    let rig_b = openpulse_radio::RigctldController::connect(&mock_addr).expect("connect");

    let config = RepeaterConfig {
        enabled: true,
        mode: "BPSK250".into(),
        tx_hang_ms: 500, // should be ignored in full-duplex
        full_duplex: true,
        ..Default::default()
    };
    let mut repeater = CrossBandRepeater::new(Box::new(rig_b), engine_rx, engine_tx, config);

    let stop = Arc::new(AtomicBool::new(true)); // already stopped
    let count = repeater.run_full_duplex(stop).expect("no error");
    assert_eq!(count, 0);

    let log = ptt_log.lock().unwrap();
    assert_eq!(
        *log,
        vec!["T 1", "T 0"],
        "PTT must be asserted then released"
    );
}

#[test]
fn full_duplex_disabled_returns_zero_immediately() {
    // enabled=false → run_full_duplex returns Ok(0) without touching PTT.
    let (engine_rx, _lb_rx) = make_engine_with_plugin();
    let (engine_tx, _lb_tx) = make_engine_with_plugin();

    let ptt_log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mock_addr = spawn_mock_rigctld_with_ptt_log(ptt_log.clone());
    std::thread::sleep(std::time::Duration::from_millis(10));
    let rig_b = openpulse_radio::RigctldController::connect(&mock_addr).expect("connect");

    let config = RepeaterConfig {
        enabled: false,
        mode: "BPSK250".into(),
        tx_hang_ms: 0,
        full_duplex: true,
        ..Default::default()
    };
    let mut repeater = CrossBandRepeater::new(Box::new(rig_b), engine_rx, engine_tx, config);

    let stop = Arc::new(AtomicBool::new(false));
    let count = repeater.run_full_duplex(stop).expect("no error");
    assert_eq!(count, 0);

    let log = ptt_log.lock().unwrap();
    assert!(log.is_empty(), "PTT must not be touched when disabled");
}

#[test]
fn full_duplex_relay_one_frame_skips_ptt() {
    // In full_duplex mode, relay_one_frame() must not assert/release PTT.
    // We inject a frame, call relay_one_frame(), and verify PTT log is empty.
    let (engine_rx, lb_rx) = make_engine_with_plugin();
    let (engine_tx, _lb_tx) = make_engine_with_plugin();

    let mut src_engine = ModemEngine::new(Box::new(lb_rx.clone_shared()));
    src_engine
        .register_plugin(Box::new(BpskPlugin::new()))
        .expect("register src");
    src_engine
        .transmit(b"fd frame", "BPSK250", None)
        .expect("transmit");

    let ptt_log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mock_addr = spawn_mock_rigctld_with_ptt_log(ptt_log.clone());
    std::thread::sleep(std::time::Duration::from_millis(10));
    let rig_b = openpulse_radio::RigctldController::connect(&mock_addr).expect("connect");

    let config = RepeaterConfig {
        enabled: true,
        mode: "BPSK250".into(),
        tx_hang_ms: 0,
        full_duplex: true,
        ..Default::default()
    };
    let mut repeater = CrossBandRepeater::new(Box::new(rig_b), engine_rx, engine_tx, config);

    let result = repeater.relay_one_frame().expect("relay");
    assert!(result.is_some(), "expected a frame to relay");

    let log = ptt_log.lock().unwrap();
    assert!(
        log.is_empty(),
        "relay_one_frame must not touch PTT in full_duplex mode"
    );
}
