//! A native method returns to Java with no exception pending.
//!
//! Java calls every native method of the view from a framework callback on
//! its main thread (a drag listener, the input connection, the insets
//! listener, the node provider), and none of those catches: an exception
//! pending when the method returns is thrown there, and an exception nothing
//! catches on the main thread ends the process. So a native method clears
//! what its JNI calls leave pending on every exit: its answer, a failed call,
//! and a panic the boundary contains. [`ClearsPending`] does it as it drops,
//! which happens while a panic unwinds to the boundary too; the exception is
//! described to the log first (`ExceptionDescribe`).
//!
//! Work posted to the Java main thread needs no guard of its own: it runs
//! under android-activity's attachment of that thread, whose
//! `attach_current_thread` catches what its closure leaves pending. The
//! accessibility bridge takes the guard around each call into AccessKit's
//! adapter all the same, whose JNI calls `unwrap`, from a native method and
//! from posted work alike.

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
        // valid while the slot lives.
        let mut env = unsafe { jni::EnvUnowned::from_raw(self.0) };
        // The three calls are among the few JNI allows with an exception
        // pending, and none of them throws.
        let _ = env
            .with_env_no_catch(|env| {
                if env.exception_check() {
                    env.exception_describe();
                    env.exception_clear();
                }
                Ok::<(), jni::errors::Error>(())
            })
            .into_outcome();
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use super::{ClearsPending, ExceptionSlot};
    use crate::owner::panic_boundary::{AfterPanic, os_entry};

    /// A thread whose pending exception is a flag.
    #[derive(Clone, Default)]
    struct Thread(Rc<Cell<bool>>);

    impl Thread {
        fn throw(&self) {
            self.0.set(true);
        }

        fn pending(&self) -> bool {
            self.0.get()
        }
    }

    impl ExceptionSlot for Thread {
        fn clear(&mut self) {
            self.0.set(false);
        }
    }

    /// A native method's shape: the guard inside the boundary's body. What
    /// the method answers, and whether Java finds an exception pending.
    fn native(throws: bool, panics: bool) -> (Option<bool>, bool) {
        let thread = Thread::default();
        let slot = thread.clone();
        let answer = os_entry(
            "a native method",
            (),
            AfterPanic::Answer(|()| None),
            move || {
                let _clears = ClearsPending::new(slot.clone());
                if throws {
                    slot.throw();
                }
                assert!(!panics, "the body panicked");
                Some(true)
            },
        );
        (answer, thread.pending())
    }

    /// Java finds no exception pending on any exit of a native method: its
    /// answer, a call that threw, and a panic, which answers the method's
    /// "nothing".
    #[test]
    fn a_native_method_leaves_no_exception_pending_on_any_exit() {
        assert_eq!(native(false, false), (Some(true), false), "an answer");
        assert_eq!(
            native(true, false),
            (Some(true), false),
            "a call that threw"
        );
        assert_eq!(
            native(true, true),
            (None, false),
            "a call that threw, then a panic"
        );
        assert_eq!(native(false, true), (None, false), "a panic");
    }
}
