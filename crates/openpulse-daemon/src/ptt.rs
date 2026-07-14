//! Shared, watchdog-protected PTT state (issue #863).
//!
//! The PTT controller and its max-duration deadline live behind one `Arc<Mutex<_>>` ([`SharedPtt`]) so
//! an **independent watchdog thread** ([`SharedPtt::spawn_watchdog`]) can force-release the transmitter
//! on its deadline even while the daemon's single async command loop is blocked inside a long handler (a
//! QSY scan or an OTA send-retry burst) — a `select!` arm (PR #853) cannot, because the loop never
//! re-enters `select!` during such a handler.
//!
//! **Lock discipline:** the mutex is only ever held for the brief duration of a hardware assert/release
//! or a deadline read/write — *never* across an RF burst — so the watchdog can acquire it and preempt at
//! any point. Poisoned locks are recovered (`into_inner`) so a panicked TX path can never wedge the
//! watchdog.

use std::sync::{Arc, Mutex, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tokio::sync::broadcast;

use openpulse_radio::{PttController, PttError};

use crate::protocol::ControlEvent;

/// Default max continuous keyed time before the watchdog force-releases (Part 97 duty-cycle guidance).
pub const DEFAULT_PTT_MAX: Duration = Duration::from_secs(180);

/// How often the watchdog thread checks the deadline. Granularity is immaterial against a 180 s deadline.
const WATCHDOG_TICK: Duration = Duration::from_millis(100);

/// Result of [`SharedPtt::unkey`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnkeyOutcome {
    /// Hardware released and the watchdog was disarmed (a real keyed→unkeyed transition).
    Released,
    /// Nothing was armed (already released, e.g. the watchdog fired mid-burst) — no event emitted.
    NotKeyed,
    /// The hardware release failed; the watchdog is left **armed** so it can force-release later.
    Failed,
}

struct PttInner {
    controller: Option<Box<dyn PttController + Send>>,
    asserted_at: Option<Instant>,
    max_duration: Duration,
    /// Set once when the watchdog's force-release fails on a stuck rig, so the 100 ms retry loop logs
    /// the stuck transmitter once instead of every tick. Cleared on the next successful key/release.
    stuck_warned: bool,
}

/// PTT hardware + watchdog deadline behind a shared lock. Cheap to `clone` (shares the same lock).
#[derive(Clone)]
pub struct SharedPtt(Arc<Mutex<PttInner>>);

impl SharedPtt {
    /// Build from a PTT controller (`None` = no hardware) and the max keyed duration.
    pub fn new(controller: Option<Box<dyn PttController + Send>>, max_duration: Duration) -> Self {
        Self(Arc::new(Mutex::new(PttInner {
            controller,
            asserted_at: None,
            max_duration,
            stuck_warned: false,
        })))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, PttInner> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Key the transmitter and arm the watchdog. On hardware failure the deadline is left disarmed and no
    /// event is emitted (the caller skips the burst). `event_tx = None` keys silently (the beacon path,
    /// which historically emits no `PttChanged`).
    pub fn key(&self, event_tx: Option<&broadcast::Sender<ControlEvent>>) -> Result<(), PttError> {
        let mut g = self.lock();
        if let Some(ptt) = g.controller.as_mut() {
            ptt.assert_ptt()?;
        }
        g.asserted_at = Some(Instant::now());
        g.stuck_warned = false;
        // Emit under the lock so a concurrent watchdog force-release can't interleave its `{false}`
        // between this arm and its `{true}` (which would show unkeyed during a live keyed burst).
        if let Some(tx) = event_tx {
            let _ = tx.send(ControlEvent::PttChanged { active: true });
        }
        Ok(())
    }

    /// Release the transmitter and disarm the watchdog. A failed hardware release leaves the watchdog
    /// **armed** (so it force-releases later) and returns [`UnkeyOutcome::Failed`]. `PttChanged{false}` is
    /// emitted only on a real transition (so a burst whose deadline the watchdog already fired emits
    /// nothing — single-fire).
    pub fn unkey(&self, event_tx: Option<&broadcast::Sender<ControlEvent>>) -> UnkeyOutcome {
        let mut g = self.lock();
        if let Some(ptt) = g.controller.as_mut() {
            if let Err(e) = ptt.release_ptt() {
                tracing::warn!(error = %e, "PTT release failed; leaving the watchdog armed");
                return UnkeyOutcome::Failed;
            }
        }
        g.stuck_warned = false;
        if g.asserted_at.take().is_some() {
            // Emit under the lock (see `key`) so this `{false}` is ordered against any concurrent key.
            if let Some(tx) = event_tx {
                let _ = tx.send(ControlEvent::PttChanged { active: false });
            }
            UnkeyOutcome::Released
        } else {
            UnkeyOutcome::NotKeyed
        }
    }

    /// Key the transmitter and return an RAII guard that releases it on drop — including on an early
    /// return or a panic/unwind (REQ-PTT-01). Prefer this over paired `key`/`unkey` in any automatic-TX
    /// scope that can early-return or panic between them, so an unexpected key-down is bounded to the
    /// current stack scope instead of up to the 180 s watchdog. `event_tx` is cloned into the guard so
    /// the release edge is still emitted on unwind; `None` keys silently (the beacon path).
    pub fn keyed(
        &self,
        event_tx: Option<&broadcast::Sender<ControlEvent>>,
    ) -> Result<PttKeyGuard, PttError> {
        self.key(event_tx)?;
        Ok(PttKeyGuard {
            ptt: self.clone(),
            event_tx: event_tx.cloned(),
            released: false,
        })
    }

    /// Hardware assert only — no deadline change, no event. For the manual `PttAssert` command path,
    /// which arms the deadline + emits its event separately in `apply_command_to_engine`.
    pub fn hw_assert(&self) -> Result<(), PttError> {
        let mut g = self.lock();
        match g.controller.as_mut() {
            Some(ptt) => ptt.assert_ptt(),
            None => Ok(()),
        }
    }

    /// Hardware release only — no deadline change, no event.
    pub fn hw_release(&self) -> Result<(), PttError> {
        let mut g = self.lock();
        match g.controller.as_mut() {
            Some(ptt) => ptt.release_ptt(),
            None => Ok(()),
        }
    }

    /// Arm the watchdog deadline (deadline only, no hardware). For the manual `PttAssert` command.
    pub fn arm(&self) {
        let mut g = self.lock();
        g.asserted_at = Some(Instant::now());
        g.stuck_warned = false;
    }

    /// Disarm the watchdog deadline (deadline only, no hardware). For the manual `PttRelease` command.
    pub fn disarm(&self) {
        self.lock().asserted_at = None;
    }

    /// Whether the transmitter is currently considered keyed (the watchdog is armed).
    pub fn is_keyed(&self) -> bool {
        self.lock().asserted_at.is_some()
    }

    /// Time since the transmitter was keyed, or `None` when not keyed.
    pub fn elapsed(&self) -> Option<Duration> {
        self.lock().asserted_at.map(|t| t.elapsed())
    }

    /// The configured max keyed duration.
    pub fn max_duration(&self) -> Duration {
        self.lock().max_duration
    }

    /// Override the max keyed duration (tests / config).
    pub fn set_max_duration(&self, d: Duration) {
        self.lock().max_duration = d;
    }

    /// Force-release the transmitter if the deadline has elapsed. Idempotent and single-fire: the
    /// deadline is `take()`n under the lock, so exactly one `PttChanged{false}` is emitted per keying even
    /// if several callers (the watchdog thread, the rx-tick, a `select!` arm) race. Returns `true` only
    /// when the transmitter was actually released.
    ///
    /// On a **failed** hardware release (a stuck rig) the deadline is left **armed** and no `{false}` is
    /// emitted — the transmitter really is still keyed, so telling clients otherwise would be a lie. The
    /// 100 ms watchdog thread then retries every tick until the rig releases; the stuck condition is
    /// logged once (`stuck_warned`). This matches [`unkey`]'s "failed release stays armed" contract and
    /// upgrades the pre-#863 behaviour, which cleared + reported release even when the hardware failed.
    pub fn force_release_if_expired(&self, event_tx: &broadcast::Sender<ControlEvent>) -> bool {
        let mut g = self.lock();
        let expired = g
            .asserted_at
            .map(|t| t.elapsed() >= g.max_duration)
            .unwrap_or(false);
        if !expired {
            return false;
        }
        let hw_ok = match g.controller.as_mut() {
            Some(ptt) => ptt.release_ptt().is_ok(),
            None => true,
        };
        if !hw_ok {
            if !g.stuck_warned {
                g.stuck_warned = true;
                tracing::error!(
                    max_secs = g.max_duration.as_secs(),
                    "PTT watchdog: hardware release failed past max duration — transmitter may be \
                     stuck keyed; retrying every tick until it releases"
                );
            }
            return false; // stay armed so the next tick retries
        }
        g.asserted_at = None;
        g.stuck_warned = false;
        tracing::warn!(
            max_secs = g.max_duration.as_secs(),
            "PTT watchdog fired — transmitter keyed beyond max duration; released"
        );
        // Emit under the lock (see `key`) so this `{false}` is ordered against any concurrent re-key.
        let _ = event_tx.send(ControlEvent::PttChanged { active: false });
        true
    }

    /// Spawn the independent watchdog **thread**: every 100 ms it force-releases the transmitter if the
    /// deadline has passed, regardless of what the async loop is doing. A plain OS thread (not a tokio
    /// task) so it is immune to runtime flavor / worker starvation / a missing `block_in_place`. It holds
    /// only a `Weak` reference, so it exits on its own once the last `SharedPtt` is dropped (e.g. a twin
    /// daemon's `run` future ending) — no stop-flag plumbing, no leaked threads.
    pub fn spawn_watchdog(&self, event_tx: Arc<broadcast::Sender<ControlEvent>>) -> JoinHandle<()> {
        let weak: Weak<Mutex<PttInner>> = Arc::downgrade(&self.0);
        std::thread::Builder::new()
            .name("ptt-watchdog".into())
            .spawn(move || loop {
                std::thread::sleep(WATCHDOG_TICK);
                let Some(arc) = weak.upgrade() else {
                    break; // last SharedPtt dropped → nothing left to guard
                };
                SharedPtt(arc).force_release_if_expired(&event_tx);
            })
            .expect("failed to spawn ptt-watchdog thread")
    }
}

/// RAII guard from [`SharedPtt::keyed`] that releases the transmitter (and disarms the watchdog) when it
/// drops — including on an early return or a panic/unwind (REQ-PTT-01). Under the default `panic=unwind`
/// profile, `Drop` runs during unwinding, so the release happens *before* the panic reaches the task
/// boundary or crashes the process — the transmitter never stays keyed waiting for the 180 s watchdog.
#[must_use = "dropping the guard releases PTT; bind it for the transmit scope"]
pub struct PttKeyGuard {
    ptt: SharedPtt,
    event_tx: Option<broadcast::Sender<ControlEvent>>,
    released: bool,
}

impl PttKeyGuard {
    /// Release now instead of at scope end — for the half-duplex turnaround where PTT must drop before
    /// listening. Idempotent; after this the `Drop` is a no-op. Returns the underlying unkey outcome.
    pub fn release(mut self) -> UnkeyOutcome {
        self.release_inner()
    }

    fn release_inner(&mut self) -> UnkeyOutcome {
        if self.released {
            return UnkeyOutcome::NotKeyed;
        }
        self.released = true;
        self.ptt.unkey(self.event_tx.as_ref())
    }
}

impl Drop for PttKeyGuard {
    fn drop(&mut self) {
        let _ = self.release_inner();
    }
}

impl Default for SharedPtt {
    fn default() -> Self {
        Self::new(None, DEFAULT_PTT_MAX)
    }
}

impl std::fmt::Debug for SharedPtt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let g = self.lock();
        f.debug_struct("SharedPtt")
            .field("keyed_for", &g.asserted_at.map(|t| t.elapsed()))
            .field("max_duration", &g.max_duration)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A PTT double that counts releases and can be made to fail, and reports whether it is keyed.
    #[derive(Default)]
    struct FakePtt {
        releases: Arc<AtomicUsize>,
        asserts: Arc<AtomicUsize>,
        fail_assert: bool,
        fail_release: bool,
    }
    impl PttController for FakePtt {
        fn assert_ptt(&mut self) -> Result<(), PttError> {
            if self.fail_assert {
                return Err(PttError::Serial("assert failed".into()));
            }
            self.asserts.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        fn release_ptt(&mut self) -> Result<(), PttError> {
            if self.fail_release {
                return Err(PttError::Serial("stuck keyed".into()));
            }
            self.releases.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        fn is_asserted(&self) -> bool {
            false
        }
    }

    fn ev() -> Arc<broadcast::Sender<ControlEvent>> {
        Arc::new(broadcast::channel(16).0)
    }

    #[test]
    fn key_unkey_arms_and_disarms_with_single_events() {
        let releases = Arc::new(AtomicUsize::new(0));
        let ptt = SharedPtt::new(
            Some(Box::new(FakePtt {
                releases: releases.clone(),
                ..Default::default()
            })),
            DEFAULT_PTT_MAX,
        );
        let tx = ev();
        let mut rx = tx.subscribe();

        ptt.key(Some(&tx)).unwrap();
        assert!(ptt.is_keyed());
        assert!(matches!(
            rx.try_recv(),
            Ok(ControlEvent::PttChanged { active: true })
        ));

        assert_eq!(ptt.unkey(Some(&tx)), UnkeyOutcome::Released);
        assert!(!ptt.is_keyed());
        assert_eq!(releases.load(Ordering::SeqCst), 1);
        assert!(matches!(
            rx.try_recv(),
            Ok(ControlEvent::PttChanged { active: false })
        ));

        // A second unkey is a no-op (nothing armed) and emits nothing.
        assert_eq!(ptt.unkey(Some(&tx)), UnkeyOutcome::NotKeyed);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn failed_release_leaves_the_watchdog_armed() {
        let ptt = SharedPtt::new(
            Some(Box::new(FakePtt {
                fail_release: true,
                ..Default::default()
            })),
            DEFAULT_PTT_MAX,
        );
        ptt.arm();
        assert_eq!(ptt.unkey(None), UnkeyOutcome::Failed);
        assert!(
            ptt.is_keyed(),
            "a failed release must leave the watchdog armed"
        );
    }

    #[test]
    fn force_release_is_single_fire_and_idempotent() {
        let releases = Arc::new(AtomicUsize::new(0));
        let ptt = SharedPtt::new(
            Some(Box::new(FakePtt {
                releases: releases.clone(),
                ..Default::default()
            })),
            Duration::from_nanos(1),
        );
        let tx = ev();
        let mut rx = tx.subscribe();

        ptt.arm(); // deadline (1 ns) already elapsed
        assert!(ptt.force_release_if_expired(&tx));
        assert!(!ptt.is_keyed());
        assert_eq!(releases.load(Ordering::SeqCst), 1);
        assert!(matches!(
            rx.try_recv(),
            Ok(ControlEvent::PttChanged { active: false })
        ));

        // Not armed → no second fire, no second event.
        assert!(!ptt.force_release_if_expired(&tx));
        assert_eq!(releases.load(Ordering::SeqCst), 1);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn not_expired_does_not_fire() {
        let ptt = SharedPtt::new(Some(Box::new(FakePtt::default())), DEFAULT_PTT_MAX);
        ptt.arm();
        let tx = ev();
        assert!(
            !ptt.force_release_if_expired(&tx),
            "180 s deadline not yet reached"
        );
        assert!(ptt.is_keyed());
    }

    /// The independent watchdog **thread** force-releases while nothing else touches the PTT (a blocked
    /// loop *is* the absence of cooperative calls). Proves the preemption the `select!` arm can't give.
    #[test]
    fn watchdog_thread_force_releases_a_blocked_loop() {
        let releases = Arc::new(AtomicUsize::new(0));
        let ptt = SharedPtt::new(
            Some(Box::new(FakePtt {
                releases: releases.clone(),
                ..Default::default()
            })),
            Duration::from_millis(20),
        );
        let tx = ev();
        let mut rx = tx.subscribe();

        ptt.key(None).unwrap(); // keyed; deadline 20 ms
        let _wd = ptt.spawn_watchdog(tx.clone());

        // Simulate a blocked loop: make NO further SharedPtt calls; wait (deadline-bounded) for the
        // independent thread to force-release.
        let mut released = false;
        for _ in 0..200 {
            if !ptt.is_keyed() {
                released = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            released,
            "the watchdog thread must force-release without any loop cooperation"
        );
        assert_eq!(
            releases.load(Ordering::SeqCst),
            1,
            "exactly one hardware release"
        );
        assert!(matches!(
            rx.try_recv(),
            Ok(ControlEvent::PttChanged { active: false })
        ));
    }

    #[test]
    fn watchdog_thread_exits_when_the_last_shared_ptt_drops() {
        let ptt = SharedPtt::new(Some(Box::new(FakePtt::default())), DEFAULT_PTT_MAX);
        let handle = ptt.spawn_watchdog(ev());
        drop(ptt); // last strong ref gone
                   // The thread upgrades a Weak each tick; with no strong ref it breaks within ~one tick.
        assert!(handle.join().is_ok());
    }

    /// A stuck rig (release keeps failing) must keep the watchdog armed and stay silent — the
    /// transmitter really is still keyed — and only release + notify once the hardware recovers.
    #[test]
    fn watchdog_retries_a_stuck_rig_and_stays_armed_until_release() {
        use std::sync::atomic::AtomicBool;
        struct TogglePtt {
            fail: Arc<AtomicBool>,
            releases: Arc<AtomicUsize>,
        }
        impl PttController for TogglePtt {
            fn assert_ptt(&mut self) -> Result<(), PttError> {
                Ok(())
            }
            fn release_ptt(&mut self) -> Result<(), PttError> {
                if self.fail.load(Ordering::SeqCst) {
                    return Err(PttError::Serial("stuck keyed".into()));
                }
                self.releases.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
            fn is_asserted(&self) -> bool {
                false
            }
        }
        let fail = Arc::new(AtomicBool::new(true));
        let releases = Arc::new(AtomicUsize::new(0));
        let ptt = SharedPtt::new(
            Some(Box::new(TogglePtt {
                fail: fail.clone(),
                releases: releases.clone(),
            })),
            Duration::ZERO,
        );
        let tx = ev();
        let mut rx = tx.subscribe();
        ptt.arm(); // immediately expired

        // Stuck: the deadline is past but the hardware release fails → stays armed, no successful
        // release, no `{false}` event (clients must not be told the still-keyed rig is down).
        assert!(!ptt.force_release_if_expired(&tx));
        assert!(ptt.is_keyed(), "a stuck rig keeps the watchdog armed");
        assert_eq!(releases.load(Ordering::SeqCst), 0);
        assert!(
            rx.try_recv().is_err(),
            "no PttChanged{{false}} while the rig is still keyed"
        );

        // Rig recovers → the next tick releases, disarms, and emits exactly one `{false}`.
        fail.store(false, Ordering::SeqCst);
        assert!(ptt.force_release_if_expired(&tx));
        assert!(!ptt.is_keyed());
        assert_eq!(releases.load(Ordering::SeqCst), 1);
        assert!(matches!(
            rx.try_recv(),
            Ok(ControlEvent::PttChanged { active: false })
        ));
    }

    /// #863 mid-burst race: the watchdog thread force-releases *during* a long blocked burst; when the
    /// burst finally finishes and the loop calls `unkey`, it must be a single-fire no-op — one `{false}`
    /// total across the thread and the loop, and outcome `NotKeyed`.
    #[test]
    fn a_late_unkey_after_the_watchdog_fired_is_a_silent_noop() {
        let releases = Arc::new(AtomicUsize::new(0));
        let ptt = SharedPtt::new(
            Some(Box::new(FakePtt {
                releases: releases.clone(),
                ..Default::default()
            })),
            Duration::from_millis(10),
        );
        let tx = ev();
        let mut rx = tx.subscribe();

        ptt.key(Some(&tx)).unwrap(); // {true}; deadline 10 ms
        assert!(matches!(
            rx.try_recv(),
            Ok(ControlEvent::PttChanged { active: true })
        ));

        let _wd = ptt.spawn_watchdog(tx.clone());
        // The "burst" runs long: no cooperative call while the thread force-releases.
        let mut fired = false;
        for _ in 0..200 {
            if !ptt.is_keyed() {
                fired = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(fired, "the watchdog thread fired mid-burst");
        assert!(
            matches!(
                rx.try_recv(),
                Ok(ControlEvent::PttChanged { active: false })
            ),
            "exactly one {{false}} from the watchdog"
        );

        // Burst finishes → the loop's late unkey is a no-op: no second event. (The redundant hardware
        // release is defensive and idempotent, so the release count is not asserted here.)
        assert_eq!(ptt.unkey(Some(&tx)), UnkeyOutcome::NotKeyed);
        assert!(
            rx.try_recv().is_err(),
            "no duplicate PttChanged{{false}} from the late unkey"
        );
    }

    #[test]
    fn key_guard_releases_at_scope_end() {
        let releases = Arc::new(AtomicUsize::new(0));
        let ptt = SharedPtt::new(
            Some(Box::new(FakePtt {
                releases: releases.clone(),
                ..Default::default()
            })),
            DEFAULT_PTT_MAX,
        );
        let tx = ev();
        let mut rx = tx.subscribe();
        {
            let _g = ptt.keyed(Some(&tx)).unwrap();
            assert!(ptt.is_keyed());
            assert!(matches!(
                rx.try_recv(),
                Ok(ControlEvent::PttChanged { active: true })
            ));
        } // guard drops here
        assert!(!ptt.is_keyed(), "the guard releases at scope end");
        assert_eq!(releases.load(Ordering::SeqCst), 1);
        assert!(matches!(
            rx.try_recv(),
            Ok(ControlEvent::PttChanged { active: false })
        ));
    }

    /// REQ-PTT-01: a panic inside a keyed scope must release the transmitter during unwinding — not leave
    /// it keyed for the 180 s watchdog.
    #[test]
    fn key_guard_releases_on_panic_unwind() {
        let releases = Arc::new(AtomicUsize::new(0));
        let ptt = SharedPtt::new(
            Some(Box::new(FakePtt {
                releases: releases.clone(),
                ..Default::default()
            })),
            DEFAULT_PTT_MAX,
        );
        let tx = ev();
        let ptt_in = ptt.clone();
        let tx_in = tx.clone();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _g = ptt_in.keyed(Some(&tx_in)).unwrap();
            assert!(ptt_in.is_keyed());
            panic!("boom mid keyed scope");
        }));
        assert!(result.is_err(), "the keyed scope panicked");
        assert!(
            !ptt.is_keyed(),
            "the guard released the transmitter on unwind"
        );
        assert_eq!(
            releases.load(Ordering::SeqCst),
            1,
            "hardware released exactly once during unwind"
        );
    }

    #[test]
    fn key_guard_explicit_release_is_single_fire() {
        let releases = Arc::new(AtomicUsize::new(0));
        let ptt = SharedPtt::new(
            Some(Box::new(FakePtt {
                releases: releases.clone(),
                ..Default::default()
            })),
            DEFAULT_PTT_MAX,
        );
        let g = ptt.keyed(None).unwrap();
        assert_eq!(g.release(), UnkeyOutcome::Released); // consumes the guard; its Drop then no-ops
        assert!(!ptt.is_keyed());
        assert_eq!(
            releases.load(Ordering::SeqCst),
            1,
            "explicit release + the moved guard's Drop release once total"
        );
    }
}
