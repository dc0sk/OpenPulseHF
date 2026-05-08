//! Mock rigctld TCP server for scanner tests.
//!
//! Starts a listener on an OS-assigned port, handles one connection, responds to
//! `\get_freq`, `\set_freq`, and `\get_level STRENGTH` commands with fixture values.

#![allow(dead_code)]

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

/// Starts a mock rigctld server and returns its port.
///
/// The server handles one connection. `home_freq` is returned for `\get_freq`.
/// `strength` is returned for `\get_level STRENGTH`.
pub fn start_mock_rigctld(home_freq: u64, strength: i32) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock rigctld");
    let port = listener.local_addr().unwrap().port();

    thread::spawn(move || {
        let (stream, _) = listener.accept().expect("accept");
        handle_connection(stream, home_freq, strength);
    });

    port
}

/// Starts a mock rigctld server that serves multiple connections and records
/// `\set_freq` calls.  Returns (port, recorded_freqs).
pub fn start_recording_rigctld(home_freq: u64, strength: i32) -> (u16, Arc<Mutex<Vec<u64>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock rigctld");
    let port = listener.local_addr().unwrap().port();
    let freqs: Arc<Mutex<Vec<u64>>> = Arc::new(Mutex::new(vec![]));
    let freqs2 = Arc::clone(&freqs);

    thread::spawn(move || {
        // handle up to 1 connection
        if let Ok((stream, _)) = listener.accept() {
            handle_connection_recording(stream, home_freq, strength, freqs2);
        }
    });

    (port, freqs)
}

fn handle_connection(mut stream: TcpStream, home_freq: u64, strength: i32) {
    let reader_stream = stream.try_clone().expect("clone stream");
    let mut reader = BufReader::new(reader_stream);
    let mut line = String::new();
    while reader.read_line(&mut line).unwrap_or(0) > 0 {
        let cmd = line.trim().to_string();
        respond(&mut stream, &cmd, home_freq, strength, &mut vec![]);
        line.clear();
    }
}

fn handle_connection_recording(
    mut stream: TcpStream,
    home_freq: u64,
    strength: i32,
    freqs: Arc<Mutex<Vec<u64>>>,
) {
    let reader_stream = stream.try_clone().expect("clone stream");
    let mut reader = BufReader::new(reader_stream);
    let mut line = String::new();
    while reader.read_line(&mut line).unwrap_or(0) > 0 {
        let cmd = line.trim().to_string();
        // Record set_freq calls immediately so callers don't need to wait for disconnect.
        if cmd.starts_with("\\set_freq ") {
            if let Ok(f) = cmd["\\set_freq ".len()..].trim().parse::<u64>() {
                freqs.lock().unwrap().push(f);
            }
        }
        respond(&mut stream, &cmd, home_freq, strength, &mut vec![]);
        line.clear();
    }
}

fn respond(
    stream: &mut TcpStream,
    cmd: &str,
    home_freq: u64,
    strength: i32,
    recorded: &mut Vec<u64>,
) {
    if cmd.starts_with("\\set_freq ") {
        if let Ok(f) = cmd["\\set_freq ".len()..].trim().parse::<u64>() {
            recorded.push(f);
        }
        writeln!(stream, "RPRT 0").ok();
    } else if cmd == "\\get_freq" {
        writeln!(stream, "Frequency: {home_freq}").ok();
        writeln!(stream, "RPRT 0").ok();
    } else if cmd == "\\get_level STRENGTH" {
        writeln!(stream, "Level: {strength}").ok();
        writeln!(stream, "RPRT 0").ok();
    } else {
        // Unknown — still respond OK so the controller doesn't hang
        writeln!(stream, "RPRT 0").ok();
    }
}
