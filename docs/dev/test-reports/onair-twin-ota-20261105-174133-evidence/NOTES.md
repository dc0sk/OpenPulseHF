# Task 24 Part A, run f1d — A sends, `fast` profile, 600 s, interval 8 s, build with the fallback after silence
- rpi51 `a845ce45`; server `e74e6f4c…`, CLI `d8ef0861…`, `twin-traffic.sh` `c9e152f3…` (sha256 prefixes, identical on both stations). AGC FAST on both rigs, FT-991A SQL 0, PREAMP 20, A's drive 0.15, `auto_id_signoff_idle_secs = 30` (`config-a.toml`, `config-b.toml`), `DAEMON_LOG_LEVEL=info,ofdm_plugin=debug`. The runner's `SIGNOFF_IDLE_SECS` is a local, uncommitted edit.
- Tag `f1d`. PTT/ALC watcher (1 s polling): `ptt-alc-watch.txt`; no keying stretch over 70 s. No RSPdx capture.
- Receive audio (8 kHz): rx-audio-2026-11-01/rx-{a,b}-f1d-8k.wav.gz, 1950 s each, started 16:40:46.70 (rpi51) UTC.
