---
project: openpulsehf
doc: docs/dev/research/varac-feature-gap-analysis.md
status: living
last_updated: 2026-07-10
---

# VarAC feature-gap analysis — ideas OpenPulseHF is missing

VarAC (<https://www.varac-hamradio.com/>) is the most popular keyboard-to-keyboard ARQ HF chat application, built on the proprietary VARA HF/FM modems. This document inventories VarAC's feature set (from its website, release history, and community documentation), de-duplicates it against what OpenPulseHF already has, and ranks the genuine gaps with concrete fit sketches against our actual architecture. **Research/ideas only — nothing here is implemented or scheduled.**

Companion research: `docs/dev/research/vara-research.md` (VARA modem/ACK taxonomy — already mined), `docs/dev/design/js8-discovery-rendezvous-plan.md` (approved plan; several VarAC-shaped gaps are partially covered by it and are marked as such throughout).

---

## 1. Summary

VarAC's modem-layer ideas were already harvested (`vara-research.md` → our ACK taxonomy and rate ladder). What remains missing is almost entirely **application/operator layer**: VarAC wins on *social mechanics on a shared calling frequency*, not on DSP. The top genuinely-missing, high-value items:

1. **Direct P2P file transfer** — send a file/image inside a session, with progress, size-gated auto-accept, and integrity verification. Our substrate (SAR + LZ4 compression + HPX ARQ + signed `TransferManifest`) is 90% built; nothing wires it into an operator-facing "send file" flow.
2. **Native calling-frequency CQ + slots + monitoring** — a published OpenPulse calling frequency per band, CQ frames, passive monitoring of who is calling, and answer-with-auto-QSY-to-slot. The JS8 rendezvous plan covers *discovery via JS8*; a native OpenPulse CQ flow on our own calling frequency is the complementary gap.
3. **VMail-style P2P store-and-forward** — park a message at a relay station; the relay notifies the recipient when it hears their beacon. We have Winlink mail (infrastructure-dependent) and mesh envelope relay (no message store); the peer-hosted mailbox in between is missing.
4. **Live keyboard-to-keyboard chat** — VarAC's core product. We have message send (subject+body) but no interactive chat session surface.
5. **Operator alerting** (keyword alert tags, CQ/beacon popups, external notification hooks) and **canned messages/macros** — small panel/daemon features with outsized daily-use value.

Also worth considering: PSK-Reporter-style spotting from the discovery `StationTable`, a decentralized SMTP/IMAP email-gateway mode alongside the Winlink CMS gateway, and live position sharing. Explicitly *not* worth copying: the VARA modem or VarAC's closed wire formats, the AI/internet-content gateway, and the gamification layer (§5).

---

## 2. VarAC feature inventory (cited)

Sources: H = homepage <https://www.varac-hamradio.com/>, R = releases <https://www.varac-hamradio.com/varac-releases>, E = EmComm <https://www.varac-hamradio.com/emcomm>, G = email gateway <https://www.varac-hamradio.com/varac-email-gateway>, X = extensions <https://www.varac-hamradio.com/extensions>, QS = quick-start guide <https://www.varac-hamradio.com/post/varac-quick-start-guide>, TD = technical deep dive <https://k0rv.org/wp-content/uploads/2024/02/VarAC_slide_show.pdf>, F53 = V5.3.1 announcement <https://www.varac-hamradio.com/forum/varac-hf-discussion-forum/varac-v5-3-1-is-here-with-vmail-store-forward-offline-vmail-compose-slot-based-qsy-psk-self-report-and-much-more-1>, FR = relay forum threads <https://www.varac-hamradio.com/forum/varac-hf-discussion-forum/relaying-vmail>.

| # | Feature | What it does | Source |
|---|---|---|---|
| V1 | Keyboard-to-keyboard chat | Live P2P chat over an ARQ link; "is typing" indicator; gestures/emojis; spell checker; message queue | H |
| V2 | Calling frequency + slots | One published calling frequency (CF) per band with 10 QSO slots at ±750 Hz steps; CQ on the CF, work the QSO on a slot | QS, TD |
| V3 | CQ flow with auto-QSY | Call CQ on the CF announcing a slot; answering station double-clicks the CQ → both auto-QSY to the slot and connect; special CQs (POTA/QRP/DX/custom) | QS, R (V7.0.8, V8.6.2) |
| V4 | Slot sniffer | Temporarily QSY the RX (no PTT) to a slot to listen/decode before committing | QS |
| V5 | Frequency monitor | Passive monitoring of the CF: see beacons, CQs, and ongoing QSOs scroll by; "show my beacons and CQs" | H, R (V13.0.1) |
| V6 | Beacons | Periodic presence beacons (15 min; 5 min in EmComm) carrying callsign/locator; beacon list of heard stations | H, F53, E |
| V7 | VMail mailbox | Inbox/Outbox/Sent/Parking folders; offline compose; multiple recipients; urgent flag; compact packets | R (V5.3.1, V10.2.1, V11.4.0) |
| V8 | VMail store & forward + relay | Park a VMail at a third station; the relay listens for the recipient's beacon and notifies them; recipient connects and collects; manual multi-hop re-parking | F53, FR, R (V6.0.8) |
| V9 | File/image transfer | Send files, images, binary content in-session; size-threshold auto-accept; image shrinker tool; received-files gallery; transfer time estimation; ~100 KB practical cap (15 KB advised on HF) | H, R (V8.0.6, V11.0.7), QS |
| V10 | Broadcasts | Asynchronous one-to-many text (APRS-like, no ACK) on the CF, to ALL or a callsign; queued/re-broadcast; VARA-FM digipeater propagation (up to 2 hops); multilingual | R (V6.2.4, V6.4.15, V8.6.2), E |
| V11 | Alert tags + alert center | Keyword tags in broadcasts/datastream trigger audible+visual alerts with per-tag colors/sounds; consolidated alert center; CQ popup alerts; email notification on alert | R (V6.4.15, V7.3.0, V11.2.0, V12.0.0) |
| V12 | Email gateway | Decentralized RF↔internet email: any station runs SMTP/IMAP (e.g. Gmail); bidirectional VMail↔email with delivery confirmations; 500 chars HF / 10 k EmComm outgoing | G |
| V13 | EmComm mode | One-click emergency profile: streamlined UI, auto file-accept, all-urgent, rapid beaconing, ICS message templates, check-in requests, groups | E, R (V9.1.0, V11.2.0) |
| V14 | Remote inquiry tags | `<INFO>`, `<VER>`, `<SNRR>`, `<LHR>`, `<GPSR>`, `<GPSLOC>` — pull partner/station data automatically; blockable; broadcast-based inquire (locator/GPS/version) | R (V4.1.6, V6.6.13, V13.0.1), TD |
| V15 | GPS/position | NMEA GPS integration; share Lat/Long; verbose (per-minute) GPS mode; bearing/distance; Google Maps links | R (V9.2.3, V10.4.3, V11.4.0, V13.0.1) |
| V16 | Spotting | Every CQ/beacon self-reported to PSK Reporter (~50 k spots/day); DX-cluster integration and spot upload | F53, R (V6.0.8) |
| V17 | Logging | Automatic ADIF QSO log; QSO log viewer; secondary logger; integrations (Log4OM, Logger32, Swisslog, UcxLog, Winlog32, QRZ lookup); QSL card generator; DXCC counter | H, R (V6.0.8–V11.0.7) |
| V18 | Canned messages / macros | Canned texts on F-keys; automatic messages; CAT macros with `[WAIT:XXX]` chaining | H, R (V4.1.6, V5.1.8, V11.2.0) |
| V19 | Rig control breadth | CAT/RTS-DTR, OmniRig, FLrig, hamlib-rigctld TCP; per-band audio level; auto antenna-tuner trigger and antenna switching on band change | H, R (V7.1.4, V9.1.0, V10.2.1) |
| V20 | Auto QSY (in-session) | Free-form QSY with accept/reject and recovery; QSY sniffer progress; SNR+ID exchange after QSY | R (V7.1.4, V9.1.0, V11.4.0) |
| V21 | Frequency scheduler / scanner | Time-of-day scheduled QSY between bands with notifications; frequency scanner (V14, in dev) | R (V5.0.2, V13.0.1, V14) |
| V22 | Path finder / path analyzer | Find an intermediary station to reach a peer you can't hear; historical propagation data analyzer | R (V8.0.6, V13.0.1), E |
| V23 | SNR analytics | Live SNR graphing during QSO; historical SNR per callsign; verbose SNR mode; ping with mutual SNR exchange | R (V6.2.4, V9.2.3, V10.2.1), H |
| V24 | VarAC Cluster | Multiple VarAC+VARA instances on different bands with a PTT coordinator (single-station multi-band monitoring) | R (V6.2.4, V8.2.0) |
| V25 | BBS file sharing | Connect to a station and browse/pull files it hosts (V13.2.7) | R |
| V26 | AI gateway / AI translate | Internet AI query relay over RF ("ACIe"); AI translation of broadcasts (V14) | R (V13.0.1), <https://www.varac-hamradio.com/ai-gateway> |
| V27 | UX polish | Multilingual UI, themes/skins, dark mode, narration (TTS), visually-impaired mode, simple/advanced/EmComm UI levels | H, R (V6.4.15, V8.0.6, V14) |
| V28 | Social/gamification | HamPlay games, Ice Breaker, reactions/likes, awards, academy/diploma, hall of fame | R (V10.1.0, V13.2.7, V8.2.0), H |
| V29 | Extensions ecosystem | Telegram notification bot, Cloudlog uploader, log mapper, web-services bridge — third-party tools on VarAC's data | X |

---

## 3. Gap analysis

Legend: **Have** = exists in OpenPulse (crate/doc cited). **Partial** = substrate or a different flavor exists. **Missing** = no equivalent. **Planned** = covered by the approved JS8 discovery/rendezvous plan (`docs/dev/design/js8-discovery-rendezvous-plan.md`). Effort: S ≤ 1 wk, M ≈ 1–3 wk, L ≈ 1–2 mo, XL > 2 mo (single dev, honest).

Missing/Partial rows ranked by value within each tier.

### Tier 1 — high value

| Feature | VarAC behaviour | OpenPulse status | Value | Effort | How it'd fit | JS8-plan overlap |
|---|---|---|---|---|---|---|
| V9 File/image transfer | In-session file send, auto-accept threshold, gallery, progress | **Missing** as a feature. Substrate ~complete: `sar_encode`/`SarReassembler` (64 005 B max, `openpulse-core/src/sar.rs`), LZ4 (`compression.rs`), signed `TransferManifest` (`manifest.rs`), HPX ARQ + OTA rate ladder, daemon `SendMessage` precedent | High — the #1 "can your modem do what VarAC does" question; EmComm table stakes | **M** | New `FileTransfer` command/event pair in `openpulse-daemon`, riding SAR+manifest over the OTA session; panel Files tab. §4.1 | None — orthogonal; runs after rendezvous |
| V2/V3/V5 Calling frequency, CQ, slots, monitoring | Published CF per band; CQ announces a slot; answerer auto-QSYs; passive CF monitor UI | **Partial/Planned** — JS8 plan gives discovery + rendezvous *via JS8*; `openpulse-qsy` gives in-session QSY + `QsyScanner` + `BandplanPolicy`. No native OpenPulse CQ frame, no published OpenPulse CF, no "heard stations calling" monitor for native traffic | High — the mechanic that makes random contacts happen; maintainer-identified gap | **M–L** (M if built as an extension of the JS8-plan daemon scheduler) | CQ/QRZ frame type + calling-channel dwell in the daemon; slots = `rendezvous_channels_hz` table already in the JS8 plan; monitor UI = the plan's Discovery tab generalized. §4.2 | **Large** — the plan's dwell/scheduler/StationTable/panel-tab are 70% of this; decide JS8-first vs native-CF-first |
| V8 VMail store & forward via relay | Park mail at a 3rd station; relay hears recipient's beacon → notifies; recipient collects; multi-hop re-park | **Missing** as P2P mail. Adjacent: Winlink path (`openpulse-b2f`, `openpulse-gateway`) is infrastructure-mail; `RelayForwarder`/`RelayDataChunk` (`openpulse-core/src/relay.rs`, `wire_query.rs`) forward live envelopes but store nothing; daemon inbox (`ListMessages`) is local-only | High — offline messaging without Winlink infrastructure; EmComm resilience story | **L** | New `openpulse-mailbox` store + parked-mail wire messages; beacon-triggered notify plugs into mesh/JS8 heartbeat hearing; signed with existing Ed25519 identities. §4.3 | Medium — beacon-hearing (StationTable upsert) is the notification trigger; VMail-waiting flag could ride the OPHF hint's reserved bits |
| V1 Live keyboard chat | Interactive chat over ARQ; typing indicator; gestures | **Partial** — `SendMessage{to,subject,body}` + inbox exists (`openpulse-daemon/src/protocol.rs:259`, panel Messages tab), but it's mail-shaped: no chat session UI, no line-by-line exchange, no typing indicator | High — this *is* VarAC's product; we have the ARQ session but no conversational surface | **M** | Chat = small typed payloads over the existing HPX secure session; panel Chat tab; typing indicator = 1-byte status frame (send sparingly; VarAC suppresses it at low SNR) | Small — rendezvous hands off to an HPX session; chat is the natural payload |

### Tier 2 — medium value

| Feature | VarAC behaviour | OpenPulse status | Value | Effort | How it'd fit | JS8-plan overlap |
|---|---|---|---|---|---|---|
| V11 Alert tags + alert center | Keyword-triggered audible/visual alerts; CQ popups; per-tag sound/color; email-on-alert | **Missing** — panel has no alerting (no alert/notification code in `apps/openpulse-panel`); daemon already broadcasts NDJSON `ControlEvent`s | Medium-high — unattended-monitoring usefulness for near-zero cost | **S** | `[alerts]` config (keyword → sound/level); daemon matches on `MessageReceived`/`StationHeard`/decode text → new `Alert` event; panel toast + sound; external hooks get it free via the control port (V29 for free) | Direct — `StationHeard`/`Js8Traffic` events are prime alert sources |
| V18 Canned messages / macros | F-key canned texts, auto-messages, CAT macro chains | **Missing** (no canned/macro support in panel/daemon/config) | Medium — daily-driver ergonomics | **S** | `[messages] canned = [...]` in `openpulse-config`; panel buttons/F-keys prefill the send box; CAT macros later via `RigctldController` | None |
| V16 Spotting (PSK Reporter / cluster) | Auto-report every heard CQ/beacon; self-report | **Missing** — no internet-reporting anywhere; JS8 plan *explicitly* de-scopes PSK Reporter (non-goal §1) | Medium — visibility drives adoption; propagation feedback | **S–M** | Opt-in reporter task in the daemon consuming `StationHeard`/decode events → PSK Reporter UDP/API. JS8-mode decodes are directly reportable (standard practice); an OpenPulse-native spot type needs PSK Reporter mode-name registration | Direct — the StationTable is exactly a spot source |
| V12 Decentralized email gateway | Any station bridges RF↔SMTP/IMAP; live bidirectional; delivery confirmations | **Partial** — `openpulse-gateway` does Winlink CMS over TCP (infrastructure, poll-based); no per-station SMTP/IMAP bridge | Medium — redundancy story is good; Winlink covers most of the need | **M** | `openpulse-gateway` gains an `smtp-imap` backend (lettre + imap crates); RF side reuses the B2F session or the (new) VMail format; **must** enforce third-party-traffic and auto-station rules (`docs/regulatory.md`) | None |
| V10 Broadcasts + digipeat | Unacked one-to-many text on the CF; alert-tag integration; FM digipeater hops | **Partial** — `openpulse-mesh` broadcasts signed envelopes with TTL re-broadcast and `openpulse-repeater` digipeats, but there is no operator-facing *text bulletin* type or UI | Medium — EmComm bulletin mechanism | **S–M** | New broadcast-text payload over the existing mesh envelope (`WireEnvelope`); panel compose + display; alert tags (V11) match on it | Medium — a JS8 `@ALLCALL`/group message is an interoperable alternative carrier |
| V14 Remote inquiry | Query partner's info/version/SNR/last-heard/GPS by tag; blockable | **Partial/Planned** — JS8 plan implements `INFO?`/`GRID?`/`SNR?` queries *in JS8*; native HPX sessions have no in-band query verbs; `PeerQueryRequest` (`wire_query.rs`) queries *caches*, not stations | Medium | **S–M** (piggyback) | Add a small query/response frame type to the HPX session layer (or reuse JS8 directed queries once shipped); respect a `[privacy]` blocklist like VarAC's INFO-block | **Large** — plan G4 covers the JS8 flavor |
| V15 GPS / position sharing | Live NMEA position, per-minute verbose mode, bearing/distance, map links | **Partial** — static `station.grid_square` config (`openpulse-config/src/lib.rs:180`), ARDOP `GRIDSQUARE` host command; no live GPS, no position exchange, no bearing/distance UI | Medium for EmComm/portable; low for fixed stations | **M** | `nmea` crate reader in the daemon → dynamic grid; position payload in beacons/handshake info; panel distance/bearing (grid math is trivial); JS8 `packGrid` already planned | Medium — JS8 heartbeats carry grid natively |

### Tier 3 — lower value / later

| Feature | VarAC behaviour | OpenPulse status | Value | Effort | Fit note | JS8 overlap |
|---|---|---|---|---|---|---|
| V21 Frequency scheduler/scanner | Timed band QSY; scan a frequency list | **Missing**; `QsyScanner` (`openpulse-qsy`) is the scanning substrate | Low-medium | S–M | Daemon cron-style `[schedule]` driving `SetFreq`; JS8 plan defers band-hopping — same mechanism | Medium |
| V13 EmComm mode | One-click emergency preset (UI + policy bundle) | **Missing** as a preset; the individual policies would exist once file auto-accept/urgent/beacon-rate exist | Low-medium (bundle of other rows) | S (after Tier 1) | A named config profile + panel toggle; ICS templates = canned messages (V18) | Small |
| V23 SNR analytics | Live/historical SNR graphs per callsign | **Partial** — receiver-led OTA already exchanges link quality (MODCOD feedback, PRs #489–#500); testbench/panel show live spectra; no per-callsign history | Low-medium | S–M | Persist per-peer SNR series next to the ADIF logbook (`openpulse-daemon/src/logbook.rs`); panel sparkline | Medium — StationTable keeps EWMA SNR already |
| V4 Slot sniffer | Listen-before-QSY UX | **Have (protocol) / Partial (UX)** — `QsyScanner` live-scans candidates inside the QSY negotiation; no manual panel "sniff this frequency" button | Low | S | Panel button → `SetFreq` + squelch/DCD readout (all existing) | — |
| V22 Path finder | Find an intermediary to reach a peer | **Have (stronger)** — route discovery + trust-weighted scoring + multi-hop relay (`wire_query.rs` RouteDiscovery*, `relay.rs` score_route); missing only an operator surface | Low (surface only) | S | Panel action: "find path to CALL" → existing query propagation | Small |
| V24 Multi-band cluster | Several modem instances + PTT coordinator | **Partial** — one daemon per rig works today; no coordinator | Low | L | Skip until real demand; a second daemon + shared PTT lock file would do | — |
| V25 BBS file hosting | Browse/pull files from a station | **Missing** | Low (niche; big attack surface) | M–L | Only atop V9 + signed manifests + explicit allowlist | — |
| V17 Logging extras | QSL cards, DXCC counter, logger integrations | **Have core** — automatic ADIF logbook shipped (daemon `logbook.rs`, `[logbook]` config); extras are cosmetic | Low | S each | ADIF is the interchange point; let external tools do QSL/DXCC | — |
| V19 Rig-control extras | OmniRig/FLrig, auto-tuner trigger, per-band audio | **Have core** — CAT via `RigctldController`, PTT backends, per-band squelch/attenuation (`openpulse-radio`, daemon) ; OmniRig is Windows-only, skip | Low | — | rigctld already fronts most rigs on our platforms | — |
| V20 Auto QSY | In-session QSY w/ accept-reject | **Have** — `openpulse-qsy`: signed frames, `QsySession`, scanner, bandplan; daemon-wired (PR #321) | — | — | We exceed VarAC here (signed frames) | Plan keeps it as the in-session mechanism |
| V6 Beacons | Presence beacons w/ locator | **Have/Planned** — mesh `BeaconScheduler` (peer discovery envelopes); JS8 heartbeats (plan G4) cover human-visible presence | — | — | Gap only if native-CF flow (V2/V3) is built: it needs a native presence beacon too | Large |
| V27 UX polish | Themes, languages, narration | **Have partial** — panel Dark/Light/Contrast/System themes (`theme.rs`); no i18n/narration | Low | — | Not protocol work; revisit post-1.0 | — |

---

## 4. Deep dives on the top gaps

### 4.1 Direct P2P file transfer (V9) — mostly assembly, not construction

**What exists already** (verified):

- Segmentation: `sar_encode()` / `SarReassembler` — 64 005 B max object, 4-byte header, timeout + duplicate-idempotent reassembly (`crates/openpulse-core/src/sar.rs`). VarAC's practical HF ceiling is ~15–100 KB, so the SAR cap is not the binding constraint; for larger files, chain segments (`segment_id` is u16).
- Integrity + provenance: `TransferManifest` — SHA-256 payload hash + sender ID + Ed25519 signature + `verify_manifest()` (`crates/openpulse-core/src/manifest.rs`). This is *better* than VarAC (which has no cryptographic verification): a received file is provably intact and provably from the claimed sender.
- Compression: `compress_if_smaller()` LZ4 (`compression.rs`), already negotiated in CONREQ/CONACK.
- Reliable transport: HPX ARQ + HARQ soft combining + the receiver-led OTA rate ladder — the transfer rides whatever rate the channel supports, which is exactly what "VarAC dynamically adjusts its speed" markets.

**What's missing** (the actual work):

1. A **file-offer/accept handshake**: small typed control payload `FileOffer { name, size, sha256, mime, compressed }` → `FileAccept | FileReject(reason)`. Mirrors VarAC's size-threshold auto-accept: `[files] auto_accept_max_bytes` in `openpulse-config` (0 = always ask). EmComm preset (V13) later flips it high.
2. **Daemon surface**: `ControlCommand::SendFile { to, path }` (or bytes-inline for the panel), `ControlEvent::FileOffered/FileProgress/FileReceived` following the serde-tagged NDJSON shapes in `crates/openpulse-daemon/src/protocol.rs`. Progress = fragments-acked / fragment-total from the SAR layer — VarAC's "time estimation" falls out of the OTA rate.
3. **Storage + gallery**: received files under `~/.local/share/openpulse/files/<peer>/`, listed via `ListFiles` (the `ListMessages` pattern); panel Files tab with per-file verify badge (manifest OK / FAILED) — the verify badge is our differentiator.
4. **Resume** (VarAC lacks true resume): keep the `SarReassembler` state keyed on `(session_id, segment_id)` across reconnects, and add a `FileResume { sha256, have_fragments_bitmap }` control message. Worth doing — HF links drop constantly. Ship v1 without it; design the offer message so it can carry it.

Effort M: no new DSP, no new wire-envelope machinery; risk is mostly in the daemon/panel plumbing and the accept-policy UX. Test story: twin-daemon harness (`crates/openpulse-daemon/src/twin.rs`) file round-trip over a Watterson channel, tamper test asserting the manifest badge fails.

### 4.2 Native calling-frequency CQ, slots, and monitoring (V2/V3/V5)

VarAC's contact mechanic: everyone parks on one CF per band; CQs and beacons are visible to all; answering a CQ auto-moves both stations to a slot. The JS8 plan solves *discovery* by borrowing JS8's calling channel and userbase — the right first move. What it deliberately does not provide is a **native OpenPulse CQ flow** (an OpenPulse-waveform CQ that any idle OpenPulse station decodes and can answer with one click).

Proposal — build it as **phase 2 of the JS8-plan daemon machinery**, not a separate subsystem:

- **Calling channel**: publish an OpenPulse calling frequency per band in `[discovery.calling_freqs_hz]`-style config (could even be *the JS8 channel neighborhood* initially — but a dedicated CF avoids QRM-ing JS8; needs community coordination, document in `docs/regulatory.md` bandplan terms).
- **CQ frame**: a small broadcast frame (BPSK31/63 for reach — our SL2 floor decodes far below the dense modes) carrying `callsign, grid, slot_index, caps, flavor (CQ/POTA/QRP/DX)`, signed optionally (a 64-byte Ed25519 sig doubles the frame; make it a config choice, and note VarAC CQs are unsigned too). Reuses `transmit_with_fec_mode(ShortRs)` for the ≤213 B path.
- **Slots**: exactly the `rendezvous_channels_hz` table from the JS8 plan §5.3 — same `BandplanPolicy` validation, same 2-char channel indices. A CQ *announces* the slot (VarAC model) instead of negotiating it, collapsing rendezvous to zero round trips: answerer QSYs to the slot and initiates the signed CONREQ handshake there. Collision handling = the handshake itself (first CONACK wins; losers return to the CF).
- **Monitoring**: the idle-dwell + `StationTable` + panel Discovery tab from the JS8 plan, fed by a second decoder path listening for native CQ frames on the OpenPulse CF. `DwellRing` isn't needed here — CQ frames are normal burst traffic through `accumulate_capture`; only the *dwell scheduling* (idle predicate, home-frequency save/restore, operator preempt) is shared. That machinery should therefore be built channel-agnostic: `dwell(target_freq, decoder)` with JS8 and native-CF as two instantiations.
- **Answer flow**: panel row action `[Answer]` → `SetFreq(slot)` → `ConnectPeer` — all existing commands.

Sequencing decision to make explicitly: JS8-first (bigger discovery pool, plan approved) with the dwell scheduler written channel-agnostic, then native CF as a fast follow (S–M incremental). Building native-CF first would duplicate the plan's scheduler work.

### 4.3 VMail-style store-and-forward (V8) vs what we have

Three existing things *almost* do this, none does:

| Existing | What it gives | Why it isn't VMail |
|---|---|---|
| Winlink path (`openpulse-b2f`, `openpulse-b2f-driver`, `openpulse-gateway`) | Real store-and-forward mail | Depends on CMS/RMS infrastructure and internet; not peer-hosted |
| `RelayForwarder` + `RelayDataChunk` (`openpulse-core/src/relay.rs`, `wire_query.rs:0x05/0x06`) | Live multi-hop forwarding with hop ACKs, dup suppression, trust policy | Forwards *now* or never — no persistence, no "wait for recipient" |
| Daemon inbox (`ListMessages`/`GetMessage`, `MessageSummary`) | Local received-message store + panel UI | Local only; no parking at a third station, no notify-on-beacon |

The missing concept is a **peer-hosted mailbox with beacon-triggered delivery**:

1. **Park**: sender connects to relay R (normal HPX session), sends `MailPark { envelope }` where the envelope is `recipient, sender, body, timestamp` signed by the sender (existing Ed25519 identity; optionally encrypted to the recipient's key — something VarAC cannot offer, but see §5 on encryption rules: encryption of content over amateur RF is prohibited in most jurisdictions, so default to *signed-plaintext* and gate any encryption behind explicit non-amateur/network-use config).
2. **Store**: R persists it (`openpulse-mailbox` store crate or a module in the daemon next to `logbook.rs`); policy knobs: max messages/bytes per sender, TTL, trust filter via existing `TrustStore` (park only for `TrustedOrUnknown`, say).
3. **Notify**: when R hears the recipient — mesh beacon (`openpulse-mesh` upserts peers today), JS8 heartbeat (plan's `StationHeard`), or native CQ (§4.2) — it transmits a tiny `MailWaiting { relay, count }` notice. The JS8 plan's OPHF hint has 7 reserved bits; one could be a "mail waiting at me for you" flag piggybacked at zero TX cost.
4. **Collect**: recipient connects to R, sends `MailList`/`MailFetch`, verifies sender signatures locally (relay is untrusted for content — it only stores).
5. **Multi-hop**: VarAC's manual re-parking maps onto our route-discovery machinery (`RouteDiscoveryRequest`) for *automatic* placement later; v1 = manual, like VarAC.

Effort L: new store + 4–5 wire messages + daemon/panel surface + the notify wiring across three hearing paths. High EmComm value and a clean differentiation ("Winlink without infrastructure, with signatures"). Natural *after* the JS8 plan ships (hearing paths) and after V1 chat/V9 files define the in-session typed-payload convention.

### 4.4 Live keyboard chat (V1) + canned messages (V18)

We have sessions, ARQ, and rate adaptation but no conversational surface — the panel Messages tab is mail-shaped (`SendMessage{to,subject,body}`). Sketch:

- **Payload**: a `ChatLine { seq, text }` typed payload over the established HPX secure session (post-CONREQ/CONACK, so chat inherits authentication — VarAC chat is unauthenticated). Small lines ride `FecMode::ShortRs` (`transmit_with_fec_mode`) to keep latency low at SL2–SL4.
- **Panel**: a Chat tab activated by `RfConnectionChanged`; input box + history; canned-message buttons (V18) prefill/send — `[messages] canned` config list, F-key bindings via the existing iced keyboard handling.
- **Typing indicator**: optional 1-byte status frame, rate-limited and auto-suppressed when the OTA ladder sits at SL2/SL3 (VarAC suppresses it at low SNR for the same reason — TX time is precious). Honestly a v2 nicety, not v1.
- **QSO summary on disconnect** (VarAC V6.2.4): we already have the ADIF logbook hook on session end — append a chat transcript path to the log record.

Effort M total, and it converts the existing machinery into the product category people compare us to VarAC on.

### 4.5 Alerting + external notifications (V11, V29)

Smallest work, broadest daily payoff. The daemon already emits typed NDJSON `ControlEvent`s to every control-port client — VarAC's third-party ecosystem (Telegram bot, log mapper) exists because people scrape its display; ours would exist because the control port is a real API. Missing pieces: (a) a keyword/alert-tag matcher in the daemon (config: `[alerts] tags = [{ pattern, severity, sound }]`) emitting a first-class `Alert` event; (b) panel toast + sound on `Alert`; (c) document a webhook example (`curl` the control port → ntfy/Telegram) instead of building bespoke integrations. Effort S.

---

## 5. What NOT to copy

- **The VARA modem, or interop with it.** VARA is closed-source, license-fee proprietary, spec-unpublished. Reverse-engineering it for interop is legally and etiquette-fraught and philosophically backwards for us — our answer to VARA is the open HPX waveform family. (Already the settled position in `docs/dev/research/vara-research.md`.)
- **VarAC's wire formats.** VarAC is closed freeware with undocumented framing (VMail packets, broadcast format, tags). Do not attempt VMail/broadcast interop; build open, documented equivalents (`docs/dev/design/protocol-wire-spec.md` is the register). Interop energy is better spent on standards we can verify from source: JS8 (planned), Winlink B2F (shipped), KISS/AX.25 (shipped).
- **The AI gateway (V26) and web-browsing bridge (VRWS in V29).** Relaying arbitrary internet/AI content over amateur RF sits badly with third-party-traffic, music/broadcast-content, and pecuniary-interest rules across jurisdictions, and it is off-target for an open *protocol* project. If someone wants it, the control port + email gateway pattern lets them build it externally; we should not ship it.
- **Unauthenticated remote commands.** VarAC's `<INFO>`/`<GPSR>` tags execute on the remote station with only a blocklist. Any OpenPulse remote-query verb must default to responding only inside an authenticated session or with signed queries, with per-field privacy config — our trust machinery exists precisely for this.
- **Content encryption by default.** A signed-*and-encrypted* VMail is technically easy for us (ML-KEM is already in-tree) but content encryption over amateur RF is prohibited in most jurisdictions. Sign always; encrypt never by default; document the distinction in `docs/regulatory.md` when V8/V9 land.
- **Gamification/social layer (V28)** — awards, games, reactions, diplomas. Community-building for a commercial-adjacent freeware product; not protocol work. Skip without prejudice.
- **Windows-ecosystem rig plumbing (OmniRig)** and Wine-oriented UI accommodations — rigctld covers our platforms.
- **Centralized single-point services.** VarAC's strength we *should* copy is the decentralized email gateway idea; its website-account-based features (hall of fame, superstation scoring servers) we should not.

---

## 6. Recommendations

**Adopt** (ranked):

1. **Direct P2P file transfer** (V9, effort M) — highest value-to-effort in the table; 90% substrate exists (SAR + manifest + compression + ARQ); standalone (no JS8-plan dependency); closes the most-cited VarAC comparison. Includes size-gated auto-accept + verify badge.
2. **Alert tags + `Alert` event + canned messages** (V11/V18/V29, effort S+S) — trivially small, big daily-use and ecosystem payoff; standalone; do it opportunistically.
3. **Native CQ/calling-frequency flow with slots** (V2/V3/V5, effort M incremental) — **as a follow-on to the JS8 rendezvous work**: build the plan's dwell scheduler channel-agnostic, then add the native CQ frame + answer-QSYs-to-announced-slot flow and generalize the Discovery tab.
4. **Live keyboard chat surface** (V1, effort M) — converts existing session machinery into VarAC's product category; natural companion to #1 (same typed-payload convention, same panel investment).

**Consider** (in rough order): VMail store-and-forward with beacon-triggered notification (V8 — high value but L effort; schedule after JS8 plan + files/chat define the payload conventions); PSK Reporter spotting from the discovery StationTable (V16 — S–M, adoption visibility; revisit the JS8 plan's non-goal once StationTable exists); decentralized SMTP/IMAP gateway mode (V12 — M, redundancy for the existing Winlink path); live GPS/position sharing (V15 — M, EmComm/portable); frequency scheduler (V21 — S–M, shares the dwell/QSY machinery); EmComm preset bundling the above policies (V13 — S once dependencies exist); per-peer SNR history graphs (V23 — S–M).

**Skip**: VARA interop, VarAC wire-format interop, AI gateway/web bridge, unauthenticated remote commands, default content encryption, gamification, OmniRig, BBS hosting (revisit only after V9 + a hardened allowlist story), multi-instance PTT coordinator.

**Sequencing note**: the JS8 discovery/rendezvous plan is the spine — V2/V3/V5, V8's notification trigger, V14's query verbs, and V16's spot source all reuse its scheduler, StationTable, and panel tab. Items 1, 2, and 4 above are independent of it and can proceed in parallel.
