//! The boundary every OS entry goes through: an entry's panic is logged with
//! its text and arguments and never leaves it, its answer is what the entry
//! said it would be, and the ivars of a class are destroyed inside it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use flui_foundation::panic::{AfterPanic, ContainedIvars, contain_os_entry, os_entry};

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

/// A class's ivars whose destructor panics are destroyed inside the
/// boundary: the panic is logged and does not reach objc2's `dealloc`,
/// and what the ivars hold after the failing field is still destroyed.
/// Red-check: destroy the value without the boundary.
#[test]
fn contained_ivars_keep_a_destructors_panic_inside_the_boundary() {
    let (ivars, destroyed) = failing_ivars();
    assert!(
        !ivars.captures.0.load(Ordering::SeqCst),
        "the ivars are read through the wrapper, untouched"
    );

    let (recorder, seen) = Recorder::recording();
    tracing::subscriber::with_default(recorder, || drop(ivars));
    assert_eq!(
        *seen.lock().expect("the recorder is not poisoned"),
        [
            "logged: a destructor's failure".to_owned(),
            format!("ivars: {}", std::any::type_name::<Ivars>()),
        ],
        "the failure names the class's ivars"
    );
    assert!(
        destroyed.load(Ordering::SeqCst),
        "the captures after the failing field are destroyed"
    );
}

/// A subscriber that panics on the diagnostics after an ivars' destructor
/// panicked leaves the teardown complete and the process running: each
/// log after the caught panic is contained, as the first one is.
/// Red-check: log the ivars' type without its own containment (the
/// answer's panic ends the process).
#[test]
fn contained_ivars_survive_a_subscriber_that_panics() {
    let (ivars, destroyed) = failing_ivars();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorder = Recorder {
        panics: true,
        seen: Arc::clone(&seen),
    };
    tracing::subscriber::with_default(recorder, || drop(ivars));
    assert!(
        destroyed.load(Ordering::SeqCst),
        "the captures after the failing field are destroyed"
    );
    assert!(
        seen.lock()
            .expect("the recorder is not poisoned")
            .is_empty(),
        "no event was recorded: the subscriber panicked on each"
    );
}

/// A destructor that panics.
struct Fails;

impl Drop for Fails {
    fn drop(&mut self) {
        panic!("a destructor's failure");
    }
}

/// Ivars whose first field's destructor panics, and whose second records
/// that it was destroyed.
struct Ivars {
    _handler: Fails,
    captures: Payload,
}

fn failing_ivars() -> (ContainedIvars<Ivars>, Arc<AtomicBool>) {
    let destroyed = Arc::new(AtomicBool::new(false));
    let ivars = ContainedIvars::new(Ivars {
        _handler: Fails,
        captures: Payload(Arc::clone(&destroyed)),
    });
    (ivars, destroyed)
}

/// A panic payload whose destructor is user code: it records that it ran.
struct Payload(Arc<AtomicBool>);

impl Drop for Payload {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// An entry's answer passes through; a panic is `None` and an opaque
/// payload is dropped inside the entry, not handed to the OS. Red-check:
/// retain the payload instead of dropping it.
#[test]
fn an_os_entry_contains_its_panic_and_drops_its_payload() {
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
        destroyed.load(Ordering::SeqCst),
        "the payload is dropped inside the entry"
    );
}
