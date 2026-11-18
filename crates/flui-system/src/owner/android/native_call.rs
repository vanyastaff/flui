//! A native method of the crate's Java classes, from the call Java makes to
//! what it answers.
//!
//! Java calls `FluiSystem.answered` from a callback on its main thread (a
//! runnable posted to it, a dialog's listener), and none of those catches:
//! an exception pending when the method returns is thrown there, and an
//! exception nothing catches on the main thread ends the process. So a
//! native method is one call of [`jni_native`], which `cargo xtask
//! panic-boundary` holds it to, as it holds `flui-native`'s to that crate's
//! adapter of the same name and contract: the body runs inside the panic
//! boundary with the method's environment, Java receives the body's answer
//! or, after a failed JNI call or a panic, the method's "nothing" (`false`,
//! `0`, `null`, no value), and never a pending exception, which
//! [`ClearsPending`] describes to the log and clears on every exit, while a
//! panic unwinds too. A contained failure costs the one call; the Activity
//! goes on.
//!
//! The JNI side is [`JniCall`]: on Android a native method's `EnvUnowned`,
//! in the tests a stand-in, so the sequence the tests run is the one Android
//! runs.

use std::fmt;

use crate::owner::panic_boundary::{AfterPanic, os_entry};

/// The Java exception slot of the thread a native method runs on.
pub(crate) trait ExceptionSlot {
    /// Describe the pending exception, if one is, to the log and clear it.
    fn clear(&mut self);
}

/// Clears its slot when it drops: on the way out of the scope it guards,
/// whether by a return or by a panic unwinding to the boundary.
pub(crate) struct ClearsPending<S: ExceptionSlot>(S);

impl<S: ExceptionSlot> ClearsPending<S> {
    /// A guard over `slot`.
    pub(crate) fn new(slot: S) -> Self {
        Self(slot)
    }
}

impl<S: ExceptionSlot> Drop for ClearsPending<S> {
    fn drop(&mut self) {
        self.0.clear();
    }
}

/// The JNI side of one native call.
pub(crate) trait JniCall {
    /// What the body works with: the call's environment.
    type Env;
    /// The slot of the call's thread.
    type Slot: ExceptionSlot;
    /// A failed JNI call.
    type Error: fmt::Display;

    /// The slot of the thread the call runs on.
    fn slot(&self) -> Self::Slot;

    /// Run `body` with the call's environment, catching nothing: a panic
    /// goes on to the boundary.
    fn enter<T>(
        &mut self,
        body: impl FnOnce(&mut Self::Env) -> Result<T, Self::Error>,
    ) -> Result<T, Self::Error>;
}

/// Run a native method's `body` inside the boundary (see the module doc):
/// its answer, or `nothing` after a failed JNI call or a panic, with no
/// exception left pending.
pub(crate) fn jni_native<C: JniCall, T>(
    method: &'static str,
    call: &mut C,
    nothing: fn(()) -> T,
    body: impl FnOnce(&mut C::Env) -> Result<T, C::Error>,
) -> T {
    os_entry(method, (), AfterPanic::Answer(nothing), || {
        answered(method, call, nothing, body)
    })
}

/// The boundary's body for a native method: the guard first, so it clears
/// after the environment is let go, on every exit.
fn answered<C: JniCall, T>(
    method: &'static str,
    call: &mut C,
    nothing: fn(()) -> T,
    body: impl FnOnce(&mut C::Env) -> Result<T, C::Error>,
) -> T {
    let _clears = ClearsPending::new(call.slot());
    match call.enter(body) {
        Ok(answer) => answer,
        Err(error) => {
            tracing::warn!(
                target: "flui_system::android",
                method,
                %error,
                "a native method's JNI call failed"
            );
            nothing(())
        }
    }
}

/// The slot of the thread a JNI environment belongs to.
#[cfg(target_os = "android")]
pub(crate) struct EnvSlot(*mut jni::sys::JNIEnv);

#[cfg(target_os = "android")]
impl EnvSlot {
    /// The slot of `env`'s thread.
    ///
    /// # Safety
    ///
    /// `env` is a JNI environment of this thread (a native method's, or an
    /// attachment's), valid for as long as the slot lives.
    #[expect(
        unsafe_code,
        reason = "the slot holds the raw environment of a native call"
    )]
    pub(crate) unsafe fn new(env: *mut jni::sys::JNIEnv) -> Self {
        Self(env)
    }
}

#[cfg(target_os = "android")]
impl ExceptionSlot for EnvSlot {
    #[expect(
        unsafe_code,
        reason = "the slot holds the raw environment of a native call"
    )]
    fn clear(&mut self) {
        // SAFETY: `EnvSlot::new`'s contract: an environment of this thread,
        // valid while the slot lives. The guard stays on this stack frame,
        // above every guard alive when the slot drops.
        let mut guard = unsafe { jni::AttachGuard::from_unowned(self.0) };
        let env = guard.borrow_env_mut();
        // The three calls are among the few JNI allows with an exception
        // pending, and none of them throws.
        if env.exception_check() {
            env.exception_describe();
            env.exception_clear();
        }
    }
}

/// A native method's own environment.
#[cfg(target_os = "android")]
impl<'local> JniCall for jni::EnvUnowned<'local> {
    type Env = jni::Env<'local>;
    type Slot = EnvSlot;
    type Error = jni::errors::Error;

    #[expect(
        unsafe_code,
        reason = "the slot holds the raw environment of a native call"
    )]
    fn slot(&self) -> EnvSlot {
        // SAFETY: the environment Java passed this native method, on its
        // thread; the guard holding the slot drops before the method returns.
        unsafe { EnvSlot::new(self.as_raw()) }
    }

    #[expect(
        unsafe_code,
        reason = "the call's environment is borrowed for the body"
    )]
    fn enter<T>(
        &mut self,
        body: impl FnOnce(&mut jni::Env<'local>) -> Result<T, jni::errors::Error>,
    ) -> Result<T, jni::errors::Error> {
        // SAFETY: the environment Java passed this native method, attached
        // to this thread; the guard stays on this stack frame, which the
        // body cannot outlive (what `EnvUnowned::with_env_no_catch` does,
        // without the outcome type that offers a panic it never holds).
        let mut guard = unsafe { jni::AttachGuard::from_unowned(self.as_raw()) };
        body(guard.borrow_env_mut())
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use super::{ExceptionSlot, JniCall, jni_native};

    /// A thread whose pending exception is a flag.
    #[derive(Clone, Default)]
    struct Thread(Rc<Cell<bool>>);

    impl Thread {
        fn throw(&self) {
            self.0.set(true);
        }
    }

    impl ExceptionSlot for Thread {
        fn clear(&mut self) {
            self.0.set(false);
        }
    }

    /// A native call on that thread: its environment is the thread.
    struct Call(Thread);

    impl JniCall for Call {
        type Env = Thread;
        type Slot = Thread;
        type Error = &'static str;

        fn slot(&self) -> Thread {
            self.0.clone()
        }

        fn enter<T>(
            &mut self,
            body: impl FnOnce(&mut Thread) -> Result<T, &'static str>,
        ) -> Result<T, &'static str> {
            body(&mut self.0)
        }
    }

    #[derive(Clone, Copy, Debug)]
    enum Body {
        Answers,
        ThrowsThenAnswers,
        Fails,
        ThrowsThenPanics,
        Panics,
    }

    /// What the native method answers Java, and whether Java finds an
    /// exception pending.
    fn native(body: Body) -> (Option<u8>, bool) {
        let thread = Thread::default();
        let mut call = Call(thread.clone());
        let answer = jni_native(
            "a native method",
            &mut call,
            |()| None,
            |env| match body {
                Body::Answers => Ok(Some(7)),
                Body::ThrowsThenAnswers => {
                    env.throw();
                    Ok(Some(7))
                }
                Body::Fails => {
                    env.throw();
                    Err("the call threw")
                }
                Body::ThrowsThenPanics => {
                    env.throw();
                    panic!("the body panicked");
                }
                Body::Panics => panic!("the body panicked"),
            },
        );
        (answer, thread.0.get())
    }

    /// Java receives the body's answer, or the method's "nothing" after a
    /// failed call or a panic, and finds no exception pending on any exit.
    #[test]
    fn a_native_method_answers_and_leaves_no_exception_pending_on_any_exit() {
        for (body, expected) in [
            (Body::Answers, (Some(7), false)),
            (Body::ThrowsThenAnswers, (Some(7), false)),
            (Body::Fails, (None, false)),
            (Body::ThrowsThenPanics, (None, false)),
            (Body::Panics, (None, false)),
        ] {
            assert_eq!(native(body), expected, "{body:?}");
        }
    }
}
