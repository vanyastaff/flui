//! Native sampling admission shared by the bounded owner samplers.

use crate::PlatformError;
use crate::owner::signal::OwnerSignal;
use parking_lot::Mutex;
use std::sync::{Arc, Weak};
use std::time::Duration;

const RETRY_INTERVAL: Duration = Duration::from_millis(500);

/// A source that is told, never polled: the admission of its reads and the
/// owner it wakes. The only wake it asks for on its own is the retry of a
/// failed read ([`retry_deadline`](Self::retry_deadline)); a healthy source
/// has no deadline at all, so the AppKit and Android sources cost the owner
/// nothing while no preference changes.
pub(crate) struct WakingRead {
    admission: ReadAdmission,
    signal: Weak<OwnerSignal>,
}

impl WakingRead {
    pub(crate) fn new(signal: &Arc<OwnerSignal>) -> Self {
        Self {
            admission: ReadAdmission::default(),
            signal: Arc::downgrade(signal),
        }
    }

    /// [`ReadAdmission::read_observation`], without the wake: the flag says
    /// whether [`announce`](Self::announce) is owed.
    pub(crate) fn observe(
        &self,
        now: web_time::Instant,
        sample: impl FnOnce() -> Result<flui_platform::SystemPreferences, PlatformError>,
    ) -> Result<(flui_platform::SystemPreferences, bool), PlatformError> {
        self.admission.read_observation(now, sample)
    }

    /// Wake the owner once when `needs_delivery`. The signal keeps the wake
    /// owed until the owner's turn takes it, so changes that arrive before
    /// that turn share one wake.
    pub(crate) fn announce(&self, needs_delivery: bool) {
        if needs_delivery && let Some(signal) = self.signal.upgrade() {
            let _ = signal.wake();
        }
    }

    /// Read at `now` and wake the owner if the observation changed or
    /// recovered; an unchanged one is quiet.
    #[cfg(any(target_os = "android", test))]
    pub(crate) fn read(
        &self,
        now: web_time::Instant,
        sample: impl FnOnce() -> Result<flui_platform::SystemPreferences, PlatformError>,
    ) -> Result<flui_platform::SystemPreferences, PlatformError> {
        let (current, needs_delivery) = self.observe(now, sample)?;
        self.announce(needs_delivery);
        Ok(current)
    }

    pub(crate) fn retry_deadline(&self) -> Option<web_time::Instant> {
        self.admission.retry_deadline()
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn signal(&self) -> &Weak<OwnerSignal> {
        &self.signal
    }
}

#[derive(Default)]
pub(crate) struct ReadAdmission {
    state: Mutex<State>,
    accepted: Mutex<Option<flui_platform::SystemPreferences>>,
}

#[derive(Clone, Copy, Default)]
enum State {
    #[default]
    Ready,
    Reading,
    RetryAt(web_time::Instant),
}

impl ReadAdmission {
    /// Read and commit a native observation before outgoing notification. A
    /// cold source has no accepted value; errors never manufacture one or return
    /// an older accepted value as a successful refresh. The returned flag
    /// requests owner delivery after either a changed observation or a recovered
    /// read obligation; healthy unchanged reads remain quiet.
    pub(crate) fn read_observation(
        &self,
        now: web_time::Instant,
        sample: impl FnOnce() -> Result<flui_platform::SystemPreferences, PlatformError>,
    ) -> Result<(flui_platform::SystemPreferences, bool), PlatformError> {
        let attempt = self.begin(now)?;
        let recovering = attempt.recovering;
        let current = sample()?;
        let changed = {
            let mut accepted = self.accepted.lock();
            let changed = accepted.as_ref() != Some(&current);
            *accepted = Some(current.clone());
            changed
        };
        attempt.accept();
        Ok((current, changed || recovering))
    }

    /// When the read a failure postponed may run, while one is owed. A source
    /// that has no timer of its own wakes its owner then.
    pub(crate) fn retry_deadline(&self) -> Option<web_time::Instant> {
        let state = *self.state.lock();
        match state {
            State::RetryAt(deadline) => Some(deadline),
            State::Ready | State::Reading => None,
        }
    }

    pub(crate) fn begin(&self, now: web_time::Instant) -> Result<ReadAttempt<'_>, PlatformError> {
        let mut state = self.state.lock();
        match *state {
            State::Reading => return Err(PlatformError::PreferencesDeferred),
            State::RetryAt(deadline) if now < deadline => {
                return Err(PlatformError::PreferencesDeferred);
            }
            State::Ready | State::RetryAt(_) => {}
        }
        let recovering = matches!(*state, State::RetryAt(_));
        *state = State::Reading;
        Ok(ReadAttempt {
            admission: self,
            now,
            accepted: false,
            recovering,
        })
    }
}

pub(crate) struct ReadAttempt<'a> {
    admission: &'a ReadAdmission,
    now: web_time::Instant,
    accepted: bool,
    recovering: bool,
}

impl ReadAttempt<'_> {
    /// Commit the successful read before invoking any outgoing wake or callback.
    pub(crate) fn accept(mut self) {
        self.accepted = true;
    }
}

impl Drop for ReadAttempt<'_> {
    fn drop(&mut self) {
        let mut state = self.admission.state.lock();
        *state = if self.accepted {
            State::Ready
        } else {
            State::RetryAt(self.now + RETRY_INTERVAL)
        };
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::owner::signal::OwnerTurnSlot;

    /// A source behind an owner signal whose posts are counted, and the
    /// owner's turn that takes what a post asked for.
    struct Rig {
        source: WakingRead,
        signal: Arc<OwnerSignal>,
        slot: OwnerTurnSlot,
        posts: Arc<AtomicUsize>,
        turns: Arc<AtomicUsize>,
        origin: web_time::Instant,
    }

    impl Rig {
        fn new() -> Self {
            let posts = Arc::new(AtomicUsize::new(0));
            let signal = OwnerSignal::new({
                let posts = Arc::clone(&posts);
                Arc::new(move || {
                    posts.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })
            });
            signal.start().expect("the owner signal starts");
            let turns = Arc::new(AtomicUsize::new(0));
            let slot = OwnerTurnSlot::default();
            signal
                .register_in(&slot, {
                    let turns = Arc::clone(&turns);
                    Box::new(move || {
                        turns.fetch_add(1, Ordering::SeqCst);
                    })
                })
                .expect("the owner takes its turn callback");
            Self {
                source: WakingRead::new(&signal),
                signal,
                slot,
                posts,
                turns,
                origin: web_time::Instant::now(),
            }
        }

        /// The virtual instant `elapsed` after the rig began; no clock runs.
        fn at(&self, elapsed: Duration) -> web_time::Instant {
            self.origin + elapsed
        }

        /// Read the preference that holds `interval` at `elapsed`.
        fn read(&self, elapsed: Duration, interval: Duration) {
            let preferences = flui_platform::SystemPreferences::default().with_gestures(
                flui_platform::GesturePreferences::default().with_double_click_interval(interval),
            );
            self.source
                .read(self.at(elapsed), || Ok(preferences))
                .expect("the read succeeds");
        }

        /// The owner takes the turn a post asked for, if one was asked.
        fn take_turn(&self) {
            assert!(!self.signal.drive_in(&self.slot));
        }
    }

    /// The AppKit and Android sources are told, never polled: whatever span
    /// passes, a healthy source asks for no deadline and posts nothing; a
    /// change posts the owner once, and a re-read of the same value is quiet.
    /// A source that comes back with a periodic timer shows a deadline, or
    /// posts, in the idle span.
    #[test]
    fn a_healthy_source_never_wakes_the_owner_but_for_a_change() {
        let rig = Rig::new();
        let double_click = |millis| Duration::from_millis(millis);
        rig.read(Duration::ZERO, double_click(500));
        assert_eq!(rig.posts.load(Ordering::SeqCst), 1, "the first reading");
        rig.take_turn();
        assert_eq!(rig.turns.load(Ordering::SeqCst), 1);

        // An hour of idle, a second at a time: the owner hears nothing and
        // the source names no instant it needs the owner at.
        for second in 1..=3600 {
            let now = rig.at(Duration::from_secs(second));
            assert_eq!(rig.source.retry_deadline(), None, "a timer at {now:?}");
            rig.take_turn();
        }
        assert_eq!(rig.posts.load(Ordering::SeqCst), 1, "idle woke the owner");
        assert_eq!(rig.turns.load(Ordering::SeqCst), 1, "idle ran a turn");

        // The owner reads again on each notice and finds the same value.
        rig.read(Duration::from_secs(3601), double_click(500));
        assert_eq!(rig.posts.load(Ordering::SeqCst), 1, "an unchanged read");

        // Each change, each followed by the owner's turn, wakes it once.
        for (change, millis) in (1..=5).zip([600, 700, 800, 900, 1000]) {
            rig.read(Duration::from_secs(3602 + change), double_click(millis));
            rig.take_turn();
            assert_eq!(rig.posts.load(Ordering::SeqCst), 1 + change as usize);
            assert_eq!(rig.turns.load(Ordering::SeqCst), 1 + change as usize);
        }

        // Changes before the owner's turn share the one post the signal
        // keeps owed.
        let posted = rig.posts.load(Ordering::SeqCst);
        rig.read(Duration::from_secs(4000), double_click(1100));
        rig.read(Duration::from_secs(4001), double_click(1200));
        assert_eq!(rig.posts.load(Ordering::SeqCst), posted + 1);
        rig.take_turn();
        assert_eq!(rig.source.retry_deadline(), None);
    }

    /// A failed read is the one thing that asks for the owner later: one
    /// deadline at the retry interval, one wake when the retry recovers, and
    /// nothing after it.
    #[test]
    fn a_failed_read_owes_one_retry_and_then_nothing() {
        let rig = Rig::new();
        rig.read(Duration::ZERO, Duration::from_millis(500));
        rig.take_turn();
        let posted = rig.posts.load(Ordering::SeqCst);

        let failed = rig.source.read(rig.at(Duration::from_secs(10)), || {
            Err(PlatformError::Preferences {
                message: "getter failure".into(),
            })
        });
        assert!(failed.is_err());
        assert_eq!(
            rig.source.retry_deadline(),
            Some(rig.at(Duration::from_secs(10) + RETRY_INTERVAL))
        );
        assert_eq!(
            rig.posts.load(Ordering::SeqCst),
            posted,
            "a failure is quiet"
        );

        rig.read(
            Duration::from_secs(10) + RETRY_INTERVAL,
            Duration::from_millis(500),
        );
        assert_eq!(rig.posts.load(Ordering::SeqCst), posted + 1, "the recovery");
        rig.take_turn();
        assert_eq!(rig.source.retry_deadline(), None, "the retry is spent");
    }

    #[test]
    fn cold_native_observation_recovers_without_forged_defaults() {
        let now = web_time::Instant::now();
        let cold = ReadAdmission::default();
        let (_, needs_delivery) = cold
            .read_observation(now, || Ok(flui_platform::SystemPreferences::default()))
            .expect("first legitimate unknown observation");
        assert!(
            needs_delivery,
            "an unknown observation is accepted only after the first successful query"
        );
        let reader = ReadAdmission::default();
        let failed = reader.read_observation(now, || {
            Err(PlatformError::Preferences {
                message: "initial getter failure".into(),
            })
        });
        assert!(matches!(failed, Err(PlatformError::Preferences { .. })));
        assert!(matches!(
            reader.read_observation(now, || panic!("early native retry")),
            Err(PlatformError::PreferencesDeferred)
        ));
        let observation = flui_platform::SystemPreferences::default().with_gestures(
            flui_platform::GesturePreferences::default()
                .with_double_click_interval(Duration::from_millis(700)),
        );
        let (accepted, needs_delivery) = reader
            .read_observation(now + RETRY_INTERVAL, || Ok(observation.clone()))
            .expect("first accepted native observation");
        assert_eq!(accepted, observation);
        assert!(
            needs_delivery,
            "cold source must notify its first accepted observation"
        );
        let failed = reader.read_observation(now + RETRY_INTERVAL, || {
            Err(PlatformError::Preferences {
                message: "later getter failure".into(),
            })
        });
        assert!(
            matches!(failed, Err(PlatformError::Preferences { .. })),
            "failure cannot publish a stale success"
        );
        let (accepted, needs_delivery) = reader
            .read_observation(now + RETRY_INTERVAL * 2, || Ok(observation.clone()))
            .expect("recovered native observation");
        assert_eq!(accepted, observation);
        assert!(
            needs_delivery,
            "successful recovery must retry owner delivery even for an unchanged observation"
        );
        let (_, needs_delivery) = reader
            .read_observation(now + RETRY_INTERVAL * 2, || Ok(observation))
            .expect("healthy unchanged observation");
        assert!(
            !needs_delivery,
            "healthy unchanged observations remain quiet"
        );
    }

    #[test]
    fn bounded_native_read_admission() {
        let admission = ReadAdmission::default();
        let now = web_time::Instant::now();
        let failed = admission.begin(now).expect("first admission");
        assert!(matches!(
            admission.begin(now),
            Err(PlatformError::PreferencesDeferred)
        ));
        assert_eq!(
            admission.retry_deadline(),
            None,
            "a read in flight owes nothing yet"
        );
        drop(failed);
        assert_eq!(
            admission.retry_deadline(),
            Some(now + RETRY_INTERVAL),
            "a failed read owes a retry"
        );
        for elapsed in [0, 10, 100, 499] {
            assert!(matches!(
                admission.begin(now + Duration::from_millis(elapsed)),
                Err(PlatformError::PreferencesDeferred)
            ));
        }
        admission
            .begin(now + RETRY_INTERVAL)
            .expect("bounded retry")
            .accept();
        assert_eq!(
            admission.retry_deadline(),
            None,
            "an accepted read owes nothing"
        );
        admission
            .begin(now + RETRY_INTERVAL)
            .expect("healthy reads remain admitted")
            .accept();
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _attempt = admission
                .begin(now + RETRY_INTERVAL)
                .expect("unwind admission");
            panic!("native sampling panic");
        }));
        assert!(failure.is_err());
        assert_eq!(
            admission.retry_deadline(),
            Some(now + RETRY_INTERVAL * 2),
            "an unwound read owes a retry too"
        );
        assert!(matches!(
            admission.begin(now + RETRY_INTERVAL),
            Err(PlatformError::PreferencesDeferred)
        ));
        admission
            .begin(now + RETRY_INTERVAL * 2)
            .expect("recovery after unwind")
            .accept();
    }
}
