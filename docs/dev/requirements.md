---
project: openpulsehf
doc: docs/dev/requirements.md
status: living
last_updated: 2026-07-30
---

# Requirements

> **Numbered REQ-IDs and end-to-end traceability** (requirement → capability → design
> decision → implementation → tests → result → assets → PRs) live in
> [traceability-matrix.md](project/traceability-matrix.md). Each requirement below maps to a
> `REQ-<CAT>-NN` ID in that matrix's Requirements table.

## Functional requirements

- **REQ-FUN-01** — Provide a CLI capable of transmit, receive, device listing, and mode listing.
- **REQ-FUN-02** — Support at least one production modulation plugin (BPSK family).
- **REQ-FUN-03** — Preserve a loopback backend for hardware-free development and testing.
- **REQ-FUN-04** — Support cross-platform audio through CPAL-backed implementations.
- **REQ-FUN-05** — Validate frame integrity with versioning, sequence handling, and CRC checks.
- **REQ-FUN-06** — Define and implement a high-performance plugin mode (HPX) with adaptive modulation and coding.
- **REQ-FUN-07** — Support occupied bandwidth classes centered on 500 Hz and 2300-2400 Hz operation.
- **REQ-FUN-08** — Provide deterministic session state handling: discovery, training, active transfer, recovery, teardown.
- **REQ-FUN-09** — Support selective retransmission for ARQ-capable sessions.
- **REQ-FUN-10** — Support signed transfer handshake.
- **REQ-FUN-11** — Support signed transfer manifests.
- **REQ-FUN-12** — Support trust-store-based verification for station identities.
- **REQ-FUN-13** — Support peer caching of identity, capability, and link-quality metadata.
- **REQ-FUN-14** — Support local and network query interfaces for peer discovery and filtering.
- **REQ-FUN-15** — Support relayed transfers across multiple hops with configurable hop limits.
- **REQ-FUN-16** — Support route selection policies based on trust, reliability, and latency estimates.
- **REQ-FUN-17** — Define versioned wire-level envelopes for peer query, route discovery, and relay transfer control messages.

## Physical layer and radio interface requirements

- **REQ-PHY-01** — Audio backend must support a minimum sample rate of 48 kHz at 16-bit integer or 32-bit float resolution.
- **REQ-PHY-02** — The receive pipeline must apply a high-pass filter (cutoff ≤ 10 Hz) before demodulation to remove DC bias introduced by SSB radio audio paths.
- **REQ-PHY-03** — The demodulator must track station-to-station frequency offsets of up to ±50 Hz without operator intervention (automatic frequency control, AFC).
- **REQ-PHY-04** — The AFC subsystem must handle transmitter drift up to 1 Hz per second for normal SSB radio operation.
- **REQ-PHY-05** — Transmitter release (PTT drop) must occur within 50 ms of the last transmitted sample to preserve turnaround timing budgets.
- **REQ-PHY-06** — The receive path must begin acquiring signal within 150 ms of remote key-down to honour the turnaround timing contract.
- **REQ-PHY-07** — PTT keying must support at minimum: serial port RTS/DTR assertion and software-controlled VOX.
- **REQ-PHY-08** — CAT-based PTT control via Hamlib/rigctld is a recommended integration path that provides access to the majority of amateur transceivers without per-rig code.
- **REQ-PHY-09** — Audio input gain must remain within a range that preserves symbol amplitude stability; the system must document the expected input level range and provide a level indicator.

## Platform and dependency requirements

- **REQ-PLAT-01** — Linux support is the primary target and requires ALSA development headers for CPAL builds.
- **REQ-PLAT-02** — macOS support uses CoreAudio through CPAL.
- **REQ-PLAT-03** — Windows support uses WASAPI through CPAL.
- **REQ-PLAT-04** — Raspberry Pi 4 and Raspberry Pi 5 must be supported as first-class Linux deployment targets.
- **REQ-PLAT-05** — ARM64 builds for Raspberry Pi 4/5 must be part of regular compatibility testing.
- **REQ-PLAT-06** — Any development environment must support loopback mode for hardware-free testing.
- **REQ-PLAT-07** — Rust toolchain must build the full workspace and no-default-features variant.

## Non-functional requirements

- **REQ-NFR-01** — Maintain workspace-level buildability on Linux and macOS CI runners.
- **REQ-NFR-02** — Keep tests runnable without physical audio hardware in default CI workflows.
- **REQ-NFR-03** — Ensure crate boundaries are clear enough for independent testing.
- **REQ-NFR-04** — Keep plugin additions from requiring broad refactors across unrelated crates.
- **REQ-NFR-05** — Define objective benchmark suites and publish method and result artifacts.
- **REQ-NFR-06** — Track goodput, completion rate, retry efficiency, and completion latency across channel profiles.
- **REQ-NFR-07** — Require HPX performance claims to be tied to reproducible benchmark runs.
- **REQ-NFR-08** — Benchmark channel profiles must include parameterized Watterson model scenarios (Good/Moderate/Poor path conditions) and Gilbert-Elliott burst error scenarios; AWGN-only benchmarks are insufficient for HF performance claims.
- **REQ-NFR-09** — Maintain deterministic timeout and retry behavior for session-state transitions.
- **REQ-NFR-10** — Use multithreaded execution for modem pipelines where it improves deterministic real-time behavior.
- **REQ-NFR-11** — Support optional GPU acceleration for compute-heavy signal-processing stages when it produces measurable benefit.
- **REQ-NFR-12** — GPU acceleration paths must use open frameworks (for example Vulkan via wgpu, or OpenCL) and provide a CPU fallback.
- **REQ-NFR-13** — Raspberry Pi 4/5 performance targets must be measured and published in benchmark artifacts.
- **REQ-NFR-14** — Peer cache lookup and query operations should remain bounded under large peer tables.
- **REQ-NFR-15** — Multi-hop relay control-plane traffic should include duplicate suppression and loop prevention.

## FEC and interleaving requirements

- **REQ-FEC-01** — All FEC-enabled transfer modes must pair the FEC codec with a block interleaver.
- **REQ-FEC-02** — The interleaver must shuffle symbols across multiple FEC blocks before transmission such that burst errors are dispersed into correctable random-error patterns.
- **REQ-FEC-03** — Default interleaver depth must be at least 5× the expected maximum burst error duration expressed in symbols at the target baud rate.
- **REQ-FEC-04** — Interleaver depth must be a documented parameter in each mode profile definition; it must not be a hidden constant.
- **REQ-FEC-05** — FEC and interleaver parameters must be agreed upon during session handshake and must not be assumed by either party.
- **REQ-FEC-06** — Benchmark scenarios must test FEC+interleaver effectiveness under burst-error conditions (Gilbert-Elliott model) and not only under AWGN.

## Channel access requirements

- **REQ-MAC-01** — Sessions operating in point-to-point mode may assume a dedicated channel and are not required to implement channel sensing.
- **REQ-MAC-02** — Sessions operating in broadcast or relay mode on a shared channel must implement a channel-clear detection (CCD) mechanism before transmitting.
- **REQ-MAC-03** — The reference channel access algorithm for shared-channel operation is 0.3-persistence CSMA: sense the channel, transmit immediately with 30% probability if clear, back off and retry otherwise.
- **REQ-MAC-04** — Data Carrier Detect (DCD) is the mechanism for CCD and must be derived from the demodulated signal energy, not from audio amplitude alone.
- **REQ-MAC-05** — Channel access policy must be documented per mode profile.

## Compression requirements

- **REQ-CMP-01** — Optional lossless payload compression at the session layer is in scope.
- **REQ-CMP-02** — Compression algorithm must be deterministic and produce identical output for identical input across platforms.
- **REQ-CMP-03** — Compression capability must be negotiated during session handshake and must not be assumed.
- **REQ-CMP-04** — If compression is active, compressed size must be compared to uncompressed size before transmission; a compressed frame larger than the uncompressed original must be sent uncompressed.
- **REQ-CMP-05** — Decompression failure must be treated as a frame integrity error.

## Security and trust requirements

- Signed transfers are mandatory for HPX file or object transfer mode.
- Station identities use asymmetric key pairs with operator-managed trust anchors.
- Session handshake messages must be signed and verified.
- Transfer manifests must be signed and verified before final acceptance.
- Trust status must include trusted, untrusted, revoked, and unknown states.
- Key lifecycle must include validity windows and revocation handling.
- Cryptographic defaults should use Ed25519 signatures and SHA-256 or stronger hashing.
- A post-quantum-safe signature method must be supported for identity and transfer-signing workflows.
- The implementation should support a hybrid signature mode (classical + post-quantum) during migration.
- Initial post-quantum-safe default should target ML-DSA (FIPS 204) where available.
- If session key establishment is used, a post-quantum-safe KEM option should be supported, with ML-KEM (FIPS 203) preferred.
- Trust-store metadata must record algorithm type and hybrid-policy requirements per identity.
- Relay path admission must enforce trust policy on each intermediate hop.
- Multi-hop transfers must preserve end-to-end signed integrity and fail closed on trust violations.
- Route metadata should support post-quantum-capable signing under configured policy.
- Relay and query messages must include anti-replay fields and enforce loop-prevention semantics.

### Post-quantum and frame size dependency

> **Transcribed 2026-08-07.** REQ-PQ-01…04 and 07…09 were fully stated in
> `docs/dev/project/traceability-matrix.md` and cited by it, but had never been written into this
> document — so the requirements existed in the artifact that *links* requirements to code, not in
> the one that *holds* them. Text is transcribed from the matrix, which remains the source these
> were recovered from; nothing here was derived by reading the implementation.

- **REQ-PQ-01** — Provide a post-quantum-safe signature method for identity and transfer signing.
- **REQ-PQ-02** — Provide a hybrid signature mode (classical + post-quantum) for the migration period.
- **REQ-PQ-03** — The initial post-quantum default targets ML-DSA (FIPS 204).
- **REQ-PQ-04** — Provide a post-quantum-safe KEM option; ML-KEM (FIPS 203) preferred.
- **REQ-PQ-05** — ML-DSA-44 signatures are 2420 bytes. ML-KEM-768 public keys are 1184 bytes. Both exceed the current 255-byte frame payload limit.
- **REQ-PQ-06** — In-band post-quantum handshake messages cannot be carried in the current wire format without a segmentation and reassembly (SAR) sub-layer.
- **REQ-PQ-07** — SAR must be designed and implemented before the in-band post-quantum handshake.
- **REQ-PQ-08** — Post-quantum transport is sequentially dependent on SAR delivery.
- **REQ-PQ-09** — Out-of-band post-quantum key distribution may proceed independently of SAR.

## Control-channel security requirements

The daemon control channel (TCP + WebSocket, ports 9000/9001) carries operator commands — PTT,
frequency/mode, transmit, messaging — between the daemon (server) and the operator panel and other
clients. It is distinct from the on-air/RF peer link (secured separately, above); today it is
plaintext with no authentication, bound to loopback by default. The reference is K4remote's TLS-PSK
client. See `docs/dev/design/control-channel-security.md` for the design and threat model.

- The control channel must support mutual authentication and on-the-wire encryption using a
  pre-shared key (PSK). (REQ-SEC-CTL-01)
- When the daemon binds to any non-loopback address, an authenticated + encrypted channel must be
  required; unauthenticated plaintext is permitted only on a loopback (`127.0.0.1`/`::1`) bind.
  Transmitter-keying commands (PTT, transmit) must never be accepted from an unauthenticated client
  on a non-loopback bind — fail closed. (REQ-SEC-CTL-02)
- Secrets (the control-channel PSK, station identity keys) should be storable in the operating
  system's secret store — Secret Service / GNOME Keyring / KWallet on Linux, Keychain on macOS,
  Credential Manager on Windows — as the preferred backend when available, for both the daemon
  (server) and the clients. (REQ-SEC-CTL-03)
- A file-based keystore must be available as a fallback for hosts without a usable system secret
  store, encrypting secrets at rest under an operator master password (memory-hard KDF, e.g.
  Argon2id, plus authenticated encryption). The master password must never be written to disk in
  plaintext. (REQ-SEC-CTL-04)
- Any file holding key or secret material (identity key, trust store, keystore, PSK file) must be
  owner-only: `0600` for files, `0700` for the containing directory. Both the daemon (server) and the
  panel and other clients (client) must validate permissions when loading such a file and refuse to
  read one that is group- or world-accessible, and must set owner-only permissions on write. This
  generalises the existing `validate_trust_store_permissions` / `enforce_trust_store_permissions` in
  `openpulse-cli` to every secret file on both sides. (REQ-SEC-CTL-05)
- **Third-party protocol surfaces are exempt from REQ-SEC-CTL-01/02 and carry no authentication.**
  The ARDOP TCP interface (`openpulse-tnc`) and the KISS/AX.25 TCP interface (`openpulse-kisstnc`)
  implement externally-specified protocols — Pat, Winlink and APRS/AX.25 clients speak them as
  published. Neither specification has any notion of authentication, so adding one would make the
  interface non-compliant and defeat the compatibility that is the entire reason those crates exist.
  These ports are therefore unauthenticated **by design**, not by omission. (REQ-SEC-CTL-06)

  The controls that stand in for authentication here are:
  1. **Loopback by default** — `bind_addr` defaults to `127.0.0.1` for both TNCs (and for the
     daemon's TCP and WebSocket ports), so out-of-the-box neither is reachable off-host.
  2. **Operator responsibility for network placement** — binding a TNC to a routable address is an
     explicit act that grants transmit control (including `PTT TRUE`) to anything that can reach the
     port. Documentation that shows a non-loopback bind must say so at the point of instruction.
  3. **Transmit-safety guarantees that do not depend on the caller** — the shared PTT watchdog and
     release-on-disconnect (`openpulse-radio::shared_ptt`) bound how long any client, authenticated
     or not, can hold the transmitter.

  This exemption covers *only* protocol surfaces defined by a third party. It does **not** extend to
  OpenPulseHF's own control channel, which remains bound by REQ-SEC-CTL-01/02, nor to any new
  interface of our own design. (Recorded 2026-07-19 in response to audit finding #3, which reported
  the missing auth as a defect; the compatibility constraint makes it a deliberate trade instead.)
- SAR must be designed and implemented before in-band PQ handshake requirements can be satisfied.
- PQ signature transport requirements are therefore sequentially dependent on SAR delivery; planning must reflect this ordering.
- Out-of-band or application-layer PQ key distribution (for example via the PKI tooling) may proceed independently of SAR.

## Regulatory compliance requirements

Regulatory compliance is a hard requirement for any transmission on amateur radio frequencies. The following rules apply in the primary jurisdictions of interest. See docs/regulatory.md for full analysis and derivations.

### United States — FCC Part 97

- §97.307(f): The maximum symbol rate on any single carrier must not exceed 300 baud below 28 MHz in phone subbands. OpenPulseHF includes sub-300-baud modes (for example BPSK31/BPSK63/BPSK100/BPSK250) and higher-rate single-carrier modes (for example QPSK500+ and 8PSK500+). Operators must select frequencies, modes, and regional band segments consistent with local rules before transmission.
- §97.309(a)(4): Digital codes whose use is not specifically prohibited elsewhere and whose technical characteristics are publicly documented are permitted. OpenPulseHF must maintain a published technical specification sufficient for any amateur to decode the transmitted signal.
- §97.119(a): Station identification is required every 10 minutes during a transmission and at the end of each transmission series. In digital modes, identification must be in a format decodable by a receiving station.
- §97.221: Automatically controlled digital stations (unattended nodes, relay nodes) require an automatic control point. HPX relay nodes operating without a control operator present are automatically controlled stations and must comply with §97.221 including power limits and frequency restrictions.

### European Union and CEPT

- ECC/REC(05)06 and national implementations: CEPT harmonises amateur radio digital mode permissions across member administrations. Most EU member states permit amateur digital modes across all authorised bands subject to the general licence conditions (power, bandwidth, identification).
- CEPT T/R 61-01: harmonised licensing for portable cross-border operation within CEPT countries. OpenPulseHF documentation must state which modes and bandwidths are intended so that visiting operators can assess compliance with their visiting licence conditions.
- Bandwidth constraint: many EU administrations limit occupied bandwidth by band and mode class. For HF digital modes the typical permitted bandwidth is ≤ 2.7 kHz (matching SSB channel spacing). HPX2300/2400 Hz profiles must be validated against the occupied bandwidth definition used by the relevant national administration.
- Station identification: EU member states typically require identification at least every 10 minutes (consistent with FCC), though interval requirements vary (e.g. UK: every 15 minutes). The identification requirement is the same: it must be decodable by the receiving station in the digital mode in use, or transmitted in supplementary CW or voice.
- Germany (BNetzA): §12 Amateurfunkverordnung (AFuV) requires that technical characteristics of amateur emissions be determinable. Digital modes without a published open specification may be questioned by authorities; OpenPulseHF's open specification satisfies this requirement by design.
- United Kingdom (Ofcom): The UK Full licence permits digital modes on all amateur bands. Station identification every 15 minutes and at end of transmission. The UK left CEPT licensing arrangements post-Brexit; UK operators verify compliance with the current Ofcom amateur licence conditions document directly.

### IARU Region 1 and Region 2 band plans

- IARU band plans are non-binding recommendations but are widely observed to avoid mutual interference.
- Region 2 (Americas) and Region 1 (Europe/Africa/Middle East) both designate sub-bands for HF narrowband digital modes (e.g. 14.070–14.099 MHz on 20 m).
- OpenPulseHF documentation should recommend operating frequencies aligned with IARU band plan digital sub-bands for each supported band.
- Wide-band HPX2300 profiles should operate in segments where wide-band digital modes are plan-consistent (e.g. 14.099–14.112 MHz on 20 m where permitted by national administration).

## Competitive performance requirements

- Primary strategic goal: develop an independent, first-principles OpenPulse protocol stack that competes on reliability, throughput, and usability.
- HPX must target outcome parity or better versus incumbent modems in benchmarked scenarios.
- Comparisons must use equal occupied bandwidth classes and published test conditions.
- Performance evaluation must include at least HF narrow, HF wide, and VHF FM profile families.
- No claim of proprietary protocol compatibility may be made without defensible public evidence.
- Any compatibility mode targeting proprietary systems (including VARA or PACTOR-4) requires explicit legal review and approval before implementation work starts.

## Compatibility and UX requirements

- CLI usage and docs must stay aligned across releases.
- New user-facing options must be documented in docs/cli-guide.md.
- README usage examples should stay current with implemented behavior.
- The operator panel application (`apps/openpulse-panel`) shall be re-implemented on the `iced` GUI
  toolkit (replacing egui/eframe), presenting the operating surface as a scrollable stack: a controls
  band, spectrum, waterfall, ladder (adaptive rate/mode), and a tabbed lower panel (additional info /
  daemon config / messages / event log). (REQ-UX-04)

## Observability and diagnostics requirements

- OpenPulse shall provide an opt-in observability/audit mode that persists logs and structured
  events to disk, so a run can be analysed after the fact without a live client attached. Audit
  mode is off by default and enabled via configuration. (REQ-OBS-01)
- Long-running binaries (at minimum `openpulse-daemon`) shall support persistent, rotating
  file logging in addition to stdout, enabled via a `[logging]` config path, with the resolved
  log path visible at startup. Log level continues to honour `RUST_LOG` over config over default.
  (REQ-OBS-02)
- OpenPulse shall provide a single command to collect a diagnostic bundle — recent logs, the
  latest session diagnostics/metrics, a config snapshot with secrets redacted, and
  version/git/system metadata — packaged for handoff to a developer, generalising the existing
  on-air `bundle-evidence` script to everyday runs. (REQ-OBS-03)

## Wide-channel (VHF/UHF) requirements — release 1.x

Extending the modem from its ~2.7 kHz HF SSB channel to 12.5 kHz and 25 kHz VHF/UHF-class channels.
Targeted at a future **1.x** release; not part of the current line. Design and phased action list in
`docs/dev/design/wide-channel-extension.md`. These requirements are gated on the RF-architecture
decision (REQ-BW-01).

- The audio sample rate shall be configurable rather than fixed at 8 kHz: the modem engine and all
  rate-parameterized DSP must run at a `[audio] sample_rate` selected from at least {8000, 48000,
  96000} Hz, defaulting to 8000. (REQ-BW-02)
- The system shall support wide modes occupying up to ~12.5 kHz at a 48 kHz audio path (e.g.
  clock-scaled OFDM/SC-FDMA and the existing 9600-baud RRC modes), reachable via the adaptive ladder.
  (REQ-BW-03)
- The system shall support wide modes occupying up to ~25 kHz, via a 96 kHz real-audio path or a
  48 kHz complex-IQ path. (REQ-BW-04)
- The system shall provide a direct-IQ receive path (complementing the existing IQ transmit seam) so
  wide operation can use an SDR front-end rather than a bandwidth-limited soundcard/SSB path.
  (REQ-BW-05)
- Bandplan awareness shall be extended to VHF/UHF bands (6 m/2 m/1.25 m/70 cm) with per-segment
  occupied-bandwidth limits (12.5/25 kHz where regionally permitted) and channel-raster-aligned QSY.
  (REQ-BW-06)
- Wide-mode SNR floors shall be calibrated against a VHF/UHF mobile-fading channel model (flat
  Rayleigh/Rician + vehicle Doppler) with an explicitly documented SNR reference bandwidth. (REQ-BW-07)
- **RF-architecture decision (blocking):** the wide path shall use a direct-IQ SDR path and/or a
  linear wide exciter; a constant-envelope (e.g. 4FSK) wide mode family is an optional fallback for
  class-C FM transmitters. This decision governs all of REQ-BW-02..07 and must be recorded before
  implementation. (REQ-BW-01)

## JS8-based station discovery and rendezvous requirements (FF-15)

Idle-time discovery of other OpenPulse stations on the shared JS8 calling frequency, and negotiated
handoff from JS8 to a native HPX session. Shipped (Phases A–G); only on-air validation (Phase H) is
deferred. Design and locked decisions D1–D7 in `docs/dev/design/js8-discovery-rendezvous-plan.md`;
capability rows CAP-70 in the traceability matrix.

- The station shall implement a native JS8-compatible weak-signal waveform (8-GFSK, 79 symbols,
  Costas 3×7 sync, LDPC(174,87), CRC-12) that interoperates with stock JS8Call, without depending on an
  external JS8Call process at runtime. (REQ-DISC-01)
- When discovery is enabled and the station is idle, it shall QSY to the current band's JS8 calling
  frequency, participate as a well-behaved JS8 station (heartbeats at community-norm cadence), and
  restore its home frequency when discovery stands down or is preempted. (REQ-DISC-02)
- The station shall mark itself with an in-band `@OPULSE` capability hint, recognize other OpenPulse
  stations from that hint, and cache them (identity, capability, link-quality) in the shared peer
  cache. (REQ-DISC-03)
- All discovery transmission shall be **off by default**; when enabled the default mode shall be
  receive-only. Beacon and rendezvous transmission shall each require an explicit opt-in plus a
  configured callsign. (REQ-DISC-04)
- The station shall not transmit unless its clock is NTP-disciplined to within ±2 s of UTC (residual
  bias estimated from decode timing); beyond that bound it shall hard-refuse transmission and degrade
  to receive-only. (REQ-DISC-05)
- Unattended beacon transmission shall satisfy §97.221 automatic control — a reachable control point
  able to terminate transmission, off-by-default gating, periodic identification, and operator-set
  power — as documented in `docs/regulatory.md` (see REQ-REG-04). (REQ-DISC-06)
- The station shall negotiate a working frequency with a discovered peer via a compact 2-message
  rendezvous exchange over JS8, then QSY and hand off to the signed HPX handshake (CONREQ/CONACK),
  which provides authentication; the rendezvous exchange itself carries no signature. (REQ-DISC-07)

## Direct peer-to-peer file transfer requirements (FF-16)

Sending a file to a connected peer over an RF session, with cryptographic end-to-end verification.
Shipped (Phases A–E); on-air validation (Phase F) is deferred. Design and locked decisions D1–D5 in
`docs/dev/design/file-transfer-plan.md`; capability row CAP-71 in the traceability matrix.

- The station shall send a file to a connected peer over an RF session using a dedicated framed
  protocol (offer / accept / reject / data / block-ack / complete / cancel), carried over the shared
  SAR segmentation layer. (REQ-FX-01)
- File objects shall be split into fixed-size blocks (default 16 KiB, ≤48 KiB) each carried as one SAR
  segment, lifting the single-object SAR size limit so multi-megabyte transfers are supported, with a
  configurable hard cap (default 1 MiB). (REQ-FX-02)
- Each transfer shall carry an inline signed `TransferManifest` with a SHA-256 payload hash; the
  receiver shall verify it against the peer's handshake key before final acceptance and shall
  quarantine (mark UNVERIFIED) any file that fails verification. (REQ-FX-03)
- File acceptance shall be operator-controlled: a verified-peer requirement, size-gated auto-accept
  (default off), an optional per-peer retained-bytes quota, and prompt-on-offer by default. (REQ-FX-04)
- Reliable delivery shall use a hybrid scheme — over-the-air per-burst rate feedback plus a
  block-level acknowledgement bitmap for selective retransmission — and shall support resuming an
  interrupted transfer from the last completed block. (REQ-FX-05)
- Transmission shall be airtime-bounded into bursts so PTT keying stays within the radio's watchdog
  limit and the channel is yielded between bursts. (REQ-FX-06)

## Reference-derived requirements (software-defined modem study, 2026-07-14)

Derived from studying modern open-source modems (`docs/dev/research/references.md`:
RFnexus/modem73, chrissnell/omnimodem, chrissnell/graywolf). **We re-implement independently — no code
is copied**; these capture techniques worth building from first principles. Scheduling and priority live
in the roadmap; each is a candidate, not a committed deliverable.

- The receive **AGC / input-level normalization** front-end (`openpulse_dsp::agc::Agc`) — wired at the
  single `route_audio_stage(InputCapture)` seam (DC-block → notch → DCD → AGC, with DCD read *pre-AGC*),
  gain-locked per burst so a mid-frame gain change can't corrupt soft-decision scaling, off by default,
  with an `agc_blocks_processed` tripwire and runtime `SetAgc` control (daemon/CLI/panel) — is **already
  shipped** (PRs #583/#699/#700/#826; verified by the 2026-07-14 Fable design review). The earlier "we
  have no AGC" claim was stale (predated PR #583). Remaining delta: (a) a TOML config gate
  `[modem] agc_enabled` (+ optional target-RMS / bandwidth / max-gain-dB) applied at daemon startup like
  the notch — currently AGC is runtime-toggleable only; and (b) a systematic input-amplitude-sweep
  acceptance test documenting that decode is level-invariant above the squelch (AGC on vs. off, because
  the LLR/SNR/acquisition estimators are amplitude-ratio-based) and that the AGC's value is QSB
  level-tracking + metering, not sub-squelch rescue. The hard-limiter-correlator option is **rejected**:
  a hard limiter is constant-envelope and destroys the amplitude information the calibrated soft-LLR path
  needs (QAM/APSK), and acquisition is already amplitude-invariant (`search_normalized` / relative
  `refine_onset`), so nothing motivates it. (REQ-AGC-01)
- The receiver's **automatic notch shall be enabled by default**. It was built, documented as "a
  clear win against out-of-band QRM", and left opt-in — so it was **off in every recorded on-air
  failure**, which is how "we already harden against interference" and "the station could not decode"
  were both true at once. Built-and-never-enabled is a distinct failure from a seam gap: the wiring
  was correct throughout, nothing switched it on. Measured on the recorded hot floor with a 2200 Hz
  interferer just outside `BPSK250`'s occupied band, the decode **fails with the notch off and
  succeeds with it on at amplitude 0.30**; at 0.05–0.15 it is unnecessary and at 0.60 the interferer
  wins either way — so it buys a real band of conditions and costs nothing where there is nothing to
  notch. The protected band tracks the active mode, so the signal itself is never notched, and an
  in-band interferer remains a QSY case. Acceptance: a gate asserting BOTH edges — the decode fails
  without the notch at the rescue level, and a strong enough interferer defeats it regardless.
  (REQ-QRM-01)
- The receiver's **carrier detect shall track the band's noise floor** rather than compare against a
  fixed threshold, at the single shared `InputCapture` seam and independently of the active mode. The
  shipped `DcdState` used a constant 0.01 RMS squelch; the recorded IC-9700 idle capture measures
  **0.126 RMS, twelve times that**, so on such a band the daemon's DCD reads permanently busy, the
  burst never ends on a carrier drop, only the runaway cap flushes it, and a real frame is never handed
  to the decoder at all. This is the daemon's half of the hot-floor failure the scanning receive path
  hit five times (#1020/#1021/#1039/#1040/#1045/#1049) — **none of that path's machinery
  (`EnergyGate`, the AFC settle, the correlation veto) runs on the daemon's `accumulate_capture`
  route.** The floor shall be estimated from the **passband spectral distribution**, not from block
  energies: a carrier that stays on raises every block and drags a time-domain percentile up with it,
  which is exactly how `EnergyGate` saturates, whereas a narrowband signal cannot reach a low
  percentile across bins (Mercury uses the same spectral-minimum floor for its channel-busy decision).
  Level, floor and interference are properties of the environment, not of the waveform, so this is
  deliberately mode-independent — frame *detection* remains per-waveform. Acceptance: recorded idle at
  0.126 RMS produces no burst, and a real frame in that same floor still produces one bounded burst
  that ends on the carrier drop rather than at the cap. (REQ-DCD-ADAPT)
- Every PTT-keyed transmit scope shall release the transmitter **deterministically on scope exit** —
  including on an early return or a panic/unwind — via an RAII guard, rather than relying solely on the
  max-duration watchdog (REQ-REG-10 / #863). This bounds an unexpected key-down to the current stack
  scope instead of up to `ptt_max_duration`. Acceptance: a test that panics inside a keyed transmit scope
  and asserts the transmitter was released without waiting for the watchdog timer. (REQ-PTT-01)
- `openpulse-radio` shall support keying via the **CM108/CM119 sound-chip GPIO over USB-HID** (the common
  cheap-interface PTT path), selectable from config like the existing backends. Acceptance: unit tests
  for the HID output-report encoding; documented in the PTT backend list. (REQ-PTT-02)
- `openpulse-radio` shall support keying via a **Linux GPIO line** (Raspberry Pi header), selectable from
  config, behind a target/feature gate with a mockable line interface. Acceptance: unit-tested report
  path; documented. (REQ-PTT-03)
- A purpose-built **robust narrowband weak-signal waveform** (~500–600 Hz, fading-tolerant) shall be
  evaluated as the sub-floor rung below the current SL floor — the direction chosen over
  frequency-diversity repetition (measured net-negative in #864). Acceptance: a coded frame-success
  bake-off on Watterson good/moderate/poor showing a margin gain over the current floor at matched
  occupied bandwidth, or an honest no-ship finding. (REQ-WSIG-01)
- The receiver shall optionally **decode multiple registered waveforms concurrently** from a single
  capture stream (off the shared `InputCapture` tap) rather than committing to one mode, for a
  discovery/monitor role. Acceptance: a loopback test injecting two different modes into one capture
  buffer and decoding both in one tick. (REQ-RX-01)
- Audio device selection shall be **hotplug-safe**, surviving OS renaming/reorder by keying on a stable
  device identity rather than an ordinal index or path. Acceptance: a test that a configured device
  resolves after a simulated reorder. (REQ-DEV-01)

## Rig control (CAT) reliability requirements

> **Why this family exists.** The 2026-07-30 on-air session lost most of a keyed window to rig state
> the software believed it had set or read, and had not. Every item below is a defect that actually
> happened that day, not a hypothetical: (a) `hamlib` returned an **empty** value for `l RFGAIN` on
> both an IC-9700 and an FT-991A, so the preflight printed `rfgain = na` and warned nothing — while
> the FT-991A's RF gain sat at **37/255**, crippling its receiver; (b) the runner's tune step *set*
> frequency and mode and never read them back, and the FT-991A was later found sitting **700 Hz off**
> a commanded value; (c) the preflight corrected and reported the NB/NR **level** (`l/L NR`) while the
> DSP is gated by the **function switch** (`u/U NR`), producing a false alarm on both stations and
> false silence for a switch on at level 0. Each was invisible to the software, and each was
> indistinguishable at the time from a modem defect. Reliable operation requires rig control that
> cannot report success it did not achieve.

- Rig control shall live in a **dedicated crate** with a per-transceiver driver per supported model,
  rather than depending on a lowest-common-denominator abstraction whose unimplemented controls are
  indistinguishable from controls that are genuinely absent. The crate shall expose, for every
  control, whether it is *supported*, *unsupported*, or *unknown on this model* — never a bare empty
  value. Acceptance: a driver-trait conformance test asserting that an unsupported control returns a
  typed `Unsupported` rather than an empty/default value, and that no caller can silently coerce it
  to a number. (REQ-CAT-01)
- Each supported transceiver's driver shall cover **100 % of that model's published CAT command set**,
  derived from the manufacturer's own specification, with the source document and its revision
  recorded. Coverage shall be **mechanically demonstrated** — a machine-readable command inventory
  per model, diffed against the implemented set in CI — never asserted from a reading of the code.
  Acceptance: a coverage test that fails when a command in the model's inventory has no
  implementation, and a recorded inventory-vs-manual provenance note. (REQ-CAT-02)
- Every control the modem's operation depends on shall be **set-then-verified**: the driver reads the
  value back from the rig and reports a typed error when the rig did not take the command. A write
  whose effect was never confirmed shall not be reported as success. Acceptance: a test that a driver
  whose readback disagrees with the commanded value returns an error rather than `Ok` — the direct
  regression for the FT-991A found 700 Hz off a commanded frequency. (REQ-CAT-03)
- Support for each transceiver shall be **validated against the real hardware** before that model is
  advertised as supported, and the validation shall record the evidence tier, the firmware revision,
  the operator, and the date. Simulation and hardware are separate, non-substitutable gates: a model
  that has only ever been exercised against a mock is *not* supported. Validation evidence shall
  carry an explicit expiry/re-validation trigger so it cannot silently age into a false claim.
  Acceptance: a per-model support table whose "validated" state is derived from recorded hardware-run
  evidence, and a docs gate that fails when a model is listed as supported without it. (REQ-CAT-04)
- The project shall publish a **guided validation procedure** so users and co-developers with access
  to a model can validate it themselves and contribute the result — a scripted run that exercises the
  model's command inventory, captures readbacks, and emits a submittable evidence bundle. This is the
  mechanism by which the supported-model list grows beyond hardware the maintainers own. Acceptance:
  the procedure is runnable end-to-end against a rig by someone who did not write it, and produces an
  evidence bundle that satisfies REQ-CAT-04. (REQ-CAT-05)
- Initial transceiver candidates, ordered by nothing but availability for validation: **Elecraft** K4,
  K3, KX3, KX2; **QRP Labs** QMX, QMX+; **Yaesu** FT-710, FT-991A, FT-817, FT-818; **ICOM** IC-9700,
  IC-705. The list is deliberately open — any further model is in scope once someone with access can
  complete the REQ-CAT-05 guided validation. A model's presence on this list is a *candidate* claim
  and confers no support status until REQ-CAT-04 is satisfied for it. (REQ-CAT-06)

## Documentation requirements

- Version bumps require updates to docs/dev/project/changelog.md and docs/releasenotes.md.
- Docs files under docs/ must pass frontmatter validation in CI.
- HPX benchmark assumptions and result summaries must be captured in docs/high-performance-mode.md.
