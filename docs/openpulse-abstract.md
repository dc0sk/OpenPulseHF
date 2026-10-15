---
project: openpulsehf
doc: docs/openpulse-abstract.md
status: living
last_updated: 2026-10-15
---

# OpenPulseHF — abstract

**OpenPulseHF is an open-source, software-defined data modem and messaging stack for HF (shortwave)
and VHF radio.** It turns an ordinary SSB transceiver and a computer sound interface into a digital
link for text, files and structured messages. On HF that link can span hundreds to thousands of
kilometres **without intervening infrastructure**: no cell towers, no internet, no satellites. It is
written in Rust, developed and tested on Linux including the Raspberry Pi (CI also builds it on
macOS), and its waveforms, forward error correction and security are open and inspectable.

At its core is an **adaptive modem**: it measures the channel and steps its speed and modulation up
or down as propagation changes. Its one rate ladder, `hpx_hf`, runs from a robust 16-tone MFSK mode
at about 9 bit/s through coded BPSK and differential QPSK to a 52-subcarrier OFDM waveform at up to
7.7 kbit/s, with forward error correction on every rung and soft-combining retransmission. Two
profiles choose how much of it to use: `fast` (the whole ladder, up to ≈2 kHz wide) and `robust`
(capped at 500 Hz, single-carrier). Station identities are protected by signed handshakes.

**Release status.** The first release, **Release 1 (`v0.17.0`)**, is in preparation; the latest
tagged release is v0.16.0. Release 1 covers the adaptive modem, the background daemon and command
line, an ARDOP-compatible TNC for Pat/Winlink, session compression, PTT control, station
identification and the signed Ed25519 handshake. It is being validated on air on 2 m first, and on
HF with the release candidate. The workspace also contains a broader stack — KISS/AX.25, a direct
Winlink gateway, mesh relaying, frequency agility (QSY), JS8-based station discovery, file transfer,
a post-quantum handshake and a graphical operator panel — which is **experimental and not part of
Release 1**. The notes below say which is which.

## For radio amateurs

An open alternative to proprietary HF data modes. Release 1 gives you the adaptive `hpx_hf` ladder
(MFSK16, BPSK, differential QPSK, OFDM up to 64QAM) with FEC on every rung and ARQ, and an
ARDOP-compatible TNC so Pat can send Winlink mail through it. It runs on a Raspberry Pi, keys
transceivers by serial RTS/DTR, CM108, GPIO, VOX or rigctld, and needs no closed-source components.
Experimental, outside Release 1: KISS TNC applications and AX.25, QSY frequency agility, multi-hop
relaying, a live operator panel with waterfall and constellation displays, and many further
waveforms (8PSK, QAM, SC-FDMA, pilot-aided) selectable as fixed modes.

## For IT and communications professionals

A layered, testable Rust codebase (see the [OSI layer map](osi-layer-map.md)): a plugin modem core,
a transport layer with segmentation and rate-adaptive ARQ, and a session layer with signed
handshakes. It ships with a hardware-free channel simulator (Watterson fading, burst-error and noise
models), so the stack from bits to waveform is tested in CI without a radio. Secrets can be held in
the OS keychain or an Argon2id-encrypted keystore, and the operator control channel can be
authenticated and encrypted with a pre-shared key. Experimental, outside Release 1: trust-weighted
multi-hop relaying, peer discovery and a post-quantum handshake option.

## For authorities and civil service

An **infrastructure-independent communications capability**. When power, cellular and internet
service are degraded or absent, OpenPulseHF provides a low-cost, long-range data path on modest
hardware and open standards. Because it is open source it can be audited, self-hosted and
maintained without dependence on a single vendor, and operated within the radio allocations a user
is licensed for. It suits business-continuity and emergency-preparedness planning as a fallback
messaging bearer, once Release 1 has completed its on-air validation.

## For civil protection, police, and rescue services (EmComm)

A tool for **emergency communications** when primary networks fail. OpenPulseHF carries messages
and small files between fixed and field stations and adapts to changing propagation automatically,
so operators can concentrate on the incident rather than the radio. Through Pat and the ARDOP TNC it
connects into Winlink message workflows. It runs on portable hardware such as a Raspberry Pi,
supports automatic station identification, and can keep a per-contact ADIF log (opt-in). Experimental, outside
Release 1: multi-hop relaying into shadowed areas, frequency agility around interference, and direct
file transfer between stations.

## For military and defence

A **resilient data bearer** for degraded or contested environments, beyond the reach of terrestrial
and satellite infrastructure: adaptive HF data with modern forward error correction and channel
equalisation for fading paths. Its security model is explicit and inspectable: authenticated
station handshakes, message-integrity signatures, and an authenticated, encrypted operator control
channel with OS-keychain or master-password key storage. A post-quantum signature and
key-establishment path (ML-DSA / ML-KEM) exists but is experimental and not part of Release 1. Being
open source, it is auditable, free of vendor lock-in, and can be tailored to specific requirements.

---

*OpenPulseHF is amateur-radio and research software. Operation is subject to the radio-licensing and
regulatory rules of each jurisdiction (e.g. permitted bands, modes, occupied bandwidth, encryption
restrictions, and station identification). Users are responsible for lawful operation. Encryption
features such as the control-channel PSK secure the local operator link and are independent of any
restrictions on obscuring the meaning of on-air amateur communications.*
