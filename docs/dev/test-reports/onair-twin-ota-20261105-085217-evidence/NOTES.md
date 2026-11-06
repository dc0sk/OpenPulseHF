# Task 23 Part A, run f2c — B sends, `fast` profile, 600 s, interval 8 s, notch-fix build
- Same build, settings and logging as f1c (`../onair-twin-ota-20261105-081132-evidence/NOTES.md`): rpi51 `5a3f6c5b`, `auto_id_signoff_idle_secs = 30` on both stations (`config-a.toml`, `config-b.toml`), `DAEMON_LOG_LEVEL=info,ofdm_plugin=debug`.
- Tag `f2c`. PTT/ALC watcher: `ptt-alc-watch.txt`; no keying stretch over 70 s. No RSPdx capture.
- Receive audio (8 kHz): rx-audio-2026-11-01/rx-{a,b}-f2c-8k.wav.gz, 1950 s each, started 07:51:31.07 (rpi51) UTC, B's start about 0.4 s later (`~/rec-b-f2c.txt` on station B).
- The link stalled at about 07:57:30 UTC; see the Task 23 results.
