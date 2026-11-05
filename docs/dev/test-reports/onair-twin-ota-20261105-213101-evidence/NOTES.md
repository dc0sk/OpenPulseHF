# Task 25 Part A, run f2e — B sends, `fast` profile, 600 s, interval 8 s, build with the decline line
- rpi51 `e0832e49`; server `aefe38fb…`, CLI `27318f37…`, `twin-traffic.sh` `c9e152f3…` (sha256 prefixes, identical on both stations). AGC FAST on both rigs, FT-991A SQL 0, PREAMP 20, A's drive 0.15, `auto_id_signoff_idle_secs = 30` (`config-a.toml`, `config-b.toml`).
- Logging: station A `info,ofdm_plugin=debug`; station B `info,ofdm_plugin=debug,openpulse_daemon=debug` (a local `DAEMON_LOG_LEVEL_B` override of the runner, uncommitted; so is `SIGNOFF_IDLE_SECS`).
- Tag `f2e`. PTT/ALC watcher: `ptt-alc-watch.txt`; no keying stretch over 70 s. No RSPdx capture.
- Receive audio (8 kHz): rx-audio-2026-11-01/rx-{a,b}-f2e-8k.wav.gz, 1950 s each, started 20:30:15.13 (rpi51) UTC.
