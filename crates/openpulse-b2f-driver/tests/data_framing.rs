//! `DataPort` u16-BE framing must fail cleanly on malformed input, never hang or mis-frame.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use openpulse_b2f_driver::{DataPort, DriverError};

/// A server that writes `script` verbatim, then optionally closes.
fn mock_raw(script: Vec<u8>, close: bool) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let _ = stream.write_all(&script);
        let _ = stream.flush();
        if !close {
            thread::sleep(Duration::from_secs(3));
        }
    });
    addr
}

fn port(addr: SocketAddr) -> DataPort {
    let mut p = DataPort::new(TcpStream::connect(addr).unwrap());
    p.set_timeout(Some(Duration::from_millis(500))).unwrap();
    p
}

/// A length prefix that promises more than the peer ever sends must error, not hang or return short.
#[test]
fn truncated_payload_is_an_error() {
    let mut script = 64u16.to_be_bytes().to_vec();
    script.extend_from_slice(b"only ten b");
    let mut p = port(mock_raw(script, true));
    let got = p.recv_frame();
    assert!(
        got.is_err(),
        "a truncated frame must not decode, got {got:?}"
    );
}

/// A half-delivered length prefix is the same class of failure.
#[test]
fn truncated_length_prefix_is_an_error() {
    let mut p = port(mock_raw(vec![0x00], true));
    assert!(p.recv_frame().is_err(), "a 1-byte length prefix must error");
}

/// A zero-length frame is legal framing and must round-trip as an empty payload, not an error.
#[test]
fn zero_length_frame_decodes_as_empty() {
    let script = 0u16.to_be_bytes().to_vec();
    let mut p = port(mock_raw(script, false));
    assert_eq!(p.recv_frame().unwrap(), Vec::<u8>::new());
}

/// The largest frame the u16 prefix can describe must decode intact — the boundary case where an
/// off-by-one in the length handling would show up.
#[test]
fn max_length_frame_decodes_intact() {
    let payload = vec![b'z'; u16::MAX as usize];
    let mut script = (u16::MAX).to_be_bytes().to_vec();
    script.extend_from_slice(&payload);
    let mut p = port(mock_raw(script, false));
    assert_eq!(p.recv_frame().unwrap(), payload);
}

/// Frames must not run together: two back-to-back frames decode as two distinct payloads.
#[test]
fn back_to_back_frames_do_not_merge() {
    let mut script = Vec::new();
    for part in [b"first".as_slice(), b"second".as_slice()] {
        script.extend_from_slice(&(part.len() as u16).to_be_bytes());
        script.extend_from_slice(part);
    }
    let mut p = port(mock_raw(script, false));
    assert_eq!(p.recv_frame().unwrap(), b"first".to_vec());
    assert_eq!(p.recv_frame().unwrap(), b"second".to_vec());
}

/// A peer that connects and says nothing must time out rather than block forever.
#[test]
fn silent_peer_times_out() {
    let mut p = port(mock_raw(Vec::new(), false));
    assert!(matches!(p.recv_frame(), Err(DriverError::Timeout)));
}

/// `send_frame` must refuse a payload the u16 prefix cannot describe rather than truncate it.
#[test]
fn oversized_send_is_refused() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let mut sink = Vec::new();
        let _ = stream.read_to_end(&mut sink);
    });
    let mut p = DataPort::new(TcpStream::connect(addr).unwrap());
    let too_big = vec![0u8; u16::MAX as usize + 1];
    assert!(
        matches!(p.send_frame(&too_big), Err(DriverError::Ardop(_))),
        "a payload over u16::MAX must be refused, not truncated"
    );
}
