---
project: openpulsehf
doc: docs/on-air_testplan.md
status: living
last_updated: 2026-07-19
---

# OpenPulseHF On-Air Test Plan

This document describes how two licensed amateur radio operators set up their stations,
verify the software stack, and execute a structured series of on-air tests using
OpenPulseHF. All tests should be completed before any public release or the Phase 5.5-reg
regulatory compliance report is filed.

**Status:** Pending station setup by both operators.  
**Prerequisite gate:** `cargo test --workspace --no-default-features` passes on both machines.  
**Execution order:** this document is the *matrix* (what to run). The *sequence* — which phase to run
first and why the receive path must be fixed before anything else — is in
[onair-execution-plan.md](dev/onair-execution-plan.md). Start there.

---

## 1. Participants and roles

| Role | Description |
|------|-------------|
| Station A (ISS) | Initiating station — sends messages, proposes connections |
| Station B (IRS) | Responding station — listens, receives, replies |

Both stations must hold valid amateur radio licences for the bands used. Agree on
operating frequencies before the session and coordinate by voice or a secondary
messaging channel (e.g. Winlink via `pat`).

---

## 2. Hardware prerequisites

Each station needs:

- A computer running Linux, macOS, or Windows with Rust installed
- A USB or built-in sound card connected to the transceiver's audio in/out
- A transceiver capable of SSB (USB) operation on the test bands
- PTT control: serial cable (RTS/DTR), VOX, or rigctld-compatible rig
- An antenna appropriate for each test band

Recommended audio interface: any interface with separate TX/RX audio and hardware PTT,
e.g. SignaLink USB, Digirig Mobile, or Yaesu SCU-17.

---

## 3. Software build (production binary)

CPAL audio support must be compiled in. Use the `cpal` feature flag:

```bash
# Build all binaries with real audio support
cargo build --release -p openpulse-kiss --features cpal
cargo build --release -p openpulse-ardop --features cpal
cargo build --release -p openpulse-gateway
cargo build --release -p openpulse-cli --features cpal-backend
```

The resulting binaries are in `target/release/`.

Verify audio devices are visible:

```bash
./target/release/openpulse devices
```

Expected output lists your sound card input and output devices by name.

Run local preflight checks before any live session:

```bash
./scripts/onair-preflight.sh --strict
```

All orchestration scripts support `--help` for usage details:

```bash
./scripts/onair-preflight.sh --help
./scripts/run-onair-tests.sh --help
./scripts/onair-bundle-evidence.sh --help
./scripts/run-onair-validation-flow.sh --help
```

This gate verifies required tooling, config presence, non-placeholder callsign,
and release binaries needed by the on-air matrix scripts.

The orchestrated matrix runner now executes this preflight by default:

```bash
./scripts/run-onair-tests.sh --quick
```

Use `--no-preflight` only when the preflight was already executed in the same shell session.

Generated JSON reports now include explicit preflight metadata:

```json
"preflight": {
  "ran": true,
  "mode": "strict"
}
```

For a full runbook execution in one command (matrix + evidence bundle + markdown report):

```bash
source config/onair-stations.sh
./scripts/run-onair-validation-flow.sh --quick --label 20m-bpsk250 --notes path/to/operator-notes.txt
```

This command writes all artifacts under `docs/dev/test-reports/` by default.

---

## 4. Station configuration

The first on-air pass uses two nearby stations with low RF power and inline
attenuation so the pair can be tested safely while still validating the full RF
path over the air.

Use the example configs under [docs/config/](config/README.md):

- [docs/config/onair-tx500-kx3.example.sh](config/onair-tx500-kx3.example.sh) — two-remote-SSH variant
- [docs/config/onair-tx500-kx3-local.example.sh](config/onair-tx500-kx3-local.example.sh) — KX3 local + TX500 on Pi via SSH
- [docs/config/openpulse-tx500.toml](config/openpulse-tx500.toml)
- [docs/config/openpulse-kx3.toml](config/openpulse-kx3.toml)

Generate a fresh runtime config on each station only if you want to start from the
binary defaults:

```bash
./target/release/openpulse config init > ~/.config/openpulse/config.toml
```

#### 4.1 Lab599 TX500 on Raspberry Pi via Digirig

This station is the Raspberry Pi side of the pair. Keep the transceiver in low-power
mode for the first run, connect the Digirig for audio/PTT, and insert external
attenuation between the two radios.

Example runtime config:

```toml
[station]
callsign = "TX500TEST"
grid_square = "AA00"

[audio]
backend = "cpal"

[modem]
mode = "BPSK250"
ptt_backend = "rigctld"

[radio]
rigctld_addr = "127.0.0.1:4532"

[radio.rig_a]
rigctld_addr = "127.0.0.1:4532"

[logging]
level = "debug"
```

#### 4.2 Elecraft KX3 on Linux laptop via Digirig

This station is the Linux laptop side of the pair. Use the Digirig for audio/PTT,
keep the RF output low for the first pass, and apply the same external attenuation
between both rigs.

Example runtime config:

```toml
[station]
callsign = "KX3TEST"
grid_square = "AA00"

[audio]
backend = "cpal"

[modem]
mode = "BPSK250"
ptt_backend = "rigctld"

[radio]
rigctld_addr = "127.0.0.1:4532"

[radio.rig_a]
rigctld_addr = "127.0.0.1:4532"

[logging]
level = "debug"
```

#### 4.3 SSH setup and supervision

Load your SSH keys first and confirm the agent is available:

```bash
ssh-add -l
```

**KX3 local + TX500 on Raspberry Pi (the primary test setup):**

```bash
# Fill in PI_SSH, callsigns, and serial ports, then:
source docs/config/onair-tx500-kx3-local.example.sh
./scripts/run-onair-tx500-kx3.sh supervise --quick --label tx500-kx3-first-run
```

This single command builds cpal-enabled binaries on both machines, starts rigctld,
tunes both rigs to `TEST_FREQ_HZ`, runs the test matrix, and writes a JSON report
plus evidence bundle.  Individual sub-commands are also available:

```bash
./scripts/run-onair-tx500-kx3.sh setup             # build binaries, start rigctld, tune rigs
./scripts/run-onair-tx500-kx3.sh run --full         # start rigctld, run full matrix, write report
./scripts/run-onair-tx500-kx3.sh status             # check both stations without side effects
./scripts/run-onair-tx500-kx3.sh cleanup            # kill rigctld and TNC processes
```

**Two-remote-SSH variant** (both stations on remote hosts):

```bash
source docs/config/onair-tx500-kx3.example.sh
./scripts/onair-tx500-kx3-supervisor.sh supervise --all --label tx500-kx3-first-run
```

The first pass should use the closest safe spacing, low power, and inline attenuation.
Once that passes, remove the extra attenuation only if you need to characterize the
link margin at a less constrained setup.

---

## 5. Verify audio path before going on-air

Before transmitting, verify that OpenPulseHF can modulate and demodulate through the
sound card using loopback mode (cable from TX audio out to RX audio in):

**Terminal 1 — start KISS TNC in loopback:**
```bash
./target/release/openpulse-kisstnc --backend loopback --mode BPSK250
```

**Terminal 2 — send a test KISS DATA frame:**
```bash
# KISS DATA frame: FEND(0xC0) + TYPE(0x00) + payload + FEND(0xC0)
python3 -c "
import socket, time
s = socket.socket()
s.connect(('127.0.0.1', 8100))
s.send(bytes([0xC0, 0x00]) + b'TEST' + bytes([0xC0]))
time.sleep(0.5)
s.close()
"
```

Switch to `cpal` backend and cable the audio path for a hardware loopback test:

```bash
./target/release/openpulse-kisstnc --backend cpal --mode BPSK250
```

Confirm the received frame appears in the TNC log at `DEBUG` level.

---

## 6. Test matrix

Agree on a test frequency for each band segment. Use USB (upper sideband). Dial frequency
is the carrier; OpenPulseHF places audio tones in the 300–2700 Hz audio passband.

Recommended test frequencies (check current band plans and IARU region):

| Band | Suggested dial (USB) | Max mode (baud-limited) | Notes |
|------|----------------------|-------------------------|-------|
| 40m  | 7.070 MHz            | BPSK250                 | 300-baud limit below 28 MHz (FCC) |
| 20m  | 14.070 MHz           | BPSK250                 | Same limit; busy Winlink band |
| 17m  | 18.100 MHz           | BPSK250                 | Quieter; good for initial tests |
| 10m  | 28.120 MHz           | QPSK500 / 8PSK500       | No baud limit; use for high-speed tests |

CEPT operators: QPSK500 and higher modes are permitted provided occupied bandwidth
stays within the authorised emission designator. Verify per national band plan.

### 6.1 Basic BPSK250 beacon exchange

**Goal:** Confirm a complete ISS→IRS B2F session over the air at the baseline mode.

**Station A (ISS):**
```bash
./target/release/openpulse-gateway \
  --callsign K1ABC \
  --host <Station-B-gateway-not-CMS> \
  send \
  --to K2DEF \
  --subject "OpenPulseHF on-air test 001" \
  --message "Hello from Station A. BPSK250 on-air session test."
```

*(For station-to-station tests, run `openpulse-tnc` on Station B and point the gateway
at its ARDOP address instead of cms.winlink.org.)*

**Station B (IRS):**
```bash
./target/release/openpulse-tnc \
  --mode BPSK250 \
  --backend cpal
```

**Pass criteria:**
- Station B log shows `CONNECTED` and decoded header with correct From/To/Subject
- Station A log shows `FS +` (accepted) with no errors
- No PTT overlap or session abort

### 6.2 Rate adaptation on `hpx_hf` under QRM

**Goal:** Verify the **`hpx_hf`** ladder climbs and demotes against a real fading HF channel — the
one claim in `release-1.0-criteria.md` group A that a simulator cannot make (criterion A2).

> **Why this replaces the old HPX500 procedure.** The previous version drove `openpulse-tnc --mode
> BPSK31` and expected `RateChange` events. That could not work: the TNC has no `--profile` flag,
> adaptive ARQ is off by default there, and HPX500 is not the ladder shipped since v0.14.0. It also
> started on uncoded BPSK31, which the fade-aware re-seat found decodes **0.00 at every SNR** on a
> Watterson `moderate_f1` fade — the entry rung was the first thing that arc fixed.

The adaptive path is `openpulse arq` (`send` / `listen`), which carries `--profile` and performs the
ACK-driven rate stepping. Both stations must build with real audio: `cargo build --release -p openpulse-cli
--features cpal-backend`.

**Station B (receiver, start first):**
```bash
RUST_LOG=info ./target/release/openpulse --backend cpal --ptt rigctld --rig 127.0.0.1:4532 \
  arq listen --profile hpx_hf --frames 20 --session onair-hpxhf
```

**Station A (sender):**
```bash
RUST_LOG=info ./target/release/openpulse --backend cpal --ptt rigctld --rig 127.0.0.1:4532 \
  arq send --profile hpx_hf --retries 5 \
  --payload "hpx_hf ladder test $(date -u +%H%M%SZ) de K1ABC"
```

Send repeatedly over a session long enough for propagation to change (30–60 min). To force a
demotion, introduce QRM (a carrier near the passband) or reduce power; remove it to allow recovery.

**The `hpx_hf` rungs**, so a log line can be read against the ladder:

| SL | Mode | Note |
|----|------|------|
| 1 | `MFSK16` | non-coherent sub-floor; ~17 s/frame |
| 2–5 | `BPSK31` → `BPSK63` → `BPSK100` → `BPSK250` | all **coded**; no uncoded rung survives a fade |
| 6 | `QPSK250-D` | **differential**; requires FEC, has no soft-LLR path |
| 7–11 | `OFDM52` → `-8PSK` → `-16QAM` → `-32QAM` → `-64QAM` | |
| 12–14 | `OFDM52-16QAM/-32QAM/-64QAM` | high-rate LDPC variants |

**Pass criteria:**
- At least one **climb** and one **demotion** logged, with the mode at each step matching the table
- The ladder reaches **SL6 (`QPSK250-D`) or above** on a workable path — SL6 is the rung the
  differential work exists for, and it has never been exercised on real audio or on air
- After a demotion, the session **recovers** rather than pinning at the floor
- No permanent session abort under moderate QRM

**Record for each transition:** UTC time, from-SL → to-SL, mode, the reported SNR, and whether the
step followed a decode success or a NACK. The SNR figures are **per-waveform-family**: single-carrier
rungs read near true channel SNR, OFDM rungs read a plugin-domain value that saturates around 16 dB.
Do not compare an OFDM reading against a single-carrier floor — that mismatch is by design and is
pinned by `snr_scale_boundary`.

### 6.3 Winlink CMS gateway via RF RMS

**Goal:** Send a message through a Winlink RMS (Radio Message Server) gateway on-air,
demonstrating real interoperability with the Winlink network.

**Prerequisites:**
- Identify the nearest HF RMS for the test band (see winlink.org/RMSlist)
- Both operators have Winlink accounts

**Station A:**
```bash
# Direct TCP to CMS — confirms the gateway binary works with real Winlink infrastructure
./target/release/openpulse-gateway \
  --callsign K1ABC \
  send \
  --to K2DEF \
  --subject "OpenPulseHF Winlink gateway test" \
  --message "This message was sent via OpenPulseHF direct TCP gateway."
```

For the over-RF path to an RMS, use `openpulse-tnc` as the TNC backend for `pat`:

```bash
# Start OpenPulseHF ARDOP TNC
./target/release/openpulse-tnc --mode BPSK250 --backend cpal

# In a second terminal, configure pat to use it
pat configure   # set ARDOP host to 127.0.0.1:8515/8516
pat connect ardop:///K1RMS   # replace K1RMS with the RMS callsign
```

**Pass criteria:**
- Message appears in Station B's Winlink inbox within 5 minutes
- `pat` session log shows clean ARDOP connect/disconnect cycle
- OpenPulseHF TNC log shows no B2F protocol errors

### 6.4 Dense-rung ladder walk (10 m only, CEPT or FCC above 28 MHz)

**Goal:** Confirm each dense `hpx_hf` rung produces a clean decoded frame on a real path, walking up
in order.

> **Why this replaces the old gateway loop.** The previous version ran `openpulse-gateway --mode
> $MODE`. `openpulse-gateway` has **no `--mode` flag**, no RF path at all (it is a direct TCP client
> for the Winlink CMS), and produces no TNC log — so the loop could not have run, and its mode list
> (`QPSK500 QPSK1000 8PSK1000 64QAM500 64QAM1000 64QAM2000-RRC`) matches **no rung of any current
> profile**. The dense rungs of `hpx_hf` are OFDM, not single-carrier QAM.

Walk the rungs with a fixed mode per step, so a failure identifies one waveform rather than a ladder
decision:

```bash
# Station B (receiver), one per mode:
./target/release/openpulse --backend cpal --ptt rigctld --rig 127.0.0.1:4532 \
  receive --mode "$MODE" --fec soft-concatenated --listen-ms 45000

# Station A (sender):
for MODE in OFDM52 OFDM52-8PSK OFDM52-16QAM OFDM52-32QAM OFDM52-64QAM; do
  echo "--- $MODE ---"
  ./target/release/openpulse --backend cpal --ptt rigctld --rig 127.0.0.1:4532 \
    transmit --mode "$MODE" --fec soft-concatenated \
    "dense rung test $MODE de K1ABC"
  sleep 15
done
```

**Note:** the dense OFDM rungs need a clean, strong path. Start at `OFDM52` and only proceed upward
while frames decode; `OFDM52-64QAM` occupies the full SSB passband, so confirm the transceiver's
audio bandwidth covers it before transmitting. Every rung is FEC-protected — running one uncoded is
not a valid test of it.

**Pass criteria:**
- Each attempted rung decodes at least one frame intact
- Throughput increases monotonically with rung on a path that carries them all
- A rung that fails does so by **not decoding**, not by an error or panic — record the RX log tail
- Report the highest rung that decoded, with the SNR at which it stopped working

### 6.5 Station identification compliance

**Goal:** Verify that the station ID interval requirement is met during long sessions.

Run a continuous receive session for 25 minutes. The transceiver should transmit the
station callsign in CW or voice at 10-minute intervals.

```bash
# Long-running IRS session with full protocol trace
RUST_LOG=debug ./target/release/openpulse-tnc --mode BPSK250 --backend cpal
```

Observe that the operator manually transmits station ID at least twice during the session.
Log the exact times in the compliance report.

**Pass criteria:**
- Station ID transmitted at T+0, T+10, T+20 minutes (at minimum)
- No transmission exceeds 10 minutes without an ID
- All IDs audible and readable to a monitoring station

---

## 7. Compliance checklist

Before filing the Phase 5.5-reg regulatory compliance report, confirm:

- [ ] Occupied bandwidth measured with an SDR or spectrum analyser for each mode
- [ ] BPSK31 ≤ 50 Hz, BPSK250 ≤ 260 Hz, QPSK500 ≤ 540 Hz, 64QAM500 ≤ 540 Hz, 64QAM2000-RRC ≤ 2700 Hz (±10%)
- [ ] Station ID transmitted at ≤10-minute intervals throughout all sessions
- [ ] No unattended automatic transmissions without valid automatic control authority
- [ ] Test log includes date, time, frequency, mode, power, and both callsigns
- [ ] QPSK500 and higher modes only used on bands without 300-baud symbol rate limits

---

## 8. Failure modes and diagnostics

| Symptom | Likely cause | Diagnostic command |
|---------|--------------|--------------------|
| No PTT | Wrong backend or serial port | `--backend loopback` to isolate; check `dmesg` for serial device |
| No decode at Station B | Audio level mismatch | Run `openpulse devices`, check input level with a VU meter |
| `FS -` every time | Mode mismatch, baud too high for propagation | Drop to `BPSK31`, check AFC offset in log |
| Session aborts mid-transfer | Read timeout (30s default) | Add `RUST_LOG=debug`; check if blobs are being sent |
| CMS rejects message | `N0CALL` callsign | Set `callsign` in config or pass `--callsign` |
| Winlink message not received | RMS gateway offline | Check winlink.org/RMSlist for gateway status |

Enable full protocol tracing:

```bash
RUST_LOG=openpulse_b2f=trace,openpulse_modem=debug ./target/release/openpulse-tnc --backend cpal
```

---

## 9. Recording results

For each test in Section 6, record:

```
Date/time (UTC):
Station A callsign + grid:
Station B callsign + grid:
Band + dial frequency:
Mode:
TX power (W):
Session result (pass/fail):
Notes:
```

Attach the `RUST_LOG=debug` output for any failed session.

Create a reproducible evidence bundle immediately after each run:

```bash
./scripts/onair-bundle-evidence.sh \
  --report docs/dev/test-reports/onair-<timestamp>.json \
  --notes path/to/operator-notes.txt \
  --label 20m-qpsk500
```

For compliance runs, use strict validation so the bundle fails fast if required artifacts are missing:

```bash
./scripts/onair-bundle-evidence.sh \
  --report docs/dev/test-reports/onair-<timestamp>.json \
  --require-report \
  --require-config \
  --require-preflight
```

The bundle is written under `docs/dev/test-reports/on-air/bundle-<utc>-<label>/` and includes:
- `metadata.json` (git SHA/branch, git dirty flag, host/user, optional preflight metadata extracted from report)
- `git-status.short.txt` (short `git status` snapshot for traceability)
- copied on-air report JSON
- config snapshot (`config.toml.snapshot`, when available)
- operator notes (when provided)

Generate a standardized Phase 5.5-reg report scaffold from the JSON and optional bundle metadata:

```bash
./scripts/onair-generate-report.sh \
  --report docs/dev/test-reports/onair-<timestamp>.json \
  --metadata docs/dev/test-reports/on-air/bundle-<utc>-<label>/metadata.json
```

By default, this writes to `docs/dev/test-reports/on-air/phase-5.5-reg-<timestamp>.md`.
