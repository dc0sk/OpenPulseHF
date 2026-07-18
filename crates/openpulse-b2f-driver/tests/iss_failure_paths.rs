//! ISS must report a refused or empty transfer as a failure, not as success.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::thread;

use openpulse_b2f::{banner, frame, B2fFrame, FsAnswer};
use openpulse_b2f_driver::{B2fDriver, DriverError};

use common::test_header;

/// Mock ARDOP command port whose reply to CONNECT is chosen by the caller.
fn mock_cmd_with_connect_reply(reply: &'static str) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut writer = stream;
        let mut line = String::new();
        loop {
            line.clear();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                return;
            }
            let cmd = line.trim();
            if cmd.starts_with("MYID") {
                let call = cmd.split_whitespace().nth(1).unwrap_or("UNKNOWN");
                let _ = write!(writer, "MYID {call}\r\n");
            } else if cmd.starts_with("CONNECT") {
                let _ = write!(writer, "{reply}");
            } else if cmd.starts_with("DISCONNECT") {
                let _ = write!(writer, "DISCONNECTED\r\n");
                return;
            }
            let _ = writer.flush();
        }
    });
    addr
}

/// Mock data port that plays IRS and rejects every proposal.
fn mock_data_rejecting_all() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let send = |s: &mut TcpStream, payload: &[u8]| {
            let len = (payload.len() as u16).to_be_bytes();
            let _ = s.write_all(&len);
            let _ = s.write_all(payload);
            let _ = s.flush();
        };
        let recv = |s: &mut TcpStream| -> Option<Vec<u8>> {
            use std::io::Read;
            let mut len = [0u8; 2];
            s.read_exact(&mut len).ok()?;
            let mut buf = vec![0u8; u16::from_be_bytes(len) as usize];
            s.read_exact(&mut buf).ok()?;
            Some(buf)
        };
        // IRS banner first, then read FC + FF, then answer Reject.
        send(&mut stream, banner::encode("W2AW").as_bytes());
        let _fc = recv(&mut stream);
        let _ff = recv(&mut stream);
        let fs = frame::encode(&B2fFrame::Fs {
            answers: vec![FsAnswer::Reject],
        });
        send(&mut stream, fs.as_bytes());
        thread::sleep(std::time::Duration::from_millis(200));
    });
    addr
}

/// The gateway bails when the CMS rejects everything; the driver silently returned Ok, reporting
/// "sent" for a message that never left the queue (audit 2026-07-17, low tier — control-surface
/// divergence).
#[test]
fn run_iss_reports_all_proposals_rejected() {
    let cmd_addr = mock_cmd_with_connect_reply("CONNECTED PEER\r\n");
    let data_addr = mock_data_rejecting_all();
    let mut driver = B2fDriver::new(
        TcpStream::connect(cmd_addr).unwrap(),
        TcpStream::connect(data_addr).unwrap(),
    )
    .unwrap();

    let err = driver.run_iss(
        "W1AW",
        "W2AW",
        vec![(test_header("MSG001"), b"hello".to_vec())],
    );
    assert!(
        matches!(err, Err(DriverError::AllProposalsRejected { count: 1 })),
        "a fully-rejected transfer must be an error, got {err:?}"
    );
}

/// A peer that answers CONNECT with a terminal event is a refused session, not a timeout.
#[test]
fn run_iss_reports_a_refused_connect_as_aborted() {
    let cmd_addr = mock_cmd_with_connect_reply("NEWSTATE DISCONNECTING\r\nDISCONNECTED\r\n");
    let data_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let data_addr = data_listener.local_addr().unwrap();
    thread::spawn(move || {
        let _hold = data_listener.accept();
        thread::sleep(std::time::Duration::from_secs(2));
    });

    let mut driver = B2fDriver::new(
        TcpStream::connect(cmd_addr).unwrap(),
        TcpStream::connect(data_addr).unwrap(),
    )
    .unwrap();

    let err = driver.run_iss(
        "W1AW",
        "W2AW",
        vec![(test_header("MSG001"), b"hello".to_vec())],
    );
    assert!(
        matches!(err, Err(DriverError::Aborted)),
        "a refused CONNECT must report Aborted, got {err:?}"
    );
}
