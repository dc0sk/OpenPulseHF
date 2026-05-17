---
project: openpulsehf
doc: docs/contributing-plugins.md
status: living
last_updated: 2026-04-24
---

# Contributing a Modulation Plugin

This guide explains how to create, test, and submit a new modulation plugin for OpenPulse.

## Overview

A modulation plugin implements the `ModulationPlugin` trait (defined in `openpulse-core`) to add support for a new waveform or mode (e.g., ARDOP, PSK125, FSK).

**Scope of a plugin**:
- Encode bytes → modulated audio samples (the `modulate` function)
- Decode audio samples → original bytes (the `demodulate` function)
- Declare supported modes, sample rates, and compatibility
- No direct filesystem, networking, or UI access (kept simple and sandboxed)

**Out of scope**:
- Error correction codes (separate layer; plugin consumes/produces raw bytes)
- Audio preprocessing or filtering (handled by audio backend)
- User interface (handled by CLI/TUI/GUI)
- Protocol-level state machines (handled by modem engine)

---

## Prerequisites

1. **Rust toolchain** (stable 2021 edition or later)
   ```bash
   rustup update stable
   cargo --version  # Should be 1.70+
   ```

2. **Familiarity with**:
   - Basic Rust (traits, error handling, Vec, f32 math)
   - Digital signal processing concepts (sampling, normalization, symbols)
   - The modulation scheme you're implementing (BPSK, FSK, OFDM, etc.)

3. **Access to OpenPulseHF repository**:
   ```bash
   git clone https://github.com/dc0sk/OpenPulseHF.git
   cd OpenPulseHF
   cargo build --features cpal-backend
   ```

---

## Step 1: Understand the Plugin Contract

### The `ModulationPlugin` Trait

```rust
pub trait ModulationPlugin: Send + Sync {
    /// Return static metadata about this plugin.
    fn info(&self) -> &PluginInfo;

    /// Encode data bytes into normalised audio samples (-1.0 … +1.0).
    fn modulate(
        &self,
        data: &[u8],
        config: &ModulationConfig,
    ) -> Result<Vec<f32>, ModemError>;

    /// Decode audio samples back to the original bytes.
    fn demodulate(
        &self,
        samples: &[f32],
        config: &ModulationConfig,
    ) -> Result<Vec<u8>, ModemError>;

    /// Check if this plugin handles a specific mode string.
    fn supports_mode(&self, mode: &str) -> bool { … }
}
```

### The `PluginInfo` Struct

```rust
pub struct PluginInfo {
    pub name: String,                    // e.g., "ARDOP"
    pub version: String,                 // semver, e.g., "1.0.0"
    pub description: String,             // one-liner
    pub author: String,                  // you!
    pub supported_modes: Vec<String>,    // e.g., ["ARDOP1200", "ARDOP2400"]
    pub trait_version_required: String,  // e.g., "1.0"
}
```

### The `ModulationConfig` Struct

```rust
pub struct ModulationConfig {
    pub center_frequency: f32,  // Hz (e.g., 1500)
    pub sample_rate: u32,       // Hz (e.g., 8000)
    pub mode: String,           // Mode selector (e.g., "BPSK31")
}
```

---

## Step 2: Create the Plugin Crate

### 2a. Create a new crate

```bash
cd plugins
cargo new mymode_plugin --lib
cd mymode_plugin
```

### 2b. Update `Cargo.toml`

```toml
[package]
name = "mymode_plugin"
version = "1.0.0"
edition = "2021"
authors = ["Your Name <email@example.com>"]
description = "Modulation plugin for MyMode."

[dependencies]
openpulse-core = { path = "../../crates/openpulse-core" }
serde = { version = "1", features = ["derive"] }

[dev-dependencies]
```

### 2c. Scaffold `src/lib.rs`

```rust
//! MyMode modulation plugin for OpenPulse.
//!
//! Supported modes: MyMode1200, MyMode2400

pub mod modulate;
pub mod demodulate;

use openpulse_core::error::ModemError;
use openpulse_core::plugin::{ModulationConfig, ModulationPlugin, PluginInfo};

/// MyMode plugin.
pub struct MymodePlugin {
    info: PluginInfo,
}

impl MymodePlugin {
    /// Create the plugin.
    pub fn new() -> Self {
        Self {
            info: PluginInfo {
                name: "MyMode".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: "MyMode modulation with [brief description]".to_string(),
                author: "Your Name".to_string(),
                supported_modes: vec![
                    "MyMode1200".to_string(),
                    "MyMode2400".to_string(),
                ],
                trait_version_required: "1.0".to_string(),
            },
        }
    }
}

impl ModulationPlugin for MymodePlugin {
    fn info(&self) -> &PluginInfo {
        &self.info
    }

    fn modulate(
        &self,
        data: &[u8],
        config: &ModulationConfig,
    ) -> Result<Vec<f32>, ModemError> {
        modulate::mymode_modulate(data, config)
    }

    fn demodulate(
        &self,
        samples: &[f32],
        config: &ModulationConfig,
    ) -> Result<Vec<u8>, ModemError> {
        demodulate::mymode_demodulate(samples, config)
    }
}

impl Default for MymodePlugin {
    fn default() -> Self {
        Self::new()
    }
}
```

---

## Step 3: Implement Modulation and Demodulation

### 3a. Create `src/modulate.rs`

```rust
use openpulse_core::error::ModemError;
use openpulse_core::plugin::ModulationConfig;

/// Modulate data bytes to audio samples.
pub fn mymode_modulate(
    data: &[u8],
    config: &ModulationConfig,
) -> Result<Vec<f32>, ModemError> {
    // 1. Parse mode-specific parameters (baud rate, etc.)
    let baud_rate = match config.mode.as_str() {
        "MyMode1200" => 1200.0,
        "MyMode2400" => 2400.0,
        _ => return Err(ModemError::Configuration(
            format!("Unsupported mode: {}", config.mode)
        )),
    };

    let sample_rate = config.sample_rate as f32;
    let samples_per_symbol = (sample_rate / baud_rate) as usize;

    // 2. Encode data to symbols (e.g., bits, QPSK symbols)
    let symbols = encode_data_to_symbols(data)?;

    // 3. Generate baseband (I/Q or real) waveform
    let baseband = symbols_to_samples(
        &symbols,
        config.center_frequency,
        sample_rate,
        samples_per_symbol,
    )?;

    // 4. Normalize to ±1.0 range
    let normalized = normalize_samples(&baseband);

    Ok(normalized)
}

fn encode_data_to_symbols(data: &[u8]) -> Result<Vec<f32>, ModemError> {
    // TODO: implement
    Ok(vec![])
}

fn symbols_to_samples(
    symbols: &[f32],
    center_freq: f32,
    sample_rate: f32,
    samples_per_symbol: usize,
) -> Result<Vec<f32>, ModemError> {
    // TODO: implement
    Ok(vec![])
}

fn normalize_samples(samples: &[f32]) -> Vec<f32> {
    if samples.is_empty() {
        return vec![];
    }
    let max = samples
        .iter()
        .map(|s| s.abs())
        .fold(0.0_f32, f32::max);
    if max > 0.0 {
        samples.iter().map(|s| s / max * 0.95).collect()
    } else {
        samples.to_vec()
    }
}
```

### 3b. Create `src/demodulate.rs`

```rust
use openpulse_core::error::ModemError;
use openpulse_core::plugin::ModulationConfig;

/// Demodulate audio samples back to data bytes.
pub fn mymode_demodulate(
    samples: &[f32],
    config: &ModulationConfig,
) -> Result<Vec<u8>, ModemError> {
    // 1. Parse mode-specific parameters
    let baud_rate = match config.mode.as_str() {
        "MyMode1200" => 1200.0,
        "MyMode2400" => 2400.0,
        _ => return Err(ModemError::Configuration(
            format!("Unsupported mode: {}", config.mode)
        )),
    };

    let sample_rate = config.sample_rate as f32;
    let samples_per_symbol = (sample_rate / baud_rate) as usize;

    // 2. Extract baseband (e.g., mix down from center frequency)
    let baseband = extract_baseband(samples, config.center_frequency, sample_rate)?;

    // 3. Clock recovery and symbol slicing
    let symbols = recover_symbols(&baseband, samples_per_symbol)?;

    // 4. Decode symbols back to data bytes
    let data = decode_symbols_to_data(&symbols)?;

    Ok(data)
}

fn extract_baseband(
    samples: &[f32],
    center_freq: f32,
    sample_rate: f32,
) -> Result<Vec<f32>, ModemError> {
    // TODO: implement (mix down, filter if needed)
    Ok(samples.to_vec())
}

fn recover_symbols(
    baseband: &[f32],
    samples_per_symbol: usize,
) -> Result<Vec<f32>, ModemError> {
    // TODO: implement (clock recovery, symbol slicer)
    Ok(vec![])
}

fn decode_symbols_to_data(symbols: &[f32]) -> Result<Vec<u8>, ModemError> {
    // TODO: implement
    Ok(vec![])
}
```

---

## Step 4: Write Tests

### 4a. Add tests to `src/lib.rs` or separate test module

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use openpulse_core::plugin::ModulationConfig;

    #[test]
    fn can_create_plugin() {
        let plugin = MymodePlugin::new();
        assert_eq!(plugin.info().name, "MyMode");
        assert!(plugin.supports_mode("MyMode1200"));
        assert!(!plugin.supports_mode("UNKNOWN"));
    }

    #[test]
    fn modulate_returns_samples() {
        let plugin = MymodePlugin::new();
        let config = ModulationConfig {
            center_frequency: 1500.0,
            sample_rate: 8000,
            mode: "MyMode1200".to_string(),
        };
        let data = b"Hello";
        let result = plugin.modulate(data, &config);
        assert!(result.is_ok());
        let samples = result.unwrap();
        assert!(!samples.is_empty());
        assert!(samples.iter().all(|s| s.abs() <= 1.0)); // Normalized
    }

    #[test]
    fn demodulate_rejects_empty_input() {
        let plugin = MymodePlugin::new();
        let config = ModulationConfig {
            center_frequency: 1500.0,
            sample_rate: 8000,
            mode: "MyMode1200".to_string(),
        };
        let result = plugin.demodulate(&[], &config);
        // Should return error or empty vec (design choice)
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn modulate_then_demodulate_roundtrip() {
        let plugin = MymodePlugin::new();
        let config = ModulationConfig {
            center_frequency: 1500.0,
            sample_rate: 8000,
            mode: "MyMode1200".to_string(),
        };
        let data = b"HELLO";

        // Modulate
        let samples = plugin.modulate(data, &config).expect("modulate");

        // Demodulate (in ideal conditions, should recover original)
        let recovered = plugin.demodulate(&samples, &config).expect("demodulate");

        // In practice, may need error correction; exact match not always possible
        assert!(!recovered.is_empty());
        // Optional: assert_eq!(recovered, data); if bit-perfect
    }
}
```

### 4b. Run tests

```bash
cd plugins/mymode_plugin
cargo test --lib
```

---

## Step 5: Integration with OpenPulse

### 5a. Update `plugins/Cargo.toml` (workspace)

If there's a workspace toml, add your plugin:

```toml
members = [
    "bpsk",
    "mymode_plugin",
]
```

### 5b. Update CLI to register your plugin

Edit `crates/openpulse-cli/src/main.rs`:

```rust
// Add import
use mymode_plugin::MymodePlugin;

// Register in engine setup
let mut engine = ModemEngine::new(audio);
engine
    .register_plugin(Box::new(BpskPlugin::new()))
    .context("failed to register BPSK plugin")?;
engine
    .register_plugin(Box::new(MymodePlugin::new()))
    .context("failed to register MyMode plugin")?;
```

Update `crates/openpulse-cli/Cargo.toml`:

```toml
[dependencies]
mymode_plugin = { path = "../plugins/mymode_plugin" }
```

### 5c. Test end-to-end

```bash
cd OpenPulseHF
cargo build --features cpal-backend
cargo run -p openpulse-cli -- transmit "TEST" MyMode1200
cargo run -p openpulse-cli -- receive MyMode1200
```

---

## Step 6: Documentation

Create `plugins/mymode_plugin/README.md`:

```markdown
# MyMode Plugin

Modulation plugin for MyMode (baud rates: 1200, 2400).

## Modes

| Mode | Baud rate | Description |
|------|-----------|---|
| `MyMode1200` | 1200 | Narrow-band |
| `MyMode2400` | 2400 | Wide-band |

## Encoding

[Brief description of wire format, preamble, FEC, etc.]

## Performance

- Processing latency: ~50 ms (target)
- CPU usage: ~10% on 2 GHz core (measured at 1200 baud)
```

Create `plugins/mymode_plugin/DESIGN.md` if the mode is complex:

```markdown
# MyMode Design

## Modulation scheme

[Detailed explanation: FSK/BPSK/OFDM, symbol mapping, etc.]

## Clock recovery

[Explanation of timing recovery algorithm]

## Error correction

[If applicable: FEC scheme, interleaving, etc.]
```

---

## Step 7: Testing Checklist

Before submitting, verify:

- [ ] Plugin crate compiles without warnings: `cargo build -p mymode_plugin`
- [ ] All unit tests pass: `cargo test -p mymode_plugin --lib`
- [ ] Plugin integrates with CLI: `cargo build -p openpulse-cli --features cpal-backend`
- [ ] Plugin is properly registered and can be enumerated
- [ ] Mode strings are recognized: `cargo run -p openpulse-cli -- receive MyMode1200` (should not error on unknown mode)
- [ ] Basic modulate/demodulate works: `cargo run -p openpulse-cli -- transmit "X" MyMode1200 && cargo run -p openpulse-cli -- receive MyMode1200`
- [ ] No unsafe code (unless documented and audited)
- [ ] Trait version compatibility is declared: `trait_version_required: "1.0"`

---

## Step 8: Submission Process

### 8a. Create a feature branch

```bash
git checkout -b feat/mymode-plugin
```

### 8b. Commit your work

```bash
git add plugins/mymode_plugin/
git commit -m "feat: add MyMode modulation plugin with 1200/2400 baud support"
```

### 8c. Push and open a pull request

```bash
git push -u origin feat/mymode-plugin
# Then create PR on GitHub, describing:
# - What modes are supported
# - Why this mode is useful for OpenPulse
# - Any performance or compatibility notes
```

### 8d. Address CI feedback

- Compilation must succeed on all platforms (Linux, macOS, Windows)
- Tests must pass: `cargo test -p mymode_plugin`
- Documentation must be clear
- Code style must follow Rust conventions (checked by `cargo fmt`)

### 8e. Merge

Once approved, your plugin is merged into main and becomes part of OpenPulse.

---

## Best Practices

### Audio normalization

Always normalize output samples to ±1.0 range:

```rust
let max_amplitude = samples.iter().map(|s| s.abs()).fold(0.0, f32::max);
let normalized: Vec<f32> = if max_amplitude > 0.0 {
    samples.iter().map(|s| s / max_amplitude * 0.95).collect()
} else {
    samples.to_vec()
};
```

### Error handling

Use `ModemError` for all error cases:

```rust
return Err(ModemError::Modulation(
    format!("Invalid mode: {}", mode)
));
```

### Performance considerations

- Pre-allocate buffers where possible
- Avoid unnecessary allocations in `modulate()` and `demodulate()`
- Minimize floating-point operations in inner loops
- Profile with `perf` or `cargo flamegraph` if needed

### Reusable components

If your plugin uses common DSP routines (FFT, filtering, resampling), consider contributing them to `openpulse-core` or a shared DSP library for future use.

### Testing

- Test edge cases: empty input, single byte, very long input
- Test all supported modes
- Test with multiple sample rates (8000, 16000, 44100, 48000)
- Test with different center frequencies (1500, 2000, 3000)
- Verify error handling for invalid configurations

---

## Common Pitfalls

| Pitfall | Solution |
|---|---|
| Forgetting to normalize samples to ±1.0 | Use a peak-detection + scaling approach |
| Using `panic!()` instead of `Result` | Always propagate errors via `ModemError` |
| Hardcoding center frequency or sample rate | Use `config` struct to stay flexible |
| Not testing demodulation of own output | Include roundtrip tests |
| Assuming 8000 Hz sample rate | Support multiple rates (configurable via config) |
| Not handling empty or very short input | Test boundary cases explicitly |
| Ignoring `supports_mode()` case sensitivity | Use case-insensitive comparison in trait impl |

---

## Examples

### BPSK Plugin (reference)

See `plugins/bpsk/` in the repository for a complete example:
- `src/lib.rs`: plugin struct and trait implementation
- `src/modulate.rs`: NRZI encoding, Hann windowing, mixer
- `src/demodulate.rs`: band-pass filter, symbol slicer, NRZI decoding
- `tests/`: integration tests

### FSK Plugin (hypothetical)

A future FSK plugin would:
- Map bits → frequency shifts (mark/space tones)
- Use Goertzel algorithm or FFT for demodulation
- Track phase transitions for clock recovery

---

## Getting Help

- **Questions about DSP**: OpenPulse Issues tracker
- **Rust syntax/compilation**: [Rust Book](https://doc.rust-lang.org/book/) or [rustlings](https://github.com/rust-lang/rustlings)
- **Cpal audio backend**: [cpal documentation](https://docs.rs/cpal/)
- **Modulation theory**: [GNU Radio tutorials](https://wiki.gnuradio.org/) or DSP textbooks

---

## License

Your plugin must be compatible with OpenPulseHF's license (see `LICENSE` file in the repository). Typically:
- Code: Apache 2.0 or GPL-compatible
- Documentation: CC-BY-4.0

---

