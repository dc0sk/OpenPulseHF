//! Pipeline boundary types for modem stage decomposition.
//!
//! These types make stage interfaces explicit so execution can be moved
//! to threaded workers later without changing higher-level call sites.

/// Logical modem pipeline stages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineStage {
    InputCapture,
    DemodulateDecode,
    HpxStateUpdate,
    EncodeModulate,
    OutputEmit,
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