---
project: openpulsehf
doc: docs/raspberry-pi-4-5-tuning-and-benchmarks.md
status: living
last_updated: 2026-10-15
---

# Raspberry Pi 4/5 Tuning Guide and Benchmark Appendix

## Purpose

This guide gives practical tuning recommendations for OpenPulseHF on Raspberry Pi 4 and
Raspberry Pi 5, the numbers measured so far on the project's own Pi stations, and a benchmark
appendix template for repeatable reporting.

The project's on-air station pair runs on Pis: **rpi51 + IC-9700** (station A) and
**rpi53 + FT-818 + Yaesu SCU-17** (station B).

## Hardware assumptions

- Raspberry Pi 4 (4 GB or 8 GB) and Raspberry Pi 5 (4 GB or 8 GB)
- 64-bit Raspberry Pi OS
- stable power supply and adequate thermal solution
- USB audio interface (the radio's built-in USB CODEC or an external interface) for on-air use

## Performance goals

- keep real-time encode/decode stable under sustained load
- avoid audio underruns and overruns (XRUNs)
- keep PTT-to-first-sample short so the preamble is not clipped
- maintain deterministic benchmark runs for CI-comparable reports

## Baseline system preparation

1. Update OS and firmware.
2. Use the performance CPU governor during benchmark runs and on-air sessions.
3. Ensure thermal throttling is not active.
4. Disable non-essential background services on test images.

Suggested commands:

```sh
sudo apt-get update && sudo apt-get upgrade -y
sudo apt-get install -y alsa-utils libasound2-dev
echo performance | sudo tee /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor
vcgencmd get_throttled     # 0x0 = no throttling now or since boot
```

The sysfs governor setting does not survive a reboot; make it persistent with your distribution's
mechanism if you need it.

## Build and runtime tuning

### Build profile

- use release builds for all throughput or latency measurements
- keep profile settings consistent across compared runs
- **build audio-capable binaries on the Pi itself.** A cross-compiled aarch64 build
  (`cross`, `scripts/deploy-rpi-pair.sh`) is `--no-default-features`: it has no cpal backend and
  falls back to loopback, so it can neither transmit nor receive over a radio. The on-air runners
  build on the Pi over SSH for this reason.

```sh
# On the Pi
cargo build --release -p openpulse-cli                      # cpal-backend on by default
cargo build --release -p openpulse-daemon --features cpal    # openpulse-server
cargo build --release -p openpulse-ardop --features cpal    # openpulse-tnc

# On a workstation: compile check only, no audio
cross check --workspace --target aarch64-unknown-linux-gnu --no-default-features
```

### Audio/backend considerations

- the modem runs at 8 kHz; select a `plughw:CARD=...` device (or `pulse` where PulseAudio holds
  the CODEC) so ALSA converts the rate
- name the device explicitly: `--device` on the CLI, `[audio] device` for the daemon and the TNC.
  Use the stable `CARD=` form; an index like `hw:1,0` moves when cards are added
- the resolved device name is cached after the first open, so later opens skip the full device
  enumeration; the first open in a process still pays for it
- turn off any capture AGC on the USB interface and re-check after every replug (mixer state is
  not persistent)
- avoid device auto-switching during benchmark runs

### Threading and scheduling

- pin benchmark workload to isolated cores when possible
- avoid mixing benchmark execution or on-air sessions with heavy background compile jobs

Example affinity usage:

```sh
taskset -c 2,3 cargo test --workspace --no-default-features
```

## Measured on the project's stations

Key-to-first-sample (PTT edge to the first audio sample leaving the process), after the device-name
cache, 2026-10-14, five runs per station (source:
[docs/dev/design/key-to-audio-gap.md](dev/design/key-to-audio-gap.md)):

| | rpi51 + IC-9700 | rpi53 + FT-818 / SCU-17 |
|---|---|---|
| full device enumeration (first open) | 514–541 ms | 192–193 ms |
| key-to-first-sample, cached name | 99–101 ms (one run 228 ms) | 31 ms |
| before the cache | 567–684 ms | 239–256 ms |

The two stations differ because rpi51 lists 16 output devices and rpi53 lists 8; do not carry one
Pi's number to another. Set `[modem] ptt_leader_ms` from a measurement of the rig's own key-up
time, not from these figures (default 0).

Open (needs the rigs): the receive-path CPU cost on the Pi is due to be re-measured after the
receive changes since v0.16.0.

## Raspberry Pi 4 recommendations

- start with conservative concurrency settings for long runs
- monitor thermal state every 30-60 seconds
- prefer lower-buffer jitter settings over aggressive throughput targets in warm ambient conditions

Validation checklist:

- no thermal throttle flags during run
- no benchmark harness validation failures
- no regression script threshold violations

## Raspberry Pi 5 recommendations

- use the higher single-thread headroom for tighter latency targets
- still enforce thermal checks for long-duration stress scenarios
- use same scenario+seed policy as Pi 4 to keep comparisons valid

Validation checklist:

- sustained clock behavior within expected range
- deterministic scenario seeds in all scenario files
- stable pass/fail outcomes across at least 3 repeated runs

## Benchmark execution recipe

Run the CLI benchmark gate and the artifact and regression checks:

```sh
openpulse --backend loopback --log error benchmark run > /tmp/bench.json
jq '.passed == .total and .mean_transitions <= 20.0' /tmp/bench.json   # must print true

bash scripts/validate-benchmark-artifacts.sh
bash scripts/check-benchmark-regressions.sh benchmark/baselines benchmark/results/aggregate
```

Recommended per-platform run set (scenario files in `benchmark/scenarios/`):

1. HF500-NOM-01
2. HF500-FADE-02
3. HF500-BURST-03
4. HF2300-NOM-01
5. VHF-FM-NOM-01

## Benchmark appendix template

Use this section format in release PRs and performance reports.

### Environment metadata

- board: Raspberry Pi 4/5 model
- os_image: exact image build/version
- kernel: uname -r
- cooling: passive/active with fan profile
- power_supply: model and wattage
- audio_device: backend + hardware (exact `[audio] device` string)
- git commit of the binaries

### Aggregate results table template

| Platform | Scenario | Profile / level | Success Rate | Median Goodput (bps) | p95 Completion (ms) | Mean ARQ Efficiency |
|---|---|---|---:|---:|---:|---:|
| Pi 4 | HF500-NOM-01 | robust | 0.00 | 0 | 0 | 0.00 |
| Pi 4 | HF500-FADE-02 | robust | 0.00 | 0 | 0 | 0.00 |
| Pi 5 | HF500-NOM-01 | robust | 0.00 | 0 | 0 | 0.00 |
| Pi 5 | HF500-FADE-02 | robust | 0.00 | 0 | 0 | 0.00 |

Replace placeholder values with measured outputs from `benchmark/results/aggregate/*.json`.
HF500 scenarios fit the `robust` profile (≤500 Hz); use `fast` for the HF2300 scenarios.

### Regression verdict template

- baseline comparison script result: pass/fail
- goodput regression threshold violations: count
- p95 completion regression threshold violations: count
- operator notes: brief explanation for any deviation

## Quality gates for accepting Pi benchmark reports

- benchmark artifacts validated successfully
- baseline comparison executed with no unexpected threshold failures
- environment metadata included and complete
- at least one nominal and one adverse channel scenario reported per platform

## Open questions

- whether Pi-specific default runtime presets should be auto-selected by board detection
- whether thermal telemetry should be captured directly into benchmark result artifacts
