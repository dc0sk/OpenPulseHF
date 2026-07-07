---
project: openpulsehf
doc: docs/openpulse-abstract.md
status: living
last_updated: 2026-07-07
---

# OpenPulseHF — abstract

**OpenPulseHF is an open-source, software-defined data modem and messaging stack for HF (shortwave)
radio.** It turns an ordinary SSB transceiver and a computer sound interface into a resilient digital
link that can carry text, files, and structured messages over hundreds to thousands of kilometres —
**without any intervening infrastructure**: no cell towers, no internet, no satellites. It is written
entirely in Rust for reliability and portability (Linux, macOS, Windows, and Raspberry Pi), and its
waveforms, forward-error-correction, and security are open and inspectable.

At its core is an **adaptive modem**: it continuously measures the channel and steps its speed and
modulation up or down to keep a link alive as propagation changes — from robust, slow modes that get
through when conditions are poor, to high-throughput modes when they are good. Messages are protected
by forward error correction and automatic retransmission, and station identities and message integrity
are protected by modern cryptographic signatures, including **post-quantum-safe** options. It
interoperates with existing amateur infrastructure (ARDOP, KISS/AX.25, Winlink/B2F) and adds its own
mesh relaying, frequency-agility, and an operator control interface.

The following notes describe what OpenPulseHF offers to different audiences.

## For radio amateurs

A modern, open alternative to proprietary HF data modes. OpenPulseHF gives you an adaptive speed
ladder (BPSK/QPSK/8PSK/QAM, OFDM, SC-FDMA and pilot-aided waveforms), strong FEC, and ARQ, plus
compatibility bridges so it works with the tools you already run — Pat/Winlink over ARDOP, KISS TNC
applications, and AX.25. It runs on a Raspberry Pi at a field site, keys most transceivers over CAT,
includes a live operator panel with waterfall and constellation displays, and needs no closed-source
components. Frequency-agility (QSY) and multi-hop relaying let stations route around interference and
extend reach.

## For IT and communications professionals

A clean, layered, testable Rust codebase (see the [OSI layer map](osi-layer-map.md)): a plugin modem
core, a transport layer with segmentation and rate-adaptive ARQ, a network layer with trust-weighted
multi-hop relaying and peer discovery, and a session/security layer with signed handshakes and a
post-quantum option. It ships with a hardware-free channel simulator (Watterson fading, burst-error
and noise models) so the whole stack — from bits to waveform — is continuously tested in CI without a
radio. Secrets are stored in the OS keychain or an Argon2id-encrypted keystore, key files are
permission-checked, and the operator control channel can be authenticated and encrypted with a
pre-shared key. It is a dependable last-resort data bearer for when IP networks are unavailable.

## For authorities and civil service

An **infrastructure-independent communications capability**. When power, cellular, and internet
service are degraded or absent — natural disaster, major outage, or a deliberate attack on
infrastructure — OpenPulseHF provides a low-cost, long-range data path that runs on modest hardware
and open standards. Because it is open source, it can be audited, self-hosted, and maintained without
dependence on a single vendor, and it can be operated within existing amateur and government radio
allocations. It is well suited to business-continuity and emergency-preparedness planning as a
fallback messaging bearer.

## For civil protection, police, and rescue services (EmComm)

A practical tool for **emergency communications** when primary networks fail. OpenPulseHF carries
structured messages, forms, and small files over HF between fixed and field stations, adapting
automatically to changing propagation so operators can concentrate on the incident rather than the
radio. Multi-hop relaying extends coverage into shadowed areas, frequency-agility routes around
interference, and Winlink compatibility connects into established emergency-message workflows. It runs
on portable, battery-friendly hardware (e.g. Raspberry Pi), supports automatic station identification
for regulatory compliance, and keeps a per-contact log — making it deployable by trained volunteer and
professional EmComm teams alike.

## For military and defence

A **resilient, low-probability-of-dependence bearer** for degraded or contested environments. Beyond
the reach of terrestrial and satellite infrastructure, OpenPulseHF provides adaptive HF data with
modern forward error correction and channel equalization for difficult, fading paths. Its security
model is explicit and inspectable: authenticated station handshakes, message-integrity signatures, an
opt-in **post-quantum-safe** signature and key-establishment path (ML-DSA / ML-KEM), and an
authenticated, encrypted operator control channel with OS-keychain or master-password key storage.
Being open source, it is auditable and free of vendor lock-in, and it can be tailored and hardened for
specific operational requirements.

---

*OpenPulseHF is amateur-radio and research software. Operation is subject to the radio-licensing and
regulatory rules of each jurisdiction (e.g. permitted bands, modes, occupied bandwidth, encryption
restrictions, and station identification). Users are responsible for lawful operation. Encryption
features such as the control-channel PSK secure the local operator link and are independent of any
restrictions on obscuring the meaning of on-air amateur communications.*
