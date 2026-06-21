//! Integration test for the daemon-protocol serve mode.
//!
//! Starts the server on an ephemeral port, connects a raw TCP client (standing in for
//! `openpulse-panel`), and asserts it receives both a parseable `ControlEvent` JSON line
//! and a binary `OPSP` spectrum frame.

#![cfg(feature = "serve")]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

use openpulse_core::compression::CompressionAlgorithm;
use openpulse_core::fec::FecMode;
use openpulse_linksim::{serve::serve_on, ChannelSpec, LinkParams};

#[test]
fn panel_client_receives_events_and_spectrum() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local addr");

    // hpx_wideband starts at a fast mode (QPSK500), so frames are short and JSON events
    // arrive promptly. usize::MAX runs continuously until the client disconnects.
    let params = LinkParams {
        profile_name: "hpx_wideband".into(),
        forward: ChannelSpec::Awgn(20.0),
        reverse: ChannelSpec::Awgn(25.0),
        payload_bytes_per_frame: 32,
        total_frames: usize::MAX,
        fec: FecMode::Rs,
        compression: CompressionAlgorithm::None,
        turnaround_s: 0.2,
        max_attempts: 4,
        seed: 99,
    };

    std::thread::spawn(move || {
        let _ = serve_on(listener, &params, 50);
    });

    let mut stream = TcpStream::connect(addr).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    stream
        .write_all(b"{\"cmd\":\"subscribe_spectrum\",\"fps\":20}\n")
        .unwrap();

    let mut buf: Vec<u8> = Vec::new();
    let mut saw_json = false;
    let mut saw_spectrum = false;
    let deadline = Instant::now() + Duration::from_secs(8);

    while Instant::now() < deadline && !(saw_json && saw_spectrum) {
        let mut tmp = [0u8; 8192];
        match stream.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
            Err(ref e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(_) => break,
        }

        // Drain complete frames: 'O' => binary OPSP, otherwise a newline-terminated JSON line.
        loop {
            if buf.is_empty() {
                break;
            }
            if buf[0] == b'O' {
                if buf.len() < 10 {
                    break;
                }
                assert_eq!(&buf[0..4], b"OPSP", "spectrum magic");
                let fft = u16::from_le_bytes([buf[4], buf[5]]) as usize;
                let need = 10 + fft * 4;
                if buf.len() < need {
                    break;
                }
                saw_spectrum = true;
                buf.drain(..need);
            } else {
                let Some(nl) = buf.iter().position(|&b| b == b'\n') else {
                    break;
                };
                let line: Vec<u8> = buf.drain(..=nl).collect();
                let text = String::from_utf8_lossy(&line);
                let text = text.trim();
                if text.is_empty() {
                    continue;
                }
                let v: serde_json::Value =
                    serde_json::from_str(text).expect("each JSON line must parse");
                assert!(v.get("type").is_some(), "ControlEvent has a type tag");
                saw_json = true;
            }
        }
    }

    assert!(saw_json, "client should receive at least one ControlEvent");
    assert!(
        saw_spectrum,
        "client should receive at least one spectrum frame"
    );
}
