//! Pipeline boundary types for modem stage decomposition.
//!
//! These types make stage interfaces explicit so execution can be moved
//! to threaded workers later without changing higher-level call sites.

use std::sync::mpsc::{sync_channel, Receiver, SyncSender};

/// Logical modem pipeline stages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineStage {
    InputCapture,
    DemodulateDecode,
    HpxStateUpdate,
    EncodeModulate,
    OutputEmit,
}

const STAGE_COUNT: usize = 5;

impl PipelineStage {
    fn index(self) -> usize {
        match self {
            PipelineStage::InputCapture => 0,
            PipelineStage::DemodulateDecode => 1,
            PipelineStage::HpxStateUpdate => 2,
            PipelineStage::EncodeModulate => 3,
            PipelineStage::OutputEmit => 4,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            PipelineStage::InputCapture => "input_capture",
            PipelineStage::DemodulateDecode => "demodulate_decode",
            PipelineStage::HpxStateUpdate => "hpx_state_update",
            PipelineStage::EncodeModulate => "encode_modulate",
            PipelineStage::OutputEmit => "output_emit",
        }
    }
}

/// Wire-level payload exchanged between codec/modulator stages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WirePayload {
    pub bytes: Vec<u8>,
}

/// Audio samples exchanged between audio/modem stages.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioSamples {
    pub samples: Vec<f32>,
}

/// Decoded frame payload passed to consumers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedFrame {
    pub sequence: u16,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackpressurePolicy {
    /// Producer waits for capacity.
    Block,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PipelineMetrics {
    pub enqueued: [u64; STAGE_COUNT],
    pub dequeued: [u64; STAGE_COUNT],
    pub dropped: [u64; STAGE_COUNT],
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StageMetrics {
    pub stage: String,
    pub enqueued: u64,
    pub dequeued: u64,
    pub dropped: u64,
    pub in_flight: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PipelineMetricsSnapshot {
    pub stages: Vec<StageMetrics>,
}

impl Default for PipelineMetrics {
    fn default() -> Self {
        Self {
            enqueued: [0; STAGE_COUNT],
            dequeued: [0; STAGE_COUNT],
            dropped: [0; STAGE_COUNT],
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PipelineMessage {
    Wire(WirePayload),
    Audio(AudioSamples),
    Decoded(DecodedFrame),
}

#[derive(Debug)]
struct StageQueue {
    sender: SyncSender<PipelineMessage>,
    receiver: Receiver<PipelineMessage>,
}

impl StageQueue {
    fn new(capacity: usize) -> Self {
        let (sender, receiver) = sync_channel(capacity);
        Self { sender, receiver }
    }
}

#[derive(Debug)]
pub struct PipelineScheduler {
    policy: BackpressurePolicy,
    input_capture: StageQueue,
    demodulate_decode: StageQueue,
    hpx_state_update: StageQueue,
    encode_modulate: StageQueue,
    output_emit: StageQueue,
    metrics: PipelineMetrics,
}

impl PipelineScheduler {
    pub fn new(capacity: usize, policy: BackpressurePolicy) -> Self {
        Self {
            policy,
            input_capture: StageQueue::new(capacity),
            demodulate_decode: StageQueue::new(capacity),
            hpx_state_update: StageQueue::new(capacity),
            encode_modulate: StageQueue::new(capacity),
            output_emit: StageQueue::new(capacity),
            metrics: PipelineMetrics::default(),
        }
    }

    pub fn route_wire(
        &mut self,
        stage: PipelineStage,
        payload: WirePayload,
    ) -> Result<WirePayload, PipelineError> {
        let out = self.route(stage, PipelineMessage::Wire(payload))?;
        match out {
            PipelineMessage::Wire(v) => Ok(v),
            _ => Err(PipelineError::MessageTypeMismatch),
        }
    }

    pub fn route_audio(
        &mut self,
        stage: PipelineStage,
        payload: AudioSamples,
    ) -> Result<AudioSamples, PipelineError> {
        let out = self.route(stage, PipelineMessage::Audio(payload))?;
        match out {
            PipelineMessage::Audio(v) => Ok(v),
            _ => Err(PipelineError::MessageTypeMismatch),
        }
    }

    pub fn route_decoded(
        &mut self,
        stage: PipelineStage,
        payload: DecodedFrame,
    ) -> Result<DecodedFrame, PipelineError> {
        let out = self.route(stage, PipelineMessage::Decoded(payload))?;
        match out {
            PipelineMessage::Decoded(v) => Ok(v),
            _ => Err(PipelineError::MessageTypeMismatch),
        }
    }

    pub fn metrics(&self) -> &PipelineMetrics {
        &self.metrics
    }

    pub fn metrics_snapshot(&self) -> PipelineMetricsSnapshot {
        let stages = [
            PipelineStage::InputCapture,
            PipelineStage::DemodulateDecode,
            PipelineStage::HpxStateUpdate,
            PipelineStage::EncodeModulate,
            PipelineStage::OutputEmit,
        ]
        .into_iter()
        .map(|stage| {
            let idx = stage.index();
            let enqueued = self.metrics.enqueued[idx];
            let dequeued = self.metrics.dequeued[idx];
            let dropped = self.metrics.dropped[idx];
            StageMetrics {
                stage: stage.as_str().to_string(),
                enqueued,
                dequeued,
                dropped,
                in_flight: enqueued.saturating_sub(dequeued.saturating_add(dropped)),
            }
        })
        .collect();

        PipelineMetricsSnapshot { stages }
    }

    fn route(
        &mut self,
        stage: PipelineStage,
        message: PipelineMessage,
    ) -> Result<PipelineMessage, PipelineError> {
        let idx = stage.index();
        match self.policy {
            BackpressurePolicy::Block => {
                {
                    let queue = self.queue(stage);
                    queue
                        .sender
                        .send(message)
                        .map_err(|_| PipelineError::SendFailure(stage))?;
                }
                self.metrics.enqueued[idx] += 1;

                let out = {
                    let queue = self.queue(stage);
                    queue
                        .receiver
                        .recv()
                        .map_err(|_| PipelineError::RecvFailure(stage))?
                };
                self.metrics.dequeued[idx] += 1;
                Ok(out)
            }
        }
    }

    fn queue(&mut self, stage: PipelineStage) -> &mut StageQueue {
        match stage {
            PipelineStage::InputCapture => &mut self.input_capture,
            PipelineStage::DemodulateDecode => &mut self.demodulate_decode,
            PipelineStage::HpxStateUpdate => &mut self.hpx_state_update,
            PipelineStage::EncodeModulate => &mut self.encode_modulate,
            PipelineStage::OutputEmit => &mut self.output_emit,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelineError {
    SendFailure(PipelineStage),
    RecvFailure(PipelineStage),
    MessageTypeMismatch,
}

impl std::fmt::Display for PipelineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PipelineError::SendFailure(stage) => {
                write!(f, "pipeline send failure at stage {stage:?}")
            }
            PipelineError::RecvFailure(stage) => {
                write!(f, "pipeline recv failure at stage {stage:?}")
            }
            PipelineError::MessageTypeMismatch => write!(f, "pipeline message type mismatch"),
        }
    }
}

impl std::error::Error for PipelineError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheduler_routes_wire_and_updates_metrics() {
        let mut scheduler = PipelineScheduler::new(4, BackpressurePolicy::Block);
        let payload = WirePayload {
            bytes: vec![1, 2, 3],
        };

        let out = scheduler
            .route_wire(PipelineStage::EncodeModulate, payload.clone())
            .expect("route wire");

        assert_eq!(out, payload);
        let m = scheduler.metrics();
        assert_eq!(m.enqueued[PipelineStage::EncodeModulate.index()], 1);
        assert_eq!(m.dequeued[PipelineStage::EncodeModulate.index()], 1);
    }

    #[test]
    fn scheduler_routes_audio_and_decoded() {
        let mut scheduler = PipelineScheduler::new(2, BackpressurePolicy::Block);

        let audio = AudioSamples {
            samples: vec![0.1, 0.2],
        };
        let decoded = DecodedFrame {
            sequence: 7,
            payload: vec![9, 9],
        };

        let audio_out = scheduler
            .route_audio(PipelineStage::InputCapture, audio.clone())
            .expect("route audio");
        let frame_out = scheduler
            .route_decoded(PipelineStage::HpxStateUpdate, decoded.clone())
            .expect("route decoded");

        assert_eq!(audio_out, audio);
        assert_eq!(frame_out, decoded);
    }
}