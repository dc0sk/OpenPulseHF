use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;

use openpulse_radio::{PttController, RigMode, RigctldController};

fn spawn_mock_rigctld() -> (String, Arc<AtomicU64>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock rigctld");
    let addr = listener.local_addr().expect("local addr").to_string();
    let freq_store = Arc::new(AtomicU64::new(14_074_000));
    let freq_clone = freq_store.clone();

    thread::spawn(move || {
        for stream in listener.incoming() {
            let stream = stream.expect("accept");
            let freq_ref = freq_clone.clone();
            thread::spawn(move || handle_mock_client(stream, freq_ref));
        }
    });

    (addr, freq_store)
}

fn handle_mock_client(stream: TcpStream, freq_store: Arc<AtomicU64>) {
    let mut writer = stream.try_clone().expect("clone");
    let reader = BufReader::new(stream);
    let mut mode = "USB".to_string();

    for line in reader.lines() {
        let cmd = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let cmd = cmd.trim();

        match cmd {
            "T 1" | "T 0" => {
                writeln!(writer, "RPRT 0").ok();
            }
            "\\get_freq" => {
                let hz = freq_store.load(Ordering::SeqCst);
                writeln!(writer, "Frequency: {hz}.000000").ok();
                writeln!(writer, "RPRT 0").ok();
            }
            s if s.starts_with("\\set_freq ") => {
                let hz_str = s.trim_start_matches("\\set_freq ").trim();
                if let Ok(hz) = hz_str.parse::<u64>() {
                    freq_store.store(hz, Ordering::SeqCst);
                    writeln!(writer, "RPRT 0").ok();
                } else {
                    writeln!(writer, "RPRT -1").ok();
                }
            }
            "\\get_mode" => {
                writeln!(writer, "Mode: {mode}").ok();
                writeln!(writer, "Passband: 2400").ok();
                writeln!(writer, "RPRT 0").ok();
            }
            s if s.starts_with("\\set_mode ") => {
                let parts: Vec<&str> = s.splitn(3, ' ').collect();
                if parts.len() >= 2 {
                    mode = parts[1].to_string();
                }
                writeln!(writer, "RPRT 0").ok();
            }
            "\\get_level STRENGTH" => {
                writeln!(writer, "Level: -87.000000").ok();
                writeln!(writer, "RPRT 0").ok();
            }
            "\\get_level RFPOWER_METER_WATTS" => {
                writeln!(writer, "Level: 50.0").ok();
                writeln!(writer, "RPRT 0").ok();
            }
            "\\get_level ALC" => {
                writeln!(writer, "Level: 0.12").ok();
                writeln!(writer, "RPRT 0").ok();
            }
            "\\get_level SWR" => {
                writeln!(writer, "Level: 1.4").ok();
                writeln!(writer, "RPRT 0").ok();
            }
            _ => {
                writeln!(writer, "RPRT -1").ok();
            }
        }
    }
}

#[test]
fn frequency_round_trip() {
    let (addr, _store) = spawn_mock_rigctld();
    let mut ctl = RigctldController::connect(&addr).expect("connect");
    ctl.set_frequency(14_200_000).expect("set_freq");
    let hz = ctl.get_frequency().expect("get_freq");
    assert_eq!(hz, 14_200_000);
}

#[test]
fn mode_round_trip() {
    let (addr, _store) = spawn_mock_rigctld();
    let mut ctl = RigctldController::connect(&addr).expect("connect");
    ctl.set_mode(&RigMode::Fm).expect("set_mode");
    let mode = ctl.get_mode().expect("get_mode");
    assert_eq!(mode, RigMode::Fm);
}

#[test]
fn initial_frequency_readback() {
    let (addr, _store) = spawn_mock_rigctld();
    let mut ctl = RigctldController::connect(&addr).expect("connect");
    let hz = ctl.get_frequency().expect("get_freq");
    assert_eq!(hz, 14_074_000);
}

#[test]
fn signal_strength_parse() {
    let (addr, _store) = spawn_mock_rigctld();
    let mut ctl = RigctldController::connect(&addr).expect("connect");
    let dbm = ctl.get_signal_strength().expect("get_signal_strength");
    assert_eq!(dbm, -87);
}

#[test]
fn power_out_parse() {
    let (addr, _store) = spawn_mock_rigctld();
    let mut ctl = RigctldController::connect(&addr).expect("connect");
    let watts = ctl.get_power_out().expect("get_power_out");
    assert!((watts - 50.0).abs() < 0.01);
}

#[test]
fn alc_and_swr_parse() {
    let (addr, _store) = spawn_mock_rigctld();
    let mut ctl = RigctldController::connect(&addr).expect("connect");
    let alc = ctl.get_alc().expect("get_alc");
    let swr = ctl.get_swr().expect("get_swr");
    assert!((alc - 0.12).abs() < 0.001);
    assert!((swr - 1.4).abs() < 0.01);
}

#[test]
fn ptt_via_controller() {
    let (addr, _store) = spawn_mock_rigctld();
    let mut ctl = RigctldController::connect(&addr).expect("connect");
    assert!(!ctl.is_asserted());
    ctl.assert_ptt().expect("assert ptt");
    assert!(ctl.is_asserted());
    ctl.release_ptt().expect("release ptt");
    assert!(!ctl.is_asserted());
}
