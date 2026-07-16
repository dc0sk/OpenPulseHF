---
project: openpulsehf
doc: docs/dev/project/changelog.md
status: living
last_updated: 2026-07-15
---

# Changelog

> Phase/roadmap history lives in [roadmap.md](roadmap.md); this file tracks
> user-visible changes. "Unreleased" = merged to `main`, not yet in a tagged release.

## v0.9.0 — 2026-07-16

Second security release in the audit series — a fresh RX-decode and protocol-bridge sweep plus follow-ups.
Fixes one **CRITICAL** remote-panic DoS on the receive path, hardens the network-facing protocol bridges,
and lands the relay originator allow-list. The minor bump reflects the file-offer signature wire-format
change (below) and the new relay config; no config break.

### Security fixes

- **A crafted transmission could crash the receiver (CRITICAL, audit RX-1).** The short-FEC decode path
  (used by the OTA acknowledgement listener and short-FEC receive) passed attacker-length-controlled
  demodulator output to a Reed–Solomon decoder backed by a fixed 256-byte buffer, which **panics** on any
  input ≥ 256 bytes. Any station on-air could be crashed by transmitting enough audio. The decoder now
  rejects over-length input before it can panic. (#903)
- **Two more receive-path panics on 32-bit / WASM builds (audit RX-2/RX-3).** Length-prefix arithmetic in
  the FEC / convolutional / soft-Viterbi decoders could overflow a 32-bit index on a crafted frame and
  panic; now uses checked arithmetic. (#903)
- **The SAR reassembler is now bounded (audit RX-4).** A sender flooding distinct, never-completed segments
  could grow reassembly memory unbounded; capped at a fixed number of pending segments. (#903)
- **The ARDOP TNC could be driven out of memory (audit A-1).** A client that streamed bytes with no newline
  grew the command buffer without limit; the read is now length-bounded. (#901)
- **The Winlink gzip decompressor had no size cap (audit B-1).** A decompression bomb from a malicious CMS
  could allocate without bound (the LZHUF path was already capped); gzip is now capped too. (#901)
- **Unbounded B2F proposals (audit B-2).** A peer could make the receiver accept, decompress, and retain an
  unbounded number of messages per session; capped. (#901)
- **A signed file offer's metadata is now covered by its signature (audit F-2).** Previously only the
  content hash, size, and sender were signed; the filename, MIME hint, and block geometry rode along
  unauthenticated, so an on-path attacker could replay a legitimately-signed offer with a spoofed filename
  while it still showed as signature-valid. The offer now carries its own signature over the whole offer.
  (File content was already protected by the signed hash.) **Wire-format change** to what the offer
  signature covers; direct file transfer is off by default. (#900)

### Features

- **Relay originator allow-list (audit E1).** `[relay] allow_list` restricts an enabled relay to forwarding
  only frames from listed originator peer IDs (alongside the existing deny-list) — a defense-in-depth
  control for scoping a club/mesh relay to known stations. (#902)
- **The mesh can now carry control responses larger than one modem frame** by SAR-fragmenting and
  reassembling them. Transparent to current traffic; groundwork for future signed control messages. (#904)

### Known limitations (tracked)

- Envelope-level authentication of relayed traffic (route/query floods) remains future work: the modem's
  255-byte frame cap means a signed envelope must fragment, which needs a mesh reception-model change.
  Documented in `docs/dev/reviews/2026-07-15-handshake-trust-audit.md` (finding E1/E3).

Full audit write-ups: `docs/dev/reviews/2026-07-15-rx-decode-audit.md`,
`docs/dev/reviews/2026-07-15-protocol-bridge-audit.md`,
`docs/dev/reviews/2026-07-15-filexfer-relay-seams-audit.md`.

## v0.8.0 — 2026-07-15

Security release folding in three back-to-back adversarial audits of the handshake/trust, session, and
file-transfer subsystems. Fixes two CRITICAL and one SEVERE issue plus supporting hardening. Several fixes
deliberately change runtime behaviour (see Behaviour changes / migration in the [release notes](../../releasenotes.md)) —
hence the minor bump — but no wire-protocol or config break.

### Security fixes

- **A station could impersonate any trusted callsign (CRITICAL).** The classical signed handshake verified a
  CONREQ/CONACK signature against the frame's *own* public key and then consulted the trust store by
  callsign — but never checked that the frame key matched the trusted key for that callsign. Any station
  could therefore present its own key under a trusted callsign and be accepted at full trust, defeating the
  file-transfer signature gate. The frame key is now bound to the trusted key (mirroring the post-quantum
  path, which already did this). (#896)
- **A file offer could land far more data on disk than it declared (CRITICAL).** Received transfer blocks
  were never checked against the offer's declared size, so a small, quota-approved offer could write blocks
  that each expand to ~64 KB — up to gigabytes on disk, bypassing the file-size cap and per-peer quota. Each
  block is now rejected unless its decoded length matches the geometry the offer declared. (#898)
- **A file send to a silent peer locked up the whole transfer subsystem (SEVERE).** Transfer timeouts were
  implemented but never actually fired in the daemon, and there was no way to cancel an outbound send. A
  `send` to a peer that never answered — the norm on HF — pinned the subsystem until a restart, refusing
  every later send. Timeouts now fire each receive tick, and cancelling a transfer also cancels an outbound
  send. (#898)

### Fixes

- **A racing station could be recorded as the peer you dialled.** The connection initiator accepted a CONACK
  that merely echoed the (guessable, time-based) session id; it now also requires the reply to come from the
  callsign it actually dialled. (#896)
- **A no-callsign daemon kept the transmitter identified.** Autonomous responders (handshake reply, QSY, OTA
  acknowledgement, relay forward) no longer key the transmitter when no valid callsign is configured, so the
  station can never transmit unidentified (§97.119). (#897)
- **The QSY trust filter now works.** The frequency-move responder was pinned at "unverified", so a trust
  allowlist either did nothing or rejected everyone; it now reflects the peer's over-air trust. (#897)
- **A trusted signed offer is verified against its own sender's key**, not whoever completed a handshake most
  recently, so a legitimate offer is no longer checked against the wrong key on a multi-peer frequency. (#898)
- **A malformed peer-query response can no longer force a large allocation** from a tiny frame. (#898)
- **A received file can no longer overwrite an existing one** after a large number of same-name collisions;
  the write fails instead. (#898)
- **A trust store that fails to load now stops startup** instead of silently continuing with an empty store
  (which would have dropped revocations). (#896)
- **A transfer with the maximum block count no longer stalls** on its last block (a SAR id collided with the
  control channel). (#898)

### Behaviour changes

- A daemon configured with a trust-store path that can't be read now **refuses to start** (previously it
  started with an empty store). A *missing/unset* path is still fine.
- A daemon with no callsign (or `N0CALL`) **will not transmit** autonomous responses.
- `[discovery] group` is now documented as **reserved** — it was never wired, and setting it now logs a
  warning. The `@OPULSE` group is used regardless.

### Known limitations (tracked)

- A signed offer's filename/geometry are not yet covered by the signature (content is — the payload hash is
  signed), so an on-path attacker can spoof the displayed filename under a "verified" badge; closing this
  needs a manifest wire-format change (next cycle).
- Relay forwarding and OTA rate adoption still act on unauthenticated traffic when their (default-off)
  features are enabled — the signed handshake is an identity label, not an access gate.

Full audit write-ups: `docs/dev/reviews/2026-07-15-handshake-trust-audit.md` and
`docs/dev/reviews/2026-07-15-filexfer-relay-seams-audit.md`.

## v0.7.3 — 2026-07-15

Final hardening patch for the MFSK16 sub-floor ARQ rung — the last open finding from the audit. No breaking
changes; **all ARQ-seam audit findings are now addressed.**

### Fixes

- **Weak-rung acknowledgements now reach a peer running a different rate profile.** When a station drops to
  the MFSK16 sub-floor rung and answers with the robust three-copy acknowledgement, a peer whose profile
  doesn't include that rung previously couldn't decode it — its return channel went dark. The
  acknowledgement now leads with a short standard (FSK4) copy that any peer can hear, followed by the
  three-copy weak-signal version for a deep fade; the receiver acquires the leading copy out of the combined
  transmission. (#894)

The full audit and its resolutions are recorded in `docs/dev/research/mfsk16-arq-seam-audit.md`.

## v0.7.2 — 2026-07-15

Hardening patch for the MFSK16 sub-floor ARQ rung — the findings deferred from the v0.7.1 audit. No breaking
changes.

### Fixes

- **Anti-babble on the weak-signal ACK channel.** The receiver used to answer *every* undecodable burst with
  an acknowledgement; two adaptive stations (or repetitive co-channel QRM) could keep each other keying
  those replies. A consecutive-Nack budget now stops the negative acknowledgements after a few in a row (and
  resets on any real decode), so the station can't become a babbling transmitter — while a genuine
  retransmission still gets through, since the sender retries on its own timeout. (#892)
- **Cross-session acknowledgement filtering.** During the (up to 9 s) weak-rung ACK listen, a *different*
  station pair on the same frequency could have its acknowledgement adopted, silently marking the message
  delivered when the intended peer never got it. The sender now only accepts an acknowledgement carrying the
  addressed peer's session hash. (#892)
- A station whose ARDOP TNC is configured with an adaptive profile that includes the MFSK16 sub-floor rung
  now warns at startup that the rung is a background-daemon feature, not supported on the ARDOP adaptive
  path. (#892)

One known limitation (a rare mixed-profile acknowledgement blackout) remains tracked in
`docs/dev/research/mfsk16-arq-seam-audit.md`.

## v0.7.1 — 2026-07-15

Correctness patch for the v0.7.0 MFSK16 sub-floor ARQ rung. A 4-finder adversarial audit found the rung was
**non-functional on real (sound-card) hardware** — three independent breaks that every v0.7.0 test missed
because they shared masking artifacts (a buffered loopback, a 40 dB / level-locked twin test, a slope-only
SNR check). No breaking changes.

### Fixes

- **The weak-signal ACK is now capturable on real audio.** The sender's ACK-listen re-opened a fresh audio
  capture per read, discarding everything a real sound card buffered between reads — so the ~5 s three-copy
  ACK arrived as unusable fragments and never decoded off-loopback. It now holds one capture stream open for
  the whole listen. (#890)
- **The ACK decodes across turnaround timing at the rung's real SNR.** The copy-alignment step keyed on
  broadband energy, which at the sub-floor's low SNR just locked onto noise; the ACK then decoded for only
  ~28% of turnaround timings at the 0 dB design point. It now aligns on the waveform's own sync (Costas)
  acquisition, which is robust at low SNR. (#890)
- **The rung no longer immediately abandons itself.** The MFSK16 SNR estimate read ~21 dB too high, so the
  rate ladder always thought the link had recovered and jumped straight back to a mode that can't decode at
  that fade — bouncing off the sub-floor rung after every frame. The estimate is now on the true channel
  scale. (#890)
- **Oversized messages on the sub-floor rung fail loudly, not silently.** A message too large for the single
  small MFSK16 frame previously burned transmit airtime on a doomed larger-mode attempt and then dropped
  without a word; it now logs a clear "waiting for the link to climb off the sub-floor rung" and skips. (#890)
- **Removed an unsafe HARQ optimisation** for the sub-floor rung that could, in a corner case, combine soft
  data from an abandoned message into a later one. (#890)
- A malformed `[modem] ota_lock_level` now warns instead of silently leaving the station adaptive. (#890)

Known limitations tracked but not addressed in this patch are listed in `docs/dev/research/mfsk16-arq-seam-audit.md`.

## v0.7.0 — 2026-07-15

The `MFSK16` weak-signal waveform (shipped broadcast-only in v0.6.0) becomes a full **adaptive-ARQ sub-floor
rung**: the receiver-led OTA rate ladder now has a robust deep-fade rung *below* BPSK31. No breaking changes.

### Features

- **MFSK16 sub-floor ARQ rung (SL1)**: on the `hpx_hf` HF profile the rate ladder gains a non-coherent
  constant-envelope MFSK16 rung at **SL1** — the deep-fade rung the ladder drops to when the link falls
  below BPSK31's 3 dB floor. It carries data with RS FEC (one 255-byte block, ≤ 209 B/frame) and climbs
  back out to BPSK31 automatically once the SNR recovers past SL1's ceiling. (#886)
- **Robust K=3 union-decoded ACK**: the sub-floor rung can't be acknowledged over FSK4 (which dies far above
  the MFSK16 floor), so the receiver answers with **three time-spaced MFSK16-ACK copies** and the sender
  **union-decodes** them (decode each copy standalone, MAP-combine only as a fallback). Measured to clear
  ≥ 0.99 at 3 dB below the data floor where a single ACK held only ~0.6. The sender **union-listens** for
  both the FSK4 and the K=3 ACK on one window, so crossing the SL1 boundary can't desync the link. (#885,
  #886)
- **Payload-capacity guard**: a message larger than one MFSK16 frame (209 B) is transmitted on the next rung
  that fits, instead of hard-erroring and being silently dropped. (#887)
- **HARQ soft-combining across MFSK16 retransmissions**: failed sub-floor bursts are retained and
  MAP-combined with later ones, decoding more often than a single attempt on a faded channel. (#887)

### Notes

- The robust ACK was resolved by measurement: the earlier claim that the ACK was the binding constraint
  (~0.6 decode) was a **40-trial small-sample artifact** — at 400 trials it decodes ~0.9, and the winning
  fix (K=3 union, no frequency hop, stays 500 Hz) is cheap. The "longer contiguous frame" alternative was
  measured and *loses*. (#885)
- End-to-end validated across two real daemons (`twin_daemon_bridge::subfloor_sl1_message_crosses_with_k3_ack`),
  plus an `OTA_LOCK` knob on the snd-aloop real-audio rig for sound-card validation. (#888)

## v0.6.0 — 2026-07-15

Post-v0.5.0 block-B/D backlog plus the reference-derived requirements track (PTT backends, hotplug device
resolution, multi-mode monitor, AGC gate, the `MFSK16` weak-signal waveform). No breaking changes.

### Features

- **PTT watchdog preempts a blocked command loop**: the transmitter's max-keyed-duration force-release now
  runs on an independent watchdog thread, so it fires even while the daemon's async command loop is blocked
  inside a long handler (a QSY scan or an OTA send-retry burst) — the previous `select!`-arm watchdog (#853)
  could not, because the loop never re-enters `select!` during such a handler. A stuck rig (release keeps
  failing) is retried every tick and never falsely reported as released. (#863)
- **Transmitter-release RAII guard (unkey-on-Drop)**: every automatic transmit scope now releases PTT on
  scope exit — including on an early return or a panic/unwind — so an unexpected key-down is bounded to the
  current scope instead of waiting up to the 180 s watchdog. (REQ-PTT-01, #872)
- **CM108 USB-HID PTT backend** (`--ptt cm108` / `[modem] ptt_backend = "cm108"`): key PTT via the
  CM108/CM109/CM119 sound-chip GPIO on cheap USB interfaces (DMK URI, RA-series, AIOC, homebrew). A plain
  `/dev/hidrawN` write — no extra dependency; `ptt_device` selects the path (empty = auto-detect a C-Media
  device) and `ptt_gpio` the pin (default 3). (REQ-PTT-02)
- **GPIO-line PTT backend** (`--ptt gpio` / `[modem] ptt_backend = "gpio"`, `gpio` feature): key PTT via a
  Linux GPIO line (e.g. a Raspberry Pi header pin) over the `gpiocdev` char-dev uAPI; `ptt_device` carries
  a `chip:line[:active_low]` spec (e.g. `gpiochip0:17`). (REQ-PTT-03)
- **Daemon serial PTT**: the daemon now supports `rts`/`dtr` serial PTT (behind its new `serial` feature)
  using `[modem] ptt_device` as the port path, instead of silently disabling it.
- **Hotplug-safe audio device selection**: `[audio] device` is now resolved with a match ladder (exact
  name → ALSA `CARD=` token → case-insensitive substring, ambiguity is an error), so a device the OS
  renames or reorders (e.g. gains a `(2)` suffix, or its `hw:N` index shifts) still resolves instead of
  failing with `DeviceNotFound`. (REQ-DEV-01)
- **Simultaneous multi-mode receive** (`[monitor]`, off by default): the daemon can decode a list of extra
  modes from every capture burst in parallel with the active session, emitting a `MonitorFrame { mode,
  bytes }` event per decode — a monitor/discovery role for seeing what else is on frequency. (REQ-RX-01)
- **Receiver AGC config gate** (`[modem] agc_enabled`, off by default): the existing receiver AGC can now
  be enabled from the config file (with `agc_target_rms`/`agc_bandwidth`/`agc_max_gain_db`). Decode is
  already level-invariant, so the AGC stabilises the level through deep QSB and provides a metering
  readout rather than rescuing a decode. (REQ-AGC-01)
- **`MFSK16` weak-signal waveform** (`plugins/mfsk16`, mode `MFSK16`): a constant-envelope non-coherent
  16-GFSK sub-floor mode that decodes on deep-fade HF where coherent BPSK31 fails — measured ~4 dB better
  on moderate multipath and decoding on fast fade where BPSK31 fails entirely, at a PAPR credit. Registered
  in the CLI/daemon; usable now as a robust broadcast/beacon and explicit `--mode MFSK16` data mode. (The
  ARQ-rung integration — an MFSK16 ACK channel + ladder placement — is deferred.) (REQ-WSIG-01)
- **Mesh route discovery — source-accumulated multi-hop paths**: a `RouteDiscoveryRequest` now accumulates
  the traversed path as it floods (each forwarder appends itself), so the destination answers with the real
  end-to-end route instead of only `destination → [self]`. (#861)
- **KISS FullDuplex → CSMA**: the KISS `FullDuplex` control frame now toggles the engine's carrier-sense
  channel access (non-zero → full duplex → CSMA off; zero → CSMA on). The keying-delay control frames
  (TXDELAY/TXtail/P/SlotTime) remain no-ops — this TNC has no PTT-keying layer. (#862)

### Library

- **`ModemEngine::combine_and_decode_llrs`**: the audio-free union LLR decode (decode-each-alone, then
  MAP-combine) extracted from `receive_with_llr_combining` and made public — for an external diversity/HARQ
  combiner that has already demodulated its branches. Behaviour-preserving refactor. (#869)

### Notes

- **Weak-signal frequency-diversity rung: measured, not shipped.** A dual-carrier frequency-diversity rung
  (#864) was measured end to end (a ρ=0 ideal upper bound + the real-waveform net). The ideal cleared the
  kill-gate (~4 dB on slow fade), but the real waveform's ~2.6 dB two-tone PAPR consumes almost all of the
  ~1–2.6 dB matched-power gain → net on-air ≈ break-even at 2× bandwidth, dominated by the existing
  baud-drop and HARQ levers. The reproducible measurements and the analysis
  (`docs/dev/research/weak-signal-diversity-measurement.md`) landed; the rung did not. (#869)

## v0.5.0 — 2026-07-14

The 2026-07-13 "loose-ends" audit fix-down (issue #830, roadmap Phase 12): a 10-dimension refute-by-default
audit whose deferred tail was worked to completion. No breaking changes.

### Features

- **Route discovery — fully driven (0x03–0x08)**: the wire codecs had no driver. Added the request/response
  drive — a node originates a `RouteDiscoveryRequest`, answers when it is the destination or holds a cached
  route (self-authenticating Ed25519), and applies the `RouteDiscoveryResponse` into a bounded/TTL route
  table; the mesh daemon originates (`discover_route`), applies, and **consumes** a route for relay send
  (`send_via_route`, scored via `select_best_scored_route`). Plus the route-**maintenance** drive: signed
  `RelayRouteUpdate` (0x07, authoritative table refresh) and on-path-authorized `RelayRouteReject` (0x08,
  teardown only from a hop actually on the route), with `send_route_update`/`send_route_reject`. (#840,
  #841, #850, #856)
- **Per-band TX attenuation**: `SetTxAttenuation { band }` now honors the optional band — an engine-side
  per-band store applied on retune (mirrors per-band DCD squelch); a matching override wins on the current
  band. (#851)
- **PTT state resync**: a new `GetPttState` control command (and `openpulse daemon ptt-state`)
  re-broadcasts the current PTT state so a client that missed an edge can recover. (#843)
- **Declared TX power**: new `[station] tx_power_watts` config, recorded in the §97 regulatory TX log. (#849)

### Fixes

- **Regulatory (§97.119)**: the ARDOP TNC refuses on-air TX without a valid host `MYID` (host data / IRS
  ACK / auto-ID / relay), and the KISS TNC gates on the AX.25 source callsign per frame; the mesh daemon
  refuses to run as `N0CALL`, and the cross-band repeater station-IDs its transmitting rig. The regulatory
  TX-metadata log now records the operator callsign + declared power on the daemon/ARDOP/KISS/mesh paths
  (previously empty/0 W). `transmit_iq` is routed through the same compliance bookkeeping as the audio seam.
  (#847, #848, #827, #819, #849, #852)
- **DSP soft-LLR calibration**: 64QAM / OFDM / pilot / GPU soft demods emitted over-confident LLRs on dense
  grids (up to ~1599×); recalibrated from a known preamble/pilot residual plus a channel-estimation-error
  term — matters for HARQ combining. (#833, #834, #835, #837)
- **Robustness / concurrency**: the ARDOP CONNECT/DISCONNECT engine lock and the daemon PTT watchdog no
  longer block on / get starved by the async command loop (`spawn_blocking`; a dedicated watchdog
  `select!` arm; `biased` removed); the WebSocket control port fails closed when auth is required; the
  ARDOP data port no longer silently drops frames; the filexfer per-peer quota counts the `.partial`
  subtree; the InputCapture seam is not re-applied per decode-burst slice. (#846, #853, #817, #818, #820,
  #842, #826)
- **Discovery / JS8**: real off-air overs decode via a time search; the rendezvous timing/RxOnly cluster is
  fixed; `jsc_decompress` is guarded against a u32 overflow; the clock-skew TX gate is now live. (#814,
  #815, #816, #822)
- **Validation / correctness**: inconsistent file-offer geometry is rejected; `SetMode`/`SetConfig`
  validate before mutating shared state; the BPSK crossfade-ISI cancellation is kept off the soft
  (differential) path so it doesn't break HARQ LLR calibration. (#824, #823, #821, #832)

### Tests & docs

- New coverage for the command-path PTT hardware-failure guard, the discovery `server::run` handoffs
  (DCD-defer, dwell-tee, rendezvous-connect), and the daemon filexfer resume composition. (#836, #839,
  #845, #844)
- `docs/cli-guide.md` gains the daemon / FF-15 / FF-16 control CLI; the README `hpx_hf` ladder row, the
  panel mode list (12 PILOT modes), and roadmap Phase 12 are brought current. (#838, #828, #854, #857)

## v0.4.0 — 2026-07-12

- **JS8 station discovery + rendezvous (FF-15)**: a native JS8-compatible weak-signal waveform (8-GFSK, 79 symbols, Costas 3×7 sync, LDPC(174,87), CRC-12 — ported bit-exact from GPL-3.0 JS8Call, validated against compiled Boost/Qt5, **not** a JS8Call bridge) plus an idle-time discovery service in the new `crates/openpulse-discovery`. When enabled, an idle station QSYs to the current band's JS8 calling frequency, participates as a real JS8 station, marks itself with an in-band `@OPULSE` capability hint, and folds recognized OpenPulse stations into the shared `PeerCache`. Operator surface: `openpulse daemon {enable-discovery, disable-discovery, stations, peers}` + a panel `Tab::Discovery`; `[discovery]` config. **Beacon TX** (heartbeat + `@OPULSE` hint via a new `transmit_raw_audio` seam, Phase E) and **rendezvous → HPX handoff** (a 2-message Propose/Accept/Reject over JS8 directed free text → scheduled QSY → the signed CONREQ/CONACK handshake, Phase F, via the `RendezvousWith` control command) are **off by default**, gated behind `[discovery] mode = "beacon"`/`"full"` + a configured callsign + ±2 s clock-skew/DCD/self-ID gates; §97.221 automatic-control documentation in `docs/regulatory.md`. RX-only until opted in. Only on-air validation (Phase H) is deferred. (PRs #744–#805)
- **Direct P2P file transfer (FF-16)**: send a file to a connected peer over an RF session with an offer/accept handshake, progress, and size-gated auto-accept — plus an inline signed `TransferManifest` + SHA-256 verified against the peer's handshake key, so a tampered or wrong-key file is quarantined with an UNVERIFIED badge (verification VarAC's file transfer lacks). New `OPFX` wire (`crates/openpulse-filexfer`), files split into ≤48 KiB blocks over SAR (multi-megabyte transfers, config-capped at 1 MiB), hybrid delivery (OTA per-burst rate + block-ack bitmap selective retransmit) with block-level `.partial` resume and airtime-bounded PTT bursts. `[file_transfer]` config; `SendFile`/`AcceptFile`/`RejectFile`/`CancelFile`/`ListFiles` control + CLI; panel `Tab::Files`. On-air validation (Phase F) deferred. (PRs #730–#743, #787)
- **Adaptive rate ladder + DSP (signal-chain audit, Phase 11)**: the OTA rate ladder now climbs into the dense high-throughput rungs on good links — the M2M4 SNR estimator was waveform-blind and capped it mid-ladder (replaced by a per-plugin symbol-domain estimate that tracks to ~SL17). The dispersive-HF ladder (`hpx_hf`) is re-seated from SC-FDMA to **OFDM**, which measurably beats SC-FDMA on selective fading at matched rate; HARQ soft-LLR combining now engages across retransmissions in the daemon OTA path; and a batch of correctness fixes landed (inverted DFE feedback sign, AGC/DCD seam-ordering, SC-FDMA sync back-off / delay-cliff, CE-SSB whitening on low-entropy frames). Channel-model measurement fidelity was corrected (Watterson unity-power, Gilbert-Elliott per-symbol bursts, opt-in continuous fade) and a real-modem CI goodput-regression gate added. (PRs #697–#717)

## v0.3.0 — 2026-06-29

- **Security/Identity**: The daemon now performs the Ed25519 signed handshake over RF on connect — the initiator sends a signed `ConReq`, the responder verifies it and replies with a signed `ConAck`, and the initiator verifies that (both SAR-fragmented, since the frames exceed one modem frame). The verified peer callsign + Maidenhead grid are stored, a `PeerVerified` event is emitted, and the verified grid is written to the ADIF logbook (ahead of the `[logbook.peer_grids]` fallback). New `[station] identity_key_path`; 30 s handshake timeout (PR #584).
- **ARDOP TNC**: Opt-in adaptive ARQ session via `[ardop] enable_adaptive_arq` / `adaptive_profile`. With it on, the host `ARQBW` hint now caps the adaptive rate ladder by occupied bandwidth and `ARQTIMEOUT` drops an idle connection (both were accepted-and-echoed no-ops before). New rate-policy bandwidth-cap API (`set_arq_max_tx_level`), distinct from the OTA bounds (PR #585).
- **Radio/CAT**: The generic serial CAT backend is now selectable from the daemon for rigs Hamlib/rigctld doesn't support — `[radio] cat_backend = "generic"` with `serial_port` + `rig_file`, built with `--features generic-serial` (Unix). `RigctldController` gained its `CatController` impl (PR #586).
- **Logbook**: Automatic opt-in ADIF logbook — one record per contact (connect→disconnect); worked-station `GRIDSQUARE` from the verified handshake or a `[logbook.peer_grids]` config map; runtime `SetLogbook` toggle (CLI + panel).
- **Receiver auto-notch**: Productionized into the engine (multicarrier-aware, persistence, user controls); automatic QSY on a confirmed in-band interferer a notch can't remove; three seam-gap fixes from the notch-class audit and a single RX front-end seam.
- **Operator Panel**: AGC on/off toggle (PR #583); controls moved to a resizable right side-panel with a full-width waterfall and status below it (PR #579); `SetFreq` panel control; control-surface parity closed on both CLI and panel sides; `daemon set-tx-attenuation` (PR #587).
- **linksim**: I/Q constellation views with symbol-spaced (crisp-dot) sampling (PRs #574/#575), regrouped Station B views with waterfall/constellation toggles (PR #578), QR-branded info band, CE-SSB toggle, SNR plot, LDPC/Turbo/RS-Strong/Concatenated FEC modes, and a `--serve` mode so the panel attaches with no radio (PRs #580/#581).
- **Fix**: CE-SSB is gated off for dense OFDM higher-order modes (8PSK and above), where it caused a ~6 dB decode regression.
- **Docs**: Sorted `docs/dev/` into topic subfolders (`design/`, `pki/`, `research/`, `project/`) with all references updated (PR #582); manual + changelog + release notes brought current (PRs #588, #589).

## v0.2.2 — 2026-06-25

- **Drive tuning**: Live rig-meter polling (ALC / power-out / SWR) over a dedicated rigctld connection, surfaced as panel `RigStatus`; guided ALC drive tuning via `openpulse calibrate drive` (steps TX attenuation toward a target ALC band).
- **Tooling**: On-air SDR spectral-measurement script set and a one-shot twin-station demo.

## v0.2.1 — 2026-06-24

- **CE-SSB**: Controlled-envelope SSB TX conditioning (`openpulse_dsp::cessb`) — an adaptive, per-mode, default-on conditioner for the high-PAPR multicarrier modes (OFDM / SC-FDMA) that raises average TX power at fixed PEP. `[modem] cessb_enabled`, `SetCessb` control, `openpulse daemon set-cessb`, panel toggle. Channel-sim **+1.6 / +2.7 / +3.8 dB** on OFDM52 at zero BER cost; on-air confirmed **+1.18 dB** (FT-991A). Tests: `cessb_benefits_hold_on_ofdm_hom`, `cessb_acpr_spectral_regrowth`.
- **Operator Panel**: Messages presented as a tab alongside the Event Log.

## v0.2.0 — 2026-06-21

- **`openpulse-linksim`** (new crate): two-station ARQ link simulator proving the effective two-way transfer rate under simulated SNR / noise / fading — real forward frames + real FSK4 ACKs over a reverse channel, over-the-air rate adaptation, honest goodput accounting, compression modes; CLI sweep + GUI.
- **Signal-path testbench**: explicit 2×4 spectrum/waterfall grid (fixes unrendered waterfalls), all modes with measured per-mode bitrates, and new sources (virtual loop, dual-card hardware loop, test-matrix runner, adaptive ladder).
- **Bandplan Guardrails**: occupied-bandwidth coverage for active `-RRC` variants and `SCFDMA52-64QAM-P4` (no longer rejected as `UnknownOperatingMode`); `BandplanPolicy::default()` → `HamIaruRegion1`; Region 3 exposes a conservative-proxy warning.
- **Regulatory Logging**: `TxSessionLog::log_frame` rejects cross-station metadata.
- **Session Metrics**: throughput labeled as an upper-bound proxy with a dedicated `throughput_bps_note` field.
- **Waveform Validation**: BL-TP-7 SC-FDMA pilot-density Doppler review test (`plugins/scfdma/tests/pilot_density_review.rs`).
- **Performance**: cached benchmark corpus via `LazyLock` (PR #275); `qpsk-plugin` demod hot-path reduction (single-pass sin/cos + phase-step accumulation); `QPSK1000-HF` LMS pinned to `mu=0.015`.
- **Quality**: clippy `needless_borrow` fix and `HamIaru` → `HamIaruRegion1` in tests (PR #276); benchmark cached-corpus stability assertions.
- **Rate adaptation**: ACK-UP skips unmapped reserved profile rungs (e.g. HPX wideband SL9 → SL11); SNR-gated admission limited to HPX wideband-HD SL13 → SL14.
- **On-air tooling**: `onair-preflight.sh`, `run-onair-tests.sh` (default preflight), `onair-bundle-evidence.sh` — all with `--help`; preflight metadata in reports; strict validation flags; repo-state traceability (`git_dirty` + `git-status.short.txt`) in evidence bundles.

## 0.1.0

- Initial OpenPulseHF workspace with core modem architecture; BPSK plugin and CLI transmit/receive; audio backends (loopback + CPAL).

- Added `FecCodec` to `openpulse-core`: Reed-Solomon GF(2^8) codec (ECC_LEN=32, corrects up to 16 byte errors per 255-byte block).
- Added `ModemError::Fec` variant for FEC-specific error propagation.
- Added `ModemEngine::transmit_with_fec` and `receive_with_fec` for transparent FEC-protected transmission.
- Added FEC loopback hardening tests: 20-scenario fixture matrix (2 modes × 10 payloads) plus BER-injection correctness and capacity-exceeded failure tests.

- Added `qpsk-plugin` crate with Gray-mapped QPSK modulation and demodulation.
- Registered QPSK plugin in CLI engine, exposing modes `QPSK125`, `QPSK250`, and `QPSK500` via `openpulse modes`.
- Added QPSK loopback fixture matrix (3 modes × 14 payload profiles = 42 scenarios).
- Added spectral efficiency benchmarks confirming QPSK250 carries more bits per sample than BPSK250 at equal baud rate.

- Added documentation framework with standardized frontmatter.
- Added docs CI checks and automated last_updated stamping for pull requests.
- Expanded `openpulse-modem` BPSK hardening coverage with a deterministic
  loopback fixture matrix executing 56 scenarios across supported modes and
  payload profiles.
- Strengthened `openpulse-modem` structured HPX event logging so diagnostic
  entries preserve `event_source`, `session_id`, and `reason_string`, and
  transition events are counted consistently in session diagnostics.
- Improved `openpulse session state --diagnostics` output so text mode renders
  a readable summary plus event lines while JSON mode keeps the raw structured
  diagnostics payload and uses persisted peer context when available.

### HPX conformance & session audit (2026-04-25)

- Added 10 HPX spec conformance integration tests in `openpulse-modem` covering
  all major state-machine paths (happy path, timeouts, signature rejection,
  quality recovery, ARQ exhaustion, local/remote teardown, relay activation).
- Fixed missing `RelayActive + TrainingOk → ActiveTransfer` state-machine transition
  in `openpulse-core::hpx` required by the relay conformance scenario.
- Added `hpx_session_id()` and `hpx_transitions()` public accessors to `ModemEngine`.
- Added `POST /api/v1/session-audit-events` endpoint to `pki-tooling` that validates
  and persists HPX transition logs to the `audit_events` table.
- Added `PkiClient::create_session_audit_event` and `record_handshake_session_audit`
  to the CLI, wiring `diagnose handshake` to post audit events on every execution.
- Added `openpulse session` CLI subcommand group with four commands:
  `start`, `state`, `end`, and `log`, exposing the full HPX lifecycle through the CLI.
- Added 5 integration tests for the `session` command group using mockito.
- Added `live_pki_integration.rs` test suite that spins up the real `pki-tooling`
  axum router on a random TCP port and validates CLI commands end-to-end against
  a live Postgres database (skips gracefully when `PKI_TEST_DATABASE_URL` is unset).
