//! The one boundary every OS entry into the crate goes through
//! (ADR-0151 §16): a window procedure, an enumeration callback, a WinRT
//! delegate. A panic unwinding out of an `extern "system"` frame aborts the
//! process; contained here, it is logged and the OS gets the entry's answer
//! after a panic. `cargo xtask panic-boundary` holds every entry of the
//! crate to one call of [`os_entry`], as it holds `flui-native`'s to that
//! crate's boundary of the same name and contract.

use std::any::Any;
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};

/// What an entry answers the OS after its body panicked.
pub(crate) enum AfterPanic<A, R> {
    /// What this makes of the entry's arguments: a plain `fn`, so the answer
    /// captures nothing and nothing of it is made unless the body panicked.
    Answer(fn(A) -> R),
}

/// Run the body of an entry the OS calls, so that no panic leaves it: the
/// body's answer, or, when it panicked, what `after_panic` makes of `args`,
/// what the OS called the entry with.
///
/// The panic is logged first, with its text, under `entry` and with `args`,
/// and the log is contained, since a subscriber is user code; the payload is
/// then destroyed under containment, its destructor being user code too.
/// `args` are `Copy`, so nothing of them is destroyed outside the boundary.
pub(crate) fn os_entry<A, R>(
    entry: &'static str,
    args: A,
    after_panic: AfterPanic<A, R>,
    body: impl FnOnce() -> R,
) -> R
where
    A: Copy + fmt::Debug,
{
    #[expect(
        clippy::disallowed_methods,
        reason = "ADR-0287: the boundary the crate's OS entries go through (ADR-0151 §16)"
    )]
    let ran = catch_unwind(AssertUnwindSafe(body));
    match ran {
        Ok(answer) => answer,
        Err(payload) => {
            let text = payload_text(&*payload);
            #[expect(
                clippy::disallowed_methods,
                reason = "ADR-0287: a log subscriber is user code, contained at the boundary"
            )]
            let logged = catch_unwind(AssertUnwindSafe(|| {
                tracing::error!(
                    entry,
                    ?args,
                    panic = text,
                    "a panic was contained at an OS entry"
                );
            }));
            if let Err(log_panic) = logged {
                discard(log_panic);
            }
            discard(payload);
            let AfterPanic::Answer(answer) = after_panic;
            answer(args)
        }
    }
}

/// The text a panic was raised with, when it carries one.
fn payload_text(payload: &(dyn Any + Send)) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("a panic payload with no text")
}

/// Destroy a caught payload under containment, each panic its destructor
/// raises too, until none is left (ADR-0281 §3).
fn discard(payload: Box<dyn Any + Send>) {
    let mut pending = Some(payload);
    while let Some(payload) = pending.take() {
        #[expect(
            clippy::disallowed_methods,
            reason = "ADR-0287: a payload's destructor is user code, contained at the boundary"
        )]
        if let Err(next) = catch_unwind(AssertUnwindSafe(|| drop(payload))) {
            pending = Some(next);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AfterPanic, os_entry};

    #[test]
    fn a_panicking_entry_answers_its_after_panic_value_and_a_quiet_one_its_own() {
        assert_eq!(
            os_entry("test", 7_i32, AfterPanic::Answer(|args| args * 2), || 1),
            1
        );
        assert_eq!(
            os_entry(
                "test",
                7_i32,
                AfterPanic::Answer(|args| args * 2),
                || -> i32 { panic!("the entry fails") }
            ),
            14
        );
    }
}
