//! The boundary a panic meets before it reaches the operating system.
//!
//! The OS calls into the crate through function pointers it was handed: a
//! Win32 window procedure (`extern "system"`), a JNI native, an Objective-C
//! method or block (`extern "C-unwind"`), a COM vtable entry, a GCD work item.
//! A panic must not leave any of them. Out of an `extern "C"` or
//! `extern "system"` frame the language aborts the process (Rust Reference,
//! *Panic* chapter, "unwinding across FFI boundaries"; before Rust 1.81 it
//! was undefined behaviour) and says nothing through the framework's
//! diagnostics; out of an `extern "C-unwind"` frame the panic unwinds through
//! AppKit's or libdispatch's frames, which were not written to be unwound.
//!
//! `os_entry` is the one boundary: every entry's body is a call of it, or
//! of a function of the same file whose body is that call (an adapter, such
//! as a COM object's `com_entry`), and `cargo xtask panic-boundary` checks
//! that of every entry in the crate (ADR-0151 §16). An entry says what it
//! answers after a panic (`AfterPanic`): a failure the OS understands, or,
//! where the state behind it may be torn mid-dispatch, the end of the process
//! (`docs/PANIC-POLICY.md`: a panic is a bug report, never control flow).
//!
//! Recoverable owner callbacks have a boundary of their own inside an entry:
//! the call point (`super::call_point`) settles a handler's slot apart from
//! its call, and callback invocation and capture cleanup are contained
//! separately ([`contain_owner_callback`]).
//!
//! The payload-to-message rule is the pure, decidable part; it lives here,
//! compiled under `test` on every host, so the Linux-executed suite pins it.

/// Fallback text for a panic payload that is neither a `&'static str` nor a
/// `String` (someone used `std::panic::panic_any` with an arbitrary type).
#[cfg(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "ios",
    target_os = "android",
    target_os = "linux",
    all(test, not(target_arch = "wasm32"))
))]
pub(crate) const OPAQUE_PANIC_PAYLOAD: &str = "non-string panic payload";

/// Extracts the human-readable message from a caught panic payload.
///
/// `panic!("literal")` produces a `&'static str` payload; `panic!("{x}")`
/// and every formatted variant produce a `String`; anything else (via
/// `panic_any`) is opaque and reported as [`OPAQUE_PANIC_PAYLOAD`].
#[cfg(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "ios",
    target_os = "android",
    target_os = "linux",
    all(test, not(target_arch = "wasm32"))
))]
#[must_use]
pub(crate) fn panic_payload_message(payload: &(dyn std::any::Any + Send)) -> &str {
    flui_foundation::panic::payload_text(payload).unwrap_or(OPAQUE_PANIC_PAYLOAD)
}

/// What an OS entry answers once its body panicked.
#[cfg(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "ios",
    target_os = "android",
    target_os = "linux",
    all(test, not(target_arch = "wasm32"))
))]
pub(crate) enum AfterPanic<A, R> {
    /// The entry failed and the state behind it holds: the OS gets what this
    /// makes of the entry's arguments (a session-end query is granted, any
    /// other message answered 0). A plain `fn`, so the answer captures
    /// nothing, and nothing of it is made unless the body panicked (a COM
    /// error reads the thread's error information as it is made).
    Answer(fn(A) -> R),
    /// The state behind the entry may be torn mid-dispatch, and answering
    /// would let the OS run more of it: the process ends once the panic is
    /// logged. Only the Win32, AppKit and UIKit entries have no answer to
    /// give.
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "ios"))]
    Abort,
}

/// Run the body of an entry the OS calls, so that no panic leaves it: the
/// body's answer, or, when it panicked, what `after_panic` makes of `args`,
/// what the OS called the entry with (a window message and its handle).
///
/// The panic is logged first, with its text, under `entry` and with `args`
/// (unless they are `()`), so a failure that follows (an answer that panics
/// too, the abort) never hides it; the log is contained like any owner
/// callback, since a subscriber is user code. The payload is then retained
/// rather than dropped: its destructor is user code too. An answer that
/// panics leaves nothing to give the OS, so the process ends. `args` are
/// `Copy`, so nothing of them is destroyed outside the boundary either.
///
/// An entry's body is one call of this function, or of an adapter of its file
/// whose body is that call; `cargo xtask panic-boundary` holds every entry in
/// the crate to it.
#[cfg(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "ios",
    target_os = "android",
    target_os = "linux",
    all(test, not(target_arch = "wasm32"))
))]
pub(crate) fn os_entry<A, R>(
    entry: &'static str,
    args: A,
    after_panic: AfterPanic<A, R>,
    body: impl FnOnce() -> R,
) -> R
where
    A: Copy + std::fmt::Debug,
{
    let shown: Option<&dyn std::fmt::Debug> = (size_of::<A>() != 0).then_some(&args);
    if let Some(answer) = contained(entry, shown, body) {
        return answer;
    }
    match after_panic {
        AfterPanic::Answer(answer) => {
            contained(entry, shown, move || answer(args)).unwrap_or_else(|| {
                end_process(entry, shown, "its answer after the panic panicked too")
            })
        }
        #[cfg(any(target_os = "windows", target_os = "macos", target_os = "ios"))]
        AfterPanic::Abort => end_process(entry, shown, "the state behind it may be torn"),
    }
}

/// End the process from inside an entry that has nothing to answer.
#[cfg(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "ios",
    target_os = "android",
    target_os = "linux",
    all(test, not(target_arch = "wasm32"))
))]
fn end_process(entry: &'static str, args: Option<&dyn std::fmt::Debug>, why: &'static str) -> ! {
    contain_owner_callback(|| {
        tracing::error!(
            entry,
            args = args.map(|args| format!("{args:?}")),
            why,
            "an OS entry panicked and cannot answer; aborting"
        );
    });
    std::process::abort()
}

/// Contain owner callback and capture cleanup panics without dropping hostile
/// payloads; the panic is logged with its text.
pub(crate) fn contain_owner_callback(body: impl FnOnce()) {
    let _ = owner_callback_panicked(body);
}

/// [`contain_owner_callback`], answering whether `body` panicked, so a caller
/// retiring several values retains the rest after the first failure
/// (ADR-0127 §2).
pub(crate) fn owner_callback_panicked(body: impl FnOnce()) -> bool {
    let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)) else {
        return false;
    };
    if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tracing::error!(
            panic = flui_foundation::panic::payload_text(&*payload),
            "contained owner callback or cleanup panic"
        );
    })) {
        std::mem::forget(payload);
    }
    std::mem::forget(payload);
    true
}

/// [`os_entry`]'s containment, for one step inside an entry that answers its
/// failure itself (a broadcast subscriber, a drag provider the OLE data
/// object serves): `None` when `body` panicked. The panic is logged with its
/// text, under `entry`, the log contained like any owner callback, and its
/// payload is then retained rather than dropped, since a payload's destructor
/// is user code that would unwind into the OS (ADR-0127 §2).
#[cfg(any(
    target_os = "windows",
    target_os = "macos",
    all(test, not(target_arch = "wasm32"))
))]
pub(crate) fn contain_os_entry<R>(entry: &'static str, body: impl FnOnce() -> R) -> Option<R> {
    contained(entry, None, body)
}

/// `body`, or `None` once its panic is logged and its payload retained.
#[cfg(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "ios",
    target_os = "android",
    target_os = "linux",
    all(test, not(target_arch = "wasm32"))
))]
fn contained<R>(
    entry: &'static str,
    args: Option<&dyn std::fmt::Debug>,
    body: impl FnOnce() -> R,
) -> Option<R> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)) {
        Ok(value) => Some(value),
        Err(payload) => {
            contain_owner_callback(|| {
                tracing::error!(
                    entry,
                    args = args.map(|args| format!("{args:?}")),
                    panic = panic_payload_message(&*payload),
                    "an OS entry panicked"
                );
            });
            flui_foundation::panic::retain_opaque_payload(payload);
            None
        }
    }
}

/// Keep an owner-affine resource whose final owner was dropped on another thread.
/// Retention precedes diagnostics: a subscriber panic must not retire the resource
/// on the wrong thread, including while the caller is already unwinding.
#[cfg(any(target_os = "windows", target_os = "macos", test))]
pub(crate) fn retain_off_owner_resource<T>(resource: T, report: impl FnOnce()) {
    std::mem::forget(resource);
    contain_owner_callback(report);
}

// A panic on wasm32 aborts instead of unwinding, so there is nothing to catch.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    use super::{AfterPanic, contain_os_entry, contain_owner_callback, os_entry};

    /// A subscriber that records, in order, the `args` and `panic` fields of
    /// each event and the events whose message is `answered`; or panics on
    /// every event when told to.
    struct Recorder {
        panics: bool,
        seen: Arc<Mutex<Vec<String>>>,
    }

    impl Recorder {
        fn recording() -> (Self, Arc<Mutex<Vec<String>>>) {
            let seen = Arc::new(Mutex::new(Vec::new()));
            let recorder = Self {
                panics: false,
                seen: Arc::clone(&seen),
            };
            (recorder, seen)
        }
    }

    struct Fields(Vec<String>);

    impl tracing::field::Visit for Fields {
        fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
            match field.name() {
                "panic" => self.0.push(format!("logged: {value}")),
                "args" => self.0.push(format!("args: {value}")),
                _ => {}
            }
        }

        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            let value = format!("{value:?}");
            if field.name() == "message" && value == "answered" {
                self.0.push(value);
            }
        }
    }

    impl tracing::Subscriber for Recorder {
        fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
            true
        }

        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }

        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

        fn event(&self, event: &tracing::Event<'_>) {
            assert!(!self.panics, "a subscriber that panics on every event");
            let mut fields = Fields(Vec::new());
            event.record(&mut fields);
            self.seen
                .lock()
                .expect("the recorder is not poisoned")
                .extend(fields.0);
        }

        fn enter(&self, _: &tracing::span::Id) {}

        fn exit(&self, _: &tracing::span::Id) {}
    }

    /// The body's answer passes through; after a panic the entry answers what
    /// it said it would, and the panic is logged with its text before the
    /// answer is made, so nothing that follows hides the first failure.
    /// Red-check: answer before logging, or skip the log.
    #[test]
    fn an_os_entry_logs_the_first_failure_then_answers() {
        assert_eq!(
            os_entry("a healthy entry", (), AfterPanic::Answer(|()| 0), || 7),
            7
        );

        let (recorder, seen) = Recorder::recording();
        let answered = tracing::subscriber::with_default(recorder, || {
            os_entry(
                "a failing entry",
                (),
                AfterPanic::Answer(|()| {
                    tracing::info!("answered");
                    -1
                }),
                || -> i32 { panic!("the first failure") },
            )
        });
        assert_eq!(answered, -1);
        assert_eq!(
            *seen.lock().expect("the recorder is not poisoned"),
            ["logged: the first failure", "answered"]
        );
    }

    /// The answer after a panic is made from what the OS called the entry
    /// with, and the log names those arguments: a session-end query is
    /// granted where any other message is answered 0. Red-check: answer
    /// without the arguments, or log without them.
    #[test]
    fn an_os_entry_answers_from_its_arguments_and_logs_them() {
        const QUERY: u32 = 0x11;
        let after_panic = || AfterPanic::Answer(|message: u32| isize::from(message == QUERY));
        let failing = || -> isize { panic!("a subscriber's failure") };

        let (recorder, seen) = Recorder::recording();
        let answers = tracing::subscriber::with_default(recorder, || {
            [
                os_entry("a window message", QUERY, after_panic(), failing),
                os_entry("a window message", 0x0F_u32, after_panic(), failing),
            ]
        });
        assert_eq!(answers, [1, 0]);
        assert_eq!(
            *seen.lock().expect("the recorder is not poisoned"),
            [
                "args: 17",
                "logged: a subscriber's failure",
                "args: 15",
                "logged: a subscriber's failure"
            ]
        );
    }

    /// A step contained inside an entry is logged with its panic's text, so
    /// the first failure is never a generic line. Red-check: log without the
    /// text.
    #[test]
    fn a_contained_callback_is_logged_with_its_text() {
        let (recorder, seen) = Recorder::recording();
        tracing::subscriber::with_default(recorder, || {
            contain_owner_callback(|| panic!("a hook's failure"));
        });
        assert_eq!(
            *seen.lock().expect("the recorder is not poisoned"),
            ["logged: a hook's failure"]
        );
    }

    /// A subscriber is user code: one that panics while the entry logs its
    /// panic does not unwind out of the entry. Red-check: log outside the
    /// containment.
    #[test]
    fn a_panicking_log_stays_inside_the_entry() {
        let recorder = Recorder {
            panics: true,
            seen: Arc::default(),
        };
        let answered = tracing::subscriber::with_default(recorder, || {
            os_entry(
                "a failing entry",
                (),
                AfterPanic::Answer(|()| 3_u8),
                || -> u8 { panic!("the body's failure") },
            )
        });
        assert_eq!(answered, 3);
    }

    /// A panic payload whose destructor is user code: it records that it ran.
    struct Payload(Arc<AtomicBool>);

    impl Drop for Payload {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    /// An entry's answer passes through; a panic is `None` and an opaque
    /// payload is retained, never destroyed inside the OS's call. Red-check:
    /// drop the payload instead of retaining it.
    #[test]
    fn an_os_entry_contains_its_panic_and_retains_its_payload() {
        assert_eq!(contain_os_entry("a healthy entry", || 7), Some(7));
        assert_eq!(
            contain_os_entry("a formatted panic", || -> u8 { panic!("entry {}", 7) }),
            None
        );

        let destroyed = Arc::new(AtomicBool::new(false));
        let payload = Payload(Arc::clone(&destroyed));
        let answered = contain_os_entry("an opaque panic", move || -> u8 {
            std::panic::panic_any(payload)
        });
        assert_eq!(answered, None);
        assert!(
            !destroyed.load(Ordering::SeqCst),
            "the payload's destructor ran inside the entry"
        );
    }
}
