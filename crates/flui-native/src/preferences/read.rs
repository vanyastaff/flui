//! How the AppKit and Android preference sources read the system and wake
//! their owner: told, never polled (ADR-0377).
//!
//! [`NoticedPreferences`] is the one model both sources are. What differs per
//! system sits behind [`SystemReads`]: the read itself, the clock, and how a
//! failed read is retried ([`Retrying`]). A change reaches the source as a
//! notice (an AppKit notification through
//! [`NoticedPreferences::notice_handler`]; `FluiView`'s report, a configuration
//! change or `Start` through the Android loop, which calls
//! [`NoticedPreferences::notice`] and reads), and the owner's application turn
//! reads it and delivers it.
//!
//! The owner wakes once per change and never while idle:
//!
//! - a healthy source names no instant it needs the owner at, so nothing in it
//!   runs between notices;
//! - the source counts a value as accepted only once its caller confirms the
//!   delivery ([`NoticedPreferences::delivered`]). A change read inside the
//!   application's turn is owed to that turn: delivered there, it costs no
//!   further wake; dropped there, the turn's end wakes the owner once more
//!   for it. A change read anywhere else (the Android loop on a notice, the
//!   first read, an owner-work turn) wakes the owner;
//! - a read that fails or panics is retried on [`READ_RETRIES`]; once the
//!   retries are spent, or the application is where a retry may not wake it,
//!   nothing reads until the next change notice.
//!
//! A read is refused, changing nothing, off the owner thread.

use std::sync::{Arc, Weak};
use std::time::Duration;

use flui_foundation::panic::{FirstPanic, discard_payload, payload_text};
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
pub(crate) trait SystemReads {
    /// Every preference the system reports now.
    ///
    /// # Errors
    /// [`PlatformError::Preferences`] when the system cannot be read.
    fn read(&self) -> Result<SystemPreferences, PlatformError>;

    /// The instant the source is at.
    fn now(&self) -> web_time::Instant;

    /// How the system's failed reads are retried.
    fn retrying(&self) -> Retrying<'_>;
}

/// How a failed read is retried.
pub(crate) enum Retrying<'a> {
    /// The owner's loop blocks no longer than
    /// [`NoticedPreferences::retry_deadline`] and reads then; it gates its
    /// own reads on the application running (the Android loop).
    #[cfg_attr(
        not(any(target_os = "android", test)),
        expect(dead_code, reason = "only the Android loop retries at its deadline")
    )]
    AtLoopDeadline,
    /// A timer wakes the owner, armed only while the application is active
    /// (AppKit).
    #[cfg_attr(
        not(any(target_os = "macos", test)),
        expect(dead_code, reason = "only AppKit retries on a timer")
    )]
    OnTimer(&'a dyn RetryTimer),
}

/// The timer a [`Retrying::OnTimer`] system retries on.
pub(crate) trait RetryTimer {
    /// Whether a timer may wake the application now. Where it may not, the
    /// retry waits for the next change notice (on AppKit, the activation
    /// notification).
    fn application_active(&self) -> bool;

    /// Wake the owner behind `signal` once, `delay` from now.
    fn arm(&self, delay: Duration, signal: Weak<OwnerSignal>);
}

/// A preference source that is told of changes and reads when told: the
/// system's reads under one admission, and the owner they wake.
pub(crate) struct NoticedPreferences<S> {
    system: S,
    signal: Weak<OwnerSignal>,
    admission: ReadAdmission,
}

impl<S> std::fmt::Debug for NoticedPreferences<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NoticedPreferences").finish_non_exhaustive()
    }
}

impl<S: SystemReads> NoticedPreferences<S> {
    /// A source reading `system` that wakes the owner behind `signal`.
    pub(crate) fn new(system: S, signal: &Arc<OwnerSignal>) -> Self {
        Self {
            system,
            signal: Arc::downgrade(signal),
            admission: ReadAdmission::default(),
        }
    }

    /// The system the source reads.
    #[cfg(any(target_os = "android", test))]
    pub(crate) fn system(&self) -> &S {
        &self.system
    }

    /// When the retry a failed read owes is due; `None` for a healthy source
    /// and for one waiting for a notice. The Android loop blocks no longer
    /// than this.
    #[cfg(any(target_os = "android", test))]
    pub(crate) fn retry_deadline(&self) -> Option<web_time::Instant> {
        self.admission.retry_deadline()
    }

    /// A change notice arrived: a source whose retries ran out, or whose
    /// retry the application's state held, may read again. The turn the
    /// notice brings reads whatever is not delivered yet, a change an earlier
    /// turn dropped included. A retry still pending keeps its
    /// pause, so a burst of notices never reads a failing system faster than
    /// [`READ_RETRIES`].
    pub(crate) fn notice(&self) {
        self.admission.notice();
    }

    /// Arm the retry a failed read left owed, or let it wait for a notice
    /// where the application's state holds it.
    fn arm_retry(&self) {
        let Some(deadline) = self.admission.retry_deadline() else {
            return;
        };
        match self.system.retrying() {
            Retrying::AtLoopDeadline => {}
            Retrying::OnTimer(timer) if timer.application_active() => {
                let delay = deadline.saturating_duration_since(self.system.now());
                timer.arm(delay, self.signal.clone());
            }
            Retrying::OnTimer(_) => self.admission.await_notice(),
        }
    }
}

#[cfg(any(target_os = "macos", test))]
impl<S: SystemReads + Send + Sync + 'static> NoticedPreferences<S> {
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

impl<S: SystemReads> SystemPreferenceSource for NoticedPreferences<S> {
    /// Read the system under the source's admission and return what it
    /// reports. A change is owed to the application's turn the read runs in,
    /// or wakes the owner outside one. A failed or panicking read arms its
    /// retry and is an error; a read off the owner thread is refused before
    /// the admission and changes nothing.
    fn sample(&self) -> Result<SystemPreferences, PlatformError> {
        let Some(signal) = self.signal.upgrade().filter(|signal| signal.accepting()) else {
            return Err(PlatformError::Preferences {
                message: "the preference owner has stopped".into(),
            });
        };
        if signal.owner() != std::thread::current().id() {
            return Err(PlatformError::Preferences {
                message: "system preferences are read on the owner thread".into(),
            });
        }
        let mut caught = FirstPanic::new();
        let read = caught.contain(|| {
            self.admission
                .read_observation(self.system.now(), || self.system.read())
        });
        match read {
            Some(Ok((current, Change::Unchanged))) => Ok(current),
            Some(Ok((current, Change::New | Change::OwedBefore)))
                if !signal.in_application_turn() =>
            {
                // OwnerSignal keeps the wake owed while no turn callback is
                // registered yet.
                let _ = signal.wake();
                Ok(current)
            }
            Some(Ok((current, Change::New))) => {
                // Delivered in this turn it costs nothing more; dropped, the
                // turn's end wakes the owner once for it.
                self.admission.owe(&current);
                signal.owe_delivery();
                Ok(current)
            }
            Some(Ok((current, Change::OwedBefore))) => {
                // A turn dropped this value and its one wake is spent: this
                // turn tries it again, and nothing more wakes for it until a
                // notice.
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
                // The read's panic is counted as a failed read and retried
                // like one; nobody resumes it.
                let message = caught
                    .held()
                    .and_then(payload_text)
                    .unwrap_or("non-string panic payload")
                    .to_owned();
                if let Some(payload) = caught.into_payload() {
                    discard_payload(payload);
                }
                self.arm_retry();
                Err(PlatformError::Preferences {
                    message: format!("the system preference read panicked: {message}"),
                })
            }
        }
    }

    /// The caller delivered `values`: they are the source's accepted value,
    /// and the delivery the turn owed is settled.
    fn delivered(&self, values: &SystemPreferences) {
        self.admission.deliver(values);
        if let Some(signal) = self.signal.upgrade() {
            signal.confirm_delivery();
        }
    }
}

/// What a read found against the value last delivered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Change {
    /// The value delivered last.
    Unchanged,
    /// A value not delivered yet.
    New,
    /// A value an application turn was owed and has not confirmed: its one
    /// wake is spent.
    OwedBefore,
}

/// The admission of the source's reads: one at a time, a failed one retried
/// on [`READ_RETRIES`], and the value last delivered.
#[derive(Default)]
struct ReadAdmission {
    state: Mutex<Admission>,
}

#[derive(Default)]
struct Admission {
    stage: Stage,
    /// Reads that failed in a row since the last one that answered.
    failures: u32,
    /// The value the owner's caller last confirmed it delivered; `None` on
    /// a cold source.
    delivered: Option<SystemPreferences>,
    /// A changed value an application turn was owed, which no delivery
    /// confirmed since; reading it again in a turn owes nothing more until a
    /// notice.
    owed: Option<SystemPreferences>,
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
    /// The retries are spent, or a retry was held: the next change notice
    /// lets a read run.
    AwaitingNotice,
}

impl ReadAdmission {
    /// Read the system and compare it with what was delivered. A cold source
    /// has nothing delivered; an error never manufactures a value or returns
    /// an older one as a successful refresh. Nothing is accepted here: only
    /// a confirmed delivery is ([`Self::deliver`]).
    fn read_observation(
        &self,
        now: web_time::Instant,
        sample: impl FnOnce() -> Result<SystemPreferences, PlatformError>,
    ) -> Result<(SystemPreferences, Change), PlatformError> {
        let attempt = self.begin(now)?;
        let current = sample()?;
        let change = attempt.answered(&current);
        Ok((current, change))
    }

    /// An application turn is owed `values`.
    fn owe(&self, values: &SystemPreferences) {
        self.state.lock().owed = Some(values.clone());
    }

    fn deliver(&self, values: &SystemPreferences) {
        let mut state = self.state.lock();
        state.delivered = Some(values.clone());
        state.owed = None;
    }

    fn retry_deadline(&self) -> Option<web_time::Instant> {
        let stage = self.state.lock().stage;
        match stage {
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
            answered: false,
        })
    }
}

/// One admitted read. Dropped unanswered (an error, a panic) it records the
/// failure and the retry it owes.
struct ReadAttempt<'a> {
    admission: &'a ReadAdmission,
    now: web_time::Instant,
    answered: bool,
}

impl ReadAttempt<'_> {
    /// The system answered `current`: the admission is ready again, and the
    /// answer is compared with what was delivered.
    fn answered(mut self, current: &SystemPreferences) -> Change {
        self.answered = true;
        let mut state = self.admission.state.lock();
        state.stage = Stage::Ready;
        state.failures = 0;
        if state.delivered.as_ref() == Some(current) {
            Change::Unchanged
        } else if state.owed.as_ref() == Some(current) {
            Change::OwedBefore
        } else {
            Change::New
        }
    }
}

impl Drop for ReadAttempt<'_> {
    fn drop(&mut self) {
        if self.answered {
            return;
        }
        let mut state = self.admission.state.lock();
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
