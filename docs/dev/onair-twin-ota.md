---
title: On-Air Twin-OTA Scenario
status: living
last_updated: 2026-06-23
---

# On-air twin-OTA scenario

The real-radio counterpart of the in-process twin-station rig: two real
`openpulse-server` daemons on two RF stations, driving **receiver-led OTA adaptive
rate-stepping over the air**, observed live in `openpulse-twinview`. Where the
in-process rig (manual §13.2.A2) bridges two daemons through a channel model in
one process, here each daemon keys a real radio and the channel is the air.

This ties together the whole stack: the daemon OTA send path with PTT turnaround,
the receiver-led ladder, the spectrum/waterfall tap, the traffic generator, and
the combined both-directions viewer.

## Topology

```
 Station A (ISS)                 air                 Station B (IRS)
 openpulse-server  ── rig TX ─────────────► rig RX ── openpulse-server
   cpal + rigctld   ◄───────── rig TX ◄──── rig RX    cpal + rigctld
   (drives traffic)                                   (receiver-led ACKs)
        │ control :9000                                    │ control :9000
        └────────────── SSH-forwarded ──────────► openpulse-twinview ◄──┘
```

Station A sends data frames at its OTA mode; B decodes and ACKs with an absolute
`recommended_level`; A adopts it and its TX rate climbs (or falls) to track the
channel. Both directions' levels/spectra show side by side in twinview.

## Prerequisites

- Two stations, each with: a CAT/PTT-capable rig, `rigctld`/`rigctl` installed, a
  cpal-visible soundcard (e.g. a Digirig), and SSH access from your workstation.
- A clear, **legal** frequency for both stations and an agreed test window; keep
  power low (`A_RFPOWER`/`B_RFPOWER` default 0.10) for first passes.
- Real callsigns (the daemon refuses to start as `N0CALL`).

## Setup

```bash
cp docs/config/onair-twin-ota.example.sh ~/onair-twin-ota.sh
$EDITOR ~/onair-twin-ota.sh        # fill in SSH targets, callsigns, Hamlib models,
                                   # CAT/PTT ports, cpal device names, frequency
source ~/onair-twin-ota.sh
./scripts/run-onair-twin-ota.sh setup     # builds openpulse-server (cpal) + CLI on both
```

Find the values with: `rigctl -l | grep -i <rig>` (Hamlib model),
`ls /dev/serial/by-id/` (CAT/PTT ports), `openpulse devices` (cpal device name).

## Run

```bash
./scripts/run-onair-twin-ota.sh run        # rigctld → tune → write config → launch daemons → drive traffic
# (or `supervise` to do setup + run in one go)
```

The script starts `rigctld` on each station (handling **both** CAT frequency and
PTT), tunes both rigs, writes a per-station `config.toml` (cpal device,
`ptt_backend = rigctld`, `cat_backend = rigctld`, `ota_enabled = true`), launches
the daemons, then drives random-data traffic from A for `TRAFFIC_DURATION`
seconds while polling A's `ota-status` into a report under
`docs/dev/test-reports/`.

## Watch both directions live

The `run` action prints the exact commands; in short, forward each control port
and open the combined viewer on your workstation:

```bash
ssh -N -L 9000:127.0.0.1:9000 <A_SSH> &
ssh -N -L 9002:127.0.0.1:9000 <B_SSH> &
cargo run -p openpulse-twinview 127.0.0.1:9000 127.0.0.1:9002
```

Expect: A's **TX level** stepping up from the profile floor as B's recommendation
rises on a good channel (and backing off under fading); both columns showing live
spectrum + waterfall; PTT toggling per frame.

## Interpret / report

- The report logs A's `ota-status` every 5 s — watch `tx_level` climb.
- Per-station daemon logs: `/tmp/twin-ota-daemon.log` on each host.
- A flat ladder usually means the modes don't line up (check both `OTA_PROFILE`
  match) or the link is too poor to decode (raise power / pick a cleaner band /
  start on VHF line-of-sight before HF fading).

## Cleanup

```bash
./scripts/run-onair-twin-ota.sh cleanup    # stops daemons + rigctld on both stations
./scripts/run-onair-twin-ota.sh status     # check what's running
```

## Notes / caveats

- PTT turnaround on a real rig: the ISS keys PTT for the data frame and releases
  it to hear the ACK (the OTA send path); the IRS keys only to ACK. Verify your
  rig's PTT actually keys (RTS/DTR/CAT per `*_PTT_TYPE`) before trusting a flat
  ladder.
- Tick-vs-frame capture: the daemon receive tick reads soundcard windows; very
  long frames may need a couple of ticks to assemble. If decode is flaky on air,
  start with a slower `START_MODE`/profile and a clean channel.
- Regulatory: you are responsible for legal frequency, bandwidth, power, and
  identification — see [regulatory.md](../regulatory.md).
