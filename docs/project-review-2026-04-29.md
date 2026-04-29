---
project: openpulsehf
doc: docs/project-review-2026-04-29.md
status: review
last_updated: 2026-04-29
---

# OpenPulseHF Project Review — 2026-04-29

## 1. Project Understanding

OpenPulseHF is an open-source, plugin-based software modem written in Rust for amateur radio HF and VHF data communication over soundcard. It is designed as an independent, first-principles alternative to established commercial modems (VARA, PACTOR, ARDOP), prioritising open protocols, reproducible benchmarks, and hardware-independent development.

### Architecture at a Glance

The workspace is organised into four core crates and two modulation plugins:

| Crate / Plugin | Role |
|---|---|
| `openpulse-core` | Shared traits, frame format, FEC codec, HPX state machine, trust, relay, peer cache |
| `openpulse-audio` | Audio backends: in-process loopback and CPAL (ALSA / CoreAudio / WASAPI) |
| `openpulse-modem` | Engine wiring plugins and audio together; benchmark harness; diagnostics |
| `openpulse-cli` | Single binary (`openpulse`) implementing all user-facing commands via clap |
| `bpsk-plugin` | BPSK 31/63/100/250 with NRZI and raised-cosine pulse shaping |
| `qpsk-plugin` | QPSK 125/250/500 with Gray mapping |

A separate standalone service, `pki-tooling`, provides REST-based station identity publication, peer discovery, key lifecycle management, and trust evaluation backed by PostgreSQL.

### What Has Been Built So Far

The project is in early alpha (v0.1.0) and has made solid progress for its stage:

- Loopback-first validation pipeline with hardware-free CI smoke tests
- Reed-Solomon FEC codec integrated into the modem engine
- BPSK and QPSK plugin families with hardening test matrices
- HPX session state machine with 10-path conformance test coverage
- PKI service with 9-migration database schema, moderation workflow, and audit trail
- ARM64 cross-compilation and Raspberry Pi 4/5 as first-class deployment targets
- Living documentation framework (38 files) with CI-validated frontmatter
- Reproducible benchmark harness with JSON schema and regression gates

---

## 2. Improvement Suggestions

### 2.1 Requirements

**What is good:** The requirements document covers functional, platform, non-functional, security, performance, compatibility, and documentation concerns. The prohibition on proprietary-compatibility claims without legal review is well-placed.

**Areas for improvement:**

**Acceptance criteria are missing.** Every requirement states what the system shall do, but none define how to verify it is done. Without measurable acceptance criteria, it is impossible to declare a requirement "closed" with confidence. Each requirement should carry a corresponding test reference or observable outcome (e.g. "Validated by `fec_loopback` integration test suite with 100% pass rate across 20 BER scenarios").

**Error budget and graceful degradation are undefined.** Requirements cover the happy path (nominal channel, nominal hardware) but do not define what acceptable degraded behaviour looks like at the boundary of the operating envelope — for example, minimum SNR at which the modem should still decode, or maximum frame-loss rate before the session should abort. These thresholds drive important design decisions in the ARQ and state-machine layers.

**Post-quantum requirements lack a timeline and priority order.** ML-DSA and ML-KEM support are listed as requirements but there is no stated priority relative to the rest of the roadmap, no evaluation of the computational cost on the Pi 4/5 target, and no decision on whether hybrid mode is a permanent feature or a migration step with a defined end-state.

**GPU acceleration requirement has no measurable acceptance criterion.** The requirement states "when it produces measurable benefit" but does not define who measures it, against what baseline, or what the minimum improvement threshold is for the feature to be included in a release.

**PKI requirements are structurally decoupled from core modem requirements.** The PKI service is a separate subdirectory with its own requirements document, but the coupling points — when the CLI invokes the PKI service, what happens when the service is unavailable, how the modem falls back — are not captured in either document. There should be at least one cross-document requirement specifying offline/degraded-PKI behaviour.

---

### 2.2 Research

**What is good:** The VARA research note exists and the project correctly treats proprietary compatibility as a legal risk. The decision to use open frameworks (wgpu for GPU, ML-DSA/ML-KEM for PQ) reflects sound technology choices.

**Areas for improvement:**

**Channel models are informal and incomplete.** BER injection in the test suite is a useful starting point but it does not represent realistic HF channel behaviour. The ITU-R and CCIR define standardised HF propagation channel models (e.g. CCIR Poor, CCIR Good, ITU-R F.520 urban). Adopting at least two of these as named channel profiles in the benchmark harness would make performance claims defensible and comparable to published literature on other modem implementations.

**No research note for ARDOP.** VARA is documented; PACTOR and ARDOP are referenced in passing. A brief research note on ARDOP's open specification (it is public-domain) would help clarify which architectural decisions OpenPulseHF shares with, or diverges from, each incumbent — and which test conditions are directly comparable.

**DSP algorithm choices are not referenced.** The raised-cosine filter parameters, Reed-Solomon block size (ECC_LEN=32), and NRZI encoding choice are reasonable but undocumented in terms of tradeoffs. A brief design rationale note (not necessarily a full paper survey) for each choice would help reviewers and future contributors understand whether these can be changed without breaking wire compatibility.

**PQ cryptography on embedded targets needs a cost study.** ML-DSA and ML-KEM key sizes and signing latencies are materially different from Ed25519. On a Raspberry Pi 4 (Cortex-A72), ML-DSA-65 signature verification takes roughly 10–30x longer than Ed25519. There is currently no estimate of whether this fits within the session-state machine timing budget.

**GPU offload candidate identification is listed as future work but has no research artefact.** Before committing engineering time, a short analysis of which DSP operations in the signal pipeline have sufficient arithmetic intensity to benefit from GPU offload (typically convolution, FFT, matched filter) would prevent over-engineering. The wgpu compute shader dependency already present in `bpsk` dev-deps signals intent, but there is no document capturing the expected speedup or minimum payload size where GPU launch overhead pays off.

---

### 2.3 Testing

**What is good:** The loopback-first philosophy is the right call for a CI-friendly project. The fixture-matrix approach in `bpsk_hardening` and `qpsk_hardening` is thorough. HPX conformance tests covering 10 major state paths demonstrate serious commitment to protocol correctness.

**Areas for improvement:**

**The main CI pipeline is disabled.** `ci.yml` has `if: false` on the `build-and-test` job that covers Ubuntu and macOS. Only the Pi smoke-test profile and ARM64 cross-compilation check are active. This means that every PR is merged without a full multi-platform test run, which is a significant regression risk as the codebase grows. Re-enabling the matrix build — even in a non-blocking advisory mode — should be prioritised.

**`openpulse-cli/src/main.rs` (2,592 lines) is not meaningfully unit-testable.** The entire CLI is implemented as one flat file. Individual subcommand handlers cannot be invoked independently in tests. This file has only 3 integration tests. Breaking it into one module per subcommand group (e.g. `cli/transmit.rs`, `cli/session.rs`, `cli/trust.rs`) would allow unit tests for argument parsing logic and command dispatch, separate from end-to-end I/O tests.

**No property-based testing.** Frame format serialisation/deserialisation, FEC codec, and trust evaluation logic are ideal candidates for property-based testing with `proptest` or `quickcheck`. Properties like "encode then decode round-trips for any payload" or "FEC corrects up to 16 byte errors for any byte error pattern" are strong invariants that hand-written fixture matrices cannot exhaust.

**No fuzzing.** The frame parser, envelope deserialiser, and PKI API handlers process untrusted inputs. Fuzzing with `cargo-fuzz` (libFuzzer) or `afl.rs` would surface panics, incorrect rejections, and off-by-one errors that structured test cases typically miss.

**Channel simulation is too idealised.** BER injection assumes independent uniformly-distributed errors. Real HF channels produce burst errors from fading, frequency-selective distortion from multipath, and timing drift from Doppler. At minimum, a burst-error model (Gilbert-Elliott model) should be added to the test harness alongside the current uniform BER injection.

**Benchmark gates are disconnected from physical channel models.** The current regression gate checks `mean_transitions ≤ 20.0` and a 100% pass rate against the loopback scenarios. These thresholds are structural correctness checks, not performance targets. There is no gate that validates goodput against a defined noisy channel at a stated SNR — which is what would make the benchmark results meaningful to an external reviewer.

---

### 2.4 Architecture and Design

**What is good:** The workspace crate separation is clean and well-motivated. The plugin trait with `PluginInfo` metadata is a sound extensibility point. Loopback and CPAL backends behind a common `AudioBackend` trait is textbook strategy pattern. The benchmark harness with JSON schema and result artefacts is a mature engineering practice for this stage.

**Areas for improvement:**

**`openpulse-core` carries heavy cryptographic dependencies.** The crate description says "no heavy dependencies for embeddability", but `hkdf`, `sha2`, and `x25519-dalek` (with `static_secrets`) are unconditionally included. Either the embeddability claim should be qualified (e.g. "embeddable with `no-default-features`") or the cryptographic functionality should be feature-gated or moved to a `openpulse-crypto` crate that core depends on optionally.

**Frame payload is capped at 255 bytes with no segmentation protocol.** The frame format uses a `u8` length field. All real data transfers will therefore require multi-frame segmentation. There is no defined segmentation and reassembly (SAR) sub-layer in the frame format specification or in the HPX state machine documentation. Without a SAR protocol, the maximum meaningful transfer unit is 255 bytes, and any larger transfer relies on application-layer logic that is not yet specified.

**`main.rs` is a monolith.** At 2,592 lines, the CLI binary conflates argument parsing, business logic, HTTP client calls, file I/O, and audio device management in a single file. This makes code navigation, testing, and future refactoring significantly harder. The standard Rust CLI pattern separates command modules into a `commands/` subdirectory under `src/`, one file per subcommand group.

**Plugin loading is compile-time, not runtime.** Despite the "plugin" framing, plugins are statically linked (`bpsk-plugin` is a direct dependency of `openpulse-modem`; `qpsk-plugin` is a dev-dependency). There is no `dlopen`-based or WASM-based dynamic plugin system. This is a pragmatic choice for the current stage, but the architecture document and plugin contribution guide imply runtime extensibility. The gap between the stated model ("contribute a plugin") and the actual integration path (add to Cargo.toml and recompile) should be documented explicitly.

**PKI service integration is implicit.** The CLI uses `reqwest::blocking` to call the PKI REST API. The PKI service URL is presumably a CLI flag or environment variable, but there is no specified fallback for when the service is unreachable, no timeout policy documented, and no definition of which CLI operations are PKI-optional vs PKI-mandatory. Blocking HTTP calls in the same thread that manages audio timing is also a latency risk.

**HPX state machine complexity.** `hpx.rs` is 15 KB with nested match arms and manual state transitions. As new states are added (relay, multi-hop, adaptive coding), this will become error-prone. A typestate encoding or a lightweight state-machine library would make illegal state transitions a compile error rather than a runtime assertion.

**QPSK is a dev-dependency in `openpulse-modem`.** The `qpsk-plugin` crate is listed under `[dev-dependencies]` in `openpulse-modem/Cargo.toml`, which means it is excluded from production builds. The path to making QPSK a first-class production plugin is not defined in the roadmap.

**No configuration file support.** All parameters are passed via CLI flags. Complex or recurring configurations (device name, mode, PKI URL, trust policy profile) require long command lines or wrapper scripts. A TOML-based configuration file with a defined precedence order (file < environment variable < CLI flag) would significantly improve operator UX, especially on embedded deployments where the modem runs as a service.

---

### 2.5 Summary Priority Table

| Area | Issue | Suggested Priority |
|---|---|---|
| Testing | Re-enable multi-platform CI pipeline | High |
| Architecture | Split `main.rs` into per-subcommand modules | High |
| Testing | Add property-based tests for frame codec and FEC | Medium |
| Requirements | Add acceptance criteria to each requirement | Medium |
| Architecture | Define frame SAR sub-layer or increase length field | Medium |
| Testing | Add burst-error (Gilbert-Elliott) channel model to benchmark | Medium |
| Research | Conduct PQ crypto cost study on Pi 4/5 | Medium |
| Architecture | Feature-gate crypto in `openpulse-core` or extract to separate crate | Medium |
| Design | Add TOML configuration file support | Medium |
| Architecture | Specify PKI service fallback behaviour and timeout policy | Medium |
| Architecture | Document compile-time-only plugin model explicitly | Low |
| Research | Add ARDOP research note | Low |
| Testing | Add fuzzing targets for frame parser and PKI API | Low |

---

### 2.6 Delivery Governance and Sequencing

**Milestone-level definition of done is missing.** The roadmap and backlog identify useful workstreams, but there is no explicit release-gate checklist that determines when a milestone can be declared complete.

Each milestone should include a lightweight, explicit closure gate with at least:

- CI status (required jobs and pass criteria)
- Test coverage scope (unit/integration/property/fuzz where applicable)
- Benchmark evidence (including at least one noisy-channel profile where performance claims are made)
- Documentation updates (architecture, CLI guide, and changelog/release notes)

**Recommended execution sequence (next 2-4 weeks):**

1. Re-enable the multi-platform CI matrix in advisory mode, then make it blocking after stability is confirmed.
2. Refactor `openpulse-cli/src/main.rs` into subcommand modules without changing behavior.
3. Add measurable acceptance criteria to requirements and link each to tests or benchmark artefacts.
4. Add a burst-error channel model and one noisy-channel benchmark regression gate.
5. Specify and document frame segmentation/reassembly (SAR) before larger payload workflows expand.
