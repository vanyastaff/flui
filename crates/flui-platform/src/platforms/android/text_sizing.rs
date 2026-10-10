//! Java-main acquisition; owner-lane queries use only a private metrics copy.

use crate::{
    CapturedTextSizing, TextSizingCaptureError as Error, TextSizingCaptureState as Capture,
    shared::owner_signal::OwnerSignal, text_sizing::NativeSizing,
};
#[cfg(target_os = "android")]
use android_activity::AndroidApp;
#[cfg(target_os = "android")]
use jni::{JValue, JavaVM, jni_sig, jni_str, objects::JObject, refs::Global};
use parking_lot::Mutex;
use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

#[cfg(target_os = "android")]
pub(crate) struct Snapshot {
    vm: JavaVM,
    metrics: Global<JObject<'static>>,
    density: f64,
}

#[cfg(target_os = "android")]
impl Snapshot {
    pub(crate) fn query(
        &self,
        request: flui_foundation::TextSizeRequest,
    ) -> Result<flui_foundation::TextSize, Error> {
        use flui_foundation::TextScaleProfile;
        match request.profile {
            TextScaleProfile::LargeTitle
            | TextScaleProfile::Title1
            | TextScaleProfile::Title2
            | TextScaleProfile::Title3
            | TextScaleProfile::Headline
            | TextScaleProfile::Body
            | TextScaleProfile::Callout
            | TextScaleProfile::Subheadline
            | TextScaleProfile::Footnote
            | TextScaleProfile::Caption1
            | TextScaleProfile::Caption2 => {}
            _ => return Err(Error::UnsupportedProfile),
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "TextSize admits its positive finite f32 representation"
        )]
        let size = request.size.value() as f32;
        let physical = self
            .vm
            .attach_current_thread(|env| -> jni::errors::Result<f32> {
                let result = env
                    .call_static_method(
                        jni_str!("android/util/TypedValue"),
                        jni_str!("applyDimension"),
                        jni_sig!("(IFLandroid/util/DisplayMetrics;)F"),
                        &[
                            JValue::Int(2),
                            JValue::Float(size),
                            JValue::Object(self.metrics.as_ref()),
                        ],
                    )
                    .and_then(|value| value.f());
                if result.is_err() {
                    env.exception_clear();
                }
                result
            })
            .map_err(native_error)?;
        flui_foundation::TextSize::new(f64::from(physical) / self.density).map_err(Into::into)
    }
}

#[cfg(target_os = "android")]
fn native_error(error: jni::errors::Error) -> Error {
    Error::Native {
        message: error.to_string(),
    }
}

#[cfg(target_os = "android")]
#[expect(
    unsafe_code,
    reason = "AndroidApp lends live VM/Activity references; no owned Activity reference is constructed"
)]
fn capture(app: &AndroidApp) -> Result<Snapshot, Error> {
    // SAFETY: AndroidApp retains the live Android VM for this capture callback.
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
    let (metrics, density) = vm
        .attach_current_thread(|env| -> jni::errors::Result<_> {
            let result = (|| {
                let raw = app.activity_as_ptr() as jni::sys::jobject;
                // SAFETY: borrow AndroidApp's global Activity; the temporary wrapper never deletes it.
                let activity = unsafe { env.as_cast_raw::<Global<JObject<'_>>>(&raw)? };
                let resources = env
                    .call_method(
                        activity.as_ref(),
                        jni_str!("getResources"),
                        jni_sig!("()Landroid/content/res/Resources;"),
                        &[],
                    )?
                    .l()?;
                let live = env
                    .call_method(
                        &resources,
                        jni_str!("getDisplayMetrics"),
                        jni_sig!("()Landroid/util/DisplayMetrics;"),
                        &[],
                    )?
                    .l()?;
                let copy = env.new_object(
                    jni_str!("android/util/DisplayMetrics"),
                    jni_sig!("()V"),
                    &[],
                )?;
                env.call_method(
                    &copy,
                    jni_str!("setTo"),
                    jni_sig!("(Landroid/util/DisplayMetrics;)V"),
                    &[JValue::Object(&live)],
                )?;
                let density = env
                    .get_field(&copy, jni_str!("density"), jni_sig!("F"))?
                    .f()?;
                Ok((env.new_global_ref(&copy)?, f64::from(density)))
            })();
            if result.is_err() {
                env.exception_clear();
            }
            result
        })
        .map_err(native_error)?;
    if !density.is_finite() || density <= 0.0 {
        return Err(Error::Native {
            message: "Android captured invalid text density".into(),
        });
    }
    Ok(Snapshot {
        vm,
        metrics,
        density,
    })
}

type NativeResult = Result<Arc<NativeSizing>, Error>;

struct Completion {
    ticket: Arc<()>,
    authority: Arc<AtomicBool>,
    result: NativeResult,
}

enum Flight {
    Awaiting { ticket: Arc<()>, deadline: Instant },
    Stalled { ticket: Arc<()> },
}

impl Flight {
    fn ticket(&self) -> &Arc<()> {
        match self {
            Self::Awaiting { ticket, .. } | Self::Stalled { ticket } => ticket,
        }
    }
}

struct State {
    authority: Arc<AtomicBool>,
    flight: Option<Flight>,
    completed: Option<Completion>,
    current: Option<Arc<NativeSizing>>,
    retry: Option<Instant>,
}

pub(super) struct Acquisition {
    state: Mutex<State>,
}

enum Admission {
    Waiting(Capture),
    Dispatch(Receipt),
}

/// Ownership of the posted callback also owns its completion obligation.
/// The dispatcher may drop it before invoking it (for example, failed attach).
struct Receipt {
    acquisition: Weak<Acquisition>,
    signal: Weak<OwnerSignal>,
    completion: Option<Completion>,
}

impl Receipt {
    fn complete(mut self, result: NativeResult) {
        let mut completion = self
            .completion
            .take()
            .expect("BUG: capture receipt settles once");
        completion.result = result;
        self.deliver(completion);
    }

    fn deliver(&self, completion: Completion) {
        let Some(acquisition) = self.acquisition.upgrade() else {
            return;
        };
        let mut outgoing = Some(completion);
        let committed = {
            let mut state = acquisition.state.lock();
            let ticket = &outgoing
                .as_ref()
                .expect("BUG: uncommitted completion exists")
                .ticket;
            if state
                .flight
                .as_ref()
                .is_some_and(|flight| Arc::ptr_eq(flight.ticket(), ticket))
                && state.completed.is_none()
            {
                state.completed = outgoing.take();
                true
            } else {
                false
            }
        };
        // Native ownership and outgoing wake never retire under the inbox guard.
        drop(outgoing);
        if committed && let Some(signal) = self.signal.upgrade() {
            let _ = signal.wake();
        }
    }
}

impl Drop for Receipt {
    fn drop(&mut self) {
        if let Some(completion) = self.completion.take() {
            crate::shared::panic_boundary::contain_owner_callback(|| self.deliver(completion));
        }
    }
}

impl Acquisition {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State {
                authority: Arc::new(AtomicBool::new(true)),
                flight: None,
                completed: None,
                current: None,
                retry: None,
            }),
        })
    }

    pub(super) fn invalidate(&self) {
        let retired = {
            let mut state = self.state.lock();
            state.authority.store(false, Ordering::Release);
            state.authority = Arc::new(AtomicBool::new(true));
            state.retry = None;
            state.current.take()
        };
        drop(retired);
    }

    #[cfg(target_os = "android")]
    pub(super) fn fence(&self) {
        self.state.lock().authority.store(false, Ordering::Release);
    }

    fn admit(
        self: &Arc<Self>,
        signal: &Weak<OwnerSignal>,
        owner: std::thread::ThreadId,
        now: Instant,
    ) -> Result<Admission, Error> {
        let completed = {
            let mut state = self.state.lock();
            let completed = state.completed.take();
            if completed.as_ref().is_some_and(|completion| {
                state
                    .flight
                    .as_ref()
                    .is_some_and(|flight| Arc::ptr_eq(flight.ticket(), &completion.ticket))
            }) {
                state.flight = None;
            }
            completed
        };
        if let Some(completion) = completed {
            let mut state = self.state.lock();
            if Arc::ptr_eq(&completion.authority, &state.authority)
                && state.authority.load(Ordering::Acquire)
            {
                match completion.result {
                    Ok(native) => state.current = Some(native),
                    Err(error) => {
                        state.retry = Some(now + Duration::from_millis(500));
                        return Err(error);
                    }
                }
            } else {
                drop(state);
                drop(completion);
            }
        }
        let completion = {
            let mut state = self.state.lock();
            if !state.authority.load(Ordering::Acquire) {
                return Err(Error::Unavailable);
            }
            if let Some(native) = &state.current {
                return Ok(Admission::Waiting(Capture::Ready(CapturedTextSizing::new(
                    Arc::clone(native),
                    Arc::clone(&state.authority),
                    owner,
                ))));
            }
            if let Some(flight) = &state.flight {
                match flight {
                    Flight::Awaiting { deadline, .. } => {
                        if let Some(check_after) = deadline
                            .checked_duration_since(now)
                            .filter(|delay| !delay.is_zero())
                        {
                            return Ok(Admission::Waiting(Capture::Pending { check_after }));
                        }
                        state.flight = Some(Flight::Stalled {
                            ticket: Arc::clone(flight.ticket()),
                        });
                    }
                    Flight::Stalled { .. } => {}
                }
                return Ok(Admission::Waiting(Capture::Stalled));
            }
            if let Some(remaining) = state
                .retry
                .and_then(|deadline| deadline.checked_duration_since(now))
                .filter(|remaining| !remaining.is_zero())
            {
                return Ok(Admission::Waiting(Capture::RetryAfter(remaining)));
            }
            state.retry = None;
            let ticket = Arc::new(());
            // This is an observation budget, not a cancellation acknowledgement
            // or a native execution SLA. Never extend it when polling.
            state.flight = Some(Flight::Awaiting {
                ticket: Arc::clone(&ticket),
                deadline: now + Duration::from_millis(500),
            });
            Completion {
                ticket,
                authority: Arc::clone(&state.authority),
                result: Err(Error::Native {
                    message: "Android discarded the text sizing capture callback".into(),
                }),
            }
        };
        Ok(Admission::Dispatch(Receipt {
            acquisition: Arc::downgrade(self),
            signal: signal.clone(),
            completion: Some(completion),
        }))
    }

    #[cfg(target_os = "android")]
    pub(super) fn poll(
        self: &Arc<Self>,
        app: &AndroidApp,
        signal: &Weak<OwnerSignal>,
        owner: std::thread::ThreadId,
    ) -> Result<Capture, Error> {
        let admitted_at = Instant::now();
        let receipt = match self.admit(signal, owner, admitted_at)? {
            Admission::Waiting(capture) => return Ok(capture),
            Admission::Dispatch(receipt) => receipt,
        };
        let activity = app.clone();
        let posted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            app.run_on_java_main_thread(Box::new(move || {
                let result = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    capture(&activity)
                })) {
                    Ok(result) => result.map(|snapshot| Arc::new(NativeSizing::Android(snapshot))),
                    Err(payload) => {
                        std::mem::forget(payload);
                        Err(Error::Native {
                            message: "Android text sizing capture panicked".into(),
                        })
                    }
                };
                crate::shared::panic_boundary::contain_owner_callback(|| receipt.complete(result));
            }));
        }));
        if let Err(payload) = posted {
            // Dropping an unposted callback settles its owned receipt. If posting
            // succeeded before panic, its queued callback still owns the ticket.
            std::mem::forget(payload);
        }
        // This is the remaining original flight deadline, not a reset from
        // posting completion. The owner will consume any immediate receipt
        // error/success on its next turn before testing that deadline.
        Ok(Capture::Pending {
            check_after: (admitted_at + Duration::from_millis(500))
                .saturating_duration_since(Instant::now()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn next(acquisition: &Arc<Acquisition>) -> Result<Admission, Error> {
        acquisition.admit(&Weak::new(), std::thread::current().id(), Instant::now())
    }

    fn receipt(acquisition: &Arc<Acquisition>) -> Receipt {
        match next(acquisition).expect("capture admission") {
            Admission::Dispatch(receipt) => receipt,
            Admission::Waiting(_) => panic!("expected new capture dispatch"),
        }
    }

    fn dropped_posted_callback_settles_and_recovers() {
        let acquisition = Acquisition::new();
        let pending = receipt(&acquisition);
        assert!(matches!(
            next(&acquisition),
            Ok(Admission::Waiting(Capture::Pending { .. }))
        ));
        // Exactly the ownership boundary android-activity drops on attach error.
        let callback: Box<dyn FnOnce() + Send> =
            Box::new(move || pending.complete(Err(Error::Unavailable)));
        drop(callback);
        assert!(
            matches!(next(&acquisition), Err(Error::Native { .. })),
            "discarded callback must settle accepted work"
        );
        assert!(
            matches!(next(&acquisition), Ok(Admission::Waiting(Capture::RetryAfter(delay))) if !delay.is_zero()),
            "retry without an accepted flight must expose its wake deadline"
        );
        acquisition.state.lock().retry = Some(Instant::now());
        drop(receipt(&acquisition));
    }

    fn stale_receipt_cannot_settle_a_new_flight() {
        let acquisition = Acquisition::new();
        let pending = receipt(&acquisition);
        let old = pending.completion.as_ref().expect("outstanding receipt");
        let duplicate = Receipt {
            acquisition: Arc::downgrade(&acquisition),
            signal: Weak::new(),
            completion: Some(Completion {
                ticket: Arc::clone(&old.ticket),
                authority: Arc::clone(&old.authority),
                result: Err(Error::Unavailable),
            }),
        };
        acquisition.invalidate();
        drop(pending);
        let replacement = receipt(&acquisition);
        drop(duplicate);
        assert!(
            matches!(
                next(&acquisition),
                Ok(Admission::Waiting(Capture::Pending { .. }))
            ),
            "late receipt must not consume replacement debt"
        );
        drop(replacement);
        assert!(matches!(next(&acquisition), Err(Error::Native { .. })));
    }

    fn retry_deadline_boundary() {
        let now = Instant::now();
        for (name, deadline, waiting) in [
            ("before_deadline", now + Duration::from_millis(1), true),
            ("at_deadline", now, false),
            (
                "after_deadline",
                now.checked_sub(Duration::from_millis(1))
                    .expect("test instant permits one millisecond subtraction"),
                false,
            ),
        ] {
            let acquisition = Acquisition::new();
            acquisition.state.lock().retry = Some(deadline);
            let admission = acquisition
                .admit(&Weak::new(), std::thread::current().id(), now)
                .expect("retry admission");
            match admission {
                Admission::Waiting(Capture::RetryAfter(delay)) => {
                    assert!(waiting && !delay.is_zero(), "{name}");
                }
                Admission::Dispatch(receipt) => {
                    assert!(!waiting, "{name}");
                    drop(receipt);
                }
                Admission::Waiting(other) => panic!("unexpected readiness at {name}: {other:?}"),
            }
        }
    }

    fn queued_capture_stalls_and_late_current_reply_recovers() {
        let acquisition = Acquisition::new();
        let now = Instant::now();
        let owner = std::thread::current().id();
        let Admission::Dispatch(pending) = acquisition
            .admit(&Weak::new(), owner, now)
            .expect("admission")
        else {
            panic!("first capture must dispatch");
        };
        assert!(
            matches!(acquisition.admit(&Weak::new(), owner, now + Duration::from_millis(250)), Ok(Admission::Waiting(Capture::Pending { check_after })) if check_after == Duration::from_millis(250))
        );
        for offset in [500, 1000, 5000] {
            assert!(
                matches!(
                    acquisition.admit(&Weak::new(), owner, now + Duration::from_millis(offset)),
                    Ok(Admission::Waiting(Capture::Stalled))
                ),
                "an outstanding queued callback must not be reset or reposted"
            );
        }
        pending.complete(Ok(Arc::new(NativeSizing::Test(Box::new(|request| {
            Ok(request.size)
        })))));
        assert!(
            matches!(
                acquisition.admit(&Weak::new(), owner, now + Duration::from_secs(6)),
                Ok(Admission::Waiting(Capture::Ready(_)))
            ),
            "late current reply must heal a stalled flight"
        );
    }

    fn completed_inbox_precedes_deadline_observation() {
        let acquisition = Acquisition::new();
        let now = Instant::now();
        let owner = std::thread::current().id();
        let Admission::Dispatch(pending) = acquisition
            .admit(&Weak::new(), owner, now)
            .expect("admission")
        else {
            panic!("first capture must dispatch");
        };
        pending.complete(Err(Error::Unavailable));
        assert!(
            matches!(
                acquisition.admit(&Weak::new(), owner, now + Duration::from_secs(1)),
                Err(Error::Unavailable)
            ),
            "settled native failure must remain authoritative over stalled observation"
        );
    }

    fn stalled_old_context_must_settle_before_replacement_dispatch() {
        let acquisition = Acquisition::new();
        let now = Instant::now();
        let owner = std::thread::current().id();
        let Admission::Dispatch(pending) = acquisition
            .admit(&Weak::new(), owner, now)
            .expect("admission")
        else {
            panic!("first capture must dispatch");
        };
        acquisition.invalidate();
        assert!(matches!(
            acquisition.admit(&Weak::new(), owner, now + Duration::from_secs(1)),
            Ok(Admission::Waiting(Capture::Stalled))
        ));
        pending.complete(Err(Error::Unavailable));
        let replacement = acquisition
            .admit(&Weak::new(), owner, now + Duration::from_secs(1))
            .expect("replacement admission");
        assert!(
            matches!(replacement, Admission::Dispatch(_)),
            "a stale settled receipt permits one latest-context dispatch"
        );
        assert!(matches!(
            acquisition.admit(&Weak::new(), owner, now + Duration::from_secs(1)),
            Ok(Admission::Waiting(Capture::Pending { .. }))
        ));
        drop(replacement);
    }

    #[test]
    fn android_capture_delivery_receipt() {
        crate::table_test::run_table(
            "android_capture_delivery_receipt",
            &[
                (
                    "dropped_posted_callback_settles_and_recovers",
                    dropped_posted_callback_settles_and_recovers,
                ),
                (
                    "stale_receipt_cannot_settle_a_new_flight",
                    stale_receipt_cannot_settle_a_new_flight,
                ),
                ("retry_deadline_boundary", retry_deadline_boundary),
                (
                    "queued_capture_stalls_and_late_current_reply_recovers",
                    queued_capture_stalls_and_late_current_reply_recovers,
                ),
                (
                    "completed_inbox_precedes_deadline_observation",
                    completed_inbox_precedes_deadline_observation,
                ),
                (
                    "stalled_old_context_must_settle_before_replacement_dispatch",
                    stalled_old_context_must_settle_before_replacement_dispatch,
                ),
            ],
        );
    }
}
