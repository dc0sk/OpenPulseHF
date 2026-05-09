use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::mpsc::{self, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use bpsk_plugin::BpskPlugin;
use openpulse_b2f::WlHeader;
use openpulse_b2f_driver::B2fDriver;
use openpulse_modem::channel_sim::ChannelSimHarness;

use crate::channels::build as build_channel;
use crate::matrix::{TestCase, TestResult};

pub fn run(case: &TestCase) -> TestResult {
    let start = Instant::now();

    let body: Vec<u8> = (0..case.payload_len.min(255)).map(|i| i as u8).collect();
    let body_clone = body.clone();

    let result = run_b2f_pair(case, body);

    let duration_ms = start.elapsed().as_millis() as u64;
    match result {
        Ok(messages) => {
            let passed = messages.len() == 1 && messages[0].body == body_clone;
            TestResult {
                case: case.clone(),
                passed,
                skipped: false,
                ber: None,
                bytes_rx: messages.first().map(|m| m.body.len()).unwrap_or(0),
                duration_ms,
                effective_bps: None,
                note: if passed {
                    None
                } else {
                    Some("body mismatch or wrong message count".into())
                },
            }
        }
        Err(e) => TestResult {
            case: case.clone(),
            passed: false,
            skipped: false,
            ber: None,
            bytes_rx: 0,
            duration_ms,
            effective_bps: None,
            note: Some(e),
        },
    }
}

struct DecodedMessage {
    pub body: Vec<u8>,
}

fn run_b2f_pair(case: &TestCase, body: Vec<u8>) -> Result<Vec<DecodedMessage>, String> {
    let (iss_out_tx, iss_out_rx) = mpsc::sync_channel::<Vec<u8>>(64);
    let (iss_in_tx, iss_in_rx) = mpsc::sync_channel::<Vec<u8>>(64);
    let (irs_out_tx, irs_out_rx) = mpsc::sync_channel::<Vec<u8>>(64);
    let (irs_in_tx, irs_in_rx) = mpsc::sync_channel::<Vec<u8>>(64);

    let (iss_data_addr, iss_data_handle) = spawn_data_server(iss_out_tx, iss_in_rx);
    let (irs_data_addr, irs_data_handle) = spawn_data_server(irs_out_tx, irs_in_rx);

    let channel_spec = case.channel.clone();
    let relay_handle = thread::spawn(move || {
        let mut channel = build_channel(&channel_spec);
        let mut iss_to_irs = make_bpsk_harness();
        let mut irs_to_iss = make_bpsk_harness();

        use mpsc::TryRecvError;
        let mut iss_done = false;
        let mut irs_done = false;
        loop {
            let mut idle = true;
            match iss_out_rx.try_recv() {
                Ok(frame) => {
                    if let Ok(decoded) = relay_frame(&mut iss_to_irs, &frame, channel.as_mut()) {
                        let _ = irs_in_tx.send(decoded);
                    }
                    idle = false;
                }
                Err(TryRecvError::Disconnected) => iss_done = true,
                Err(TryRecvError::Empty) => {}
            }
            match irs_out_rx.try_recv() {
                Ok(frame) => {
                    if let Ok(decoded) = relay_frame(&mut irs_to_iss, &frame, channel.as_mut()) {
                        let _ = iss_in_tx.send(decoded);
                    }
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
    });

    let (iss_cmd_addr, iss_cmd_handle) = mock_cmd_iss();
    let (irs_cmd_addr, irs_cmd_handle) = mock_cmd_irs();

    let header = WlHeader {
        mid: "TM001".into(),
        date: "2026/05/06 12:00".into(),
        from: "K1ABC@winlink.org".into(),
        to: vec!["K2DEF@winlink.org".into()],
        subject: "TestMatrix".into(),
        size: 0,
        body: 0,
        attachments: vec![],
    };

    let irs_thread = thread::spawn(move || {
        let cmd = TcpStream::connect(irs_cmd_addr).map_err(|e| e.to_string())?;
        let data = TcpStream::connect(irs_data_addr).map_err(|e| e.to_string())?;
        let mut driver = B2fDriver::new(cmd, data);
        driver
            .run_irs("K2DEF", Duration::from_secs(30))
            .map_err(|e| e.to_string())
    });

    {
        let cmd = TcpStream::connect(iss_cmd_addr).map_err(|e| e.to_string())?;
        let data = TcpStream::connect(iss_data_addr).map_err(|e| e.to_string())?;
        let mut iss_driver = B2fDriver::new(cmd, data);
        iss_driver
            .run_iss("K1ABC", "K2DEF", vec![(header, body)])
            .map_err(|e| e.to_string())?;
    }

    let decoded = irs_thread
        .join()
        .map_err(|_| "IRS thread panic".to_string())??;
    relay_handle.join().ok();
    iss_data_handle.join().ok();
    irs_data_handle.join().ok();
    iss_cmd_handle.join().ok();
    irs_cmd_handle.join().ok();

    Ok(decoded
        .into_iter()
        .map(|m| DecodedMessage { body: m.body })
        .collect())
}

fn make_bpsk_harness() -> ChannelSimHarness {
    let mut h = ChannelSimHarness::new();
    h.tx_engine
        .register_plugin(Box::new(BpskPlugin::new()))
        .expect("BPSK registration");
    h.rx_engine
        .register_plugin(Box::new(BpskPlugin::new()))
        .expect("BPSK registration");
    h
}

fn relay_frame(
    h: &mut ChannelSimHarness,
    frame: &[u8],
    channel: &mut dyn openpulse_channel::ChannelModel,
) -> Result<Vec<u8>, String> {
    if frame.len() > 255 {
        return Err(format!("frame {} B > 255 B limit", frame.len()));
    }
    h.tx_engine
        .transmit(frame, "BPSK250", None)
        .map_err(|e| e.to_string())?;
    h.route(channel);
    h.rx_engine
        .receive("BPSK250", None)
        .map_err(|e| e.to_string())
}

// ── Infrastructure ─────────────────────────────────────────────────────────────

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

        let read_handle = thread::spawn(move || {
            while let Some(frame) = recv_frame_ok(&mut read_stream) {
                if outgoing.send(frame).is_err() {
                    break;
                }
            }
        });

        for frame in &incoming {
            if !send_frame_ok(&mut write_stream, &frame) {
                break;
            }
        }
        read_handle.join().ok();
    });
    (addr, handle)
}

fn recv_frame_ok(stream: &mut TcpStream) -> Option<Vec<u8>> {
    let mut len_buf = [0u8; 2];
    match stream.read_exact(&mut len_buf) {
        Ok(()) => {}
        Err(e)
            if e.kind() == ErrorKind::UnexpectedEof || e.kind() == ErrorKind::ConnectionReset =>
        {
            return None;
        }
        Err(_) => return None,
    }
    let len = u16::from_be_bytes(len_buf) as usize;
    let mut payload = vec![0u8; len];
    stream.read_exact(&mut payload).ok()?;
    Some(payload)
}

fn send_frame_ok(stream: &mut TcpStream, data: &[u8]) -> bool {
    let len: u16 = match data.len().try_into() {
        Ok(n) => n,
        Err(_) => return false,
    };
    if stream.write_all(&len.to_be_bytes()).is_err() {
        return false;
    }
    stream.write_all(data).is_ok() && stream.flush().is_ok()
}

fn mock_cmd_iss() -> (SocketAddr, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut writer = stream;
        let mut line = String::new();
        loop {
            line.clear();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                break;
            }
            let cmd = line.trim();
            if cmd.is_empty() {
                continue;
            }
            if cmd.starts_with("MYID") {
                let call = cmd.split_whitespace().nth(1).unwrap_or("UNKNOWN");
                let _ = write!(writer, "MYID {call}\r\n");
            } else if cmd.starts_with("CONNECT") {
                let _ = write!(writer, "NEWSTATE CONNECTING\r\nCONNECTED PEER\r\n");
            } else if cmd.starts_with("DISCONNECT") {
                let _ = write!(writer, "NEWSTATE DISCONNECTING\r\nDISCONNECTED\r\n");
                break;
            }
            let _ = writer.flush();
        }
    });
    (addr, handle)
}

fn mock_cmd_irs() -> (SocketAddr, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut writer = stream;
        let mut line = String::new();
        loop {
            line.clear();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                break;
            }
            let cmd = line.trim();
            if cmd.is_empty() {
                continue;
            }
            if cmd.starts_with("MYID") {
                let call = cmd.split_whitespace().nth(1).unwrap_or("UNKNOWN");
                let _ = write!(writer, "MYID {call}\r\n");
            } else if cmd.starts_with("LISTEN") {
                let _ = write!(writer, "LISTEN TRUE\r\nCONNECTED ISS\r\n");
            } else if cmd.starts_with("DISCONNECT") {
                let _ = write!(writer, "NEWSTATE DISCONNECTING\r\nDISCONNECTED\r\n");
                break;
            }
            let _ = writer.flush();
        }
    });
    (addr, handle)
}
