//! The resolved system name per (direction, selector), so an open by name skips the full enumeration.
//!
//! Naming every device costs ~0.53 s on rpi51, and every keyed transmit opened its output by name
//! inside the PTT window (`docs/dev/design/key-to-audio-gap.md`, round 2). Only the name is kept:
//! a held `cpal::Device` truncates ALSA's enumeration and holds the card's capture PCM (round 1
//! review, finding 2).

use std::collections::HashMap;
use std::sync::Mutex;

use openpulse_core::audio::{resolve_device, DeviceResolution};
use openpulse_core::error::AudioError;
use tracing::debug;

/// Which enumeration a selector was resolved against; input and output name lists differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Direction {
    Input,
    Output,
}

/// One enumeration: the device named `target` if it was reached, and every name seen before it.
pub(crate) struct Scan<D> {
    pub(crate) found: Option<D>,
    pub(crate) names: Vec<String>,
}

/// Resolved names, keyed by direction and the configured selector.
#[derive(Default)]
pub(crate) struct NameCache {
    names: Mutex<HashMap<(Direction, String), String>>,
}

impl NameCache {
    /// The device for `selector`. `scan(Some(name))` enumerates up to `name` and stops there;
    /// `scan(None)` names every device.
    ///
    /// A cached name costs one scan up to its match. A miss, or a cached name that has vanished,
    /// resolves from a full name list — the failed scan's own, so a vanished card costs no second
    /// enumeration (round 2 review, finding 4) — and then fetches the match. Ambiguity is never
    /// cached.
    pub(crate) fn select<D, F>(
        &self,
        direction: Direction,
        selector: &str,
        mut scan: F,
    ) -> Result<D, AudioError>
    where
        F: FnMut(Option<&str>) -> Result<Scan<D>, AudioError>,
    {
        let key = (direction, selector.to_string());
        // Bound first: a guard in the `match` scrutinee lives through the arms, and the eviction
        // below would deadlock on it.
        let cached = self.lock().get(&key).cloned();
        let names = match cached {
            Some(cached) => {
                let hit = scan(Some(&cached))?;
                if let Some(device) = hit.found {
                    debug!("audio {direction:?} '{selector}': cached name '{cached}'");
                    return Ok(device);
                }
                debug!("audio {direction:?} '{selector}': cached name '{cached}' vanished; resolving again");
                self.lock().remove(&key);
                hit.names
            }
            None => scan(None)?.names,
        };
        let resolved = resolve_name_from(&names, selector)?;
        let device = scan(Some(&resolved))?
            .found
            .ok_or_else(|| AudioError::DeviceNotFound(resolved.clone()))?;
        self.lock().insert(key, resolved);
        Ok(device)
    }

    /// Forget `selector`'s name, so the next open resolves in full.
    pub(crate) fn evict(&self, direction: Direction, selector: &str) {
        self.lock().remove(&(direction, selector.to_string()));
    }

    // Held for one get, insert or remove, never across an enumeration; a panic elsewhere must not
    // poison every later open.
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<(Direction, String), String>> {
        self.names.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// Apply the hotplug-safe resolver to an already-collected name list.
pub(crate) fn resolve_name_from(names: &[String], selector: &str) -> Result<String, AudioError> {
    match resolve_device(selector, names) {
        DeviceResolution::Resolved(name) => {
            if name != selector {
                debug!("audio device '{selector}' resolved to '{name}' (hotplug-safe match)");
            }
            Ok(name)
        }
        DeviceResolution::Ambiguous(hits) => Err(AudioError::DeviceNotFound(format!(
            "'{selector}' matches multiple devices ({}); set an exact name",
            hits.join(", ")
        ))),
        DeviceResolution::NotFound => Err(AudioError::DeviceNotFound(selector.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// A host whose device list can change between opens, recording every scan's target.
    struct FakeHost {
        devices: RefCell<Vec<String>>,
        scans: RefCell<Vec<Option<String>>>,
    }

    impl FakeHost {
        fn new(devices: &[&str]) -> Self {
            Self {
                devices: RefCell::new(devices.iter().map(|d| d.to_string()).collect()),
                scans: RefCell::new(Vec::new()),
            }
        }

        fn scan(&self, target: Option<&str>) -> Result<Scan<String>, AudioError> {
            self.scans.borrow_mut().push(target.map(str::to_string));
            let mut names = Vec::new();
            for d in self.devices.borrow().iter() {
                if Some(d.as_str()) == target {
                    return Ok(Scan {
                        found: Some(d.clone()),
                        names,
                    });
                }
                names.push(d.clone());
            }
            Ok(Scan { found: None, names })
        }

        fn open(&self, cache: &NameCache, dir: Direction, sel: &str) -> Result<String, AudioError> {
            cache.select(dir, sel, |t| self.scan(t))
        }

        fn full_scans(&self) -> usize {
            self.scans.borrow().iter().filter(|s| s.is_none()).count()
        }

        fn take_scans(&self) -> Vec<Option<String>> {
            self.scans.borrow_mut().drain(..).collect()
        }
    }

    const CODEC: &str = "plughw:CARD=CODEC,DEV=0";

    #[test]
    fn n_opens_of_one_selector_name_every_device_once() {
        let host = FakeHost::new(&["hdmi", CODEC, "pulse"]);
        let cache = NameCache::default();
        for _ in 0..5 {
            assert_eq!(
                host.open(&cache, Direction::Output, "CODEC").unwrap(),
                CODEC
            );
        }
        assert_eq!(host.full_scans(), 1);
        let scans = host.take_scans();
        assert_eq!(
            scans.len(),
            6,
            "one full scan, then one scan to the match per open"
        );
        assert!(scans[1..].iter().all(|s| s.as_deref() == Some(CODEC)));
    }

    #[test]
    fn a_vanished_name_resolves_from_the_failed_scan_and_caches_the_new_name() {
        let host = FakeHost::new(&["hdmi", CODEC]);
        let cache = NameCache::default();
        host.open(&cache, Direction::Output, "CODEC").unwrap();
        host.take_scans();

        let renumbered = "plughw:CARD=CODEC,DEV=1";
        *host.devices.borrow_mut() = vec!["hdmi".into(), renumbered.into()];
        assert_eq!(
            host.open(&cache, Direction::Output, "CODEC").unwrap(),
            renumbered
        );
        assert_eq!(
            host.take_scans(),
            vec![Some(CODEC.to_string()), Some(renumbered.to_string())],
            "the failed scan's names feed the resolver; no full scan"
        );

        host.open(&cache, Direction::Output, "CODEC").unwrap();
        assert_eq!(host.take_scans(), vec![Some(renumbered.to_string())]);
    }

    #[test]
    fn an_unplugged_card_fails_without_a_second_enumeration_and_is_evicted() {
        let host = FakeHost::new(&["hdmi", CODEC]);
        let cache = NameCache::default();
        host.open(&cache, Direction::Output, "CODEC").unwrap();
        host.take_scans();

        *host.devices.borrow_mut() = vec!["hdmi".into()];
        assert!(matches!(
            host.open(&cache, Direction::Output, "CODEC"),
            Err(AudioError::DeviceNotFound(_))
        ));
        assert_eq!(host.take_scans(), vec![Some(CODEC.to_string())]);

        *host.devices.borrow_mut() = vec!["hdmi".into(), CODEC.into()];
        host.open(&cache, Direction::Output, "CODEC").unwrap();
        assert_eq!(host.full_scans(), 1, "the replugged card resolves in full");
    }

    #[test]
    fn input_and_output_selectors_are_cached_apart() {
        let host = FakeHost::new(&[CODEC]);
        let cache = NameCache::default();
        host.open(&cache, Direction::Output, "CODEC").unwrap();
        host.open(&cache, Direction::Input, "CODEC").unwrap();
        assert_eq!(host.full_scans(), 2);
    }

    #[test]
    fn an_ambiguous_selector_is_never_cached() {
        let host = FakeHost::new(&["plughw:CARD=CODEC,DEV=0", "plughw:CARD=CODEC_1,DEV=0"]);
        let cache = NameCache::default();
        for _ in 0..2 {
            assert!(host.open(&cache, Direction::Output, "codec").is_err());
        }
        assert_eq!(host.full_scans(), 2);
    }

    #[test]
    fn an_evicted_selector_resolves_in_full_on_the_next_open() {
        let host = FakeHost::new(&[CODEC]);
        let cache = NameCache::default();
        host.open(&cache, Direction::Output, "CODEC").unwrap();
        cache.evict(Direction::Output, "CODEC");
        host.open(&cache, Direction::Output, "CODEC").unwrap();
        assert_eq!(host.full_scans(), 2);
    }
}
