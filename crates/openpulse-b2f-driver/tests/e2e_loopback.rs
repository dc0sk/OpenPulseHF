use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::mpsc::{self, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use bpsk_plugin::BpskPlugin;
use openpulse_channel::{awgn::AwgnChannel, AwgnConfig};
use openpulse_modem::channel_sim::ChannelSimHarness;

use openpulse_b2f::WlHeader;
use openpulse_b2f_driver::B2fDriver;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_bpsk_harness() -> ChannelSimHarness {
    let mut h = ChannelSimHarness::new();
    h.tx_engine
        .register_plugin(Box::new(BpskPlugin::new()))
        .expect("tx BPSK registration");
    h.rx_engine
        .register_plugin(Box::new(BpskPlugin::new()))
        .expect("rx BPSK registration");
    h
}

fn relay_frame_awgn(h: &mut ChannelSimHarness, frame: &[u8], ch: &mut AwgnChannel) -> Vec<u8> {
    h.tx_engine.transmit(frame, "BPSK250", None).unwrap();
    h.route(ch);
    h.rx_engine.receive("BPSK250", None).unwrap()
}

fn relay_frame_clean(h: &mut ChannelSimHarness, frame: &[u8]) -> Vec<u8> {
    h.tx_engine.transmit(frame, "BPSK250", None).unwrap();
    h.route_clean();
    h.rx_engine.receive("BPSK250", None).unwrap()
}

/// Read a u16-BE-framed payload; returns `None` on EOF/error.
fn recv_frame_ok(stream: &mut TcpStream) -> Option<Vec<u8>> {
    let mut len_buf = [0u8; 2];
    stream.read_exact(&mut len_buf).ok()?;
    let len = u16::from_be_bytes(len_buf) as usize;
    let mut payload = vec![0u8; len];
    stream.read_exact(&mut payload).ok()?;
    Some(payload)
}

/// Write a u16-BE-framed payload; returns false on error.
fn send_frame_ok(stream: &mut TcpStream, data: &[u8]) -> bool {
    let len = data.len() as u16;
    stream.write_all(&len.to_be_bytes()).is_ok()
        && stream.write_all(data).is_ok()
        && stream.flush().is_ok()
}

/// TCP data-port mini-server.
///
/// Bridges between a B2fDriver's TCP connection and a pair of mpsc channels
/// that feed the modem relay.  The outer thread runs the write-to-driver
/// loop; an inner thread handles reading from the driver.
fn spawn_data_server(
    outgoing: SyncSender<Vec<u8>>,
    incoming: mpsc::Receiver<Vec<u8>>,
) -> (SocketAddr, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut write_stream = stream.try_clone().unwrap();
        let mut read_stream = stream;

        // Inner thread: driver → relay.
        let read_handle = thread::spawn(move || loop {
            match recv_frame_ok(&mut read_stream) {
                Some(frame) => {
                    if outgoing.send(frame).is_err() {
                        break;
                    }
                }
                None => break, // TCP closed
            }
        });

        // Write loop: relay → driver.
        for frame in &incoming {
            if !send_frame_ok(&mut write_stream, &frame) {
                break;
            }
        }
        read_handle.join().ok();
    });
    (addr, handle)
}

enum RelayChannel {
    Awgn(AwgnChannel),
    Clean,
}

/// Bidirectional modem relay.
///
/// Polls ISS and IRS outboxes with `try_recv`; for each frame it encodes
/// through the appropriate `ChannelSimHarness`, routes through the channel
/// model, decodes, and forwards to the other side's inbox.  Exits when both
/// senders have disconnected.
fn spawn_modem_relay(
    mut iss_to_irs: ChannelSimHarness,
    mut irs_to_iss: ChannelSimHarness,
    iss_out: mpsc::Receiver<Vec<u8>>,
    iss_in: SyncSender<Vec<u8>>,
    irs_out: mpsc::Receiver<Vec<u8>>,
    irs_in: SyncSender<Vec<u8>>,
    mut channel: RelayChannel,
) -> JoinHandle<()> {
    thread::spawn(move || {
        use mpsc::TryRecvError;
        let mut iss_done = false;
        let mut irs_done = false;
        loop {
            let mut idle = true;

            match iss_out.try_recv() {
                Ok(frame) => {
                    let decoded = match &mut channel {
                        RelayChannel::Awgn(ch) => relay_frame_awgn(&mut iss_to_irs, &frame, ch),
                        RelayChannel::Clean => relay_frame_clean(&mut iss_to_irs, &frame),
                    };
                    let _ = irs_in.send(decoded);
                    idle = false;
                }
                Err(TryRecvError::Disconnected) => iss_done = true,
                Err(TryRecvError::Empty) => {}
            }

            match irs_out.try_recv() {
                Ok(frame) => {
                    let decoded = match &mut channel {
                        RelayChannel::Awgn(ch) => relay_frame_awgn(&mut irs_to_iss, &frame, ch),
                        RelayChannel::Clean => relay_frame_clean(&mut irs_to_iss, &frame),
                    };
                    let _ = iss_in.send(decoded);
                    idle = false;
                }
                Err(TryRecvError::Disconnected) => irs_done = true,
                Err(TryRecvError::Empty) => {}
            }

            if iss_done && irs_done {
                break;
            }
            if idle {
                thread::sleep(Duration::from_millis(1));
            }
        }
    })
}

fn mock_cmd_iss() -> (SocketAddr, JoinHandle<()>) {
    use std::io::BufRead;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
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

fn mock_cmd_irs() -> (SocketAddr, JoinHandle<()>) {
    use std::io::BufRead;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
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

fn test_header(mid: &str) -> WlHeader {
    WlHeader {
        mid: mid.into(),
        date: "2026/05/04 12:00".into(),
        from: "K1ABC@winlink.org".into(),
        to: vec!["K2DEF@winlink.org".into()],
        subject: "E2E test".into(),
        size: 0,
        body: 0,
        attachments: vec![],
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// Full stack: ISS → BPSK250 → AWGN 20 dB → BPSK250 → IRS.
#[test]
fn e2e_single_message_awgn_20db() {
    let (iss_out_tx, iss_out_rx) = mpsc::sync_channel(64);
    let (iss_in_tx, iss_in_rx) = mpsc::sync_channel(64);
    let (irs_out_tx, irs_out_rx) = mpsc::sync_channel(64);
    let (irs_in_tx, irs_in_rx) = mpsc::sync_channel(64);

    let (iss_data_addr, iss_data_handle) = spawn_data_server(iss_out_tx, iss_in_rx);
    let (irs_data_addr, irs_data_handle) = spawn_data_server(irs_out_tx, irs_in_rx);

    let awgn = AwgnChannel::new(AwgnConfig::new(20.0, Some(42))).unwrap();
    let relay_handle = spawn_modem_relay(
        make_bpsk_harness(),
        make_bpsk_harness(),
        iss_out_rx,
        iss_in_tx,
        irs_out_rx,
        irs_in_tx,
        RelayChannel::Awgn(awgn),
    );

    let (iss_cmd_addr, iss_cmd_handle) = mock_cmd_iss();
    let (irs_cmd_addr, irs_cmd_handle) = mock_cmd_irs();

    let body = b"End-to-end AWGN loopback test.".to_vec();
    let body_clone = body.clone();

    let irs_thread = thread::spawn(move || {
        let cmd = TcpStream::connect(irs_cmd_addr).unwrap();
        let data = TcpStream::connect(irs_data_addr).unwrap();
        let mut driver = B2fDriver::new(cmd, data);
        driver.run_irs("K2DEF", Duration::from_secs(10)).unwrap()
    });

    // Drop iss_driver immediately after run_iss() so its TCP streams close,
    // which causes the ISS data server's read thread to exit and signals the
    // relay that the ISS side is done.
    {
        let cmd = TcpStream::connect(iss_cmd_addr).unwrap();
        let data = TcpStream::connect(iss_data_addr).unwrap();
        let mut iss_driver = B2fDriver::new(cmd, data);
        iss_driver
            .run_iss("K1ABC", "K2DEF", vec![(test_header("E2E001"), body)])
            .unwrap();
    }

    let decoded = irs_thread.join().unwrap();
    relay_handle.join().unwrap();
    iss_data_handle.join().unwrap();
    irs_data_handle.join().unwrap();
    iss_cmd_handle.join().unwrap();
    irs_cmd_handle.join().unwrap();

    assert_eq!(decoded.len(), 1);
    assert_eq!(decoded[0].body, body_clone);
}

/// Full stack: three messages through a clean (no-distortion) channel.
#[test]
fn e2e_multi_message_clean() {
    let (iss_out_tx, iss_out_rx) = mpsc::sync_channel(64);
    let (iss_in_tx, iss_in_rx) = mpsc::sync_channel(64);
    let (irs_out_tx, irs_out_rx) = mpsc::sync_channel(64);
    let (irs_in_tx, irs_in_rx) = mpsc::sync_channel(64);

    let (iss_data_addr, iss_data_handle) = spawn_data_server(iss_out_tx, iss_in_rx);
    let (irs_data_addr, irs_data_handle) = spawn_data_server(irs_out_tx, irs_in_rx);

    let relay_handle = spawn_modem_relay(
        make_bpsk_harness(),
        make_bpsk_harness(),
        iss_out_rx,
        iss_in_tx,
        irs_out_rx,
        irs_in_tx,
        RelayChannel::Clean,
    );

    let (iss_cmd_addr, iss_cmd_handle) = mock_cmd_iss();
    let (irs_cmd_addr, irs_cmd_handle) = mock_cmd_irs();

    let bodies: Vec<Vec<u8>> = vec![
        b"First e2e message".to_vec(),
        b"Second e2e message with more bytes here".to_vec(),
        b"Third and final e2e message".to_vec(),
    ];
    let expected = bodies.clone();

    let irs_thread = thread::spawn(move || {
        let cmd = TcpStream::connect(irs_cmd_addr).unwrap();
        let data = TcpStream::connect(irs_data_addr).unwrap();
        let mut driver = B2fDriver::new(cmd, data);
        driver.run_irs("K2DEF", Duration::from_secs(10)).unwrap()
    });

    let msgs: Vec<(WlHeader, Vec<u8>)> = bodies
        .into_iter()
        .enumerate()
        .map(|(i, body)| (test_header(&format!("E2E{:03}", i + 10)), body))
        .collect();

    // Drop iss_driver after run_iss() so TCP streams close and the relay can exit.
    {
        let cmd = TcpStream::connect(iss_cmd_addr).unwrap();
        let data = TcpStream::connect(iss_data_addr).unwrap();
        let mut iss_driver = B2fDriver::new(cmd, data);
        iss_driver.run_iss("K1ABC", "K2DEF", msgs).unwrap();
    }

    let decoded = irs_thread.join().unwrap();
    relay_handle.join().unwrap();
    iss_data_handle.join().unwrap();
    irs_data_handle.join().unwrap();
    iss_cmd_handle.join().unwrap();
    irs_cmd_handle.join().unwrap();

    assert_eq!(decoded.len(), 3);
    for (i, msg) in decoded.iter().enumerate() {
        assert_eq!(msg.body, expected[i], "message {i} body mismatch");
    }
}
