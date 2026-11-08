//! Receiver-led, per-direction adaptive rate control with RX lockstep.
//!
//! On a two-way link each *data receiver* leads the rate for the direction it
//! receives: it measures channel quality, picks an **absolute** target speed
//! level, and ships that level to the sender in the ACK
//! ([`crate::ack::AckFrame::recommended_level`]). The sender simply follows.
//!
//! ## Lockstep invariant
//!
//! The receiver advances its recommendation at most **one mapped step** above the
//! highest level it has actually decoded (`rx_confirmed`). So the demodulation
//! candidate set `{rx_recommended, rx_confirmed}` is always exactly the 1–2 modes
//! the sender could be transmitting:
//!
//! - sender adopted the recommendation → it sends at `rx_recommended`;
//! - the recommending ACK was lost → the sender still uses the last level it was
//!   told, which is `rx_confirmed`.
//!
//! Because the node that *decides* the mode is the node that *demodulates* it, a
//! lost ACK can never desync the two ends — it only delays the climb by one frame.
//!
//! The same holds going down only if every step is announced, so a failure moves the ladder only
//! when its NACK is keyed ([`NackVerdict`], #1460).
//!
//! ## Two ways up: SNR, and evidence
//!
//! The climb has two independent triggers, and the invariant above holds for both because each
//! advances **exactly one mapped step** above `rx_confirmed`:
//!
//! - **SNR** — the measured SNR clears the confirmed rung's ceiling. Fast, but only as trustworthy
//!   as the estimate.
//! - **Success** — [`ACK_CLIMB_THRESHOLD`] consecutive clean decodes at the confirmed rung. Slower,
//!   but it cannot lie: frames decoding *is* the evidence that the rung works.
//!
//! The second exists because the first is not always available. An SNR estimator on a fading channel
//! can be uninformative *in principle* — at 31 baud a 1 Hz Doppler fade decorrelates in ~6 symbols,
//! so no window is both short enough to track the fade and long enough to average the noise, and the
//! estimate reads a constant. With SNR as the only permission to climb, such a link sat pinned on its
//! entry rung **while every single frame decoded** (issue #934): `hpx_hf` on Watterson `moderate_f1`
//! delivered 20/20 frames at ~5 bps, where the rungs it could have reached carry ~300–1200.
//! Ignoring a perfect decode record because a number did not move is the bug; SNR is an accelerator,
//! not the sole permission.
//!
//! The SNR *downshift* is deliberately left as the sole fast-down path and is checked first — a rung
//! that decodes is not evidence that a *higher* rung will, so success may only ever propose the next
//! step, and only when SNR does not already say we are too high.
//!
//! This is pure logic: no I/O, no engine coupling, fully unit-tested. The modem
//! engine drives it by reporting which candidate decoded and the measured SNR.

use crate::ack::AckType;
use crate::fec::FecMode;
use crate::profile::SessionProfile;
use crate::rate::SpeedLevel;
use serde::{Deserialize, Serialize};

/// Consecutive clean decodes at the confirmed level that justify a climb with no SNR evidence.
///
/// Mirrors the profile's `nack_threshold` (the same hysteresis in the other direction) and is a
/// constant rather than a profile field for the same reason `nack_threshold` defaults to 3: the
/// right value is a property of the ARQ loop, not of the waveform ladder. It buys the climb an
/// airtime cost of at most one failed frame per N successful ones, so it must not be small enough to
/// thrash nor large enough to make the climb invisible on a short session.
pub const ACK_CLIMB_THRESHOLD: u8 = 3;

/// After a fall-back from a rung that went silent, the sender declines this many climbs back to it
/// before trying again (`ladder-climb-into-silence.md`, review S2): the receiver re-proposes the climb
/// every `ACK_CLIMB_THRESHOLD` decodes, and each try costs two silent ACK windows.
const TX_CLIMB_BACKOFF_FIRST: u8 = 4;
/// The most climbs declined after repeated fall-backs from the same rung (the count doubles each time).
const TX_CLIMB_BACKOFF_MAX: u8 = 32;

/// After a keyed failure at a probe rung, the receiver declines this many would-be climbs back to it
/// before probing again (`ota-climb-memory.md`). Not fitted: it mirrors `TX_CLIMB_BACKOFF_FIRST`; the
/// fallback if HF falsifies it is 1 or 2 (review R3).
const RX_CLIMB_BACKOFF_FIRST: u8 = 4;
/// The most would-be climbs declined after repeated failed probes of one rung (doubles per repeat).
const RX_CLIMB_BACKOFF_MAX: u8 = 16;
/// Clean airtime at the anchor after which a first backoff lets the rung be probed again. Below SL7
/// this binds before the count does: 120 s is 2–6 frames there (review R2).
const RX_CLIMB_BACKOFF_EXPIRY_FIRST_S: f32 = 120.0;
/// The expiry doubles with the backoff level up to this cap: a dead SL3 ceiling costs ~14 % of
/// airtime in re-probes, and a recovered one waits at most ~350 s (review R2, option c).
const RX_CLIMB_BACKOFF_EXPIRY_MAX_S: f32 = 240.0;

/// Decoded airtime in one streak that proves the confirmed rung with a single decode: just under
/// one 66.56 s SL2 frame, so it means "one SL2 frame" (`ota-climb-memory.md`, change 2). Re-read it
/// if the SL2 frame length changes.
const CLIMB_EVIDENCE_AIRTIME_S: f32 = 60.0;

/// The receiver's memory of a probe that failed: the climbs to `rung` it still declines.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RxClimbBackoff {
    /// The rung whose probe failed.
    pub rung: SpeedLevel,
    /// The confirmed rung the probe started from.
    pub anchor: SpeedLevel,
    /// Would-be climbs to `rung` still to decline.
    pub left: u8,
    /// This backoff's level: the count it started from, doubling per failed re-probe.
    pub n: u8,
    /// Clean airtime decoded at the anchor since the withdraw, in seconds.
    pub airtime_s: f32,
}

impl RxClimbBackoff {
    /// The clean airtime after which the rung may be probed again: doubles with `n`, capped.
    fn expiry_s(&self) -> f32 {
        (RX_CLIMB_BACKOFF_EXPIRY_FIRST_S * f32::from(self.n) / f32::from(RX_CLIMB_BACKOFF_FIRST))
            .min(RX_CLIMB_BACKOFF_EXPIRY_MAX_S)
    }
}

/// Outcome of demodulating a received data frame, fed to [`OtaRateController::on_rx_frame`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RxOutcome {
    /// A candidate mode decoded cleanly at the given speed level, with measured SNR.
    Decoded(SpeedLevel),
    /// No candidate decoded — treat as a NACK.
    Failed,
}

/// Whether the NACK for a failed frame will be keyed, as the receiver's NACK budget decides before
/// the decode (#1460 round 4b, `docs/dev/design/failed-burst-ownership.md`).
///
/// The ladder moves only on a failure the peer hears about. A step taken on a muted failure is
/// silent: the candidates leave the rung the sender was last told, so its frames fail too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NackVerdict {
    /// The NACK is keyed within the budget. Today's behaviour, and the default for callers with no
    /// budget (the link simulator, the unit tests).
    #[default]
    Keyed,
    /// The NACK is keyed on a leak credit after a mute. It follows at least `nack_threshold`
    /// counted failures, so it steps on its own: the hysteresis already ran, unannounced.
    Leaked,
    /// No NACK is keyed: the failure moves nothing.
    Muted,
}

/// Which branch of [`OtaRateController::on_rx_frame`] produced a recommendation.
///
/// Reporting the *reason* is the point, not the resulting level. The rate controller is among the
/// most-corrected mechanisms here — #934 alone is three recorded occurrences of an SNR estimator
/// counting a fade as noise — and its fix added branches (`ClimbOnEvidence`, the fast downshift)
/// whose whole purpose is to act correctly *when the SNR estimate is uninformative*. A level trace
/// alone cannot tell "the controller is working" from "the estimator is broken and the controller
/// is compensating for it"; a stream of `ClimbOnEvidence` with no `ClimbOnSnr` says the estimate is
/// carrying no information, which is a diagnosis rather than a symptom.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RateDecision {
    /// Session is locked to a fixed level; both directions stay pinned.
    Locked,
    /// Failure, and the SNR estimate already explains it — jump straight to the SNR-adequate
    /// level rather than crawling down one rung per NACK threshold.
    FastDownshift,
    /// Failure the SNR does not explain; the consecutive-NACK hysteresis is counting and the
    /// recommendation has not moved yet.
    NackHold,
    /// Failure the SNR does not explain, at the NACK threshold or on a leaked NACK — step the
    /// anchor down one rung.
    NackStepDown,
    /// Decode, and the SNR estimate clears the confirmed rung's ceiling — climb one step.
    ClimbOnSnr,
    /// Decode, and `ACK_CLIMB_THRESHOLD` clean decodes in a row prove the rung — climb one step
    /// on evidence. This is the path that works when the estimate is uninformative in principle.
    ClimbOnEvidence,
    /// Decode with neither a clearing SNR nor a proven streak — hold at the confirmed level.
    Hold,
    /// Keyed failure while a probe is outstanding — withdraw to the confirmed rung, keep the anchor,
    /// and remember the rung (`ota-climb-memory.md`, change 1).
    ProbeWithdrawn,
    /// Decode that would climb to a rung whose probe recently failed — hold, and count the wait down.
    ClimbDeclined,
}

impl RateDecision {
    /// Every variant, for exhaustive sweeps.
    pub const ALL: [RateDecision; 9] = [
        RateDecision::Locked,
        RateDecision::FastDownshift,
        RateDecision::NackHold,
        RateDecision::NackStepDown,
        RateDecision::ClimbOnSnr,
        RateDecision::ClimbOnEvidence,
        RateDecision::Hold,
        RateDecision::ProbeWithdrawn,
        RateDecision::ClimbDeclined,
    ];

    /// Dense index. The exhaustive `match` is the enforcement: adding a variant stops this
    /// compiling, so the domain cannot grow without `ALL` being reconsidered.
    pub fn all_index(self) -> usize {
        match self {
            RateDecision::Locked => 0,
            RateDecision::FastDownshift => 1,
            RateDecision::NackHold => 2,
            RateDecision::NackStepDown => 3,
            RateDecision::ClimbOnSnr => 4,
            RateDecision::ClimbOnEvidence => 5,
            RateDecision::Hold => 6,
            RateDecision::ProbeWithdrawn => 7,
            RateDecision::ClimbDeclined => 8,
        }
    }

    /// Whether this decision was reached on a failed demodulation.
    pub fn is_failure_path(self) -> bool {
        matches!(
            self,
            RateDecision::FastDownshift
                | RateDecision::NackHold
                | RateDecision::NackStepDown
                | RateDecision::ProbeWithdrawn
        )
    }
}

/// What the receiver should put in the ACK after [`OtaRateController::on_rx_frame`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RxAck {
    /// ACK type for legacy peers (derived from the recommendation direction / failure).
    pub ack_type: AckType,
    /// Absolute receiver-led rate target the sender should adopt.
    pub recommended_level: SpeedLevel,
    /// Which branch produced this recommendation (observability; see [`RateDecision`]).
    pub decision: RateDecision,
}

/// Receiver-led per-direction rate controller for one session.
#[derive(Debug, Clone)]
pub struct OtaRateController {
    profile: SessionProfile,
    levels: Vec<SpeedLevel>, // mapped levels, ascending
    // RX direction (we are the data receiver and lead the rate):
    rx_recommended: SpeedLevel,
    rx_confirmed: SpeedLevel,
    rx_consecutive_nack: u8,
    /// Clean decodes in a row at `rx_confirmed`, for the evidence-based climb. Reset by any failure
    /// and by any level change, so it only ever counts success *at the level being judged*.
    rx_consecutive_ok: u8,
    /// Decoded airtime in the current success streak, for the airtime-weighted climb. Reset wherever
    /// `rx_consecutive_ok` is.
    rx_ok_airtime_s: f32,
    /// After a keyed failure at a probe rung, the would-be climbs back to it the receiver declines.
    rx_climb_backoff: Option<RxClimbBackoff>,
    // TX direction (we are the data sender and follow the peer):
    tx_level: SpeedLevel,
    /// The level before the last climb the peer has not yet confirmed: where the sender returns when
    /// the climb is answered by silence (`fall_back_after_silence`). One step, used once.
    tx_before_climb: Option<SpeedLevel>,
    /// After a fall-back from a rung, the climbs back to it the sender declines before trying it
    /// again: (the rung, climbs still to decline, the next refusal count). Doubles per repeat.
    tx_climb_backoff: Option<(SpeedLevel, u8, u8)>,
    // Operator controls:
    /// Lowest level adaptation may use (`None` = the profile's lowest mapped level).
    min_level: Option<SpeedLevel>,
    /// Highest level adaptation may use (`None` = the profile's highest mapped level).
    max_level: Option<SpeedLevel>,
    /// The profile's own cap (`robust` = SL6). Kept apart from `max_level` because operator bounds
    /// overwrite that field; this one is set once and only ever lowers `hi()`.
    profile_cap: Option<SpeedLevel>,
    /// When set, both directions are pinned to this level and adaptation is off.
    locked: Option<SpeedLevel>,
}

impl OtaRateController {
    /// Create a controller for `profile`, both directions starting at the profile's
    /// initial level (clamped to a mapped level).
    pub fn new(profile: SessionProfile) -> Self {
        let levels = profile.defined_levels();
        let initial = if levels.contains(&profile.initial_level) {
            profile.initial_level
        } else {
            // Fall back to the lowest mapped level if the configured initial is unmapped.
            *levels.first().unwrap_or(&SpeedLevel::Sl1)
        };
        let profile_cap = profile.max_level();
        Self {
            profile,
            levels,
            rx_recommended: initial,
            rx_confirmed: initial,
            rx_consecutive_nack: 0,
            rx_consecutive_ok: 0,
            rx_ok_airtime_s: 0.0,
            rx_climb_backoff: None,
            tx_level: initial,
            tx_before_climb: None,
            tx_climb_backoff: None,
            min_level: None,
            max_level: None,
            profile_cap,
            locked: None,
        }
    }

    // ── Operator controls ──────────────────────────────────────────────────────

    /// Clamp adaptation to `[min, max]` (each `None` = the profile's natural bound).
    /// Current levels are immediately snapped into the new range.
    pub fn set_level_bounds(&mut self, min: Option<SpeedLevel>, max: Option<SpeedLevel>) {
        // Unchanged bounds change no level, so they keep the climb record and the backoff: an
        // older CLI's status poll sends exactly this (on-air tasks 24–26).
        if (min, max) == (self.min_level, self.max_level) {
            return;
        }
        self.min_level = min;
        self.max_level = max;
        self.rx_recommended = self.clamp_mapped(self.rx_recommended);
        self.rx_confirmed = self.clamp_mapped(self.rx_confirmed);
        self.tx_level = self.clamp_mapped(self.tx_level);
        self.tx_before_climb = None;
        self.tx_climb_backoff = None;
        self.rx_climb_backoff = None;
    }

    /// Pin both directions to `level` and stop adapting (a manual override).
    pub fn lock_level(&mut self, level: SpeedLevel) {
        let l = self.clamp_mapped(level);
        self.locked = Some(l);
        self.tx_level = l;
        self.rx_recommended = l;
        self.rx_confirmed = l;
        self.tx_before_climb = None;
        self.tx_climb_backoff = None;
        self.rx_climb_backoff = None;
    }

    /// Release a [`lock_level`](Self::lock_level) and resume adapting from the current level.
    pub fn unlock(&mut self) {
        self.locked = None;
    }

    /// Whether a manual level lock is in effect.
    pub fn is_locked(&self) -> bool {
        self.locked.is_some()
    }

    // ── Mapped-level navigation (bounds-aware) ─────────────────────────────────

    fn lo(&self) -> SpeedLevel {
        self.min_level
            .unwrap_or_else(|| *self.levels.first().unwrap_or(&SpeedLevel::Sl1))
    }

    fn hi(&self) -> SpeedLevel {
        let top = *self.levels.last().unwrap_or(&SpeedLevel::Sl1);
        [self.max_level, self.profile_cap]
            .into_iter()
            .flatten()
            .fold(top, std::cmp::min)
    }

    fn next_mapped(&self, level: SpeedLevel) -> SpeedLevel {
        let hi = self.hi();
        self.levels
            .iter()
            .copied()
            .find(|&l| l > level && l <= hi)
            .unwrap_or_else(|| self.clamp_mapped(level))
    }

    fn prev_mapped(&self, level: SpeedLevel) -> SpeedLevel {
        let lo = self.lo();
        self.levels
            .iter()
            .copied()
            .rev()
            .find(|&l| l < level && l >= lo)
            .unwrap_or_else(|| self.clamp_mapped(level))
    }

    fn clamp_mapped(&self, level: SpeedLevel) -> SpeedLevel {
        let (lo, hi) = (self.lo(), self.hi());
        let bounded = level.max(lo).min(hi);
        if self.levels.contains(&bounded) {
            return bounded;
        }
        // Snap to the nearest mapped level at or below `bounded`, staying within [lo, hi].
        self.levels
            .iter()
            .copied()
            .rev()
            .find(|&l| l <= bounded && l >= lo)
            .or_else(|| self.levels.iter().copied().find(|&l| l >= lo && l <= hi))
            .unwrap_or(bounded)
    }

    /// Highest mapped level within `[lo, hi]` the measured SNR supports — the "SNR-adequate"
    /// level, per the profile's per-level `snr_floor` thresholds. A level with no floor (the
    /// most robust rungs) is always adequate; when the SNR is below even the lowest floor this
    /// falls back to `lo`. This is the direct SNR→level lookup the fast downshift jumps to.
    fn level_for_snr(&self, snr_db: f32) -> SpeedLevel {
        let (lo, hi) = (self.lo(), self.hi());
        self.levels
            .iter()
            .copied()
            .filter(|&l| l >= lo && l <= hi)
            .filter(|&l| {
                self.profile
                    .snr_floor_for_level(l)
                    .is_none_or(|f| snr_db >= f)
            })
            .max()
            .unwrap_or(lo)
    }

    // ── TX side (we follow the peer) ───────────────────────────────────────────

    /// Adopt the peer's absolute rate recommendation as our TX level.
    ///
    /// Ignored while locked (the manual override wins); otherwise clamped into the
    /// configured `[min, max]` bounds.
    pub fn adopt_recommendation(&mut self, level: SpeedLevel) {
        self.adopt_recommendation_from(level, false);
    }

    /// As [`adopt_recommendation`](Self::adopt_recommendation), from an ACK of type NACK or not.
    ///
    /// A climb is remembered until a decode-path ACK (not a NACK) answers it: the sender sends only
    /// at its level, so such an ACK answers a frame sent at the new one. A NACK is evidence against
    /// the rung, and one NACK before a silence would otherwise leave the sender stranded
    /// (`docs/dev/design/ladder-climb-into-silence.md`, review M1).
    pub fn adopt_recommendation_from(&mut self, level: SpeedLevel, nack: bool) {
        if self.locked.is_some() {
            return;
        }
        let next = self.clamp_mapped(level);
        if next > self.tx_level {
            if let Some((rung, left, n)) = self.tx_climb_backoff {
                if rung == next && left > 0 {
                    // A rung that just went silent: decline the climb, stay where the peer decodes.
                    self.tx_climb_backoff = Some((rung, left - 1, n));
                    return;
                }
            }
            self.tx_before_climb = Some(self.tx_level);
        } else if !nack || next < self.tx_level {
            self.tx_before_climb = None;
            if !nack
                && self
                    .tx_climb_backoff
                    .is_some_and(|(rung, _, _)| next >= rung)
            {
                // The rung decodes after all: forget the backoff.
                self.tx_climb_backoff = None;
            }
        }
        if next < self.tx_level {
            self.tx_climb_backoff = None;
        }
        self.tx_level = next;
    }

    /// The peer went silent after a climb it never confirmed: return to the level before it, once.
    ///
    /// On air (task 23, f2c) the climb SL7 → SL8 dropped the sender's audio by 13 dB, the receiver
    /// heard nothing, keyed nothing, and the link never recovered: the rate follows the receiver, and
    /// a receiver that hears nothing says nothing. Bounded: one step, never below the level the
    /// climb started from, so silence cannot walk the ladder down. Returns whether it fell back.
    pub fn fall_back_after_silence(&mut self) -> bool {
        if self.locked.is_some() {
            return false;
        }
        let Some(before) = self.tx_before_climb.take() else {
            return false;
        };
        // Never onto the MFSK16 sub-floor: a body over its one-block capacity cannot ride it, the
        // send would key with nothing radiated, and the receiver (never told SL1) would hear nothing
        // again: the permanent stall of decision 3 (review M2).
        if self
            .profile
            .mode_for(before)
            .is_some_and(|m| m.starts_with("MFSK16"))
        {
            return false;
        }
        let from = self.tx_level;
        let n = match self.tx_climb_backoff {
            Some((rung, _, n)) if rung == from => n.saturating_mul(2).min(TX_CLIMB_BACKOFF_MAX),
            _ => TX_CLIMB_BACKOFF_FIRST,
        };
        self.tx_climb_backoff = Some((from, n, n));
        self.tx_level = before;
        true
    }

    /// Why [`fall_back_after_silence`](Self::fall_back_after_silence) would decline now, or `None`
    /// when it would fall back. For the log line on a silent peer (on-air task 24, f2d: it declined
    /// 51 times and the info log could not say why).
    pub fn fall_back_declined_because(&self) -> Option<&'static str> {
        if self.locked.is_some() {
            return Some("the session is locked");
        }
        let Some(before) = self.tx_before_climb else {
            return Some("no unconfirmed climb is recorded");
        };
        if self
            .profile
            .mode_for(before)
            .is_some_and(|m| m.starts_with("MFSK16"))
        {
            return Some("the level before the climb is the MFSK16 sub-floor");
        }
        None
    }

    /// Current TX speed level.
    pub fn tx_level(&self) -> SpeedLevel {
        self.tx_level
    }

    /// Mode string we should transmit data at.
    pub fn tx_mode(&self) -> Option<&'static str> {
        self.profile.mode_for(self.tx_level)
    }

    /// FEC scheme we should transmit data with at the current TX level (MODCOD).
    pub fn tx_fec(&self) -> FecMode {
        self.profile.fec_for(self.tx_level)
    }

    /// Mode string mapped to an arbitrary level in this profile (for ACK-waveform selection).
    pub fn mode_for_level(&self, level: SpeedLevel) -> Option<&'static str> {
        self.profile.mode_for(level)
    }

    /// Does this profile define a rung with the given mode? Gates the sub-floor union-listen ACK path.
    pub fn profile_has_mode(&self, mode: &str) -> bool {
        self.profile
            .defined_levels()
            .into_iter()
            .any(|l| self.profile.mode_for(l) == Some(mode))
    }

    // ── RX side (we lead) ──────────────────────────────────────────────────────

    /// Current absolute level we are recommending to the peer.
    pub fn rx_recommended_level(&self) -> SpeedLevel {
        self.rx_recommended
    }

    /// Highest level we have actually decoded (the lockstep anchor).
    pub fn rx_confirmed_level(&self) -> SpeedLevel {
        self.rx_confirmed
    }

    /// The receiver's memory of a failed probe, if any (for the engine's log line, review R3).
    pub fn rx_climb_backoff(&self) -> Option<RxClimbBackoff> {
        self.rx_climb_backoff
    }

    /// `(level, mode)` candidates to attempt when demodulating the next data
    /// frame, most-likely first.
    ///
    /// The lockstep invariant guarantees this set covers whatever the sender is
    /// using: the recommended level (if it adopted our last ACK) or the confirmed
    /// level (if that ACK was lost). At most two entries.
    pub fn rx_candidates(&self) -> Vec<(SpeedLevel, &'static str, FecMode)> {
        let mut out = Vec::with_capacity(2);
        if let Some(m) = self.profile.mode_for(self.rx_recommended) {
            out.push((
                self.rx_recommended,
                m,
                self.profile.fec_for(self.rx_recommended),
            ));
        }
        if self.rx_confirmed != self.rx_recommended {
            if let Some(m) = self.profile.mode_for(self.rx_confirmed) {
                if !out.iter().any(|&(l, _, _)| l == self.rx_confirmed) {
                    out.push((
                        self.rx_confirmed,
                        m,
                        self.profile.fec_for(self.rx_confirmed),
                    ));
                }
            }
        }
        out
    }

    /// Mode strings to attempt when demodulating the next data frame, most-likely first.
    pub fn rx_candidate_modes(&self) -> Vec<&'static str> {
        self.rx_candidates()
            .into_iter()
            .map(|(_, m, _)| m)
            .collect()
    }

    /// Update RX state from a demodulation outcome and measured SNR, and return the
    /// ACK the receiver should send (type + absolute recommendation).
    /// Apply one received frame to the receiver-side rate decision.
    ///
    /// `snr_db` is `None` when **no trustworthy reading exists** — which is the normal case on a
    /// failed decode (#1142). The receiver's estimator locks sub-symbol timing by correlating the
    /// burst's first 32 symbols against the preamble; once a lead-in pushes the frame past that
    /// window the lock is a noise argmax, and the resulting reading is a deterministic function of
    /// the accidental misalignment — measured swinging **+5.3 dB to −8.4 dB against a true 5.8 dB**,
    /// with the position on that curve set by which noise the burst happened to contain.
    ///
    /// A value that is inconsistently wrong cannot be repaired by a calibration offset, so the
    /// caller abstains instead of guessing, and `None` routes to the evidence-based paths that do
    /// not need a reading.
    pub fn on_rx_frame(&mut self, outcome: RxOutcome, snr_db: Option<f32>) -> RxAck {
        self.on_rx_frame_with(outcome, snr_db, NackVerdict::Keyed)
    }

    /// [`on_rx_frame`](Self::on_rx_frame) with the NACK budget's verdict for a failure. A muted
    /// failure answers `NackHold` and moves nothing but the climb streak; a leaked one steps at
    /// once. A decode ignores the verdict.
    pub fn on_rx_frame_with(
        &mut self,
        outcome: RxOutcome,
        snr_db: Option<f32>,
        verdict: NackVerdict,
    ) -> RxAck {
        self.on_rx_frame_timed(outcome, snr_db, verdict, None)
    }

    /// [`on_rx_frame_with`](Self::on_rx_frame_with) with the decoded frame's airtime, so a streak
    /// whose decodes add up to `CLIMB_EVIDENCE_AIRTIME_S` proves the rung after one decode, and the
    /// failed-probe memory expires on clean airtime. `None` is the count-only rule.
    pub fn on_rx_frame_timed(
        &mut self,
        outcome: RxOutcome,
        snr_db: Option<f32>,
        verdict: NackVerdict,
        airtime_s: Option<f32>,
    ) -> RxAck {
        let airtime_s = airtime_s
            .filter(|a| a.is_finite() && *a > 0.0)
            .unwrap_or(0.0);
        // While locked, keep both directions pinned and recommend the locked level.
        if let Some(l) = self.locked {
            let ack_type = match outcome {
                RxOutcome::Failed => AckType::Nack,
                RxOutcome::Decoded(_) => AckType::AckOk,
            };
            return RxAck {
                ack_type,
                recommended_level: l,
                decision: RateDecision::Locked,
            };
        }
        match outcome {
            RxOutcome::Failed => {
                // A failure is evidence against the rung, so the success streak restarts: the
                // evidence-based climb must never be reachable by alternating pass/fail.
                self.rx_consecutive_ok = 0;
                self.rx_ok_airtime_s = 0.0;
                // #1460 round 4b: a failure whose NACK is not keyed is no ladder evidence. Counting
                // it stepped the ladder silently, and the next step announced (a leaked NACK) came
                // several rungs at once, after the sender's frames had already stopped decoding.
                if verdict == NackVerdict::Muted {
                    return RxAck {
                        ack_type: AckType::Nack,
                        recommended_level: self.rx_recommended,
                        decision: RateDecision::NackHold,
                    };
                }
                // Asymmetric fast downshift: if the SNR estimate already explains the failure
                // (the SNR-adequate level is below what we're recommending), jump the
                // recommendation straight there instead of crawling down one rung per NACK
                // threshold — the "6 retries to find the step" symptom. `rx_confirmed` stays put
                // as the fallback candidate so a lost downshift ACK can't desync the receiver
                // (`rx_candidates` still covers whatever the sender is transmitting).
                // ABSTENTION IS NOT A LOW READING (#1142). With no trustworthy estimate this
                // falls through to the NACK hysteresis below — the branch whose own comment says
                // "a single blip can't drop the rate". Before this, a misframed reading reached
                // `level_for_snr`, filtered out every rung whose floor exceeded it, and fell to
                // `unwrap_or(lo)`: one failed decode crashed the recommendation to the bottom of
                // the ladder, bypassing that hysteresis entirely. From SL10 that cost ~24 clean
                // frames to undo, at 3 per rung.
                // DORMANT on air (#1438): the daemon's only feed (`ota_decode_and_ack_inner`) passes
                // `None` on every failed decode, so this branch fires only for callers that hand
                // it a reading on a failure — the unit tests and the test-only
                // `set_rx_snr_estimate`. The link simulator did so until #1438 and was aligned to the
                // daemon; do not give it a failure-path reading back without re-deciding #1142.
                let snr_level = snr_db.map(|s| self.level_for_snr(s));
                let decision = if snr_level.is_some_and(|l| l < self.rx_recommended) {
                    self.rx_consecutive_nack = 0;
                    self.rx_recommended = snr_level.expect("guarded by is_some_and above");
                    RateDecision::FastDownshift
                } else if verdict == NackVerdict::Keyed && self.rx_recommended > self.rx_confirmed {
                    // A keyed failure while a probe is outstanding: withdraw to the confirmed rung and
                    // keep the anchor. Three probe failures stepped the anchor BELOW the rung that kept
                    // decoding (f2k: ten SL10 → SL8 cycles on a proven SL9) and spent the whole NACK
                    // budget. A LEAKED failure is not one: by then the unanswered sender is back at the
                    // anchor, and the leak is the only announced move per 10 min (review R1).
                    let probe = self.rx_recommended;
                    let n = match self.rx_climb_backoff {
                        Some(b) if b.rung == probe => {
                            b.n.saturating_mul(2).min(RX_CLIMB_BACKOFF_MAX)
                        }
                        _ => RX_CLIMB_BACKOFF_FIRST,
                    };
                    self.rx_climb_backoff = Some(RxClimbBackoff {
                        rung: probe,
                        anchor: self.rx_confirmed,
                        left: n,
                        n,
                        airtime_s: 0.0,
                    });
                    self.rx_consecutive_nack = 0;
                    self.rx_recommended = self.rx_confirmed;
                    RateDecision::ProbeWithdrawn
                } else {
                    // SNR doesn't explain the failure (a transient fade or collision at an
                    // otherwise-adequate SNR): keep the consecutive-NACK hysteresis so a single
                    // blip can't drop the rate, stepping the anchor down one rung at the threshold.
                    self.rx_consecutive_nack = self.rx_consecutive_nack.saturating_add(1);
                    if self.rx_consecutive_nack >= self.profile.nack_threshold
                        || verdict == NackVerdict::Leaked
                    {
                        self.rx_consecutive_nack = 0;
                        self.rx_confirmed = self.prev_mapped(self.rx_confirmed);
                        self.rx_recommended = self.rx_confirmed;
                        RateDecision::NackStepDown
                    } else {
                        RateDecision::NackHold
                    }
                };
                // The channel changed under the memory: its evidence is about a rung pair we left.
                if self
                    .rx_climb_backoff
                    .is_some_and(|b| self.rx_confirmed < b.anchor)
                {
                    self.rx_climb_backoff = None;
                }
                RxAck {
                    ack_type: AckType::Nack,
                    recommended_level: self.rx_recommended,
                    decision,
                }
            }
            RxOutcome::Decoded(level) => {
                self.rx_consecutive_nack = 0;
                // Anchor on the level we actually decoded (recommended if the sender
                // adopted it, else the fallback level).
                let previous_confirmed = self.rx_confirmed;
                self.rx_confirmed = self.clamp_mapped(level);
                // The success streak counts clean decodes *at one level*: a level change makes the
                // streak evidence about a rung we are no longer on.
                if self.rx_confirmed == previous_confirmed {
                    self.rx_consecutive_ok = self.rx_consecutive_ok.saturating_add(1);
                    self.rx_ok_airtime_s += airtime_s;
                } else {
                    self.rx_consecutive_ok = 1;
                    self.rx_ok_airtime_s = airtime_s;
                }
                // The failed-probe memory: a decode at or above its rung is the observation that
                // the rung works, and one below its anchor means the channel moved; either drops it.
                // Otherwise the frame's clean airtime counts toward the expiry, which lets the rung
                // be probed again but keeps the level, so a re-probe that fails doubles it.
                let confirmed = self.rx_confirmed;
                if let Some(b) = self.rx_climb_backoff.as_mut() {
                    if confirmed >= b.rung || confirmed < b.anchor {
                        self.rx_climb_backoff = None;
                    } else {
                        b.airtime_s += airtime_s;
                        if b.airtime_s >= b.expiry_s() {
                            b.left = 0;
                        }
                    }
                }

                // **A decode is an observation; the SNR is a model. The observation wins.**
                //
                // This path used to fast-downshift below `rx_confirmed` when the SNR estimate said
                // the rung was unsupportable — on a frame that had *just decoded at that rung*. That
                // is preferring a model over direct evidence, and it is the other half of #934: on a
                // fade BPSK31's estimate reads far below every floor (a flat ≈ −12.6 dB after the
                // Es/N0→channel conversion, at any true SNR), so every decoded frame was answered
                // with "drop a rung". The link oscillated on its bottom two rungs at ~5 bps while
                // delivering 20/20 frames.
                //
                // Demotion now lives solely on the `Failed` path, where the SNR genuinely *explains*
                // something: a frame actually failed. Here there is nothing to explain, so the
                // recommendation never goes below the level that just decoded — it holds or climbs:
                //  • UP on SNR — the estimate clears the confirmed rung's ceiling. Fast when the
                //    estimate is informative.
                //  • UP on evidence — `ACK_CLIMB_THRESHOLD` clean decodes in a row at this rung.
                //    Slower, but it cannot lie, and it is the only path that works when the estimate
                //    is uninformative *in principle* (see the module header).
                // Both advance exactly one mapped step, preserving the lockstep invariant.
                // No reading cannot clear a ceiling, so abstention leaves the decoded path on the
                // evidence climb — which is #934's rule and is safe by construction: the cost is
                // climb latency (ACK_CLIMB_THRESHOLD decodes per rung), never a wrong rung.
                let snr_clears_ceiling = snr_db.is_some_and(|s| {
                    self.profile
                        .snr_ceiling_for_level(self.rx_confirmed)
                        .is_some_and(|c| s >= c)
                });
                // Change 2: a streak whose decodes add up to one SL2 frame of airtime proves the
                // rung as three short frames do (3.5 min per SL2 climb on air otherwise). Only with
                // the withdraw above: alone, a lucky decode's failed climb took three NACKs and the
                // third stepped onto the MFSK16 sub-floor (`ota-climb-memory.md`, "C2 alone").
                let proven_by_success = self.rx_consecutive_ok >= ACK_CLIMB_THRESHOLD
                    || (self.rx_consecutive_ok >= 1
                        && self.rx_ok_airtime_s >= CLIMB_EVIDENCE_AIRTIME_S);
                // Precedence when BOTH fire: report `ClimbOnSnr`. The distinction that matters is
                // whether the estimate was informative at all — a run of `ClimbOnEvidence` with no
                // `ClimbOnSnr` is the signature of an estimate carrying no information (#934).
                // At the top of the reachable range a climb has nowhere to go, so it is a hold: a
                // capped profile (`robust`) would otherwise log a phantom climb on every frame.
                let next = self.next_mapped(self.rx_confirmed);
                let at_top = next == self.rx_confirmed;
                let wants_climb = !at_top && (snr_clears_ceiling || proven_by_success);
                // A climb to a rung whose probe recently failed is declined, one count per would-be
                // climb; a climb on SNR wears the wait down per frame, as the decode failure is an
                // observation and the reading a model.
                let declined = wants_climb
                    && self
                        .rx_climb_backoff
                        .is_some_and(|b| b.rung == next && b.left > 0);
                let decision = if declined {
                    if let Some(b) = self.rx_climb_backoff.as_mut() {
                        b.left -= 1;
                    }
                    RateDecision::ClimbDeclined
                } else if !wants_climb {
                    RateDecision::Hold
                } else if snr_clears_ceiling {
                    RateDecision::ClimbOnSnr
                } else {
                    RateDecision::ClimbOnEvidence
                };
                self.rx_recommended = if wants_climb {
                    // Spend the streak on the attempt (or on the decline), so a rung that keeps
                    // decoding proposes the next step every N frames rather than every frame once
                    // the streak is met.
                    self.rx_consecutive_ok = 0;
                    self.rx_ok_airtime_s = 0.0;
                    if declined {
                        self.rx_confirmed
                    } else {
                        next
                    }
                } else {
                    self.rx_confirmed
                };

                let ack_type = match self.rx_recommended.cmp(&self.rx_confirmed) {
                    std::cmp::Ordering::Greater => AckType::AckUp,
                    std::cmp::Ordering::Less => AckType::AckDown,
                    std::cmp::Ordering::Equal => AckType::AckOk,
                };
                RxAck {
                    ack_type,
                    recommended_level: self.rx_recommended,
                    decision,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HIGH_SNR: f32 = 1.0e9;
    const LOW_SNR: f32 = -1.0e9;

    /// `ALL` must contain every variant exactly once. `all_index` is the compiler's half — adding a
    /// variant stops it building — and this is the other half, catching a variant missing from the
    /// list. Neither alone is sufficient.
    #[test]
    fn rate_decision_all_lists_every_variant_exactly_once() {
        let mut seen = [0usize; RateDecision::ALL.len()];
        for d in RateDecision::ALL {
            seen[d.all_index()] += 1;
        }
        assert!(
            seen.iter().all(|&n| n == 1),
            "each variant must appear exactly once in ALL; counts by index: {seen:?}"
        );
    }

    /// The failure-path classification must agree with which branch actually produced the
    /// decision, or `is_failure_path` becomes a second source of truth that can drift.
    #[test]
    fn failure_path_classification_matches_the_branches() {
        for d in RateDecision::ALL {
            let expected = matches!(
                d,
                RateDecision::FastDownshift
                    | RateDecision::NackHold
                    | RateDecision::NackStepDown
                    | RateDecision::ProbeWithdrawn
            );
            assert_eq!(d.is_failure_path(), expected, "{d:?}");
        }
    }

    fn ctrl() -> OtaRateController {
        OtaRateController::new(SessionProfile::fast())
    }

    #[test]
    fn a_climb_answered_by_silence_returns_once_to_the_level_before_it() {
        let mut c = ctrl();
        c.adopt_recommendation(SpeedLevel::Sl7);
        c.adopt_recommendation(SpeedLevel::Sl7);
        c.adopt_recommendation(SpeedLevel::Sl8);
        assert_eq!(c.tx_level(), SpeedLevel::Sl8);
        assert!(c.fall_back_after_silence());
        assert_eq!(
            c.tx_level(),
            SpeedLevel::Sl7,
            "back to the level the climb started from"
        );
        assert!(!c.fall_back_after_silence(), "once per climb");
        assert_eq!(
            c.tx_level(),
            SpeedLevel::Sl7,
            "silence never walks the ladder down"
        );
    }

    #[test]
    fn an_answer_at_the_new_level_clears_the_way_back() {
        let mut c = ctrl();
        c.adopt_recommendation(SpeedLevel::Sl7);
        c.adopt_recommendation(SpeedLevel::Sl8);
        // An AckOk (or a NACK) restating SL8: the peer hears this rung.
        c.adopt_recommendation(SpeedLevel::Sl8);
        assert!(!c.fall_back_after_silence());
        assert_eq!(c.tx_level(), SpeedLevel::Sl8);
        // A further climb remembers the confirmed rung, not the one before it.
        c.adopt_recommendation(SpeedLevel::Sl9);
        assert!(c.fall_back_after_silence());
        assert_eq!(c.tx_level(), SpeedLevel::Sl8);
    }

    #[test]
    fn a_nack_after_the_climb_keeps_the_way_back() {
        let mut c = ctrl();
        c.adopt_recommendation(SpeedLevel::Sl7);
        c.adopt_recommendation(SpeedLevel::Sl8);
        // NackHold restates SL8: heard and failed, evidence against the rung, not for it.
        c.adopt_recommendation_from(SpeedLevel::Sl8, true);
        assert!(c.fall_back_after_silence());
        assert_eq!(c.tx_level(), SpeedLevel::Sl7);
    }

    #[test]
    fn a_climb_back_to_a_rung_that_went_silent_waits_and_the_wait_doubles() {
        let mut c = ctrl();
        c.adopt_recommendation(SpeedLevel::Sl7);
        c.adopt_recommendation(SpeedLevel::Sl8);
        assert!(c.fall_back_after_silence());
        for _ in 0..TX_CLIMB_BACKOFF_FIRST {
            c.adopt_recommendation(SpeedLevel::Sl8);
            assert_eq!(c.tx_level(), SpeedLevel::Sl7, "declined while backing off");
        }
        c.adopt_recommendation(SpeedLevel::Sl8);
        assert_eq!(c.tx_level(), SpeedLevel::Sl8, "tried again after the wait");
        assert!(c.fall_back_after_silence(), "silent again");
        let mut declined = 0;
        while {
            c.adopt_recommendation(SpeedLevel::Sl8);
            c.tx_level() == SpeedLevel::Sl7
        } {
            declined += 1;
        }
        assert_eq!(declined, 2 * TX_CLIMB_BACKOFF_FIRST, "the wait doubles");
        // A decode-path ACK at the rung ends the backoff: the next fall-back starts from the first.
        c.adopt_recommendation(SpeedLevel::Sl8);
        c.adopt_recommendation(SpeedLevel::Sl9);
        assert!(c.fall_back_after_silence());
        assert_eq!(c.tx_level(), SpeedLevel::Sl8);
    }

    #[test]
    fn never_falls_back_onto_the_mfsk16_sub_floor() {
        let mut c = ctrl();
        c.adopt_recommendation(SpeedLevel::Sl1);
        assert_eq!(
            c.tx_mode(),
            Some("MFSK16"),
            "the fast profile's SL1 is the sub-floor"
        );
        c.adopt_recommendation(SpeedLevel::Sl2);
        assert!(!c.fall_back_after_silence());
        assert_eq!(c.tx_level(), SpeedLevel::Sl2);
    }

    #[test]
    fn a_lock_or_new_bounds_drop_the_way_back() {
        let mut c = ctrl();
        c.adopt_recommendation(SpeedLevel::Sl7);
        c.adopt_recommendation(SpeedLevel::Sl8);
        c.set_level_bounds(Some(SpeedLevel::Sl8), None);
        assert!(!c.fall_back_after_silence());
        let mut c = ctrl();
        c.adopt_recommendation(SpeedLevel::Sl7);
        c.adopt_recommendation(SpeedLevel::Sl8);
        c.lock_level(SpeedLevel::Sl8);
        c.unlock();
        assert!(!c.fall_back_after_silence());
    }

    #[test]
    fn unchanged_bounds_keep_the_climb_record_and_a_change_drops_it() {
        // The CLI's status poll before on-air task 27 sent the bounds already in force, every ~5 s.
        let mut c = ctrl();
        c.adopt_recommendation(SpeedLevel::Sl6);
        c.adopt_recommendation(SpeedLevel::Sl6);
        c.adopt_recommendation(SpeedLevel::Sl7);
        c.set_level_bounds(None, None);
        assert_eq!(
            c.fall_back_declined_because(),
            None,
            "unchanged bounds keep the record"
        );
        c.set_level_bounds(Some(SpeedLevel::Sl3), None);
        assert_eq!(
            c.fall_back_declined_because(),
            Some("no unconfirmed climb is recorded"),
            "new bounds drop it, so a fall-back cannot land outside them"
        );
    }

    #[test]
    fn the_declined_reason_matches_the_fall_back() {
        let mut c = ctrl();
        assert_eq!(
            c.fall_back_declined_because(),
            Some("no unconfirmed climb is recorded")
        );
        c.adopt_recommendation(SpeedLevel::Sl7);
        c.adopt_recommendation(SpeedLevel::Sl8);
        assert_eq!(c.fall_back_declined_because(), None);
        assert!(c.fall_back_after_silence());
        let mut c = ctrl();
        c.adopt_recommendation(SpeedLevel::Sl1);
        c.adopt_recommendation(SpeedLevel::Sl2);
        assert!(c.fall_back_declined_because().is_some());
        assert!(!c.fall_back_after_silence());
    }

    #[test]
    fn no_climb_or_a_lock_means_no_fall_back() {
        let mut c = ctrl();
        assert!(!c.fall_back_after_silence(), "nothing to return to");
        c.adopt_recommendation(SpeedLevel::Sl8);
        c.adopt_recommendation(SpeedLevel::Sl6);
        assert!(!c.fall_back_after_silence(), "a step down is not a climb");
        assert_eq!(c.tx_level(), SpeedLevel::Sl6);
        let mut c = ctrl();
        c.lock_level(SpeedLevel::Sl8);
        assert!(!c.fall_back_after_silence());
        assert_eq!(c.tx_level(), SpeedLevel::Sl8);
    }

    /// Every branch must report the decision that actually ran. A reason field that does not track
    /// the code is worse than none, because it reads as attribution — and attribution is the entire
    /// reason this field exists (#1081).
    ///
    /// Each case drives the controller into one branch and asserts the reported reason, so a future
    /// refactor that moves a branch without moving its label fails here. `RateDecision::ALL` is
    /// swept at the end to prove no variant is left unexercised — the pairing that stops this
    /// becoming a hand-maintained list.
    #[test]
    fn every_branch_reports_the_decision_that_actually_ran() {
        let mut exercised = [false; RateDecision::ALL.len()];
        let mut note = |d: RateDecision| exercised[d.all_index()] = true;

        // Failure the SNR explains: jump straight to the SNR-adequate level.
        let mut c = ctrl();
        let ack = c.on_rx_frame(RxOutcome::Failed, Some(LOW_SNR));
        assert_eq!(ack.decision, RateDecision::FastDownshift);
        note(ack.decision);

        // Failure the SNR does NOT explain: hysteresis counts, level holds until the threshold.
        let mut c = ctrl();
        let ack = c.on_rx_frame(RxOutcome::Failed, Some(HIGH_SNR));
        assert_eq!(ack.decision, RateDecision::NackHold);
        assert_eq!(
            c.rx_recommended_level(),
            ctrl().rx_recommended_level(),
            "NackHold must not move the recommendation"
        );
        note(ack.decision);

        // ...and at the threshold it steps down one rung.
        let mut last = ack.decision;
        for _ in 1..c.profile.nack_threshold {
            last = c.on_rx_frame(RxOutcome::Failed, Some(HIGH_SNR)).decision;
        }
        assert_eq!(last, RateDecision::NackStepDown);
        note(last);

        // Decode with an SNR clearing the rung's ceiling: climb on the estimate.
        let mut c = ctrl();
        let start = c.rx_confirmed;
        let ack = c.on_rx_frame(RxOutcome::Decoded(start), Some(HIGH_SNR));
        assert_eq!(ack.decision, RateDecision::ClimbOnSnr);
        note(ack.decision);

        // Decode with an SNR that explains nothing and no streak yet: hold. This is the case that
        // #934 used to answer with "drop a rung" while the frame had just decoded.
        let mut c = ctrl();
        let start = c.rx_confirmed;
        let ack = c.on_rx_frame(RxOutcome::Decoded(start), Some(LOW_SNR));
        assert_eq!(ack.decision, RateDecision::Hold);
        assert!(
            c.rx_recommended_level() >= start,
            "a decode must never demote below the level that just decoded"
        );
        note(ack.decision);

        // ...and once the streak reaches the threshold, the same uninformative SNR still climbs —
        // on evidence. This is the branch that works when the estimate carries no information.
        let mut last = ack.decision;
        for _ in 1..ACK_CLIMB_THRESHOLD {
            last = c
                .on_rx_frame(RxOutcome::Decoded(start), Some(LOW_SNR))
                .decision;
        }
        assert_eq!(last, RateDecision::ClimbOnEvidence);
        note(last);

        // A keyed failure while a probe is outstanding: withdraw to the confirmed rung, anchor kept.
        let mut c = ctrl();
        let start = c.rx_confirmed;
        c.on_rx_frame(RxOutcome::Decoded(start), Some(HIGH_SNR));
        assert!(
            c.rx_recommended > start,
            "precondition: a probe is outstanding"
        );
        let ack = c.on_rx_frame(RxOutcome::Failed, None);
        assert_eq!(ack.decision, RateDecision::ProbeWithdrawn);
        assert_eq!(ack.ack_type, AckType::Nack);
        assert_eq!((c.rx_recommended, c.rx_confirmed), (start, start));
        note(ack.decision);

        // ...and the next would-be climb back to the withdrawn rung is declined.
        let ack = c.on_rx_frame(RxOutcome::Decoded(start), Some(HIGH_SNR));
        assert_eq!(ack.decision, RateDecision::ClimbDeclined);
        assert_eq!(ack.ack_type, AckType::AckOk);
        assert_eq!(ack.recommended_level, start);
        note(ack.decision);

        // Locked pins both directions regardless of outcome.
        let mut c = ctrl();
        c.lock_level(c.rx_confirmed);
        let ack = c.on_rx_frame(RxOutcome::Failed, Some(LOW_SNR));
        assert_eq!(ack.decision, RateDecision::Locked);
        note(ack.decision);

        for d in RateDecision::ALL {
            assert!(
                exercised[d.all_index()],
                "{d:?} is never exercised — the sweep would not notice if it stopped being reachable"
            );
        }
    }

    /// The MFSK16 SL1 sub-floor rung is not a trapdoor: once SNR recovers above SL1's ceiling, the
    /// receiver-led controller climbs the recommendation back out to SL2. (In production the real SNR comes
    /// from MFSK16's `estimate_snr_db`; the M2M4 fallback would bias it low and pin SL1 — see PR-1.)
    #[test]
    fn subfloor_sl1_climbs_back_out_when_snr_recovers() {
        let mut c = ctrl();
        // Entry: one low-SNR failed frame fast-downshifts the recommendation to the SL1 sub-floor rung.
        c.on_rx_frame(RxOutcome::Failed, Some(-5.0));
        assert_eq!(c.rx_recommended_level(), SpeedLevel::Sl1);
        // Recovery: a decoded SL1 frame with SNR above SL1's 5 dB ceiling climbs the recommendation to SL2.
        let ack = c.on_rx_frame(RxOutcome::Decoded(SpeedLevel::Sl1), Some(6.0));
        assert_eq!(ack.ack_type, AckType::AckUp);
        assert_eq!(c.rx_recommended_level(), SpeedLevel::Sl2);
    }

    #[test]
    fn tx_follows_recommendation_and_clamps_unmapped() {
        let mut c = ctrl();
        let levels = c.levels.clone();
        let top = *levels.last().unwrap();
        c.adopt_recommendation(top);
        assert_eq!(c.tx_level(), top);
        assert!(c.tx_mode().is_some());
        // SL1 (chirp) is unmapped in hpx_hf → clamps to a mapped level, never panics.
        c.adopt_recommendation(SpeedLevel::Sl1);
        assert!(c.levels.contains(&c.tx_level()));
    }

    #[test]
    fn candidate_set_is_at_most_two_and_recommended_first() {
        let mut c = ctrl();
        // Force a one-step-ahead recommendation via a confirmed decode at high SNR.
        let start = c.rx_confirmed;
        let _ = c.on_rx_frame(RxOutcome::Decoded(start), Some(HIGH_SNR));
        let modes = c.rx_candidate_modes();
        assert!(modes.len() <= 2);
        assert_eq!(modes.first().copied(), c.profile.mode_for(c.rx_recommended));
    }

    #[test]
    fn recommendation_is_at_most_one_step_above_confirmed_but_may_drop_further() {
        let mut c = ctrl();
        // Drive a varied SNR sequence; the asymmetric invariant must hold after every frame.
        let snrs = [
            HIGH_SNR, HIGH_SNR, LOW_SNR, HIGH_SNR, 0.0, HIGH_SNR, LOW_SNR,
        ];
        for &snr in snrs.iter().cycle().take(40) {
            // Sender transmits whatever it last adopted; model it as the confirmed level.
            let _ = c.on_rx_frame(RxOutcome::Decoded(c.rx_confirmed), Some(snr));
            let conf = c.rx_confirmed;
            let rec = c.rx_recommended;
            // Cautious UP: never more than one mapped step above the confirmed anchor.
            assert!(
                rec <= c.next_mapped(conf),
                "rec {rec:?} more than one step above confirmed {conf:?}"
            );
            // Fast DOWN: may drop multiple steps, but never below the configured floor.
            assert!(rec >= c.lo(), "rec {rec:?} below the floor {:?}", c.lo());
        }
    }

    #[test]
    fn climbs_under_good_snr_without_loss() {
        let mut c = ctrl();
        let initial = c.rx_confirmed;
        // No ACK loss: sender always adopts the recommendation; receiver always
        // decodes at exactly what it recommended last round.
        let mut sender_tx = c.tx_level();
        for _ in 0..30 {
            let ack = c.on_rx_frame(RxOutcome::Decoded(sender_tx), Some(HIGH_SNR));
            sender_tx = ack.recommended_level; // delivered, sender adopts
        }
        assert!(
            c.rx_confirmed > initial,
            "expected the rate to climb above the initial level under sustained good SNR"
        );
    }

    /// The lockstep theorem: under adequate SNR and ANY ACK-loss pattern, the
    /// sender's level is always in the receiver's candidate set, so it never desyncs.
    #[test]
    fn never_desyncs_under_arbitrary_ack_loss() {
        // A few deterministic loss patterns (every Nth ACK lost, plus all-lost).
        for &period in &[1usize, 2, 3, 5, 7] {
            let mut c = ctrl();
            let mut sender_tx = c.tx_level();
            for round in 0..60 {
                // Receiver decides which candidate the sender's level matches.
                let candidate =
                    sender_tx == c.rx_recommended_level() || sender_tx == c.rx_confirmed_level();
                assert!(
                    candidate,
                    "desync: sender at {sender_tx:?} not in {{rec {:?}, conf {:?}}} (period {period}, round {round})",
                    c.rx_recommended_level(),
                    c.rx_confirmed_level()
                );
                let ack = c.on_rx_frame(RxOutcome::Decoded(sender_tx), Some(HIGH_SNR));
                // Lose this ACK if the round is on the loss period; else sender adopts.
                let lost = (round % period) == 0;
                if !lost {
                    sender_tx = ack.recommended_level;
                }
            }
        }

        // With withdraws and declines: random outcomes, verdicts and ACK loss, a real sender
        // following each delivered ACK, every rung above SL9 failing. A withdraw or a step moves
        // the candidates below a sender whose NACK is lost, so the sender may leave them, but only
        // after a KEYED ACK was lost (a muted failure keys nothing, so it must move nothing), only
        // above them, and a delivered ACK brings it back.
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        let mut rnd = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let (mut withdrawn, mut declined) = (0usize, 0usize);
        for run in 0..20 {
            let mut c = ctrl();
            let mut tx = ctrl();
            // A keyed ACK was lost since the last delivered one.
            let mut exposed = false;
            for round in 0..200 {
                let s = tx.tx_level();
                let in_set = s == c.rx_recommended_level() || s == c.rx_confirmed_level();
                if !in_set {
                    assert!(
                        exposed && s > c.rx_recommended_level(),
                        "desync with no lost keyed ACK, or below the candidates: sender {s:?}, \
                         {{rec {:?}, conf {:?}}} (run {run}, round {round})",
                        c.rx_recommended_level(),
                        c.rx_confirmed_level()
                    );
                }
                let r = rnd();
                let decodes = in_set && s <= SpeedLevel::Sl9 && r % 5 != 0;
                let outcome = if decodes {
                    RxOutcome::Decoded(s)
                } else {
                    RxOutcome::Failed
                };
                let verdict = match (r >> 8) % 8 {
                    0 => NackVerdict::Muted,
                    1 => NackVerdict::Leaked,
                    _ => NackVerdict::Keyed,
                };
                let snr = ((r >> 16) % 3 == 0).then_some(HIGH_SNR);
                let ack = c.on_rx_frame_with(outcome, snr, verdict);
                withdrawn += usize::from(ack.decision == RateDecision::ProbeWithdrawn);
                declined += usize::from(ack.decision == RateDecision::ClimbDeclined);
                let keyed = decodes || verdict != NackVerdict::Muted;
                if keyed && (r >> 24) % 4 != 0 {
                    tx.adopt_recommendation_from(
                        ack.recommended_level,
                        ack.ack_type == AckType::Nack,
                    );
                    exposed = false;
                } else if keyed {
                    exposed = true;
                }
            }
        }
        assert!(
            withdrawn > 20 && declined > 20,
            "the sweep must exercise the memory: {withdrawn} withdraws, {declined} declines"
        );
    }

    /// Even when every ACK is lost in the *climb-announcing* direction, a good
    /// channel still makes progress once an ACK gets through.
    #[test]
    fn recovers_and_climbs_through_intermittent_loss() {
        let mut c = ctrl();
        let initial = c.rx_confirmed;
        let mut sender_tx = c.tx_level();
        for round in 0..80 {
            let ack = c.on_rx_frame(RxOutcome::Decoded(sender_tx), Some(HIGH_SNR));
            // Deliver only every 3rd ACK.
            if round % 3 == 2 {
                sender_tx = ack.recommended_level;
            }
        }
        assert!(
            c.rx_confirmed > initial,
            "should still climb despite 2/3 ACK loss"
        );
    }

    #[test]
    fn fast_downshift_on_the_first_low_snr_failure() {
        // The HamRadio-2026 symptom: it took ~6 retries (one rung per NACK threshold) to reach a
        // decodable rate. A single low-SNR failure must now drop the recommendation straight to the
        // SNR-adequate floor — no crawl.
        let mut c = ctrl();
        let mut sender_tx = c.tx_level();
        for _ in 0..10 {
            let ack = c.on_rx_frame(RxOutcome::Decoded(sender_tx), Some(HIGH_SNR));
            sender_tx = ack.recommended_level;
        }
        let before = c.rx_recommended;
        assert!(
            before > c.levels[0],
            "precondition: climbed above the floor"
        );
        let ack = c.on_rx_frame(RxOutcome::Failed, Some(LOW_SNR));
        assert_eq!(ack.ack_type, AckType::Nack);
        assert_eq!(
            c.rx_recommended,
            c.lo(),
            "one low-SNR NACK jumps straight to the SNR-floor level"
        );
        assert!(c.rx_recommended < before);
    }

    #[test]
    fn transient_failure_at_good_snr_keeps_the_nack_hysteresis() {
        // A failure while the SNR is still adequate (collision / momentary fade) must NOT fast-drop;
        // the consecutive-NACK hysteresis steps the anchor down one rung only at the threshold.
        // From a settled state: a failure while a probe is outstanding is the probe's, and withdraws
        // (`a_failed_probe_withdraws_to_the_confirmed_rung_and_keeps_the_anchor`).
        let mut c = settled();
        let before = c.rx_confirmed;
        assert!(before > c.levels[0]);
        for i in 0..c.profile.nack_threshold {
            let ack = c.on_rx_frame(RxOutcome::Failed, Some(HIGH_SNR));
            assert_eq!(ack.ack_type, AckType::Nack);
            if i + 1 < c.profile.nack_threshold {
                assert_eq!(
                    c.rx_confirmed, before,
                    "must not drop before the NACK threshold at good SNR"
                );
            }
        }
        assert!(
            c.rx_confirmed < before,
            "steps down one rung at the NACK threshold"
        );
    }

    /// A controller climbed a few rungs, so it has room to step down.
    fn climbed() -> OtaRateController {
        let mut c = ctrl();
        let mut sender_tx = c.tx_level();
        for _ in 0..10 {
            sender_tx = c
                .on_rx_frame(RxOutcome::Decoded(sender_tx), Some(HIGH_SNR))
                .recommended_level;
        }
        assert!(c.rx_confirmed > c.levels[1], "room for two steps");
        c
    }

    /// [`climbed`], then a decode at the confirmed rung that does not climb: no probe outstanding.
    fn settled() -> OtaRateController {
        let mut c = climbed();
        let conf = c.rx_confirmed;
        let ack = c.on_rx_frame(RxOutcome::Decoded(conf), None);
        assert_eq!(ack.decision, RateDecision::Hold);
        assert_eq!(
            c.rx_recommended, c.rx_confirmed,
            "settled: no probe outstanding"
        );
        c
    }

    #[test]
    fn muted_failures_move_nothing_but_the_climb_streak() {
        // #1460 round 4b: three keyed failures announce one step; three more, muted by the budget,
        // must not take a second, silent one. Without the verdict the sixth failure stepped again.
        // From a settled state; the probe case is `a_muted_probe_failure_withdraws_nothing`.
        let mut c = settled();
        let start = c.rx_confirmed;
        let n = c.profile.nack_threshold;
        for _ in 0..n {
            c.on_rx_frame_with(RxOutcome::Failed, None, NackVerdict::Keyed);
        }
        let stepped = c.rx_confirmed;
        assert!(stepped < start, "keyed failures step at the threshold");
        for _ in 0..n {
            let ack = c.on_rx_frame_with(RxOutcome::Failed, None, NackVerdict::Muted);
            assert_eq!(ack.decision, RateDecision::NackHold);
            assert_eq!(ack.ack_type, AckType::Nack);
        }
        assert_eq!(
            c.rx_confirmed, stepped,
            "muted failures take no silent step"
        );
        assert_eq!(c.rx_recommended, stepped);
        assert_eq!(c.rx_consecutive_nack, 0, "muted failures are not counted");
        assert_eq!(
            c.rx_consecutive_ok, 0,
            "but they still stop the climb streak"
        );
    }

    #[test]
    fn a_muted_failure_does_not_fast_downshift() {
        let mut c = climbed();
        let before = c.rx_recommended;
        let ack = c.on_rx_frame_with(RxOutcome::Failed, Some(LOW_SNR), NackVerdict::Muted);
        assert_eq!(ack.decision, RateDecision::NackHold);
        assert_eq!(c.rx_recommended, before);
    }

    /// R1: the leak is the only announced move per 10 min after the budget is spent, and the
    /// unanswered sender is back at the anchor by then, so a leaked failure at a probe steps the
    /// anchor as today rather than withdrawing.
    #[test]
    fn a_leaked_failure_at_a_probe_steps_down() {
        let mut c = probing(SpeedLevel::Sl9);
        let ack = c.on_rx_frame_with(RxOutcome::Failed, None, NackVerdict::Leaked);
        assert_eq!(ack.decision, RateDecision::NackStepDown);
        assert_eq!(c.rx_confirmed, SpeedLevel::Sl8);
        assert_eq!(ack.recommended_level, SpeedLevel::Sl8);
        assert_eq!(c.rx_climb_backoff, None, "a leak sets no memory");
    }

    #[test]
    fn a_leaked_nack_steps_on_its_own() {
        // The leak follows at least `nack_threshold` counted failures, so it announces one step.
        let mut c = climbed();
        let before = c.rx_confirmed;
        let ack = c.on_rx_frame_with(RxOutcome::Failed, None, NackVerdict::Leaked);
        assert_eq!(ack.decision, RateDecision::NackStepDown);
        assert!(c.rx_confirmed < before);
        assert_eq!(ack.recommended_level, c.rx_confirmed);
    }

    #[test]
    fn keyed_is_todays_behaviour() {
        let mut a = climbed();
        let mut b = climbed();
        for i in 0..7 {
            let snr = (i % 2 == 0).then_some(HIGH_SNR);
            let x = a.on_rx_frame(RxOutcome::Failed, snr);
            let y = b.on_rx_frame_with(RxOutcome::Failed, snr, NackVerdict::Keyed);
            assert_eq!(x.recommended_level, y.recommended_level);
            assert_eq!(x.decision, y.decision);
        }
    }

    #[test]
    fn max_level_clamp_caps_the_climb() {
        let mut c = ctrl();
        c.set_level_bounds(None, Some(SpeedLevel::Sl4));
        let mut sender_tx = c.tx_level();
        for _ in 0..30 {
            let ack = c.on_rx_frame(RxOutcome::Decoded(sender_tx), Some(HIGH_SNR));
            sender_tx = ack.recommended_level;
        }
        assert!(
            c.rx_confirmed <= SpeedLevel::Sl4,
            "must not climb past the max bound: {:?}",
            c.rx_confirmed
        );
        assert!(
            c.rx_recommended <= SpeedLevel::Sl4,
            "recommendation must respect the max bound"
        );
    }

    /// Climbs a controller on clean high-SNR decodes and returns the highest level it confirmed.
    fn climb(c: &mut OtaRateController, frames: usize) -> SpeedLevel {
        let mut sender_tx = c.tx_level();
        let mut top = sender_tx;
        for _ in 0..frames {
            let ack = c.on_rx_frame(RxOutcome::Decoded(sender_tx), Some(HIGH_SNR));
            sender_tx = ack.recommended_level;
            top = top.max(c.rx_confirmed).max(c.rx_recommended);
        }
        top
    }

    #[test]
    fn robust_profile_never_climbs_past_sl6() {
        let mut c = OtaRateController::new(SessionProfile::robust());
        assert_eq!(climb(&mut c, 60), SpeedLevel::Sl6);
    }

    #[test]
    fn operator_bounds_never_raise_the_profile_cap() {
        // Each of these used to overwrite the only cap field: a min-only bound (what the daemon
        // applies for `ota_min_level` alone), cleared bounds (`OtaSetLevelBounds` with empty
        // fields), and a max above the cap.
        for (min, max) in [
            (Some(SpeedLevel::Sl2), None),
            (None, None),
            (None, Some(SpeedLevel::Sl10)),
        ] {
            let mut c = OtaRateController::new(SessionProfile::robust());
            c.set_level_bounds(min, max);
            assert_eq!(
                climb(&mut c, 60),
                SpeedLevel::Sl6,
                "bounds {min:?}..{max:?}"
            );
            c.adopt_recommendation(SpeedLevel::Sl11);
            assert_eq!(
                c.tx_level(),
                SpeedLevel::Sl6,
                "a peer cannot lift the cap either"
            );
        }
    }

    #[test]
    fn an_operator_bound_below_the_cap_still_applies() {
        let mut c = OtaRateController::new(SessionProfile::robust());
        c.set_level_bounds(None, Some(SpeedLevel::Sl4));
        assert_eq!(climb(&mut c, 60), SpeedLevel::Sl4);
    }

    #[test]
    fn a_decode_at_the_cap_is_a_hold_not_a_climb() {
        let mut c = OtaRateController::new(SessionProfile::robust());
        climb(&mut c, 60);
        let ack = c.on_rx_frame(RxOutcome::Decoded(SpeedLevel::Sl6), Some(HIGH_SNR));
        assert_eq!(ack.decision, RateDecision::Hold);
        assert_eq!(ack.ack_type, AckType::AckOk);
        assert_eq!(ack.recommended_level, SpeedLevel::Sl6);
    }

    #[test]
    fn min_level_clamp_floors_the_descent() {
        let mut c = ctrl();
        // Climb up, then set a floor and hammer with failures.
        let mut sender_tx = c.tx_level();
        for _ in 0..10 {
            let ack = c.on_rx_frame(RxOutcome::Decoded(sender_tx), Some(HIGH_SNR));
            sender_tx = ack.recommended_level;
        }
        c.set_level_bounds(Some(SpeedLevel::Sl4), None);
        assert!(
            c.rx_confirmed >= SpeedLevel::Sl4,
            "bounds snap current level up to the floor"
        );
        for _ in 0..30 {
            let _ = c.on_rx_frame(RxOutcome::Failed, Some(LOW_SNR));
        }
        assert!(
            c.rx_recommended >= SpeedLevel::Sl4,
            "must not drop below the min bound: {:?}",
            c.rx_recommended
        );
    }

    #[test]
    fn lock_pins_both_directions_and_ignores_peer() {
        let mut c = ctrl();
        c.lock_level(SpeedLevel::Sl4);
        assert!(c.is_locked());
        assert_eq!(c.tx_level(), SpeedLevel::Sl4);
        assert_eq!(c.rx_recommended_level(), SpeedLevel::Sl4);
        // RX decisions stay pinned regardless of SNR.
        let ack = c.on_rx_frame(RxOutcome::Decoded(SpeedLevel::Sl4), Some(HIGH_SNR));
        assert_eq!(ack.recommended_level, SpeedLevel::Sl4);
        assert_eq!(c.rx_recommended_level(), SpeedLevel::Sl4);
        // Peer recommendations are ignored while locked.
        c.adopt_recommendation(SpeedLevel::Sl6);
        assert_eq!(c.tx_level(), SpeedLevel::Sl4);
        // Unlocking resumes adaptation.
        c.unlock();
        assert!(!c.is_locked());
        c.adopt_recommendation(SpeedLevel::Sl5);
        assert_eq!(c.tx_level(), SpeedLevel::Sl5);
    }

    #[test]
    fn low_snr_fast_downshifts_past_a_single_step() {
        let mut c = ctrl();
        // Climb several steps so a multi-step drop is possible.
        let mut sender_tx = c.tx_level();
        for _ in 0..6 {
            let ack = c.on_rx_frame(RxOutcome::Decoded(sender_tx), Some(HIGH_SNR));
            sender_tx = ack.recommended_level;
        }
        let conf = c.rx_confirmed;
        assert!(
            conf > c.next_mapped(c.lo()),
            "precondition: climbed ≥2 steps above the floor"
        );
        // A FAILED frame at very low SNR drops the recommendation straight to the SNR-adequate
        // floor — several steps, not one. This is where the SNR estimate earns its keep: it
        // *explains* the failure, so acting on it saves crawling down one rung per NACK threshold.
        //
        // This used to fire on a DECODED frame too. It no longer does, deliberately (#934): a frame
        // that decoded is direct evidence the rung works, and an estimate can be flatly wrong — on a
        // fade BPSK31 reads ≈ −12.6 dB at ANY true SNR, so every successful frame was answered with
        // "drop to the floor" and the link sat pinned at ~5 bps while delivering every frame. The
        // cost of waiting for the failure is one wasted frame per genuine collapse; the cost of not
        // waiting was a permanently pinned link. See `tests/success_based_climb.rs`.
        let ack = c.on_rx_frame(RxOutcome::Failed, Some(LOW_SNR));
        assert_eq!(ack.ack_type, AckType::Nack);
        assert_eq!(
            ack.recommended_level,
            c.lo(),
            "jumps to the SNR floor level"
        );
        assert!(
            c.lo() < c.prev_mapped(conf),
            "the drop went below a single step (multi-step downshift)"
        );
    }

    /// Climbs on clean high-SNR frames until `at` is confirmed and the next rung is recommended.
    fn probing(at: SpeedLevel) -> OtaRateController {
        let mut c = ctrl();
        let mut sender_tx = c.tx_level();
        for _ in 0..30 {
            if c.rx_confirmed == at && c.rx_recommended > at {
                return c;
            }
            sender_tx = c
                .on_rx_frame(RxOutcome::Decoded(sender_tx), Some(HIGH_SNR))
                .recommended_level;
        }
        panic!("never reached a probe from {at:?}");
    }

    /// Would-be climbs declined at `rung`'s anchor before the next probe of `rung`, with each anchor
    /// decode carrying `airtime_s` and an SNR that clears the ceiling when `snr` is set.
    fn declines_before_probe(
        c: &mut OtaRateController,
        rung: SpeedLevel,
        snr: Option<f32>,
        airtime_s: Option<f32>,
    ) -> usize {
        let anchor = c.rx_confirmed;
        let mut declined = 0;
        for _ in 0..100 {
            let ack = c.on_rx_frame_timed(
                RxOutcome::Decoded(anchor),
                snr,
                NackVerdict::Keyed,
                airtime_s,
            );
            match ack.decision {
                RateDecision::ClimbDeclined => declined += 1,
                RateDecision::ClimbOnSnr | RateDecision::ClimbOnEvidence => {
                    assert_eq!(ack.recommended_level, rung);
                    return declined;
                }
                _ => {}
            }
        }
        panic!("{rung:?} was never probed again");
    }

    /// f2k: on a link whose ceiling is SL9, three failed SL10 frames stepped the anchor to SL8.
    #[test]
    fn a_failed_probe_withdraws_to_the_confirmed_rung_and_keeps_the_anchor() {
        let mut c = probing(SpeedLevel::Sl9);
        let ack = c.on_rx_frame(RxOutcome::Failed, None);
        assert_eq!(ack.decision, RateDecision::ProbeWithdrawn);
        assert_eq!(ack.ack_type, AckType::Nack);
        assert_eq!(ack.recommended_level, SpeedLevel::Sl9);
        assert_eq!(
            (c.rx_recommended, c.rx_confirmed),
            (SpeedLevel::Sl9, SpeedLevel::Sl9)
        );
        assert_eq!(
            c.rx_climb_backoff(),
            Some(RxClimbBackoff {
                rung: SpeedLevel::Sl10,
                anchor: SpeedLevel::Sl9,
                left: RX_CLIMB_BACKOFF_FIRST,
                n: RX_CLIMB_BACKOFF_FIRST,
                airtime_s: 0.0,
            })
        );
        // The NACK count restarted: two more failures at SL9 hold, the third would step.
        for _ in 1..c.profile.nack_threshold {
            let ack = c.on_rx_frame(RxOutcome::Failed, None);
            assert_eq!(ack.decision, RateDecision::NackHold);
        }
        assert_eq!(c.rx_confirmed, SpeedLevel::Sl9, "the probe moved no anchor");
    }

    #[test]
    fn a_withdrawn_rung_is_declined_then_reprobed_and_the_wait_doubles_to_16() {
        let mut c = probing(SpeedLevel::Sl9);
        let mut waits = Vec::new();
        for _ in 0..4 {
            assert_eq!(
                c.on_rx_frame(RxOutcome::Failed, None).decision,
                RateDecision::ProbeWithdrawn
            );
            // At OFDM a frame is short against the expiry: the count is the operative constant.
            waits.push(declines_before_probe(
                &mut c,
                SpeedLevel::Sl10,
                Some(HIGH_SNR),
                Some(0.79),
            ));
        }
        assert_eq!(waits, [4, 8, 16, 16]);
    }

    /// R2: below SL7 the expiry binds before the count, and it doubles 120 → 240 s, capped.
    #[test]
    fn the_memory_expires_after_120_s_then_240_s_of_clean_airtime() {
        const SL2_FRAME_S: f32 = 66.56;
        let mut c = ctrl();
        assert_eq!(c.rx_confirmed, SpeedLevel::Sl2);
        let ack = c.on_rx_frame_timed(
            RxOutcome::Decoded(SpeedLevel::Sl2),
            None,
            NackVerdict::Keyed,
            Some(SL2_FRAME_S),
        );
        assert_eq!(ack.recommended_level, SpeedLevel::Sl3);
        let mut waits = Vec::new();
        for _ in 0..3 {
            assert_eq!(
                c.on_rx_frame(RxOutcome::Failed, None).decision,
                RateDecision::ProbeWithdrawn
            );
            waits.push(declines_before_probe(
                &mut c,
                SpeedLevel::Sl3,
                None,
                Some(SL2_FRAME_S),
            ));
        }
        // 120 s: the 2nd frame (133 s) re-probes; 240 s: the 4th (266 s), and it stays capped there.
        assert_eq!(waits, [1, 3, 3]);
    }

    #[test]
    fn a_decode_at_or_above_the_rung_clears_the_memory() {
        let mut c = probing(SpeedLevel::Sl9);
        c.on_rx_frame(RxOutcome::Failed, None);
        declines_before_probe(&mut c, SpeedLevel::Sl10, Some(HIGH_SNR), None);
        c.on_rx_frame(RxOutcome::Decoded(SpeedLevel::Sl10), Some(HIGH_SNR));
        assert_eq!(c.rx_climb_backoff(), None, "the rung decodes after all");
        // So the next withdrawn probe starts from the first count, not a doubled one.
        assert_eq!(c.rx_recommended, SpeedLevel::Sl11);
        c.on_rx_frame(RxOutcome::Failed, None);
        assert_eq!(
            c.rx_climb_backoff().map(|b| b.n),
            Some(RX_CLIMB_BACKOFF_FIRST)
        );
    }

    #[test]
    fn a_step_below_the_anchor_clears_it() {
        let mut c = probing(SpeedLevel::Sl9);
        c.on_rx_frame(RxOutcome::Failed, None);
        assert!(c.rx_climb_backoff().is_some());
        let mut last = RateDecision::Hold;
        for _ in 0..c.profile.nack_threshold {
            last = c.on_rx_frame(RxOutcome::Failed, None).decision;
        }
        assert_eq!(last, RateDecision::NackStepDown);
        assert_eq!(c.rx_confirmed, SpeedLevel::Sl8);
        assert_eq!(
            c.rx_climb_backoff(),
            None,
            "the channel moved under the memory"
        );
        // And a decode below the anchor clears it too.
        let mut c = probing(SpeedLevel::Sl9);
        c.on_rx_frame(RxOutcome::Failed, None);
        c.on_rx_frame(RxOutcome::Decoded(SpeedLevel::Sl8), None);
        assert_eq!(c.rx_climb_backoff(), None);
    }

    #[test]
    fn a_muted_probe_failure_withdraws_nothing() {
        let mut c = probing(SpeedLevel::Sl9);
        let ack = c.on_rx_frame_with(RxOutcome::Failed, None, NackVerdict::Muted);
        assert_eq!(ack.decision, RateDecision::NackHold);
        assert_eq!(
            c.rx_recommended,
            SpeedLevel::Sl10,
            "a failure nobody hears moves nothing"
        );
        assert_eq!(c.rx_climb_backoff(), None);
    }

    #[test]
    fn a_lock_or_new_bounds_drop_the_climb_memory() {
        let mut c = probing(SpeedLevel::Sl9);
        c.on_rx_frame(RxOutcome::Failed, None);
        c.lock_level(SpeedLevel::Sl9);
        c.unlock();
        assert_eq!(c.rx_climb_backoff(), None);
        let mut c = probing(SpeedLevel::Sl9);
        c.on_rx_frame(RxOutcome::Failed, None);
        c.set_level_bounds(None, None);
        assert!(c.rx_climb_backoff().is_some(), "unchanged bounds keep it");
        c.set_level_bounds(Some(SpeedLevel::Sl7), None);
        assert_eq!(c.rx_climb_backoff(), None);
    }

    /// Change 2: one SL2 frame's airtime proves the rung, rather than 3 × 70 s on air.
    #[test]
    fn one_sl2_frame_of_airtime_proves_the_rung() {
        let mut c = ctrl();
        let ack = c.on_rx_frame_timed(
            RxOutcome::Decoded(SpeedLevel::Sl2),
            None,
            NackVerdict::Keyed,
            Some(66.56),
        );
        assert_eq!(ack.decision, RateDecision::ClimbOnEvidence);
        assert_eq!(ack.ack_type, AckType::AckUp);
        assert_eq!(ack.recommended_level, SpeedLevel::Sl3);
    }

    #[test]
    fn airtime_without_a_decode_proves_nothing() {
        let mut c = ctrl();
        c.on_rx_frame_timed(RxOutcome::Failed, None, NackVerdict::Keyed, Some(66.56));
        let ack = c.on_rx_frame_timed(
            RxOutcome::Decoded(SpeedLevel::Sl2),
            None,
            NackVerdict::Keyed,
            Some(10.0),
        );
        assert_eq!(ack.decision, RateDecision::Hold);
    }

    #[test]
    fn a_failure_resets_the_airtime() {
        let mut c = ctrl();
        let d = |c: &mut OtaRateController| {
            c.on_rx_frame_timed(
                RxOutcome::Decoded(SpeedLevel::Sl2),
                None,
                NackVerdict::Keyed,
                Some(40.0),
            )
            .decision
        };
        assert_eq!(d(&mut c), RateDecision::Hold);
        c.on_rx_frame(RxOutcome::Failed, None);
        assert_eq!(
            d(&mut c),
            RateDecision::Hold,
            "40 s after a failure, not 80 s"
        );
        assert_eq!(
            d(&mut c),
            RateDecision::ClimbOnEvidence,
            "80 s in one streak"
        );
    }

    #[test]
    fn no_airtime_is_todays_rule() {
        let mut c = ctrl();
        let decisions: Vec<RateDecision> = (0..ACK_CLIMB_THRESHOLD)
            .map(|_| {
                c.on_rx_frame(RxOutcome::Decoded(SpeedLevel::Sl2), None)
                    .decision
            })
            .collect();
        assert_eq!(
            decisions,
            [
                RateDecision::Hold,
                RateDecision::Hold,
                RateDecision::ClimbOnEvidence
            ]
        );
    }

    /// Change 2 alone ("C2 alone" in the design): a one-frame SL2 climb that fails took three NACKs,
    /// and the third stepped onto the MFSK16 sub-floor. There is no switch: the airtime climb exists
    /// only in a controller that withdraws, and this closed loop is what that pairing buys.
    #[test]
    fn an_airtime_climb_that_fails_never_reaches_the_sub_floor() {
        let mut rx = ctrl();
        let mut tx = ctrl();
        let mut probes = 0;
        for _ in 0..20 {
            let sent = tx.tx_level();
            let outcome = if sent == SpeedLevel::Sl3 {
                probes += 1;
                RxOutcome::Failed
            } else {
                RxOutcome::Decoded(sent)
            };
            let ack = rx.on_rx_frame_timed(outcome, None, NackVerdict::Keyed, Some(66.56));
            tx.adopt_recommendation_from(ack.recommended_level, ack.ack_type == AckType::Nack);
            assert!(
                rx.rx_confirmed >= SpeedLevel::Sl2 && tx.tx_level() >= SpeedLevel::Sl2,
                "the anchor left SL2 for the sub-floor: rx {:?}, tx {:?}",
                rx.rx_confirmed,
                tx.tx_level()
            );
        }
        assert!(probes >= 3, "SL3 was probed {probes} times");
    }

    #[test]
    fn cautious_upshift_still_climbs_one_step_only() {
        // The up direction is unchanged: even at unbounded SNR the recommendation advances by at
        // most one mapped step per confirmed decode (never leaps to the SNR-adequate top).
        let mut c = ctrl();
        let conf = c.rx_confirmed;
        let ack = c.on_rx_frame(RxOutcome::Decoded(conf), Some(HIGH_SNR));
        assert_eq!(ack.ack_type, AckType::AckUp);
        assert_eq!(
            ack.recommended_level,
            c.next_mapped(conf),
            "up-shift is one proven step, not a jump to the SNR-adequate ceiling"
        );
    }
}

#[cfg(test)]
mod abstention_tests {
    use super::*;

    fn probing() -> OtaRateController {
        let mut c = OtaRateController::new(SessionProfile::fast());
        // Climb to a rung well above the floor so a downshift has somewhere to fall from.
        for _ in 0..12 {
            c.on_rx_frame(RxOutcome::Decoded(c.rx_recommended_level()), Some(40.0));
        }
        assert!(c.rx_recommended_level() > c.rx_confirmed_level());
        c
    }

    /// As [`probing`], then a decode that does not climb: no probe outstanding, so a failure is the
    /// confirmed rung's and runs the hysteresis rather than withdrawing a probe.
    fn ctl() -> OtaRateController {
        let mut c = probing();
        let conf = c.rx_confirmed_level();
        assert_eq!(
            c.on_rx_frame(RxOutcome::Decoded(conf), None).decision,
            RateDecision::Hold
        );
        assert_eq!(c.rx_recommended_level(), conf);
        c
    }

    /// The probe case of the test below: a failure with no reading while a probe is outstanding
    /// withdraws one step to the confirmed rung, never further.
    #[test]
    fn a_failed_probe_without_an_snr_reading_withdraws_only_to_the_confirmed_rung() {
        let mut c = probing();
        let conf = c.rx_confirmed_level();
        let ack = c.on_rx_frame(RxOutcome::Failed, None);
        assert_eq!(ack.decision, RateDecision::ProbeWithdrawn);
        assert_eq!(ack.recommended_level, conf);
        assert_eq!(c.rx_confirmed_level(), conf);
    }

    // Deliberately carries NO `VERIFIES:` id. #1229's id-conformance check refused the one I first
    // wrote — an invented off-convention id, which is exactly what that check exists to catch, and
    // it is not spelled out here because the checker scans this file too — and inventing an id to decorate a comment
    // is backwards: registering a requirement is a deliberate act with its own obligations (#1235,
    // #1237), not a side effect of writing a test. The behaviour is #1142's fix — a failed decode
    // with no trustworthy SNR must not fast-downshift, but fall through to the NACK hysteresis.
    #[test]
    fn a_failed_decode_without_an_snr_reading_does_not_crash_the_ladder() {
        // POSITIVE CONTROL FIRST: a genuinely low reading still fast-downshifts, so the None case
        // below cannot pass merely because this controller never downshifts at all.
        let mut with_reading = ctl();
        let before = with_reading.rx_recommended_level();
        let ack = with_reading.on_rx_frame(RxOutcome::Failed, Some(-20.0));
        assert_eq!(
            ack.decision,
            RateDecision::FastDownshift,
            "positive control failed: a low SNR must still explain a failure and downshift fast"
        );
        assert!(
            (ack.recommended_level as u8) < (before as u8),
            "positive control failed: FastDownshift must actually move the rung"
        );

        // THE FIX: with no reading, one failure must not move the recommendation at all — the
        // hysteresis owns that decision, exactly as it does when the SNR does not explain the
        // failure.
        let mut abstaining = ctl();
        let before = abstaining.rx_recommended_level();
        let ack = abstaining.on_rx_frame(RxOutcome::Failed, None);
        assert_ne!(
            ack.decision,
            RateDecision::FastDownshift,
            "a failed decode with no SNR reading fast-downshifted — before #1142 this crashed the \
             recommendation to the bottom of the ladder on a single blip"
        );
        assert_eq!(
            ack.recommended_level, before,
            "one failure with no reading must hold the rung; the NACK hysteresis decides"
        );
    }

    // The other half: abstention must not silently freeze the ladder either — repeated failures
    // still step it down through the hysteresis.
    #[test]
    fn abstention_still_steps_down_at_the_nack_threshold() {
        let mut c = ctl();
        let start = c.rx_recommended_level();
        let mut moved = false;
        for _ in 0..c.profile.nack_threshold {
            let ack = c.on_rx_frame(RxOutcome::Failed, None);
            if ack.recommended_level != start {
                moved = true;
            }
        }
        assert!(
            moved,
            "repeated failures with no reading must still step the rung down at the NACK threshold \
             — abstaining must not mean never adapting"
        );
    }
}
