# Task 23 Part A, run f1c — A sends, `fast` profile, 600 s, interval 8 s, notch-fix build
- rpi51 commit `5a3f6c5b` (notch fix `79fa3318`); server `8630d843…`, CLI `89faacd3…`, `twin-traffic.sh` `c9e152f3…` (sha256 prefixes, identical on both stations). AGC FAST on both rigs, FT-991A SQL 0, PREAMP 20, A's drive 0.15. `DAEMON_LOG_LEVEL=info,ofdm_plugin=debug`.
- `auto_id_signoff_idle_secs = 30` on both stations (`config-a.toml`, `config-b.toml`). The runner has no setting for it; a local, uncommitted `SIGNOFF_IDLE_SECS` variable wrote it.
- Tag `f1c`. PTT/ALC watcher (1 s polling): `ptt-alc-watch.txt`; no keying stretch over 70 s. No RSPdx capture.
- Receive audio (8 kHz): rx-audio-2026-11-01/rx-{a,b}-f1c-8k.wav.gz, 1950 s each, started 07:10:45.28 (rpi51) / 07:10:45.65 (station B), UTC.
- At this log level the daemon does not log `ACK not received` (a DEBUG line in f2b) or `fec attempt`; decodes and decisions come from the `frame_received` and `ota_rate_decision` events.
