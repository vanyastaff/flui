//! The boundary a panic meets before it reaches the operating system.
//!
//! The OS calls into FLUI through function pointers it was handed: a Win32
//! window procedure (`extern "system"`), a JNI native, an Objective-C method
//! or block (`extern "C-unwind"`), a COM vtable entry, a GCD work item. A
//! panic must not leave any of them. Out of an `extern "C"` or
//! `extern "system"` frame the language aborts the process (Rust Reference,
//! *Panic* chapter, "unwinding across FFI boundaries") and says nothing
//! through the framework's diagnostics; out of an `extern "C-unwind"` frame
//! the panic unwinds through AppKit's or libdispatch's frames, which were not
//! written to be unwound.
//!
//! [`os_entry`] is the one boundary, for the core host and for every
//! capability crate alike (ADR-0151 §16): every entry's body is a call of it,
//! or of a function of the same file whose body is that call (an adapter,
//! such as a COM object's `com_entry`). An entry says what it answers after a
//! panic ([`AfterPanic`]): a failure the OS understands, or, where the state
//! behind it may be torn mid-dispatch, the end of the process
//! (`docs/PANIC-POLICY.md`: a panic is a bug report, never control flow).
//!
//! One entry has no body of its own crate: the `dealloc` objc2 generates for
//! a class declared with `define_class!`, which destroys the class's ivars. A
//! class keeps them in [`ContainedIvars`], whose destructor is that call.

use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::{discard_payload, payload_text};

/// What a log says of a payload that carries no text (one raised with
/// [`std::panic::panic_any`]).
const OPAQUE_PAYLOAD: &str = "non-string panic payload";

/// What an OS entry answers once its body panicked.
#[expect(
    clippy::exhaustive_enums,
    reason = "an entry either answers the OS or ends the process; there is no third way out"
)]
pub enum AfterPanic<A, R> {
    /// The entry failed and the state behind it holds: the OS gets what this
    /// makes of the entry's arguments (a session-end query is granted, any
    /// other message answered 0). A plain `fn`, so the answer captures
    /// nothing, and nothing of it is made unless the body panicked (a COM
    /// error reads the thread's error information as it is made).
    Answer(fn(A) -> R),
    /// The state behind the entry may be torn mid-dispatch, and answering
    /// would let the OS run more of it: the process ends once the panic is
    /// logged.
    Abort,
}

impl<A, R> fmt::Debug for AfterPanic<A, R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Answer(_) => f.write_str("Answer"),
            Self::Abort => f.write_str("Abort"),
        }
    }
}

/// Run the body of an entry the OS calls, so that no panic leaves it: the
/// body's answer, or, when it panicked, what `after_panic` makes of `args`,
/// what the OS called the entry with (a window message and its handle).
///
/// The panic is logged first, with its text, under `entry` and with `args`
/// (unless they are `()`), so a failure that follows (an answer that panics
/// too, the abort) never hides it; the log is contained, since a subscriber
/// is user code. The payload is then discarded under containment: its
/// destructor is user code too. An answer that panics leaves nothing to give
/// the OS, so the process ends. `args` are `Copy`, so nothing of them is
/// destroyed outside the boundary either.
pub fn os_entry<A, R>(
    entry: &'static str,
    args: A,
    after_panic: AfterPanic<A, R>,
    body: impl FnOnce() -> R,
) -> R
where
    A: Copy + fmt::Debug,
{
    let shown: Option<&dyn fmt::Debug> = (size_of::<A>() != 0).then_some(&args);
    if let Some(answer) = contained(entry, shown, body) {
        return answer;
    }
    match after_panic {
        AfterPanic::Answer(answer) => {
            contained(entry, shown, move || answer(args)).unwrap_or_else(|| {
                end_process(entry, shown, "its answer after the panic panicked too")
            })
        }
        AfterPanic::Abort => end_process(entry, shown, "the state behind it may be torn"),
    }
}

/// [`os_entry`]'s containment, for one step inside an entry that answers its
/// failure itself (a broadcast subscriber, a drag provider an OLE data
/// object serves): `None` when `body` panicked. The panic is logged with its
/// text, under `entry`, the log contained, and its payload is then discarded
/// under containment, since a payload's destructor is user code that would
/// unwind into the OS (ADR-0281).
pub fn contain_os_entry<R>(entry: &'static str, body: impl FnOnce() -> R) -> Option<R> {
    contained(entry, None, body)
}

/// The ivars of an Objective-C class declared with objc2's `define_class!`,
/// destroyed inside the boundary.
///
/// objc2 generates the class's `dealloc`, an `extern "C-unwind"` method that
/// drops the ivars in place. Whoever releases the object last runs it: an
/// autorelease pool UIKit drains, a session letting go of its delegate, a
/// `Retained` of the crate's own. So the destructors of the ivars are an OS
/// entry with nothing of the crate above them, and what they own may be the
/// application's (a window's handler, a callback's captures). Declared as
/// `#[ivars = ContainedIvars<T>]`, the value is destroyed by one call of
/// [`os_entry`]: a destructor's panic is logged there with the ivars' type,
/// the fields after the failing one are destroyed as it unwinds, and
/// `dealloc` goes on to the superclass's. Every class FLUI declares keeps its
/// ivars in this type.
///
/// The value is read through `Deref` (`self.ivars().field`) and never moves:
/// it is destroyed in place, where objc2 keeps it pinned.
#[derive(Debug)]
pub struct ContainedIvars<T>(std::mem::ManuallyDrop<T>);

impl<T> ContainedIvars<T> {
    /// `ivars`, for `set_ivars`.
    pub const fn new(ivars: T) -> Self {
        Self(std::mem::ManuallyDrop::new(ivars))
    }
}

impl<T> std::ops::Deref for ContainedIvars<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

#[expect(
    unsafe_code,
    reason = "the ivars are destroyed in place, once, inside the boundary"
)]
impl<T> Drop for ContainedIvars<T> {
    fn drop(&mut self) {
        // Every class's `dealloc` is this one entry, so the answer after a
        // panic names the ivars whose destructor failed. It runs inside the
        // boundary, as an argument computed before the call would not, and
        // under a containment of its own, as the first log is: a subscriber
        // is user code, and an answer that panics ends the process.
        os_entry(
            "dealloc",
            (),
            AfterPanic::Answer(|()| {
                log_contained(|| {
                    tracing::error!(
                        ivars = std::any::type_name::<T>(),
                        "the destructor that panicked was of these ivars"
                    );
                });
            }),
            || {
                // SAFETY: the value is initialized from `new` on, this is the
                // only place that destroys it, and `drop` runs once: nothing
                // reads it afterwards.
                unsafe { std::mem::ManuallyDrop::drop(&mut self.0) }
            },
        );
    }
}

/// `body`, or `None` once its panic is logged and its payload discarded.
fn contained<R>(
    entry: &'static str,
    args: Option<&dyn fmt::Debug>,
    body: impl FnOnce() -> R,
) -> Option<R> {
    #[expect(
        clippy::disallowed_methods,
        reason = "ADR-0287: the one boundary every OS entry goes through (ADR-0151 §16)"
    )]
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(value) => Some(value),
        Err(payload) => {
            log_contained(|| {
                tracing::error!(
                    entry,
                    args = args.map(|args| format!("{args:?}")),
                    panic = payload_text(&*payload).unwrap_or(OPAQUE_PAYLOAD),
                    "an OS entry panicked"
                );
            });
            discard_payload(payload);
            None
        }
    }
}

/// End the process from inside an entry that has nothing to answer.
fn end_process(entry: &'static str, args: Option<&dyn fmt::Debug>, why: &'static str) -> ! {
    log_contained(|| {
        tracing::error!(
            entry,
            args = args.map(|args| format!("{args:?}")),
            why,
            "an OS entry panicked and cannot answer; aborting"
        );
    });
    std::process::abort()
}

/// Run `log`, whose subscriber is user code, so that a panic it raises stays
/// here: its payload is discarded under containment and nothing more is
/// logged of it.
fn log_contained(log: impl FnOnce()) {
    #[expect(
        clippy::disallowed_methods,
        reason = "ADR-0287: a log subscriber is user code, contained at the OS boundary"
    )]
    if let Err(payload) = catch_unwind(AssertUnwindSafe(log)) {
        discard_payload(payload);
    }
}
