// Shared test helpers for openpulse-b2f-driver integration tests.
#![allow(dead_code)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::thread::{self, JoinHandle};

use openpulse_b2f::WlHeader;

/// Minimal mock ARDOP command port for the ISS side.
pub fn mock_cmd_iss() -> (SocketAddr, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut writer = stream;
        let mut line = String::new();
        loop {
            line.clear();
            if reader.read_line(&mut line).unwrap() == 0 {
                break;
            }
            let cmd = line.trim();
            if cmd.is_empty() {
                continue;
            }
            if cmd.starts_with("MYID") {
                let call = cmd.split_whitespace().nth(1).unwrap_or("UNKNOWN");
                write!(writer, "MYID {call}\r\n").unwrap();
            } else if cmd.starts_with("CONNECT") {
                write!(writer, "NEWSTATE CONNECTING\r\nCONNECTED PEER\r\n").unwrap();
            } else if cmd.starts_with("DISCONNECT") {
                write!(writer, "NEWSTATE DISCONNECTING\r\nDISCONNECTED\r\n").unwrap();
                break;
            }
            writer.flush().unwrap();
        }
    });
    (addr, handle)
}

/// Minimal mock ARDOP command port for the IRS side.
///
/// After `LISTEN TRUE`, immediately sends `CONNECTED ISS\r\n` as an event.
pub fn mock_cmd_irs() -> (SocketAddr, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut writer = stream;
        let mut line = String::new();
        loop {
            line.clear();
            if reader.read_line(&mut line).unwrap() == 0 {
                break;
            }
            let cmd = line.trim();
            if cmd.is_empty() {
                continue;
            }
            if cmd.starts_with("MYID") {
                let call = cmd.split_whitespace().nth(1).unwrap_or("UNKNOWN");
                write!(writer, "MYID {call}\r\n").unwrap();
            } else if cmd.starts_with("LISTEN") {
                write!(writer, "LISTEN TRUE\r\nCONNECTED ISS\r\n").unwrap();
            } else if cmd.starts_with("DISCONNECT") {
                write!(writer, "NEWSTATE DISCONNECTING\r\nDISCONNECTED\r\n").unwrap();
                break;
            }
            writer.flush().unwrap();
        }
    });
    (addr, handle)
}

/// Send a u16-BE-framed payload over a TcpStream.
pub fn send_frame(stream: &mut TcpStream, data: &[u8]) {
    stream
        .write_all(&(data.len() as u16).to_be_bytes())
        .unwrap();
    stream.write_all(data).unwrap();
    stream.flush().unwrap();
}

/// Read a u16-BE-framed payload from a TcpStream.
pub fn recv_frame(stream: &mut TcpStream) -> Vec<u8> {
    let mut len_buf = [0u8; 2];
    stream.read_exact(&mut len_buf).unwrap();
    let len = u16::from_be_bytes(len_buf) as usize;
    let mut payload = vec![0u8; len];
    stream.read_exact(&mut payload).unwrap();
    payload
}

pub fn test_header(mid: &str) -> WlHeader {
    WlHeader {
        mid: mid.into(),
        date: "2026/05/04 12:00".into(),
        from: "K1ABC@winlink.org".into(),
        to: vec!["K2DEF@winlink.org".into()],
        subject: "Test".into(),
        size: 0,
        body: 0,
        attachments: vec![],
    }
}
