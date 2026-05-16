---
project: openpulsehf
doc: docs/on-air_testplan.md
status: living
last_updated: 2026-05-16
---

# OpenPulseHF On-Air Test Plan

This document describes how two licensed amateur radio operators set up their stations,
verify the software stack, and execute a structured series of on-air tests using
OpenPulseHF. All tests should be completed before any public release or Phase 3.5
regulatory compliance report is filed.

**Status:** Pending station setup by both operators.  
**Prerequisite gate:** `cargo test --workspace --no-default-features` passes on both machines.

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

This gate verifies required tooling, config presence, non-placeholder callsign,
and release binaries needed by the on-air matrix scripts.

---

## 4. Station configuration

Generate a base config file:

```bash
./target/release/openpulse config init > ~/.config/openpulse/config.toml
```

Edit `~/.config/openpulse/config.toml` on each station:

```toml
[station]
callsign = "YOUR_CALLSIGN"   # e.g. "K1ABC"
grid_square = "FN42"

[audio]
# "default" uses cpal when compiled in, loopback otherwise.
# Set to "cpal" to require real hardware or "loopback" for software-only testing.
backend = "default"

[modem]
mode = "BPSK250"             # starting mode; override per test
ptt_backend = "rts"          # rts | dtr | vox | rigctld | none

[logging]
level = "debug"              # use debug during testing for full protocol trace
```

PTT wiring examples:

```toml
# Serial cable — RTS line asserts PTT
ptt_backend = "rts"

# VOX (transceiver detects audio)
ptt_backend = "vox"

# Hamlib rigctld (start rigctld separately before the TNC)
ptt_backend = "rigctld"
# Also set in [modem] section:
# rigctld_addr = "127.0.0.1:4532"
```

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

### 6.2 Rate adaptation under QRM

**Goal:** Verify HPX500 adaptive profile climbs and falls with changing propagation.

**Station A:**
```bash
RUST_LOG=debug ./target/release/openpulse-tnc \
  --mode BPSK31 \
  --backend cpal
```

Start at SL2 (BPSK31) and observe `RateChange` events in the log as Station B sends
NACKs/ACKs. Introduce deliberate QRM (tune a second receiver near the frequency) to
force rate down; remove it to allow rate recovery.

**Pass criteria:**
- At least one `RateChange` event logged on each side
- Session recovers to SL4+ (BPSK250 or better) when channel clears
- No permanent session abort under moderate QRM

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

### 6.4 Multi-mode quality ladder (10m only, CEPT or FCC above 28 MHz)

**Goal:** Exercise HPX2300 (SL8–SL11) and HPX Wideband HD (SL12–SL14) speed levels in order.

Run from SL8 to SL14 manually to confirm each mode produces a clean demodulated frame:

```bash
for MODE in QPSK500 QPSK1000 8PSK1000 64QAM500 64QAM1000 64QAM2000-RRC; do
  echo "--- Testing $MODE ---"
  ./target/release/openpulse-gateway \
    --callsign K1ABC \
    send \
    --to K2DEF \
    --mode $MODE \
    --subject "Mode test: $MODE" \
    --message "Signal quality check at $MODE"
  sleep 10
done
```

**Note:** 64QAM modes require approximately 20–25 dB SNR for reliable operation.
Test 64QAM500 first; proceed to 64QAM1000 and 64QAM2000-RRC only when SNR is adequate.
64QAM2000-RRC occupies the full 2700 Hz SSB passband — confirm your transceiver's audio
bandwidth covers this before transmitting.

**Pass criteria:**
- Each mode produces a successful session (no `FS -`)
- `64QAM2000-RRC` throughput is measurably higher than `8PSK1000` in the TNC log
- AFC offset reported within ±10 Hz for all modes
- 64QAM modes degrade gracefully (fall back to lower SL) when SNR drops

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

Before filing the Phase 3.5 regulatory compliance report, confirm:

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
