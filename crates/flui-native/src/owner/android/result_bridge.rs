//! The Activity a capability provider reaches (ADR-0154 §9, ADR-0340 §2):
//! requests for results through `FluiHost`, routed by [`ResultRouter`], and
//! the Activity lent to a provider's own JNI code.
//!
//! Owner thread: the provider's closures run here, with the router's borrow
//! released, and what the router settles is resolved after it. A start is
//! posted to the Java main thread through [`main_thread::post`], which the
//! owner never waits for (ADR-0348 §2). The posted work reports back
//! through [`HandOffs`]: taken, which lifts the request's deadline, or failed
//! (a JNI error, a panic), which ends it as `HandOffFailed`; a launch the
//! Java main thread never takes ends the same way at its deadline, since
//! android-activity keeps work it cannot run rather than dropping it.
//! Answers come back through `FluiHost`'s queue, which the loop drains each
//! turn and hands to [`ResultBridge::answer`].

use std::cell::RefCell;
use std::num::NonZeroU64;
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::{Arc, Weak};

use android_activity::AndroidApp;
use flui_platform::capability::{
    ActivityBridge, CapabilityId, CompletionSource, DataRefused, IntentFrame, JniFrame, RequestKey,
    ResultIntent, ResultMint, ResultRefused, ResultRequest, ResultToken,
};
use jni::objects::{JObject, JString};
use jni::refs::Global;
use jni::{Env, JValue, JavaVM, jni_sig, jni_str};
use parking_lot::Mutex;
use web_time::Instant;

use super::activity::with_activity;
use super::activity_results::{ANSWER_NUMBERS, Answer, Carried, Issuer, ResultRouter, Routed};
use super::java_host::host_class;
use super::main_thread::{self, PostFailure};
use crate::PlatformError;
use crate::owner::signal::OwnerSignal;

/// An answered `Intent`, held by a global reference on the owner thread
/// until its provider takes it or lets it go.
pub(crate) type HeldIntent = Global<JObject<'static>>;

/// The epoch `FluiHost.nextEpoch` gives this platform's requests. On the
/// thread that starts the platform.
///
/// # Errors
///
/// [`PlatformError::Init`] when `FluiHost` cannot be read or answers an
/// epoch that is not positive: the platform does not start.
pub(crate) fn next_epoch(app: &AndroidApp) -> Result<NonZeroU64, PlatformError> {
    let epoch = with_activity(app, |env, activity| {
        env.with_local_frame(4, |env| {
            let class = host_class(env, activity)?;
            env.call_static_method(&class, jni_str!("nextEpoch"), jni_sig!("()J"), &[])?
                .j()
        })
    })
    .map_err(|error| PlatformError::Init {
        message: format!("reading FluiHost's epoch for requests for results: {error}"),
    })?;
    u64::try_from(epoch)
        .ok()
        .and_then(NonZeroU64::new)
        .ok_or_else(|| PlatformError::Init {
            message: format!("FluiHost gave the epoch {epoch}, which is not positive"),
        })
}

/// What the work posted to the Java main thread reports, for the owner to
/// apply; it is woken for each.
#[derive(Default)]
struct Reports {
    taken: Vec<ResultToken>,
    failed: Vec<ResultToken>,
}

/// The Java main thread's side of [`Reports`].
struct HandOffs {
    reports: Mutex<Reports>,
    signal: Weak<OwnerSignal>,
}

impl HandOffs {
    /// Java main thread: `token`'s launch was taken (`taken`) or failed.
    fn report(&self, token: ResultToken, taken: bool) {
        {
            let mut reports = self.reports.lock();
            if taken {
                reports.taken.push(token);
            } else {
                reports.failed.push(token);
            }
        }
        if let Some(signal) = self.signal.upgrade() {
            let _ = signal.wake();
        }
    }
}

/// The running Activity's side of activity results.
pub(crate) struct ResultBridge {
    app: AndroidApp,
    router: RefCell<ResultRouter<HeldIntent>>,
    hand_offs: Arc<HandOffs>,
}

impl std::fmt::Debug for ResultBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResultBridge").finish_non_exhaustive()
    }
}

impl ResultBridge {
    /// The bridge of `app`'s Activity, whose requests carry `epoch`, waking
    /// `signal` when the Java main thread reports on a launch.
    pub(crate) fn new(app: &AndroidApp, epoch: NonZeroU64, signal: Weak<OwnerSignal>) -> Rc<Self> {
        Rc::new(Self {
            app: app.clone(),
            router: RefCell::new(ResultRouter::new(epoch)),
            hand_offs: Arc::new(HandOffs {
                reports: Mutex::new(Reports::default()),
                signal,
            }),
        })
    }

    /// What `flui-app` installs in the registry's host bridge.
    pub(crate) fn bridge(self: &Rc<Self>) -> Rc<dyn ActivityBridge> {
        Rc::clone(self) as Rc<dyn ActivityBridge>
    }

    /// When the oldest launch the Java main thread has not taken is due, for
    /// the loop's poll.
    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        self.router.borrow().next_deadline()
    }

    /// Apply what the Java main thread reported, end the launches past their
    /// deadline, then route what `FluiHost` answered since the last turn.
    /// Owner thread, every turn.
    pub(crate) fn answer(&self, answers: Vec<Answer<HeldIntent>>) {
        let reports = std::mem::take(&mut *self.hand_offs.reports.lock());
        for token in reports.taken {
            self.router.borrow_mut().handed(token);
        }
        for token in reports.failed {
            let delivery = self.router.borrow_mut().hand_off_failed(token);
            if let Some(delivery) = delivery {
                delivery.deliver();
            }
        }
        let (overdue, delivery) = self.router.borrow_mut().overdue(Instant::now());
        delivery.deliver();
        for token in overdue {
            tracing::warn!(
                target: "flui_native::android",
                "the Java main thread did not take a request in time; it ends as HandOffFailed"
            );
            // Should the work still run, its activity is finished there.
            self.post_cancel(token);
        }
        for answer in answers {
            let (routed, delivery) = self.router.borrow_mut().answer(answer);
            delivery.deliver();
            match routed {
                Routed::Settled | Routed::Reattached | Routed::Waiting => {}
                Routed::Stale => tracing::debug!(
                    target: "flui_native::android",
                    "an answer came for a request already answered or cancelled"
                ),
                Routed::LetGo => tracing::warn!(
                    target: "flui_native::android",
                    "an earlier Activity's answer has nobody to reach; it is let go"
                ),
            }
        }
        self.reconcile();
        self.release();
    }

    /// End the requests whose tickets their providers let go, as cancelled,
    /// and cancel the activities they started on the Java side, which frees
    /// their places there at once. A dropped ticket wakes the owner, so this
    /// runs at the turn after the drop.
    fn release(&self) {
        let (finish, delivery) = {
            let mut router = self.router.borrow_mut();
            router.sweep();
            router.take_released()
        };
        delivery.deliver();
        for token in finish {
            self.post_cancel(token);
        }
    }

    /// Hold every key's waiting reattachments to the answers `FluiHost` still
    /// counts to come under it, so none waits for an answer that will not
    /// come.
    fn reconcile(&self) {
        let keys = self.router.borrow().reattached_keys();
        for key in keys {
            let still_to_come = self.still_to_come(key);
            let delivery = self.router.borrow_mut().reconcile(key, still_to_come);
            delivery.deliver();
        }
    }

    /// The Activity ended: every open request ends, and later ones are
    /// refused.
    pub(crate) fn end(&self) {
        let (delivery, let_go) = self.router.borrow_mut().end();
        delivery.deliver();
        if let_go > 0 {
            tracing::warn!(
                target: "flui_native::android",
                let_go,
                "answers of an earlier Activity were never reattached; they are let go"
            );
        }
    }

    /// The epoch every request of this Activity carries, as Java's `long`.
    fn epoch(&self) -> i64 {
        self.router.borrow().epoch().get().cast_signed()
    }

    /// Finish the activity `token` started, on the Java main thread.
    fn post_cancel(&self, token: ResultToken) {
        let (app, epoch) = (self.app.clone(), self.epoch());
        main_thread::post(
            &self.app,
            "FluiHost.cancel",
            move || cancel_on_main_thread(&app, epoch, token),
            |failure: PostFailure| {
                // The request already ended; the activity's answer, if it
                // comes, is dropped as stale.
                tracing::warn!(
                    target: "flui_native::android",
                    ?failure,
                    "an ended request's activity was not finished"
                );
            },
        );
    }

    /// How many answers `FluiHost` counts still to come under `key` for
    /// earlier Activities (`FluiHost.stillToCome`: waiting to start, started,
    /// answered and not drained). A read that fails counts without bound:
    /// reattachments then wait, and end with the Activity at the latest.
    fn still_to_come(&self, key: RequestKey) -> usize {
        let epoch = self.epoch();
        with_activity(&self.app, |env, activity| {
            env.with_local_frame(4, |env| {
                let class = host_class(env, activity)?;
                let capability = env.new_string(key.capability().as_str())?;
                let name = env.new_string(key.key().as_str())?;
                env.call_static_method(
                    &class,
                    jni_str!("stillToCome"),
                    jni_sig!("(JLjava/lang/String;Ljava/lang/String;)I"),
                    &[
                        JValue::Long(epoch),
                        JValue::Object(&capability),
                        JValue::Object(&name),
                    ],
                )?
                .i()
            })
        })
        .map_or(usize::MAX, |count| usize::try_from(count).unwrap_or(0))
    }
}

/// The frame of a call on the owner thread, `None` when JNI handed a null.
fn frame(env: &Env<'_>, object: &JObject<'_>) -> Option<JniFrame> {
    let vm = JavaVM::singleton().ok()?;
    Some(JniFrame::new(
        NonNull::new(vm.get_raw().cast())?,
        NonNull::new(env.get_raw().cast())?,
        NonNull::new(object.as_raw().cast())?,
    ))
}

impl ActivityBridge for ResultBridge {
    fn bind(&self, mint: ResultMint) {
        // A dropped ticket or `ResultIntent` wakes the owner, whose turn ends
        // the request; the drop itself records and wakes, nothing more.
        let signal = Weak::clone(&self.hand_offs.signal);
        let wake: Rc<dyn Fn()> = Rc::new(move || {
            if let Some(signal) = signal.upgrade() {
                let _ = signal.wake();
            }
        });
        self.router.borrow_mut().bind(Issuer { mint, wake });
    }

    fn launch_for_result(
        &self,
        completions: &CompletionSource,
        capability: &CapabilityId,
        key: RequestKey,
        intent: &mut dyn FnMut(IntentFrame) -> bool,
    ) -> Result<ResultRequest, ResultRefused> {
        self.router.borrow_mut().admits(capability)?;
        // The provider fills the Intent with no borrow held: its closure may
        // reach this bridge again.
        let made = with_activity(&self.app, |env, activity| {
            env.with_local_frame(
                16,
                |env| -> jni::errors::Result<Option<Global<JObject<'static>>>> {
                    let empty =
                        env.new_object(jni_str!("android/content/Intent"), jni_sig!("()V"), &[])?;
                    let (Some(activity_frame), Some(raw)) =
                        (frame(env, activity), NonNull::new(empty.as_raw().cast()))
                    else {
                        return Ok(None);
                    };
                    let filled = intent(IntentFrame::new(activity_frame, raw));
                    if env.exception_check() {
                        env.exception_clear();
                        return Ok(None);
                    }
                    if !filled {
                        return Ok(None);
                    }
                    env.new_global_ref(&empty).map(Some)
                },
            )
        });
        let filled = match made {
            Ok(Some(filled)) => filled,
            Ok(None) => return Err(ResultRefused::IntentNotMade),
            Err(error) => {
                tracing::warn!(
                    target: "flui_native::android",
                    %error,
                    "the Intent of a request could not be made"
                );
                return Err(ResultRefused::IntentNotMade);
            }
        };
        // Admitted again: the provider's closure may have taken requests.
        let request =
            self.router
                .borrow_mut()
                .open_launch(completions, capability, key, Instant::now())?;
        let token = request.token();
        let (app, epoch) = (self.app.clone(), self.epoch());
        let (taken, failed) = (Arc::clone(&self.hand_offs), Arc::clone(&self.hand_offs));
        main_thread::post(
            &self.app,
            "FluiHost.launch",
            move || {
                launch_on_main_thread(&app, &filled, epoch, token, key)?;
                taken.report(token, true);
                Ok(())
            },
            move |failure: PostFailure| {
                tracing::warn!(
                    target: "flui_native::android",
                    ?failure,
                    "a request could not be handed to Android; it ends as HandOffFailed"
                );
                failed.report(token, false);
            },
        );
        Ok(request)
    }

    fn reattach(
        &self,
        completions: &CompletionSource,
        capability: &CapabilityId,
        key: RequestKey,
    ) -> Result<ResultRequest, ResultRefused> {
        if self.router.borrow().has_ended() {
            return Err(ResultRefused::ActivityEnded);
        }
        let still_to_come = self.still_to_come(key);
        let (request, delivery) =
            self.router
                .borrow_mut()
                .open_reattach(completions, capability, key, still_to_come)?;
        delivery.deliver();
        Ok(request)
    }

    fn take_data(
        &self,
        capability: &CapabilityId,
        data: ResultIntent,
        f: &mut dyn FnMut(JniFrame),
    ) -> Result<(), DataRefused> {
        // Taken out of the router first: the provider's closure runs with no
        // borrow held, and the reference is deleted when it returns.
        let intent = self.router.borrow_mut().take_data(capability, data)?;
        with_activity(&self.app, |env, _| {
            let Some(lent) = frame(env, intent.as_obj()) else {
                return Ok(false);
            };
            f(lent);
            if env.exception_check() {
                env.exception_clear();
            }
            Ok(true)
        })
        .ok()
        .filter(|lent| *lent)
        .map(drop)
        .ok_or(DataRefused::NotLent)
    }

    fn lend(&self, f: &mut dyn FnMut(JniFrame)) -> bool {
        if self.router.borrow().has_ended() {
            return false;
        }
        with_activity(&self.app, |env, activity| {
            let Some(lent) = frame(env, activity) else {
                return Ok(false);
            };
            f(lent);
            if env.exception_check() {
                env.exception_clear();
            }
            Ok(true)
        })
        .unwrap_or(false)
    }
}

/// `FluiHost.launch` for one request. Java main thread.
///
/// # Errors
///
/// The JNI call failed; the caller ends the request as `HandOffFailed`.
fn launch_on_main_thread(
    app: &AndroidApp,
    intent: &Global<JObject<'static>>,
    epoch: i64,
    token: ResultToken,
    key: RequestKey,
) -> Result<(), String> {
    with_activity(app, |env, activity| {
        env.with_local_frame(8, |env| {
            let class = host_class(env, activity)?;
            let capability = env.new_string(key.capability().as_str())?;
            let name = env.new_string(key.key().as_str())?;
            env.call_static_method(
                &class,
                jni_str!("launch"),
                jni_sig!(
                    "(Landroid/app/Activity;Landroid/content/Intent;JJLjava/lang/String;Ljava/lang/String;)V"
                ),
                &[
                    JValue::Object(activity),
                    JValue::Object(intent.as_obj()),
                    JValue::Long(epoch),
                    JValue::Long(token.get().cast_signed()),
                    JValue::Object(&capability),
                    JValue::Object(&name),
                ],
            )
            .map(drop)
        })
    })
    .map_err(|error| error.to_string())
}

/// `FluiHost.cancel` for one request. Java main thread.
///
/// # Errors
///
/// The JNI call failed.
fn cancel_on_main_thread(app: &AndroidApp, epoch: i64, token: ResultToken) -> Result<(), String> {
    with_activity(app, |env, activity| {
        env.with_local_frame(4, |env| {
            let class = host_class(env, activity)?;
            env.call_static_method(
                &class,
                jni_str!("cancel"),
                jni_sig!("(Landroid/app/Activity;JJ)V"),
                &[
                    JValue::Object(activity),
                    JValue::Long(epoch),
                    JValue::Long(token.get().cast_signed()),
                ],
            )
            .map(drop)
        })
    })
    .map_err(|error| error.to_string())
}

/// The most answers one drain reads; more wait for the next turn, which the
/// drain wakes for.
const MAX_DRAINED: usize = 2 * flui_platform::capability::MAX_OPEN_REQUESTS;

/// Clear the Java exception a failed read left pending, so the next JNI call
/// runs with none.
fn clear_pending(env: &mut Env<'_>) {
    if env.exception_check() {
        env.exception_clear();
    }
}

/// One string part of an answer's key: `None` when it cannot be read or is
/// longer than a key, which is checked before it is copied.
fn key_part<'local>(
    env: &mut Env<'local>,
    part: JObject<'local>,
) -> jni::errors::Result<Option<String>> {
    use flui_platform::capability::MAX_RESULT_KEY_LEN;

    let part = env.cast_local::<JString<'_>>(part)?;
    if part.is_null() {
        return Ok(None);
    }
    let length = env
        .call_method(&part, jni_str!("length"), jni_sig!("()I"), &[])?
        .i()?;
    if usize::try_from(length).is_ok_and(|length| length <= MAX_RESULT_KEY_LEN) {
        Ok(Some(part.try_to_string(env)?))
    } else {
        Ok(None)
    }
}

/// The oldest answers `FluiHost` queued, at most [`MAX_DRAINED`], each read
/// on its own, the exception a failed read leaves cleared before the next:
/// a key part longer than a key is not copied, a part that cannot be read is
/// `None`, and an `Intent` that cannot be kept is [`Carried::Lost`], so one
/// record never holds the others back. Each `Intent` is held as it is, never
/// read. `FluiHost` then
/// lets go of exactly the answers read; where its arrays disagree in length,
/// the answers all of them hold are read and let go, and the rest wait.
/// Answers how many are still queued.
pub(crate) fn drain_answers(
    env: &mut Env<'_>,
    activity: &JObject<'_>,
) -> jni::errors::Result<(Vec<Answer<HeldIntent>>, usize)> {
    use jni::objects::{JLongArray, JObjectArray};

    let class = host_class(env, activity)?;
    let max = i32::try_from(MAX_DRAINED).expect("BUG: a few dozen answers fit an int");
    let peeked = env
        .call_static_method(
            &class,
            jni_str!("peekAnswers"),
            jni_sig!("(I)[Ljava/lang/Object;"),
            &[JValue::Int(max)],
        )?
        .l()?;
    let peeked = env.cast_local::<JObjectArray<'_>>(peeked)?;
    let numbers = peeked.get_element(env, 0)?;
    let numbers = JLongArray::cast_local(env, numbers)?;
    let capabilities = peeked.get_element(env, 1)?;
    let capabilities = env.cast_local::<JObjectArray<'_>>(capabilities)?;
    let keys = peeked.get_element(env, 2)?;
    let keys = env.cast_local::<JObjectArray<'_>>(keys)?;
    let data = peeked.get_element(env, 3)?;
    let data = env.cast_local::<JObjectArray<'_>>(data)?;
    let count = [
        numbers.len(env)? / ANSWER_NUMBERS,
        capabilities.len(env)?,
        keys.len(env)?,
        data.len(env)?,
        MAX_DRAINED,
    ]
    .into_iter()
    .min()
    .unwrap_or(0);
    let mut values = vec![0; count * ANSWER_NUMBERS];
    numbers.get_region(env, 0, &mut values)?;
    let mut read_capabilities = Vec::with_capacity(count);
    let mut read_keys = Vec::with_capacity(count);
    let mut read_data = Vec::with_capacity(count);
    for index in 0..count {
        // Each read that can throw is cleared before the next JNI call: a
        // call with an exception pending is undefined.
        let capability = env.with_local_frame(4, |env| -> jni::errors::Result<_> {
            let part = capabilities.get_element(env, index)?;
            key_part(env, part)
        });
        clear_pending(env);
        let key = env.with_local_frame(4, |env| -> jni::errors::Result<_> {
            let part = keys.get_element(env, index)?;
            key_part(env, part)
        });
        clear_pending(env);
        let held = env.with_local_frame(4, |env| -> jni::errors::Result<_> {
            let item = data.get_element(env, index)?;
            if item.is_null() {
                Ok(None)
            } else {
                env.new_global_ref(&item).map(Some)
            }
        });
        clear_pending(env);
        let carried = match held {
            Ok(Some(held)) => Carried::Held(held),
            Ok(None) => Carried::Nothing,
            Err(error) => {
                tracing::warn!(
                    target: "flui_native::android",
                    %error,
                    "an answer's Intent could not be kept; its request ends as data lost"
                );
                Carried::Lost
            }
        };
        read_capabilities.push(capability.ok().flatten());
        read_keys.push(key.ok().flatten());
        read_data.push(carried);
    }
    let committed = i32::try_from(count).expect("BUG: at most MAX_DRAINED answers");
    let remaining = env
        .call_static_method(
            &class,
            jni_str!("commitAnswers"),
            jni_sig!("(I)I"),
            &[JValue::Int(committed)],
        )?
        .i()?;
    Ok((
        Answer::from_drained(&values, read_capabilities, read_keys, read_data),
        usize::try_from(remaining).unwrap_or(0),
    ))
}
