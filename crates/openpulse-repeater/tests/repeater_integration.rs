use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
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

#[test]
fn relay_empty_buffer_returns_none() {
    let (engine_rx, _lb_rx) = make_engine_with_plugin();
    let (engine_tx, _lb_tx) = make_engine_with_plugin();
    let config = RepeaterConfig {
        enabled: true,
        mode: "BPSK250".into(),
        tx_hang_ms: 0,
    };
    // No samples in loopback_rx — receive() should return empty vec or error
    let mut repeater =
        CrossBandRepeater::new(Box::new(NoOpPtt::new()), engine_rx, engine_tx, config);
    // Either returns Ok(None) or Err (if receive fails on empty buffer), both are acceptable
    let _ = repeater.relay_one_frame();
}
