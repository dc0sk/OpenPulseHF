//! GPU compute acceleration for OpenPulse DSP kernels.
//!
//! Provides [`GpuContext`] which holds a wgpu device and pre-compiled compute
//! pipelines for BPSK modulation and demodulation. Construction is optional:
//! [`GpuContext::init`] returns `None` when no compatible GPU adapter is
//! available, allowing callers to fall back to the CPU path transparently.

pub mod demodulate;
pub mod modulate;

pub use demodulate::{bpsk_iq_demod_gpu, timing_offset_search_gpu};
pub use modulate::bpsk_modulate_gpu;

use std::sync::Arc;

/// Errors from GPU context initialisation.
#[derive(Debug, thiserror::Error)]
pub enum GpuError {
    #[error("no GPU adapter available")]
    NoAdapter,
    #[error("failed to create wgpu device: {0}")]
    DeviceCreation(String),
}

/// Shared GPU context holding a device, command queue, and pre-compiled pipelines.
///
/// Create with [`GpuContext::init`]. The returned `Arc` can be shared across
/// plugin instances (e.g. `BpskPlugin::with_gpu`).
pub struct GpuContext {
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
    pub(crate) bpsk_mod_pipeline: wgpu::ComputePipeline,
    pub(crate) bpsk_demod_pipeline: wgpu::ComputePipeline,
    pub(crate) timing_search_pipeline: wgpu::ComputePipeline,
}

impl GpuContext {
    /// Attempt to initialise a GPU context.
    ///
    /// Returns `None` if no compatible adapter is available (e.g. headless CI).
    /// Blocks the calling thread while the wgpu async setup completes.
    pub fn init() -> Option<Arc<Self>> {
        pollster::block_on(Self::init_async())
    }

    async fn init_async() -> Option<Arc<Self>> {
        let instance = wgpu::Instance::default();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await?;

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("openpulse-gpu"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::downlevel_defaults(),
                },
                None,
            )
            .await
            .ok()?;

        let bpsk_mod_pipeline = Self::make_pipeline(
            &device,
            include_str!("shaders/bpsk_modulate.wgsl"),
            "bpsk-mod",
        );
        let bpsk_demod_pipeline = Self::make_pipeline(
            &device,
            include_str!("shaders/bpsk_demodulate.wgsl"),
            "bpsk-demod",
        );
        let timing_search_pipeline = Self::make_pipeline(
            &device,
            include_str!("shaders/timing_search.wgsl"),
            "timing-search",
        );

        Some(Arc::new(Self {
            device,
            queue,
            bpsk_mod_pipeline,
            bpsk_demod_pipeline,
            timing_search_pipeline,
        }))
    }

    fn make_pipeline(device: &wgpu::Device, wgsl: &str, label: &str) -> wgpu::ComputePipeline {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(label),
            source: wgpu::ShaderSource::Wgsl(wgsl.into()),
        });
        device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(label),
            layout: None,
            module: &shader,
            entry_point: "main",
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        })
    }
}
