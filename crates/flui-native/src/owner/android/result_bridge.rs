//! The Activity a capability provider reaches (ADR-0154 §9, ADR-0255
//! decision 8): requests for results through `FluiHost`, routed by
//! [`ResultRouter`], and the Activity lent to a provider's own JNI code.
//!
//! Owner thread: the provider's closures run here, with the router's borrow
//! released, and what the router settles is resolved after it. A start is
//! posted to the Java main thread through [`main_thread::post`], which the
//! owner never waits for (ADR-0247 item 5); a hand-off that fails there (a
//! JNI error, a panic, a dropped closure) comes back through [`HandOffs`] and
//! ends its request as `HandOffFailed`. Answers come back through
//! `FluiHost`'s queue, which the loop drains each turn and hands to
//! [`ResultBridge::answer`].

use std::cell::RefCell;
use std::num::NonZeroU64;
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::{Arc, Weak};

use android_activity::AndroidApp;
use flui_platform::capability::{
    ActivityBridge, CompletionSource, IntentFrame, JniFrame, RequestKey, ResultRefused,
    ResultRequest, ResultToken,
};
use jni::objects::{JObject, JString};
use jni::refs::Global;
use jni::{Env, JValue, JavaVM, jni_sig, jni_str};
use parking_lot::Mutex;

use super::activity::with_activity;
use super::activity_results::{ANSWER_NUMBERS, Answer, Purpose, ResultRouter, Routed, key_fits};
use super::java_host::host_class;
use super::main_thread::{self, PostFailure};
use crate::owner::signal::OwnerSignal;

/// An answered `Intent`, held by a global reference on the owner thread
/// until its provider takes it or the Activity ends.
pub(crate) type HeldIntent = Global<JObject<'static>>;

/// The launches the Java main thread could not hand to Android, for the
/// owner to end; it is woken for each.
struct HandOffs {
    failed: Mutex<Vec<ResultToken>>,
    signal: Weak<OwnerSignal>,
}

impl HandOffs {
    /// Java main thread: `token`'s hand-off failed.
    fn fail(&self, token: ResultToken) {
        self.failed.lock().push(token);
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
    /// The bridge of `app`'s Activity, with an epoch `FluiHost.nextEpoch`
    /// minted for it, waking `signal` when a hand-off fails; `None`, logged,
    /// when `FluiHost` cannot be reached, and providers are then offered no
    /// Activity. Owner thread.
    pub(crate) fn start(app: &AndroidApp, signal: Weak<OwnerSignal>) -> Option<Rc<Self>> {
        let epoch = with_activity(app, |env, activity| {
            env.with_local_frame(4, |env| {
                let class = host_class(env, activity)?;
                env.call_static_method(&class, jni_str!("nextEpoch"), jni_sig!("()J"), &[])?
                    .j()
            })
        });
        let epoch = match epoch.map(|epoch| u64::try_from(epoch).ok().and_then(NonZeroU64::new)) {
            Ok(Some(epoch)) => epoch,
            Ok(None) => {
                tracing::warn!(
                    target: "flui_native::android",
                    "FluiHost gave an epoch that is not positive; no Activity is offered"
                );
                return None;
            }
            Err(error) => {
                tracing::warn!(
                    target: "flui_native::android",
                    %error,
                    "FluiHost's epoch could not be read; no Activity is offered"
                );
                return None;
            }
        };
        Some(Rc::new(Self {
            app: app.clone(),
            router: RefCell::new(ResultRouter::new(epoch)),
            hand_offs: Arc::new(HandOffs {
                failed: Mutex::new(Vec::new()),
                signal,
            }),
        }))
    }

    /// What `flui-app` installs in the registry's host bridge.
    pub(crate) fn bridge(self: &Rc<Self>) -> Rc<dyn ActivityBridge> {
        Rc::clone(self) as Rc<dyn ActivityBridge>
    }

    /// End the requests whose hand-off failed, then route what `FluiHost`
    /// answered since the last turn. Owner thread, every turn.
    pub(crate) fn answer(&self, answers: Vec<Answer<HeldIntent>>) {
        let failed = std::mem::take(&mut *self.hand_offs.failed.lock());
        for token in failed {
            let delivery = self.router.borrow_mut().hand_off_failed(token);
            if let Some(delivery) = delivery {
                delivery.deliver();
            }
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

    /// Whether `FluiHost` still holds a request of an earlier Activity under
    /// `key` that has not been answered, or an answer not drained. A read
    /// that fails says yes: the reattachment then waits, and ends with the
    /// Activity at the latest.
    fn still_to_come(&self, key: &str) -> bool {
        let epoch = self.epoch();
        with_activity(&self.app, |env, activity| {
            env.with_local_frame(4, |env| {
                let class = host_class(env, activity)?;
                let key = env.new_string(key)?;
                env.call_static_method(
                    &class,
                    jni_str!("stillToCome"),
                    jni_sig!("(JLjava/lang/String;)Z"),
                    &[JValue::Long(epoch), JValue::Object(&key)],
                )?
                .z()
            })
        })
        .unwrap_or(true)
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
    fn launch_for_result(
        &self,
        completions: &CompletionSource,
        key: RequestKey,
        intent: &mut dyn FnMut(IntentFrame) -> bool,
    ) -> Result<ResultRequest, ResultRefused> {
        self.router.borrow().admits()?;
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
        let request = self.router.borrow_mut().open_launch(completions, key)?;
        let token = request.token();
        let (app, epoch, spelled) = (self.app.clone(), self.epoch(), key.to_string());
        let hand_offs = Arc::clone(&self.hand_offs);
        main_thread::post(
            &self.app,
            "FluiHost.launch",
            move || launch_on_main_thread(&app, &filled, epoch, token, &spelled),
            move |failure: PostFailure| {
                tracing::warn!(
                    target: "flui_native::android",
                    ?failure,
                    "a request could not be handed to Android; it ends as HandOffFailed"
                );
                hand_offs.fail(token);
            },
        );
        Ok(request)
    }

    fn reattach(
        &self,
        completions: &CompletionSource,
        key: RequestKey,
    ) -> Result<ResultRequest, ResultRefused> {
        self.router.borrow().admits()?;
        let still_to_come = self.still_to_come(&key.to_string());
        let (request, delivery) =
            self.router
                .borrow_mut()
                .open_reattach(completions, key, still_to_come)?;
        delivery.deliver();
        Ok(request)
    }

    fn cancel(&self, token: ResultToken) {
        let cancelled = self.router.borrow_mut().cancel(token);
        let Some((purpose, delivery)) = cancelled else {
            return;
        };
        delivery.deliver();
        if purpose == Purpose::Launched {
            let (app, epoch) = (self.app.clone(), self.epoch());
            main_thread::post(
                &self.app,
                "FluiHost.cancel",
                move || cancel_on_main_thread(&app, epoch, token),
                |failure: PostFailure| {
                    // The request already ended as cancelled; the activity's
                    // answer, if it comes, is dropped as stale.
                    tracing::warn!(
                        target: "flui_native::android",
                        ?failure,
                        "a cancelled request's activity was not finished"
                    );
                },
            );
        }
    }

    fn take_data(&self, token: ResultToken, f: &mut dyn FnMut(JniFrame)) -> bool {
        // Taken out of the router first: the provider's closure runs with no
        // borrow held, and the reference is deleted when it returns.
        let Some(intent) = self.router.borrow_mut().take_data(token) else {
            return false;
        };
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
        .unwrap_or(false)
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
    key: &str,
) -> Result<(), String> {
    with_activity(app, |env, activity| {
        env.with_local_frame(8, |env| {
            let class = host_class(env, activity)?;
            let key = env.new_string(key)?;
            env.call_static_method(
                &class,
                jni_str!("launch"),
                jni_sig!("(Landroid/app/Activity;Landroid/content/Intent;JJLjava/lang/String;)V"),
                &[
                    JValue::Object(activity),
                    JValue::Object(intent.as_obj()),
                    JValue::Long(epoch),
                    JValue::Long(token.get().cast_signed()),
                    JValue::Object(&key),
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

/// The most answers one drain reads: what `FluiHost` can hold.
const MAX_DRAINED: usize = 2 * flui_platform::capability::MAX_OPEN_REQUESTS;

/// The oldest answers `FluiHost` queued, at most [`MAX_DRAINED`], each key's
/// length checked before it is copied and each `Intent` held as it is, never
/// read; then `FluiHost` lets go of exactly those, and says how many are
/// still queued. A failure before that leaves every answer queued for the
/// next turn.
pub(crate) fn drain_answers(
    env: &mut Env<'_>,
    activity: &JObject<'_>,
) -> jni::errors::Result<(Vec<Answer<HeldIntent>>, usize)> {
    use flui_platform::capability::MAX_REQUEST_KEY_LEN;
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
    let keys = peeked.get_element(env, 1)?;
    let keys = env.cast_local::<JObjectArray<'_>>(keys)?;
    let data = peeked.get_element(env, 2)?;
    let data = env.cast_local::<JObjectArray<'_>>(data)?;
    let count = keys.len(env)?.min(MAX_DRAINED);
    let length = numbers.len(env)?;
    let mut values = vec![0; length.min(count * ANSWER_NUMBERS)];
    numbers.get_region(env, 0, &mut values)?;
    let mut read_keys = Vec::with_capacity(count);
    let mut read_data = Vec::with_capacity(count);
    for index in 0..count {
        let (key, held) = env.with_local_frame(4, |env| -> jni::errors::Result<_> {
            let key = keys.get_element(env, index)?;
            let key = env.cast_local::<JString<'_>>(key)?;
            let key = if key.is_null() {
                None
            } else {
                let length = env
                    .call_method(&key, jni_str!("length"), jni_sig!("()I"), &[])?
                    .i()?;
                if usize::try_from(length).is_ok_and(|length| length <= MAX_REQUEST_KEY_LEN) {
                    Some(key.try_to_string(env)?).filter(|key| key_fits(key))
                } else {
                    None
                }
            };
            let item = data.get_element(env, index)?;
            let held = if item.is_null() {
                None
            } else {
                Some(env.new_global_ref(&item)?)
            };
            Ok((key, held))
        })?;
        read_keys.push(key);
        read_data.push(held);
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
        Answer::from_drained(&values, read_keys, read_data),
        usize::try_from(remaining).unwrap_or(0),
    ))
}
