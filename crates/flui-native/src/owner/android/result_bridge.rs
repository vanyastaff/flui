//! The Activity a capability provider reaches (ADR-0154 §9, ADR-0255
//! decision 8): requests for results through `FluiHost`, routed by
//! [`ResultRouter`], and the Activity lent to a provider's own JNI code.
//!
//! Owner thread: the provider's closures run here, with the router's borrow
//! released, and what the router settles is resolved after it. A start is
//! posted to the Java main thread, which the owner never waits for (ADR-0247
//! item 5); its answer comes back through `FluiHost`'s queue, which the loop
//! drains each turn and hands to [`ResultBridge::answer`].

use std::cell::RefCell;
use std::num::NonZeroU64;
use std::ptr::NonNull;
use std::rc::Rc;

use android_activity::AndroidApp;
use flui_platform::capability::{
    Activity, ActivityBridge, CompletionSource, IntentFrame, JavaObject, JavaReference, JniFrame,
    ResultKey, ResultRefused, ResultRequest, ResultToken,
};
use jni::objects::{JObject, JString};
use jni::refs::Global;
use jni::{Env, JValue, JavaVM, jni_sig, jni_str};

use super::activity::with_activity;
use super::activity_results::{ANSWER_NUMBERS, Answer, Purpose, ResultRouter, Routed, key_fits};
use super::java_host::host_class;

/// The running Activity's side of activity results.
pub(crate) struct ResultBridge {
    app: AndroidApp,
    router: RefCell<ResultRouter>,
}

impl std::fmt::Debug for ResultBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResultBridge").finish_non_exhaustive()
    }
}

impl ResultBridge {
    /// The bridge of `app`'s Activity, with an epoch `FluiHost.nextEpoch`
    /// minted for it; `None`, logged, when `FluiHost` cannot be reached, and
    /// providers are then offered no Activity. Owner thread.
    pub(crate) fn start(app: &AndroidApp) -> Option<Rc<Self>> {
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
        }))
    }

    /// The Activity a provider is handed.
    pub(crate) fn activity(self: &Rc<Self>) -> Activity {
        Activity::new(Rc::clone(self) as Rc<dyn ActivityBridge>)
    }

    /// Route what `FluiHost` answered since the last turn.
    pub(crate) fn answer(&self, answers: Vec<Answer>) {
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
        key: ResultKey,
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
        let (epoch, token) = (self.epoch(), request.token().get().cast_signed());
        let app = self.app.clone();
        self.app.run_on_java_main_thread(Box::new(move || {
            launch_on_main_thread(&app, &filled, epoch, token, key);
        }));
        Ok(request)
    }

    fn reattach(
        &self,
        completions: &CompletionSource,
        key: ResultKey,
    ) -> Result<ResultRequest, ResultRefused> {
        let (request, delivery) = self.router.borrow_mut().open_reattach(completions, key)?;
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
            let (app, epoch, token) = (self.app.clone(), self.epoch(), token.get().cast_signed());
            self.app.run_on_java_main_thread(Box::new(move || {
                cancel_on_main_thread(&app, epoch, token);
            }));
        }
    }

    fn lend(&self, f: &mut dyn FnMut(JniFrame)) -> bool {
        if self.router.borrow().admits() == Err(ResultRefused::ActivityEnded) {
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

/// `FluiHost.launch` for one request. Java main thread. A failure of the
/// call itself leaves the request open until its Activity ends, which ends
/// it as `ActivityEnded`.
fn launch_on_main_thread(
    app: &AndroidApp,
    intent: &Global<JObject<'static>>,
    epoch: i64,
    token: i64,
    key: ResultKey,
) {
    let launched = with_activity(app, |env, activity| {
        env.with_local_frame(8, |env| {
            let class = host_class(env, activity)?;
            let key = env.new_string(key.as_str())?;
            env.call_static_method(
                &class,
                jni_str!("launch"),
                jni_sig!("(Landroid/app/Activity;Landroid/content/Intent;JJLjava/lang/String;)V"),
                &[
                    JValue::Object(activity),
                    JValue::Object(intent.as_obj()),
                    JValue::Long(epoch),
                    JValue::Long(token),
                    JValue::Object(&key),
                ],
            )
            .map(drop)
        })
    });
    if let Err(error) = launched {
        tracing::warn!(
            target: "flui_native::android",
            %error,
            "FluiHost did not take a request; it ends with its Activity"
        );
    }
}

/// `FluiHost.cancel` for one request. Java main thread.
fn cancel_on_main_thread(app: &AndroidApp, epoch: i64, token: i64) {
    let cancelled = with_activity(app, |env, activity| {
        env.with_local_frame(4, |env| {
            let class = host_class(env, activity)?;
            env.call_static_method(
                &class,
                jni_str!("cancel"),
                jni_sig!("(Landroid/app/Activity;JJ)V"),
                &[
                    JValue::Object(activity),
                    JValue::Long(epoch),
                    JValue::Long(token),
                ],
            )
            .map(drop)
        })
    });
    if let Err(error) = cancelled {
        tracing::warn!(
            target: "flui_native::android",
            %error,
            "FluiHost did not cancel a request; its activity's answer is dropped when it comes"
        );
    }
}

/// An object `FluiHost` handed over, held by a global reference.
struct HeldObject(Global<JObject<'static>>);

impl JavaReference for HeldObject {
    fn lend(&self, f: &mut dyn FnMut(JniFrame)) -> bool {
        let Ok(vm) = JavaVM::singleton() else {
            return false;
        };
        vm.attach_current_thread(|env| -> jni::errors::Result<bool> {
            let Some(lent) = frame(env, self.0.as_obj()) else {
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

/// Every answer `FluiHost.drainAnswers` returns, read with each key's length
/// checked before it is copied and each `Intent` held as it is, never read.
pub(crate) fn drain_answers(
    env: &mut Env<'_>,
    activity: &JObject<'_>,
) -> jni::errors::Result<Vec<Answer>> {
    use flui_platform::capability::{MAX_OPEN_REQUESTS, MAX_RESULT_KEY_LEN};
    use jni::objects::{JLongArray, JObjectArray};

    let class = host_class(env, activity)?;
    let drained = env
        .call_static_method(
            &class,
            jni_str!("drainAnswers"),
            jni_sig!("()[Ljava/lang/Object;"),
            &[],
        )?
        .l()?;
    let drained = env.cast_local::<JObjectArray<'_>>(drained)?;
    let numbers = drained.get_element(env, 0)?;
    let numbers = JLongArray::cast_local(env, numbers)?;
    let keys = drained.get_element(env, 1)?;
    let keys = env.cast_local::<JObjectArray<'_>>(keys)?;
    let data = drained.get_element(env, 2)?;
    let data = env.cast_local::<JObjectArray<'_>>(data)?;
    let count = keys.len(env)?;
    // `FluiHost` holds at most twice its open requests; more is a defect of
    // one side, and none of it is copied.
    if count > 2 * MAX_OPEN_REQUESTS {
        tracing::warn!(
            target: "flui_native::android",
            count,
            "FluiHost queued more answers than it holds; they are dropped"
        );
        return Ok(Vec::new());
    }
    let length = numbers.len(env)?;
    if length != count * ANSWER_NUMBERS {
        tracing::warn!(
            target: "flui_native::android",
            length,
            count,
            "FluiHost's answers are not laid out as this library reads them; they are dropped"
        );
        return Ok(Vec::new());
    }
    let mut values = vec![0; length];
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
                if usize::try_from(length).is_ok_and(|length| length <= MAX_RESULT_KEY_LEN) {
                    Some(key.try_to_string(env)?).filter(|key| key_fits(key))
                } else {
                    None
                }
            };
            let item = data.get_element(env, index)?;
            let held = if item.is_null() {
                None
            } else {
                Some(JavaObject::new(HeldObject(env.new_global_ref(&item)?)))
            };
            Ok((key, held))
        })?;
        read_keys.push(key);
        read_data.push(held);
    }
    Ok(Answer::from_drained(&values, read_keys, read_data))
}
