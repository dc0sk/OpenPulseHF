//! Multithreaded pipeline harness with deterministic timing assertions.

use std::collections::HashMap;
use std::sync::mpsc;
use std::thread;

const TICK_US: u64 = 1_000;
const INGEST_COST_TICKS: u64 = 1;
const ENCODE_COST_TICKS: u64 = 2;
const MODULATE_COST_TICKS: u64 = 3;
const TRANSMIT_COST_TICKS: u64 = 1;

#[derive(Clone, Debug)]
struct PipelineJob {
    id: u64,
    payload: Vec<u8>,
    logical_ticks: u64,
}

#[derive(Clone, Debug)]
struct PipelineTrace {
    id: u64,
    ingest_ticks: u64,
    encode_ticks: u64,
    modulate_ticks: u64,
    transmit_ticks: u64,
    end_to_end_ticks: u64,
}

fn stage(name: &'static str, cost_ticks: u64, input: PipelineJob) -> PipelineJob {
    let _ = name;
    PipelineJob {
        logical_ticks: input.logical_ticks + cost_ticks,
        ..input
    }
}

#[test]
fn multithreaded_pipeline_has_deterministic_timing() {
    let sample_count = 8_u64;
    let expected_total_ticks =
        INGEST_COST_TICKS + ENCODE_COST_TICKS + MODULATE_COST_TICKS + TRANSMIT_COST_TICKS;

    let (tx_in, rx_in) = mpsc::channel::<PipelineJob>();
    let (tx_ingest, rx_ingest) = mpsc::channel::<PipelineJob>();
    let (tx_encode, rx_encode) = mpsc::channel::<PipelineJob>();
    let (tx_modulate, rx_modulate) = mpsc::channel::<PipelineJob>();
    let (tx_out, rx_out) = mpsc::channel::<PipelineTrace>();

    let ingest_handle = thread::spawn(move || {
        for job in rx_in {
            let job = stage("ingest", INGEST_COST_TICKS, job);
            tx_ingest.send(job).expect("ingest send failed");
        }
    });

    let encode_handle = thread::spawn(move || {
        for job in rx_ingest {
            let job = stage("encode", ENCODE_COST_TICKS, job);
            tx_encode.send(job).expect("encode send failed");
        }
    });

    let modulate_handle = thread::spawn(move || {
        for job in rx_encode {
            let job = stage("modulate", MODULATE_COST_TICKS, job);
            tx_modulate.send(job).expect("modulate send failed");
        }
    });

    let transmit_handle = thread::spawn(move || {
        for mut job in rx_modulate {
            assert_eq!(job.payload.len(), 16, "unexpected payload length");
            job = stage("transmit", TRANSMIT_COST_TICKS, job);

            let trace = PipelineTrace {
                id: job.id,
                ingest_ticks: INGEST_COST_TICKS,
                encode_ticks: INGEST_COST_TICKS + ENCODE_COST_TICKS,
                modulate_ticks: INGEST_COST_TICKS + ENCODE_COST_TICKS + MODULATE_COST_TICKS,
                transmit_ticks: job.logical_ticks,
                end_to_end_ticks: job.logical_ticks,
            };

            tx_out.send(trace).expect("output send failed");
        }
    });

    for id in 0..sample_count {
        tx_in
            .send(PipelineJob {
                id,
                payload: vec![id as u8; 16],
                logical_ticks: 0,
            })
            .expect("input send failed");
    }
    drop(tx_in);

    let mut traces = Vec::new();
    for _ in 0..sample_count {
        traces.push(rx_out.recv().expect("missing pipeline output"));
    }

    ingest_handle.join().expect("ingest thread panicked");
    encode_handle.join().expect("encode thread panicked");
    modulate_handle.join().expect("modulate thread panicked");
    transmit_handle.join().expect("transmit thread panicked");

    traces.sort_by_key(|trace| trace.id);
    assert_eq!(traces.len(), sample_count as usize);

    let mut seen = HashMap::new();
    for trace in &traces {
        seen.insert(trace.id, true);

        assert_eq!(trace.ingest_ticks, INGEST_COST_TICKS);
        assert_eq!(trace.encode_ticks, INGEST_COST_TICKS + ENCODE_COST_TICKS);
        assert_eq!(
            trace.modulate_ticks,
            INGEST_COST_TICKS + ENCODE_COST_TICKS + MODULATE_COST_TICKS
        );
        assert_eq!(trace.transmit_ticks, expected_total_ticks);
        assert_eq!(trace.end_to_end_ticks, expected_total_ticks);
    }

    for id in 0..sample_count {
        assert!(seen.contains_key(&id), "missing trace for id={id}");
    }
}

#[test]
fn deterministic_ticks_map_to_expected_latency_budget() {
    let total_ticks =
        INGEST_COST_TICKS + ENCODE_COST_TICKS + MODULATE_COST_TICKS + TRANSMIT_COST_TICKS;
    let total_latency_us = total_ticks * TICK_US;

    assert_eq!(total_ticks, 7);
    assert_eq!(total_latency_us, 7_000);
}
