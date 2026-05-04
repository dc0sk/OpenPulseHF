mod common;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::thread;
use std::time::Duration;

use openpulse_b2f::{banner, B2fSession, SessionRole, WlHeader};
use openpulse_b2f_driver::B2fDriver;

use common::{mock_cmd_irs, mock_cmd_iss, recv_frame, send_frame, test_header};

// ── Helpers local to this test ────────────────────────────────────────────────

/// Create a directly-connected TCP socketpair via a loopback listener.
fn tcp_pair() -> (TcpStream, TcpStream) {
    use std::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let client = TcpStream::connect(addr).unwrap();
    let (server, _) = listener.accept().unwrap();
    (client, server)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// ISS driver sends one message; a scripted IRS B2fSession handles the other
/// end of the data pipe directly.
#[test]
fn iss_sends_one_message() {
    let (iss_data, mut irs_data) = tcp_pair();
    let (cmd_addr, cmd_handle) = mock_cmd_iss();

    let body = b"Hello from ISS".to_vec();
    let body_clone = body.clone();

    // IRS side: scripted B2fSession playing IRS role.
    let irs_thread = thread::spawn(move || {
        let mut irs = B2fSession::new(SessionRole::Irs);
        // 1. Send IRS banner.
        let my_banner = banner::encode("K2DEF");
        send_frame(&mut irs_data, my_banner.as_bytes());
        // 2. Read FC and FF.
        loop {
            let frame = recv_frame(&mut irs_data);
            let line = String::from_utf8_lossy(&frame).into_owned();
            let responses = irs.handle_line(&line).unwrap();
            for resp in &responses {
                send_frame(&mut irs_data, resp.as_bytes());
            }
            if !responses.is_empty() || irs.is_done() {
                break;
            }
        }
        // 3. Receive the compressed blob.
        let count = irs.accepted_count();
        let mut decoded = Vec::new();
        for _ in 0..count {
            let blob = recv_frame(&mut irs_data);
            decoded.push(irs.receive_data(blob).unwrap());
        }
        decoded
    });

    let cmd_stream = TcpStream::connect(cmd_addr).unwrap();
    let mut driver = B2fDriver::new(cmd_stream, iss_data);
    driver
        .run_iss("K1ABC", "K2DEF", vec![(test_header("MSG001"), body)])
        .unwrap();

    let decoded = irs_thread.join().unwrap();
    cmd_handle.join().unwrap();

    assert_eq!(decoded.len(), 1);
    assert_eq!(decoded[0], body_clone);
}

/// IRS driver receives one message; a scripted ISS B2fSession handles the
/// other end of the data pipe directly.
#[test]
fn irs_receives_one_message() {
    let (irs_data, mut iss_data) = tcp_pair();
    let (cmd_addr, cmd_handle) = mock_cmd_irs();

    let body = b"Hello from scripted ISS".to_vec();
    let body_clone = body.clone();

    // ISS side: scripted B2fSession playing ISS role.
    let iss_thread = thread::spawn(move || {
        let mut iss = B2fSession::new(SessionRole::Iss);
        iss.queue_message(test_header("MSG002"), body).unwrap();
        // 1. Read IRS banner.
        let banner_frame = recv_frame(&mut iss_data);
        let banner_line = String::from_utf8_lossy(&banner_frame).into_owned();
        // 2. Send FC + FF.
        let fc_ff = iss.handle_line(&banner_line).unwrap();
        for line in &fc_ff {
            send_frame(&mut iss_data, line.as_bytes());
        }
        // 3. Read FS.
        let fs_frame = recv_frame(&mut iss_data);
        let fs_line = String::from_utf8_lossy(&fs_frame).into_owned();
        iss.handle_line(&fs_line).unwrap();
        // 4. Send blobs.
        for blob in iss.drain_pending_data() {
            send_frame(&mut iss_data, &blob);
        }
    });

    let cmd_stream = TcpStream::connect(cmd_addr).unwrap();
    let mut driver = B2fDriver::new(cmd_stream, irs_data);
    let decoded = driver.run_irs("K2DEF", Duration::from_secs(5)).unwrap();

    iss_thread.join().unwrap();
    cmd_handle.join().unwrap();

    assert_eq!(decoded.len(), 1);
    assert_eq!(decoded[0].body, body_clone);
}

/// Full roundtrip: two B2fDriver instances communicate through a TCP socketpair.
#[test]
fn iss_irs_roundtrip() {
    let (iss_data, irs_data) = tcp_pair();
    let (iss_cmd_addr, iss_cmd_handle) = mock_cmd_iss();
    let (irs_cmd_addr, irs_cmd_handle) = mock_cmd_irs();

    let body = b"End-to-end Winlink roundtrip test payload.".to_vec();
    let body_clone = body.clone();

    let irs_thread = thread::spawn(move || {
        let cmd = TcpStream::connect(irs_cmd_addr).unwrap();
        let mut driver = B2fDriver::new(cmd, irs_data);
        driver.run_irs("K2DEF", Duration::from_secs(5)).unwrap()
    });

    let iss_cmd = TcpStream::connect(iss_cmd_addr).unwrap();
    let mut iss_driver = B2fDriver::new(iss_cmd, iss_data);
    iss_driver
        .run_iss("K1ABC", "K2DEF", vec![(test_header("MSG003"), body)])
        .unwrap();

    let decoded = irs_thread.join().unwrap();
    iss_cmd_handle.join().unwrap();
    irs_cmd_handle.join().unwrap();

    assert_eq!(decoded.len(), 1);
    assert_eq!(decoded[0].body, body_clone);
}

/// ISS queues 3 messages; IRS decodes all 3 in order.
#[test]
fn multi_message_roundtrip() {
    let (iss_data, irs_data) = tcp_pair();
    let (iss_cmd_addr, iss_cmd_handle) = mock_cmd_iss();
    let (irs_cmd_addr, irs_cmd_handle) = mock_cmd_irs();

    let messages: Vec<Vec<u8>> = vec![
        b"First message body".to_vec(),
        b"Second message body with more content here".to_vec(),
        b"Third and final message".to_vec(),
    ];
    let expected = messages.clone();

    let irs_thread = thread::spawn(move || {
        let cmd = TcpStream::connect(irs_cmd_addr).unwrap();
        let mut driver = B2fDriver::new(cmd, irs_data);
        driver.run_irs("K2DEF", Duration::from_secs(5)).unwrap()
    });

    let iss_cmd = TcpStream::connect(iss_cmd_addr).unwrap();
    let mut iss_driver = B2fDriver::new(iss_cmd, iss_data);
    let msgs: Vec<(WlHeader, Vec<u8>)> = messages
        .into_iter()
        .enumerate()
        .map(|(i, body)| (test_header(&format!("MSG{:03}", i + 10)), body))
        .collect();
    iss_driver.run_iss("K1ABC", "K2DEF", msgs).unwrap();

    let decoded = irs_thread.join().unwrap();
    iss_cmd_handle.join().unwrap();
    irs_cmd_handle.join().unwrap();

    assert_eq!(decoded.len(), 3);
    for (i, msg) in decoded.iter().enumerate() {
        assert_eq!(msg.body, expected[i], "message {i} body mismatch");
    }
}
