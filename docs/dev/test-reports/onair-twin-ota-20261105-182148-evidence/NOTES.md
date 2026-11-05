# Task 24 Part A, run f2d — B sends, `fast` profile, 600 s, interval 8 s, build with the fallback after silence
- Same build, settings and logging as f1d (`../onair-twin-ota-20261105-174133-evidence/NOTES.md`): rpi51 `a845ce45`, `auto_id_signoff_idle_secs = 30` on both stations (`config-a.toml`, `config-b.toml`), `DAEMON_LOG_LEVEL=info,ofdm_plugin=debug`.
- Tag `f2d`. PTT/ALC watcher: `ptt-alc-watch.txt`; no keying stretch over 70 s. No RSPdx capture.
- Receive audio (8 kHz): rx-audio-2026-11-01/rx-{a,b}-f2d-8k.wav.gz, 1950 s each, started 17:21:01.82 (rpi51) UTC.
- The first copy of B's recording was cut short when `/tmp` (a tmpfs) filled; it was copied again from station B and converted again, and both files pass `gzip -t`.
