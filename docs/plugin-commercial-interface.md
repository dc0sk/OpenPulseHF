---
project: openpulsehf
doc: docs/plugin-commercial-interface.md
status: design-proposal
created: 2026-05-09
---

# Commercial and Third-Party Plugin Interface

## Problem statement

OpenPulseHF is licensed under GPL v3.  Any plugin linked into the modem engine as a Rust crate
is a derived work and inherits the GPL.  This prevents commercial organisations or operators
from distributing proprietary data-transfer plugins (e.g. a custom encryption layer, a
commercial compression library, or a proprietary HF waveform) without releasing their source.

**Regulatory note:** On amateur radio allocations, regulations including FCC §97.113(a)(4) and
their international equivalents prohibit transmissions whose purpose or content is obscured.
This includes payload encryption.  The integration paths below are intended for commercial,
maritime, aeronautical, and other non-amateur spectrum allocations.  Amateur operators must
verify compliance with their national regulatory framework before deploying any proprietary
data layer.

This document describes two legally distinct integration paths that allow non-GPL code to
interact with OpenPulseHF without triggering the copyleft clause.

---

## Approach A — Dynamic plugin loading via a C ABI shim (process-internal)

### How it works

Define a C-ABI interface (a `plugin_api.h` header and a corresponding Rust `plugin_host` crate
licensed under **LGPL v2.1 or later**).  A third party implements a shared library (`.so` /
`.dll` / `.dylib`) that exposes the C ABI.  The modem engine loads it at runtime via `libloading`.

Because the GPL may not reach across a C ABI boundary when the plugin is a **separate shared
library** loaded at runtime — an argument based on the aggregation exemption and system-library
provisions of GPL §5 — the plugin itself may remain proprietary.  This interpretation is widely
accepted but not universally settled; anyone distributing a commercial plugin under this approach
should obtain independent legal review before release.

### ABI surface (proposed)

```c
// plugin_api.h — LGPL v2.1
typedef struct OpPlugin {
    const char* name;             // null-terminated, UTF-8
    const char* version;          // semver string
    uint8_t     trait_api_major;  // must match host's expected major
} OpPlugin;

typedef struct OpSamples {
    const float* ptr;
    size_t       len;
} OpSamples;

typedef struct OpBytes {
    const uint8_t* ptr;
    size_t         len;
} OpBytes;

// Plugin entry point — exported from the shared library
const OpPlugin* op_plugin_info(void);

// Modulate: convert data bytes to audio samples.
// Caller provides out_samples and out_samples_cap (capacity in float elements).
// On success, writes the number of samples produced to *out_len.
int op_modulate(
    const uint8_t* data, size_t data_len,
    float* out_samples, size_t out_samples_cap, size_t* out_len,
    const char* mode_str,
    uint32_t sample_rate, float center_frequency
);

// Demodulate: convert audio samples to data bytes.
// Caller provides out_data and out_data_cap (capacity in bytes).
// On success, writes the number of bytes produced to *out_len.
int op_demodulate(
    const float* samples, size_t samples_len,
    uint8_t* out_data, size_t out_data_cap, size_t* out_len,
    const char* mode_str,
    uint32_t sample_rate, float center_frequency
);

// Optional: return per-bit LLRs for soft-decision FEC.
// Caller provides out_llrs and out_llrs_cap (capacity in float elements).
// On success, writes the number of LLR values produced to *out_len.
// If not implemented, return -1 (host falls back to hard ±1.0).
int op_demodulate_soft(
    const float* samples, size_t samples_len,
    float* out_llrs, size_t out_llrs_cap, size_t* out_len,
    const char* mode_str,
    uint32_t sample_rate, float center_frequency
);
```

### Rust host loader (`openpulse-plugin-host`, LGPL)

A new crate `crates/openpulse-plugin-host` provides:

```rust
pub struct DynPlugin { /* libloading::Library handle + resolved symbols */ }

impl ModulationPlugin for DynPlugin { /* delegates to C ABI */ }
```

The host crate is LGPL so that users can link against it without GPL implications.  The modem
engine remains GPL; the boundary is crossed only at runtime via the C ABI.

### Limitations

- The C ABI prevents zero-copy sample passing (one memcpy per TX/RX call).
- Panics in the plugin are undefined behaviour across the FFI boundary — the plugin must catch
  Rust panics (via `std::panic::catch_unwind`) or use C++ exceptions mapped to error codes.
- The plugin cannot access `HpxSession`, `FecCodec`, or any internal engine state.

### Implementation effort

~2 weeks:  define ABI header, write `openpulse-plugin-host`, write a sample skeleton plugin,
add integration test with a passthrough plugin.

---

## Approach B — Out-of-process plugin via local socket (IPC)

### How it works

The modem engine and the proprietary plugin run as **separate processes**.  They communicate over
a Unix domain socket (Linux/macOS) or named pipe (Windows) using a simple binary IPC protocol.
There is no shared address space, no GPL boundary crossing.

The IPC protocol is defined in a separate **BSD-licensed** specification document and reference
implementation.

### IPC wire format (proposed)

```
[4B: frame_type][4B: request_id][4B: payload_len][payload_len bytes]
```

| Frame type | Direction | Payload |
|---|---|---|
| `MODULATE_REQ (0x01)` | host → plugin | u32 sample_rate, f32 center_freq, u16 mode_len, mode bytes, data bytes |
| `MODULATE_RESP (0x02)` | plugin → host | u32 sample_count, f32[] samples |
| `DEMODULATE_REQ (0x03)` | host → plugin | u32 sample_rate, f32 center_freq, u16 mode_len, mode bytes, f32[] samples |
| `DEMODULATE_RESP (0x04)` | plugin → host | u8[] data bytes |
| `DEMODULATE_SOFT_RESP (0x14)` | plugin → host | f32[] LLRs (same length as DEMODULATE_REQ samples×2) |
| `ERROR (0xFF)` | plugin → host | u16 error_code, error_message bytes |

### Latency budget

At 8 kHz, one BPSK31 symbol is 256 samples = 32 ms audio.  Unix socket round-trip latency is
typically 50–200 µs on the same host — well within the 32 ms budget.  At 9600 baud (QPSK9600),
one symbol is 5 samples = 0.6 ms; IPC latency becomes marginal.  Approach B is therefore
**not recommended for high-baud modes** (≥ 2000 baud); use Approach A instead.

### Implementation effort

~1 week for the IPC codec and host adapter; the plugin author writes a server in any language.

---

## Approach C — Proprietary data layer over the open transport (no code change required)

OpenPulseHF transfers arbitrary byte payloads.  A commercial application can operate entirely
**above** the transport layer:

1. Encrypt / encode data before passing it to `openpulse-gateway` or the ARDOP data port.
2. Decrypt / decode on the receive side using a companion application.
3. The OpenPulseHF layer never sees the proprietary data format.

This requires **no changes to OpenPulseHF** and no LGPL/GPL analysis.  It is the recommended
path for commercial applications that treat OpenPulseHF as a transparent bearer.

### Existing integration points

| Interface | Suitable for | Notes |
|---|---|---|
| ARDOP TCP data port (8516) | General proprietary data | u16-BE length-prefixed frames |
| KISS TCP port (8100) | AX.25 payloads | KISS byte-stuffing, arbitrary PID |
| `openpulse-gateway` CLI | Winlink-style messages | `--message` from stdin or file |
| B2F driver library | Embedded Winlink clients | Rust API; compile against crate |

---

## Recommendation

| Use case | Recommended approach |
|---|---|
| Proprietary waveform plugin (baud ≤ 1000) | Approach A (C ABI, LGPL shim) |
| Plugin in non-Rust language, any baud | Approach B (IPC, BSD spec) |
| Proprietary data format over HF transport | Approach C (no code changes) |
| Commercial Winlink-compatible client | Approach C via ARDOP or B2F library |

Approach A should be prototyped first: it has the lowest latency, covers the primary use
case (proprietary modulation waveform), and the `openpulse-plugin-host` shim can be reused
for Approach B by wrapping the IPC adapter behind the same `ModulationPlugin` trait.

---

## Legal notes

This document is not legal advice.  The GPL v3 / LGPL analysis above reflects the common
understanding of the FSF's "dynamic linking" and "system library" exemptions.  Anyone distributing
a commercial plugin under Approach A should obtain independent legal review before release.

The `openpulse-plugin-host` crate's LGPL license must be confirmed with all contributors before
the crate is published.  A Contributor License Agreement (CLA) may be required.
