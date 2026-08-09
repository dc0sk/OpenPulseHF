//! GPU compute acceleration for OpenPulse DSP kernels.
//!
//! Provides [`GpuContext`] which holds a wgpu device and pre-compiled compute
//! pipelines for BPSK modulation/demodulation, soft LLR demodulation, RRC FIR
//! convolution, and 256-point complex FFT. Construction is optional:
//! [`GpuContext::init`] returns `None` when no compatible GPU adapter is
//! available, allowing callers to fall back to the CPU path transparently.

pub mod demodulate;
pub mod fft256;
pub mod ldpc_bp;
pub mod modulate;
pub mod rrc_fir;
pub mod soft_demod;

pub use demodulate::{bpsk_iq_demod_gpu, timing_offset_search_gpu};
pub use fft256::gpu_fft256_batch;
pub use modulate::bpsk_modulate_gpu;
pub use rrc_fir::gpu_rrc_fir;
pub use soft_demod::gpu_soft_demod;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Cumulative wall-clock nanoseconds spent in GPU kernel calls, accumulated process-wide.
static GPU_BUSY_NANOS: AtomicU64 = AtomicU64::new(0);

/// Total nanoseconds spent in GPU kernel dispatches since process start (process-wide).
///
/// Read the delta across an interval to derive GPU-busy utilisation (busy_ns / interval_ns).
/// Stays 0 when no GPU kernels run — e.g. CPU-only builds, or when `GpuContext::init` returned
/// `None` and callers took the CPU fallback path.
pub fn gpu_busy_nanos() -> u64 {
    GPU_BUSY_NANOS.load(Ordering::Relaxed)
}

/// RAII guard that adds its lifetime to [`gpu_busy_nanos`]. One is placed at the top of each
/// GPU kernel entry point so the submit + readback wait time is accounted as GPU-busy.
pub(crate) struct GpuBusyTimer(std::time::Instant);

impl GpuBusyTimer {
    pub(crate) fn start() -> Self {
        Self(std::time::Instant::now())
    }
}

impl Drop for GpuBusyTimer {
    fn drop(&mut self) {
        GPU_BUSY_NANOS.fetch_add(self.0.elapsed().as_nanos() as u64, Ordering::Relaxed);
    }
}

/// Errors from GPU context initialisation.
///
/// The three init outcomes — disabled by `OPENPULSE_GPU_DISABLE`, no adapter, device-creation
/// failure — are all reported through these variants at the failure sites in `init`/`init_async`
/// (#1111). They are *logged*, not returned: `init` still yields `Option`, because "disabled" is a
/// deliberate choice rather than an error and a `Result` would misrepresent it. The distinction
/// that matters operationally is now visible, which it was not when all three collapsed to a silent
/// `None` — the condition that let #1080's kernel divergence run on hardware nobody expected.
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
    pub(crate) soft_demod_pipeline: wgpu::ComputePipeline,
    pub(crate) rrc_fir_pipeline: wgpu::ComputePipeline,
    pub(crate) fft256_pipeline: wgpu::ComputePipeline,
}

impl GpuContext {
    /// Attempt to initialise a GPU context.
    ///
    /// Returns `None` if no compatible adapter is available (e.g. headless CI).
    /// Blocks the calling thread while the wgpu async setup completes.
    pub fn init() -> Option<Arc<Self>> {
        if std::env::var("OPENPULSE_GPU_DISABLE").is_ok() {
            tracing::info!("GPU disabled by OPENPULSE_GPU_DISABLE; using the CPU path");
            return None;
        }
        pollster::block_on(Self::init_async())
    }

    async fn init_async() -> Option<Arc<Self>> {
        let instance = wgpu::Instance::default();
        // The three outcomes below used to be one `None` (#1111). The GPU path is default-on for the
        // daemon and linksim, so "no GPU here" and "a GPU started and may be producing different
        // numbers" are the two states that most need distinguishing on unfamiliar hardware — and
        // #1080 is the precedent, where the GPU path ran on an rpi that nobody expected to have an
        // adapter and two kernels diverged silently from CPU.
        let adapter = match instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
        {
            Some(a) => a,
            None => {
                tracing::info!("{}; using the CPU path", GpuError::NoAdapter);
                return None;
            }
        };

        let (device, queue) = match adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("openpulse-gpu"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::downlevel_defaults(),
                },
                None,
            )
            .await
        {
            Ok(dq) => dq,
            Err(e) => {
                // An adapter EXISTS and failed to start — materially different from having none,
                // and the case that warrants a warning rather than an info.
                tracing::warn!(
                    "{}; falling back to the CPU path",
                    GpuError::DeviceCreation(e.to_string())
                );
                return None;
            }
        };

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
        let soft_demod_pipeline = Self::make_pipeline(
            &device,
            include_str!("shaders/soft_demod.wgsl"),
            "soft-demod",
        );
        let rrc_fir_pipeline =
            Self::make_pipeline(&device, include_str!("shaders/rrc_fir.wgsl"), "rrc-fir");
        let fft256_pipeline =
            Self::make_pipeline(&device, include_str!("shaders/fft256.wgsl"), "fft256");

        Some(Arc::new(Self {
            device,
            queue,
            bpsk_mod_pipeline,
            bpsk_demod_pipeline,
            timing_search_pipeline,
            soft_demod_pipeline,
            rrc_fir_pipeline,
            fft256_pipeline,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The variant messages are operator-visible output now that `init` logs them (#1111), so a
    /// reword should be a deliberate act rather than a silent change to what a field report says.
    /// This also keeps the enum constructed: it was an orphan precisely because nothing built it.
    #[test]
    fn gpu_error_messages_are_pinned() {
        assert_eq!(GpuError::NoAdapter.to_string(), "no GPU adapter available");
        assert_eq!(
            GpuError::DeviceCreation("out of memory".into()).to_string(),
            "failed to create wgpu device: out of memory"
        );
    }

    /// `init` must honour the kill switch without touching wgpu at all — the one outcome that is a
    /// deliberate choice rather than a failure, and the reason `init` still returns `Option` rather
    /// than `Result`.
    #[test]
    fn init_respects_the_disable_env_var() {
        // Serialised against nothing else in this crate: no other test reads or writes this var.
        std::env::set_var("OPENPULSE_GPU_DISABLE", "1");
        let ctx = GpuContext::init();
        std::env::remove_var("OPENPULSE_GPU_DISABLE");
        assert!(
            ctx.is_none(),
            "OPENPULSE_GPU_DISABLE must suppress GPU init regardless of available hardware"
        );
    }
}
