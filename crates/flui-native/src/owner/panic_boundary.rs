//! The crate's side of the boundary a panic meets before it reaches the
//! operating system.
//!
//! The boundary itself is `flui_foundation::panic::os_entry`, the one every
//! OS entry of FLUI goes through, this crate's and each capability crate's
//! (ADR-0151 §16); it is reached here as `owner::panic_boundary::os_entry`,
//! with `AfterPanic`, `ContainedIvars` and `contain_os_entry`. Every entry's
//! body is a call of it, or of a function of the same file whose body is that
//! call (an adapter, such as a COM object's `com_entry`), and every class the
//! crate declares with objc2's `define_class!` keeps its ivars in
//! `ContainedIvars`, whose destructor is that call.
//!
//! Recoverable owner callbacks have a boundary of their own inside an entry:
//! the call point (`super::call_point`) settles a handler's slot apart from
//! its call, and callback invocation and capture cleanup are contained
//! separately ([`contain_owner_callback`]).
//!
//! `panic_payload_message` names a caught payload in the log of the hosts
//! that report one with its text (Windows' text services, Linux's clipboard
//! and display sources).

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) use flui_foundation::panic::ContainedIvars;
#[cfg(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "ios",
    target_os = "android",
    all(test, not(target_arch = "wasm32"))
))]
pub(crate) use flui_foundation::panic::contain_os_entry;
#[cfg(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "ios",
    target_os = "android",
    linux_host,
    all(test, not(target_arch = "wasm32"))
))]
pub(crate) use flui_foundation::panic::{AfterPanic, os_entry};

/// Fallback text for a panic payload that is neither a `&'static str` nor a
/// `String` (someone used `std::panic::panic_any` with an arbitrary type).
#[cfg(any(target_os = "windows", linux_host,))]
pub(crate) const OPAQUE_PANIC_PAYLOAD: &str = "non-string panic payload";

/// Extracts the human-readable message from a caught panic payload.
///
/// `panic!("literal")` produces a `&'static str` payload; `panic!("{x}")`
/// and every formatted variant produce a `String`; anything else (via
/// `panic_any`) is opaque and reported as [`OPAQUE_PANIC_PAYLOAD`].
#[cfg(any(target_os = "windows", linux_host,))]
#[must_use]
pub(crate) fn panic_payload_message(payload: &(dyn std::any::Any + Send)) -> &str {
    flui_foundation::panic::payload_text(payload).unwrap_or(OPAQUE_PANIC_PAYLOAD)
}

/// Contain owner callback and capture cleanup panics; the panic is logged with
/// its text and its payload destroyed under containment (ADR-0281).
pub(crate) fn contain_owner_callback(body: impl FnOnce()) {
    #[expect(
        clippy::disallowed_methods,
        reason = "the one OS entry boundary of flui-native (ADR-0151 §16)"
    )]
    let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)) else {
        return;
    };
    #[expect(
        clippy::disallowed_methods,
        reason = "the one OS entry boundary of flui-native (ADR-0151 §16)"
    )]
    if let Err(secondary) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tracing::error!(
            panic = flui_foundation::panic::payload_text(&*payload),
            "contained owner callback or cleanup panic"
        );
    })) {
        flui_foundation::panic::discard_payload(secondary);
    }
    flui_foundation::panic::discard_payload(payload);
}

/// Keep an owner-affine resource whose final owner was dropped on another thread.
/// Retention precedes diagnostics: a subscriber panic must not retire the resource
/// on the wrong thread, including while the caller is already unwinding.
#[cfg(any(
    target_os = "windows",
    target_os = "macos",
    all(test, not(target_arch = "wasm32"))
))]
pub(crate) fn retain_off_owner_resource<T>(resource: T, report: impl FnOnce()) {
    #[expect(
        clippy::mem_forget,
        reason = "an owner-affine resource must not be destroyed off its owner thread"
    )]
    std::mem::forget(resource);
    contain_owner_callback(report);
}

// A panic on wasm32 aborts instead of unwinding, so there is nothing to catch.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use super::contain_owner_callback;

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
                "ivars" => self.0.push(format!("ivars: {value}")),
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

    fn off_owner_retention_case(fail_report: bool, already_unwinding: bool) {
        struct Resource(Arc<AtomicUsize>);
        impl Drop for Resource {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
                panic!("owner-affine resource retired on a foreign thread");
            }
        }
        struct Guard {
            resource: Option<(Resource, Resource)>,
            report_drops: Arc<AtomicUsize>,
            fail_report: bool,
        }
        impl Drop for Guard {
            fn drop(&mut self) {
                let report_drops = Arc::clone(&self.report_drops);
                let fail_report = self.fail_report;
                crate::owner::panic_boundary::retain_off_owner_resource(
                    self.resource.take().expect("resource"),
                    || {
                        // One failing payload field: a second one whose
                        // destructor panics while the first unwinds aborts,
                        // as in std (ADR-0281 §1).
                        if fail_report {
                            std::panic::panic_any(Resource(report_drops));
                        }
                    },
                );
            }
        }
        let resource_drops = Arc::new(AtomicUsize::new(0));
        let report_drops = Arc::new(AtomicUsize::new(0));
        let result = catch_unwind(AssertUnwindSafe(|| {
            let guard = Guard {
                resource: Some((
                    Resource(Arc::clone(&resource_drops)),
                    Resource(Arc::clone(&resource_drops)),
                )),
                report_drops: Arc::clone(&report_drops),
                fail_report,
            };
            assert!(!already_unwinding, "original teardown failure");
            drop(guard);
        }));
        // The resource stays off its owner thread; the report's payload is
        // discarded under containment (ADR-0281 §2).
        assert_eq!(resource_drops.load(Ordering::SeqCst), 0);
        assert_eq!(
            report_drops.load(Ordering::SeqCst),
            usize::from(fail_report)
        );
        if already_unwinding {
            assert_eq!(
                *result
                    .expect_err("original failure")
                    .downcast::<&str>()
                    .expect("original payload"),
                "original teardown failure"
            );
        } else {
            assert!(result.is_ok(), "report failure must remain contained");
        }
        let next = AtomicUsize::new(0);
        contain_owner_callback(|| {
            next.fetch_add(1, Ordering::SeqCst);
        });
        assert_eq!(next.load(Ordering::SeqCst), 1);
    }

    /// An owner-affine resource dropped off its owner thread stays off it,
    /// whether its report succeeds or panics and whether an unwind is already
    /// under way: the report's payload is discarded, an earlier unwind stays
    /// the one that propagates, and the next callback still runs.
    #[test]
    fn an_off_owner_resource_stays_off_its_owner_whatever_its_report_does() {
        for fail_report in [false, true] {
            for already_unwinding in [false, true] {
                off_owner_retention_case(fail_report, already_unwinding);
            }
        }
    }
}
