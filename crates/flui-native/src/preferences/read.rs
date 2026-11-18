//! How the AppKit and Android preference sources read the system and wake
//! their owner: told, never polled (ADR-0172).
//!
//! [`NoticedPreferences`] is the one model both sources are. What differs per
//! system sits behind [`NativePreferences`]: the read itself, the clock, the
//! gate that says whether a retry may wake the application, and the timer
//! that wakes it for one. A change reaches the source as a notice (an AppKit
//! notification through [`NoticedPreferences::notice_handler`], `FluiView`'s
//! report or a configuration change through the Android loop, which then
//! calls [`NoticedPreferences::notice`] and reads), and the owner's turn reads
//! it.
//!
//! The owner wakes once per change and never while idle:
//!
//! - a healthy source names no instant it needs the owner at, so nothing in it
//!   runs between notices;
//! - a read inside the owner's turn returns what changed to that turn's
//!   caller, which delivers it there; only a read outside a turn (the Android
//!   loop reading on a notice, the first read before any turn) wakes the owner
//!   for it;
//! - a read that fails or panics is retried on [`READ_RETRIES`], and only
//!   while the application is where a retry may wake it; once the retries are
//!   spent, or the gate holds one, nothing reads until the next change notice.

use std::sync::{Arc, Weak};
use std::time::Duration;

use flui_foundation::panic::FirstPanic;
use flui_platform::SystemPreferences;
use parking_lot::Mutex;

use crate::PlatformError;
use crate::owner::signal::OwnerSignal;
use crate::owner::sources::SystemPreferenceSource;
use crate::preferences::retries::Retries;

/// The pause before the first retry of a failed read.
const FIRST_RETRY: Duration = Duration::from_millis(500);

/// The retries of a read that failed or panicked: 0.5 to 16 s apart, then
/// none until the next change notice.
const READ_RETRIES: Retries = Retries::new(FIRST_RETRY, 6);

/// What a system gives [`NoticedPreferences`]. Every method runs on the
/// owner thread.
pub(crate) trait NativePreferences {
    /// Every preference the system reports now.
    ///
    /// # Errors
    /// [`PlatformError::Preferences`] when the system cannot be read.
    fn read(&self) -> Result<SystemPreferences, PlatformError>;

    /// The instant the source is at.
    fn now(&self) -> web_time::Instant;

    /// Whether a timer may wake the application to retry a failed read now:
    /// AppKit's application is active. Where this says no, the retry waits
    /// for the next change notice (on AppKit, the activation notification).
    fn retry_admitted(&self) -> bool;

    /// Wake the owner behind `signal` once, `delay` from now: the retry of a
    /// failed read. A loop that asks
    /// [`NoticedPreferences::retry_deadline`] before it blocks arms nothing
    /// here.
    fn arm_retry(&self, delay: Duration, signal: Weak<OwnerSignal>);
}

/// A preference source that is told of changes and reads when told: the
/// system's reads under one admission, and the owner they wake.
pub(crate) struct NoticedPreferences<N> {
    native: N,
    signal: Weak<OwnerSignal>,
    admission: ReadAdmission,
}

impl<N> std::fmt::Debug for NoticedPreferences<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NoticedPreferences").finish_non_exhaustive()
    }
}

impl<N: NativePreferences> NoticedPreferences<N> {
    /// A source reading through `native` that wakes the owner behind
    /// `signal` when a read outside its turn finds a change.
    pub(crate) fn new(native: N, signal: &Arc<OwnerSignal>) -> Self {
        Self {
            native,
            signal: Arc::downgrade(signal),
            admission: ReadAdmission::default(),
        }
    }

    /// The system the source reads.
    #[cfg(any(target_os = "android", test))]
    pub(crate) fn native(&self) -> &N {
        &self.native
    }

    /// When the retry a failed read owes is due; `None` for a healthy source
    /// and for one waiting for a notice.
    pub(crate) fn retry_deadline(&self) -> Option<web_time::Instant> {
        self.admission.retry_deadline()
    }

    /// A change notice arrived: a source whose retries ran out, or whose
    /// retry the gate held, may read again. A retry still pending keeps its
    /// pause, so a burst of notices never reads a failing system faster than
    /// [`READ_RETRIES`].
    pub(crate) fn notice(&self) {
        self.admission.notice();
    }

    /// Arm the retry a failed read left owed, or let it wait for a notice
    /// where the gate holds it.
    fn arm_retry(&self) {
        let Some(deadline) = self.admission.retry_deadline() else {
            return;
        };
        if self.native.retry_admitted() {
            let delay = deadline.saturating_duration_since(self.native.now());
            self.native.arm_retry(delay, self.signal.clone());
        } else {
            self.admission.await_notice();
        }
    }
}

#[cfg(any(target_os = "macos", test))]
impl<N: NativePreferences + Send + Sync + 'static> NoticedPreferences<N> {
    /// What a change notification runs, on any thread: [`notice`](Self::notice),
    /// then wake the owner, whose turn reads. It holds the source weakly, so
    /// a notification that outlives the source does nothing.
    pub(crate) fn notice_handler(self: &Arc<Self>) -> impl Fn() + Send + Sync + 'static {
        let source = Arc::downgrade(self);
        move || {
            if let Some(source) = source.upgrade() {
                source.notice();
                if let Some(signal) = source.signal.upgrade() {
                    // A failed post leaves the wake owed in the signal.
                    let _ = signal.wake();
                }
            }
        }
    }
}

impl<N: NativePreferences> SystemPreferenceSource for NoticedPreferences<N> {
    /// Read the system under the source's admission. Inside the owner's turn
    /// the caller delivers what changed; outside one a change or a recovered
    /// read wakes the owner. A failed or panicking read arms its retry before
    /// the failure goes on.
    fn sample(&self) -> Result<SystemPreferences, PlatformError> {
        let Some(signal) = self.signal.upgrade().filter(|signal| signal.accepting()) else {
            return Err(PlatformError::Preferences {
                message: "the preference owner has stopped".into(),
            });
        };
        let mut caught = FirstPanic::new();
        let read = caught.contain(|| {
            self.admission
                .read_observation(self.native.now(), || self.native.read())
        });
        match read {
            Some(Ok((current, needs_delivery))) => {
                if needs_delivery && !signal.turn_active() {
                    // OwnerSignal keeps the wake owed while no turn callback
                    // is registered yet.
                    let _ = signal.wake();
                }
                Ok(current)
            }
            Some(Err(PlatformError::PreferencesDeferred)) => {
                Err(PlatformError::PreferencesDeferred)
            }
            Some(Err(error)) => {
                self.arm_retry();
                Err(error)
            }
            None => {
                caught.contain(|| self.arm_retry());
                caught.resume();
                Err(PlatformError::Preferences {
                    message: "the system preference read panicked".into(),
                })
            }
        }
    }
}

/// The admission of the source's reads: one at a time, a failed one retried
/// on [`READ_RETRIES`], and the last accepted observation.
#[derive(Default)]
struct ReadAdmission {
    state: Mutex<Admission>,
    accepted: Mutex<Option<SystemPreferences>>,
}

#[derive(Clone, Copy, Default)]
struct Admission {
    stage: Stage,
    /// Reads that failed in a row since the last accepted one; nonzero owes
    /// the owner a delivery when a read recovers, changed or not.
    failures: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Stage {
    /// A read may run.
    #[default]
    Ready,
    /// A read runs.
    Reading,
    /// A read failed; the next may run at this instant.
    RetryAt(web_time::Instant),
    /// The retries are spent, or the gate held one: the next change notice
    /// lets a read run.
    AwaitingNotice,
}

impl ReadAdmission {
    /// Read and commit a native observation before any wake. A cold source
    /// has no accepted value; an error never manufactures one or returns an
    /// older accepted value as a successful refresh. The flag asks for owner
    /// delivery after a changed observation or a recovered read; a healthy
    /// unchanged read is quiet.
    fn read_observation(
        &self,
        now: web_time::Instant,
        sample: impl FnOnce() -> Result<SystemPreferences, PlatformError>,
    ) -> Result<(SystemPreferences, bool), PlatformError> {
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

    fn retry_deadline(&self) -> Option<web_time::Instant> {
        match self.state.lock().stage {
            Stage::RetryAt(deadline) => Some(deadline),
            Stage::Ready | Stage::Reading | Stage::AwaitingNotice => None,
        }
    }

    fn notice(&self) {
        let mut state = self.state.lock();
        if state.stage == Stage::AwaitingNotice {
            state.stage = Stage::Ready;
        }
    }

    fn await_notice(&self) {
        let mut state = self.state.lock();
        if matches!(state.stage, Stage::RetryAt(_)) {
            state.stage = Stage::AwaitingNotice;
        }
    }

    fn begin(&self, now: web_time::Instant) -> Result<ReadAttempt<'_>, PlatformError> {
        let mut state = self.state.lock();
        match state.stage {
            Stage::Reading | Stage::AwaitingNotice => {
                return Err(PlatformError::PreferencesDeferred);
            }
            Stage::RetryAt(deadline) if now < deadline => {
                return Err(PlatformError::PreferencesDeferred);
            }
            Stage::Ready | Stage::RetryAt(_) => {}
        }
        state.stage = Stage::Reading;
        Ok(ReadAttempt {
            admission: self,
            now,
            accepted: false,
            recovering: state.failures > 0,
        })
    }
}

/// One admitted read. Dropped unaccepted (an error, a panic) it records the
/// failure and the retry it owes.
struct ReadAttempt<'a> {
    admission: &'a ReadAdmission,
    now: web_time::Instant,
    accepted: bool,
    recovering: bool,
}

impl ReadAttempt<'_> {
    /// Commit the successful read before any wake or callback.
    fn accept(mut self) {
        self.accepted = true;
    }
}

impl Drop for ReadAttempt<'_> {
    fn drop(&mut self) {
        let mut state = self.admission.state.lock();
        if self.accepted {
            *state = Admission::default();
            return;
        }
        let spent = state.failures;
        state.failures = spent.saturating_add(1);
        state.stage = READ_RETRIES
            .pause(spent)
            .and_then(|pause| self.now.checked_add(pause))
            .map_or(Stage::AwaitingNotice, Stage::RetryAt);
    }
}

#[cfg(test)]
mod tests;
