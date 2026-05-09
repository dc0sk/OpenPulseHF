//! Trains a zstd dictionary from a corpus of HPX/Winlink message payloads.
//!
//! Usage:
//!   openpulse-dict-trainer [--corpus-dir <dir>] [--output <file>] [--dict-size <bytes>]
//!
//! If --corpus-dir is omitted the built-in HPX/Winlink synthetic corpus is used.

use std::env;
use std::fs;
use std::path::PathBuf;

/// Built-in corpus: representative HPX/Winlink payloads used when no external corpus is given.
fn builtin_corpus() -> Vec<Vec<u8>> {
    let mut samples: Vec<Vec<u8>> = Vec::new();

    // Winlink check-in message headers (RFC-5322-like).
    let headers = [
        "Date: Thu, 01 May 2026 14:23:00 +0000\r\nFrom: N0CALL@winlink.org\r\nTo: W1AW@winlink.org\r\nSubject: Check-in 14.070 MHz\r\nMime-Version: 1.0\r\nContent-Type: text/plain\r\n\r\nAll OK. Grid: FN31. Power: 100W. Antenna: 40m dipole.\r\n",
        "Date: Fri, 02 May 2026 09:10:00 +0000\r\nFrom: KD9ABC@winlink.org\r\nTo: WB4GHI@winlink.org\r\nSubject: Weekly traffic net\r\nMime-Version: 1.0\r\nContent-Type: text/plain\r\n\r\nTraffic net check-in. Grid: EM60. No traffic to pass.\r\n",
        "Date: Sat, 03 May 2026 17:45:00 +0000\r\nFrom: VE3XYZ@winlink.org\r\nTo: VE3PDQ@winlink.org\r\nSubject: EMCOMM exercise\r\nMime-Version: 1.0\r\nContent-Type: text/plain\r\n\r\nREDCROSS SHELTER STATUS: OPEN. 45 occupants. Supplies adequate.\r\n",
        "Date: Sun, 04 May 2026 22:00:00 +0000\r\nFrom: K5QRP@winlink.org\r\nTo: KX5NET@winlink.org\r\nSubject: Propagation report\r\nMime-Version: 1.0\r\nContent-Type: text/plain\r\n\r\n40m long-path to JA good until 0100z. 20m EU path open. K-index 2.\r\n",
    ];
    for h in &headers {
        samples.push(h.as_bytes().to_vec());
        // Also push with slight variation to enrich the corpus.
        samples.push(h.replace("May 2026", "Jun 2026").as_bytes().to_vec());
    }

    // HPX ConReq JSON bodies (handshake frames).
    let conreqs = [
        r#"{"session_id":"a1b2c3d4e5f6g7h8","callsign":"N0CALL","signing_mode":"ed25519","supported_modes":["BPSK250","QPSK500"],"supported_compression":["none","lz4","zstd"],"timestamp_ms":1746100000000}"#,
        r#"{"session_id":"deadbeefcafe0011","callsign":"W1AW","signing_mode":"hybrid","supported_modes":["BPSK100","BPSK250","QPSK250","QPSK1000"],"supported_compression":["none","lz4"],"timestamp_ms":1746200000000}"#,
        r#"{"session_id":"0102030405060708","callsign":"KD9ABC","signing_mode":"ed25519","supported_modes":["QPSK1000","8PSK1000"],"supported_compression":["none","lz4",{"zstd":305419896}],"timestamp_ms":1746300000000}"#,
    ];
    for c in &conreqs {
        samples.push(c.as_bytes().to_vec());
    }

    // HPX ConAck JSON bodies.
    let conacks = [
        r#"{"session_id":"a1b2c3d4e5f6g7h8","responder_callsign":"W1AW","signing_mode":"ed25519","selected_mode":"BPSK250","selected_compression":"lz4","timestamp_ms":1746100000500}"#,
        r#"{"session_id":"deadbeefcafe0011","responder_callsign":"N0CALL","signing_mode":"hybrid","selected_mode":"QPSK500","selected_compression":{"zstd":305419896},"timestamp_ms":1746200000300}"#,
    ];
    for c in &conacks {
        samples.push(c.as_bytes().to_vec());
    }

    // Typical short message bodies.
    let bodies = [
        "73 de N0CALL. Good signal, 599. See you next week on the net.",
        "QRN heavy tonight. QSY to 7.074 MHz. Tnx for the contact. 73.",
        "SKYWARN REPORT: Tornado watch in effect. No rotation observed. Winds 25 mph from SW.",
        "ARES check-in. Operator available. HF capability: 40m and 20m. VHF: 2m FM.",
        "Traffic: NTS message for W1AW. Please relay to Eastern Area. Priority: WELFARE.",
        "PSE QSL via LoTW. DXCC needed: K, W, VE. OP: John Smith. QTH: Springfield IL.",
        "BPQ NET: All stations check in on 14.300 MHz at 1900z. Net control: W5KFT.",
    ];
    for b in &bodies {
        samples.push(b.as_bytes().to_vec());
        // Duplicate with callsign variations.
        samples.push(b.replace("N0CALL", "KD9ABC").as_bytes().to_vec());
        samples.push(b.replace("W1AW", "VE3XYZ").as_bytes().to_vec());
    }

    // B2F frame headers (Winlink over-the-air protocol).
    let b2f = [
        "[WL2K-3.0-B2FWINMOR-4.0-A1B2C3D4]\r\n",
        "FC EM ABCD123456 1200 512 0\r\n",
        "FF\r\nFS Y\r\n",
        "Fq\r\n",
        "[WL2K-3.0-B2FWINMOR-4.0-DEADBEEF]\r\nFC EM XY123456789 800 256 0\r\nFF\r\nFS Y\r\n",
    ];
    for f in &b2f {
        samples.push(f.as_bytes().to_vec());
    }

    // Grid-square and position reports.
    let pos = [
        "APRS:N0CALL>APRS,TCPIP*:=3732.10N/07954.32W-OpenPulse HF iGate/A=000200",
        "APRS:KD9ABC>APRS,WIDE1-1:!4015.00N/08845.00W#OpenPulse digipeater",
        "GRID:FN31pr ALT:150m SPEED:0km/h HEADING:0 COMMENT:QRP portable 40m",
    ];
    for p in &pos {
        samples.push(p.as_bytes().to_vec());
    }

    samples
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let mut corpus_dir: Option<PathBuf> = None;
    let mut output_path = PathBuf::from("zstd-hpx-dict.bin");
    let mut dict_size: usize = 4096;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--corpus-dir" => {
                i += 1;
                corpus_dir = Some(PathBuf::from(&args[i]));
            }
            "--output" => {
                i += 1;
                output_path = PathBuf::from(&args[i]);
            }
            "--dict-size" => {
                i += 1;
                dict_size = args[i].parse().expect("--dict-size must be a number");
            }
            other => {
                eprintln!("Unknown argument: {other}");
                std::process::exit(1);
            }
        }
        i += 1;
    }

    let samples: Vec<Vec<u8>> = if let Some(dir) = corpus_dir {
        let mut s = Vec::new();
        for entry in fs::read_dir(&dir).expect("read corpus dir") {
            let path = entry.expect("dir entry").path();
            if path.is_file() {
                s.push(fs::read(&path).expect("read corpus file"));
            }
        }
        if s.is_empty() {
            eprintln!("Warning: corpus dir is empty; using built-in samples.");
            builtin_corpus()
        } else {
            s
        }
    } else {
        builtin_corpus()
    };

    eprintln!(
        "Training zstd dictionary from {} samples ({} bytes total), target size {} bytes ...",
        samples.len(),
        samples.iter().map(|s| s.len()).sum::<usize>(),
        dict_size,
    );

    let dict_bytes =
        zstd::dict::from_samples(&samples, dict_size).expect("zstd dictionary training failed");

    // Read the dictionary ID from bytes 4–7 (big-endian) per the zstd dict format.
    // Magic: 0xEC30A437 at bytes 0–3; dict ID at bytes 4–7.
    let dict_id = u32::from_le_bytes([dict_bytes[4], dict_bytes[5], dict_bytes[6], dict_bytes[7]]);

    fs::write(&output_path, &dict_bytes).expect("write dictionary");
    eprintln!(
        "Wrote {} bytes to {}  (dict ID: {})",
        dict_bytes.len(),
        output_path.display(),
        dict_id,
    );
}
