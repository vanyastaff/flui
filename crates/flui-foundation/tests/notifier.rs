//! Notification ownership and recovery, exercised through the public API.

use flui_foundation::{
    ChangeNotifier, Listenable, ListenerId, Notifier, ValueListenable, ValueNotifier,
};
use std::io::Read;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

struct Bomb {
    drops: Arc<AtomicUsize>,
    message: &'static str,
}
impl Drop for Bomb {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
        panic!("{}", self.message);
    }
}

fn non_clone_argument() {
    struct Argument(String);
    let notifier = Notifier::<Argument>::new();
    let observed = Arc::new(Mutex::new(String::new()));
    let listener_observed = Arc::clone(&observed);
    let id = notifier.add(Arc::new(move |argument| {
        listener_observed
            .lock()
            .expect("observation")
            .clone_from(&argument.0);
    }));
    let argument = Argument("borrowed value".into());
    let cloned = notifier.clone();
    cloned.notify(&argument);
    assert_eq!(*observed.lock().expect("observation"), argument.0);
    notifier.remove(id);
    observed.lock().expect("observation").clear();
    cloned.notify(&argument);
    assert!(observed.lock().expect("observation").is_empty());
}

fn borrowed_argument_type_accepts_a_non_static_reference() {
    // The reference inside Arg must remain valid until the typed notifier is
    // dropped, as it did before terminal ownership containment. The argument
    // itself is still borrowed for notification and is not a static string.
    let source = String::from("borrowed temporary argument");
    let notifier = Notifier::<&str>::new();
    let observed = Arc::new(Mutex::new(String::new()));
    let listener_observed = Arc::clone(&observed);
    notifier.add(Arc::new(move |argument| {
        listener_observed
            .lock()
            .expect("observation")
            .clone_from(&(*argument).to_owned());
    }));
    notifier.notify(&source.as_str());
    assert_eq!(
        observed.lock().expect("observation").as_str(),
        "borrowed temporary argument"
    );
    drop(notifier);
    drop(source);
}

fn non_clone_owned_value() {
    #[derive(Default, PartialEq)]
    struct OwnedValue(String);

    fn read(listenable: &impl ValueListenable<OwnedValue>) -> &str {
        &listenable.value().0
    }

    let mut notifier = ValueNotifier::new(OwnedValue("initial".into()));
    let calls = Arc::new(AtomicUsize::new(0));
    let listener_calls = Arc::clone(&calls);
    notifier.add_listener(Arc::new(move || {
        listener_calls.fetch_add(1, Ordering::SeqCst);
    }));
    assert_eq!(read(&notifier), "initial");
    notifier.set_value(OwnedValue("initial".into()));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    notifier.set_value(OwnedValue("changed".into()));
    let replaced = notifier.replace(OwnedValue("replacement".into()));
    assert_eq!(replaced.0, "changed");
    notifier.update(|value| value.0.push_str(" updated"));
    let taken = notifier.take();
    assert_eq!(taken.0, "replacement updated");
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    assert_eq!(read(&notifier), "");
    let extracted = notifier.into_value();
    assert_eq!(extracted.0, "");
}

fn cloneable_values_keep_independent_values_and_shared_listeners() {
    let original = ValueNotifier::new(1_u32);
    let mut cloned = original.clone();
    let calls = Arc::new(AtomicUsize::new(0));
    let listener_calls = Arc::clone(&calls);
    original.add_listener(Arc::new(move || {
        listener_calls.fetch_add(1, Ordering::SeqCst);
    }));
    cloned.set_value(2);
    assert_eq!(*original.value(), 1);
    assert_eq!(*cloned.value(), 2);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(cloned.into_value(), 2);
    assert!(
        original.is_empty(),
        "extraction disposes the shared channel"
    );
}

fn contained_failure(aggregate: bool, hostile_capture: bool) {
    let notifier = Notifier::<()>::new();
    let drops = Arc::new(AtomicUsize::new(0));
    let payload_drops = Arc::clone(&drops);
    let self_id = Arc::new(Mutex::new(None::<ListenerId>));
    let callback_id = Arc::clone(&self_id);
    let callback_notifier = notifier.clone();
    let captures = hostile_capture.then(|| {
        (
            Bomb {
                drops: Arc::clone(&drops),
                message: "first capture",
            },
            Bomb {
                drops: Arc::clone(&drops),
                message: "second capture",
            },
        )
    });
    let id = notifier.add(Arc::new(move |&()| {
        let _captures = &captures;
        callback_notifier.remove(callback_id.lock().expect("listener id").expect("installed"));
        if aggregate {
            std::panic::panic_any((
                Bomb {
                    drops: Arc::clone(&payload_drops),
                    message: "first payload",
                },
                Bomb {
                    drops: Arc::clone(&payload_drops),
                    message: "second payload",
                },
            ));
        }
        std::panic::panic_any(Bomb {
            drops: Arc::clone(&payload_drops),
            message: "payload",
        });
    }));
    *self_id.lock().expect("listener id") = Some(id);
    let later = Arc::new(AtomicUsize::new(0));
    let listener_later = Arc::clone(&later);
    notifier.add(Arc::new(move |&()| {
        listener_later.fetch_add(1, Ordering::SeqCst);
    }));
    notifier.notify(&());
    assert_eq!(later.load(Ordering::SeqCst), 1);
    assert_eq!(
        drops.load(Ordering::SeqCst),
        0,
        "opaque failure obligations stay retained"
    );
    notifier.notify(&());
    assert_eq!(
        later.load(Ordering::SeqCst),
        2,
        "the next notification still progresses"
    );
}

struct HostileSubscriber {
    calls: Arc<AtomicUsize>,
    drops: Arc<AtomicUsize>,
}
impl tracing::Subscriber for HostileSubscriber {
    fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
        *metadata.level() == tracing::Level::ERROR
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, _: &tracing::Event<'_>) {
        self.calls.fetch_add(1, Ordering::SeqCst);
        std::panic::panic_any((
            Bomb {
                drops: Arc::clone(&self.drops),
                message: "first telemetry payload",
            },
            Bomb {
                drops: Arc::clone(&self.drops),
                message: "second telemetry payload",
            },
        ));
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

fn telemetry_competition() {
    let calls = Arc::new(AtomicUsize::new(0));
    let drops = Arc::new(AtomicUsize::new(0));
    tracing::subscriber::with_default(
        HostileSubscriber {
            calls: Arc::clone(&calls),
            drops: Arc::clone(&drops),
        },
        || contained_failure(true, true),
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        drops.load(Ordering::SeqCst),
        0,
        "secondary telemetry payloads stay retained"
    );
}

fn retirement_competition() {
    let notifier = Notifier::<()>::new();
    let drops = Arc::new(AtomicUsize::new(0));
    for message in ["first retirement", "second retirement"] {
        let self_id = Arc::new(Mutex::new(None::<ListenerId>));
        let callback_id = Arc::clone(&self_id);
        let callback_notifier = notifier.clone();
        let capture = Bomb {
            drops: Arc::clone(&drops),
            message,
        };
        let id = notifier.add(Arc::new(move |&()| {
            let _capture = &capture;
            callback_notifier.remove(callback_id.lock().expect("listener id").expect("installed"));
        }));
        *self_id.lock().expect("listener id") = Some(id);
    }
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| notifier.notify(&())))
        .expect_err("ordinary retirement propagates its first failure");
    assert_eq!(
        flui_foundation::panic::payload_text(&*failure),
        Some("first retirement")
    );
    assert_eq!(
        drops.load(Ordering::SeqCst),
        1,
        "the second envelope is retained"
    );
    assert!(notifier.is_empty());
    let later = Arc::new(AtomicUsize::new(0));
    let listener_later = Arc::clone(&later);
    notifier.add(Arc::new(move |&()| {
        listener_later.fetch_add(1, Ordering::SeqCst);
    }));
    notifier.notify(&());
    assert_eq!(later.load(Ordering::SeqCst), 1);
}

fn install_retirement_bombs(notifier: &Notifier<()>, drops: &Arc<AtomicUsize>) -> ListenerId {
    let mut first = None;
    for message in ["first terminal retirement", "second terminal retirement"] {
        let capture = Bomb {
            drops: Arc::clone(drops),
            message,
        };
        let id = notifier.add(Arc::new(move |&()| {
            let _capture = &capture;
        }));
        first.get_or_insert(id);
    }
    first.expect("two installed callbacks")
}

fn assert_first_terminal_failure(failure: Box<dyn std::any::Any + Send>, drops: &AtomicUsize) {
    assert_eq!(
        flui_foundation::panic::payload_text(&*failure),
        Some("first terminal retirement")
    );
    assert_eq!(
        drops.load(Ordering::SeqCst),
        1,
        "the later envelope is retained"
    );
    flui_foundation::panic::retain_opaque_payload(failure);
}

fn terminal_retirement_competition() {
    let notifier = Notifier::<()>::new();
    let drops = Arc::new(AtomicUsize::new(0));
    install_retirement_bombs(&notifier, &drops);
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(notifier)))
        .expect_err("final physical owner propagates first retirement failure");
    assert_first_terminal_failure(failure, &drops);

    let notifier = ChangeNotifier::new();
    let drops = Arc::new(AtomicUsize::new(0));
    for message in ["first terminal retirement", "second terminal retirement"] {
        let capture = Bomb {
            drops: Arc::clone(&drops),
            message,
        };
        notifier.add_listener(Arc::new(move || {
            let _capture = &capture;
        }));
    }
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(notifier)))
        .expect_err("ChangeNotifier delegates final ownership retirement");
    assert_first_terminal_failure(failure, &drops);

    for dispose in [false, true] {
        let notifier = Notifier::<()>::new();
        let drops = Arc::new(AtomicUsize::new(0));
        install_retirement_bombs(&notifier, &drops);
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if dispose {
                notifier.dispose();
            } else {
                notifier.remove_all();
            }
        }))
        .expect_err("explicit clearing propagates first retirement failure");
        assert_first_terminal_failure(failure, &drops);
        assert!(notifier.is_empty(), "ownership committed before retirement");
        assert_eq!(notifier.is_disposed(), dispose);
        if dispose {
            notifier.dispose();
        } else {
            let calls = Arc::new(AtomicUsize::new(0));
            let observed = Arc::clone(&calls);
            notifier.add(Arc::new(move |&()| {
                observed.fetch_add(1, Ordering::SeqCst);
            }));
            notifier.notify(&());
            assert_eq!(calls.load(Ordering::SeqCst), 1, "next round progresses");
        }
    }
}

fn terminal_retirement_preserves_incoming_unwind() {
    enum Cleanup {
        FinalOwner,
        Clear,
        Dispose,
        Remove(ListenerId),
    }
    struct Owner {
        notifier: Notifier<()>,
        cleanup: Cleanup,
    }
    impl Drop for Owner {
        fn drop(&mut self) {
            match self.cleanup {
                Cleanup::FinalOwner => {}
                Cleanup::Clear => self.notifier.remove_all(),
                Cleanup::Dispose => self.notifier.dispose(),
                Cleanup::Remove(id) => self.notifier.remove(id),
            }
        }
    }
    for operation in 0..4 {
        let notifier = Notifier::<()>::new();
        let drops = Arc::new(AtomicUsize::new(0));
        let first = install_retirement_bombs(&notifier, &drops);
        let cleanup = match operation {
            0 => Cleanup::FinalOwner,
            1 => Cleanup::Clear,
            2 => Cleanup::Dispose,
            _ => Cleanup::Remove(first),
        };
        let owner = Owner { notifier, cleanup };
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _owner = owner;
            panic!("incoming failure");
        }))
        .expect_err("original unwind escapes unchanged");
        assert_eq!(
            flui_foundation::panic::payload_text(&*failure),
            Some("incoming failure")
        );
        assert_eq!(
            drops.load(Ordering::SeqCst),
            0,
            "opaque captures remain retained"
        );
        flui_foundation::panic::retain_opaque_payload(failure);
    }
}

fn healthy_terminal_retirement_waits_for_the_last_owner() {
    struct Capture {
        order: Arc<Mutex<Vec<usize>>>,
        index: usize,
    }
    impl Drop for Capture {
        fn drop(&mut self) {
            self.order.lock().expect("drop order").push(self.index);
        }
    }
    let notifier = Notifier::<()>::new();
    let order = Arc::new(Mutex::new(Vec::new()));
    for index in 0..3 {
        let capture = Capture {
            order: Arc::clone(&order),
            index,
        };
        notifier.add(Arc::new(move |&()| {
            let _capture = &capture;
        }));
    }
    let last_owner = notifier.clone();
    drop(notifier);
    assert!(order.lock().expect("drop order").is_empty());
    drop(last_owner);
    assert_eq!(*order.lock().expect("drop order"), [0, 1, 2]);
}

fn value_terminal_retirement_preserves_the_first_failure() {
    for extract in [false, true] {
        let value_drops = Arc::new(AtomicUsize::new(0));
        let listener_drops = Arc::new(AtomicUsize::new(0));
        let notifier = ValueNotifier::new(Bomb {
            drops: Arc::clone(&value_drops),
            message: "value retirement failure",
        });
        let capture = Bomb {
            drops: Arc::clone(&listener_drops),
            message: "listener retirement failure",
        };
        notifier.add_listener(Arc::new(move || {
            let _capture = &capture;
        }));
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            if extract {
                let _value = notifier.into_value();
            } else {
                drop(notifier);
            }
        }))
        .expect_err("the first independent owner fails");
        assert_eq!(
            flui_foundation::panic::payload_text(&*failure),
            Some(if extract {
                "listener retirement failure"
            } else {
                "value retirement failure"
            })
        );
        assert_eq!(value_drops.load(Ordering::SeqCst), usize::from(!extract));
        assert_eq!(listener_drops.load(Ordering::SeqCst), usize::from(extract));
        flui_foundation::panic::retain_opaque_payload(failure);
    }
}

fn value_retirement_failure_releases_the_shared_channel() {
    #[derive(Clone)]
    struct Value {
        fail: bool,
    }
    impl Drop for Value {
        fn drop(&mut self) {
            assert!(!self.fail, "value retirement failure");
        }
    }
    let mut notifier = ValueNotifier::new(Value { fail: false });
    let alias = notifier.clone();
    *notifier.value_mut() = Value { fail: true };
    let calls = Arc::new(AtomicUsize::new(0));
    let listener_drops = Arc::new(AtomicUsize::new(0));
    let capture = Counted(Arc::clone(&listener_drops));
    let observed = Arc::clone(&calls);
    notifier.add_listener(Arc::new(move || {
        let _capture = &capture;
        observed.fetch_add(1, Ordering::SeqCst);
    }));
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || drop(notifier)))
        .expect_err("the value's destructor failure propagates");
    assert_eq!(
        flui_foundation::panic::payload_text(&*failure),
        Some("value retirement failure")
    );
    flui_foundation::panic::retain_opaque_payload(failure);
    alias.notify();
    assert_eq!(calls.load(Ordering::SeqCst), 1, "the clone still notifies");
    assert_eq!(listener_drops.load(Ordering::SeqCst), 0);
    drop(alias);
    assert_eq!(
        listener_drops.load(Ordering::SeqCst),
        1,
        "the surviving clone is the last channel owner"
    );
}

struct Counted(Arc<AtomicUsize>);

impl Drop for Counted {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn value_terminal_retirement_preserves_incoming_unwind() {
    let value_drops = Arc::new(AtomicUsize::new(0));
    let listener_drops = Arc::new(AtomicUsize::new(0));
    let notifier = ValueNotifier::new(Bomb {
        drops: Arc::clone(&value_drops),
        message: "value retirement failure",
    });
    let capture = Bomb {
        drops: Arc::clone(&listener_drops),
        message: "listener retirement failure",
    };
    notifier.add_listener(Arc::new(move || {
        let _capture = &capture;
    }));
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let _notifier = notifier;
        panic!("incoming value-owner failure");
    }))
    .expect_err("incoming failure escapes without callback or value retirement");
    assert_eq!(
        flui_foundation::panic::payload_text(&*failure),
        Some("incoming value-owner failure")
    );
    assert_eq!(value_drops.load(Ordering::SeqCst), 0);
    assert_eq!(listener_drops.load(Ordering::SeqCst), 0);
    flui_foundation::panic::retain_opaque_payload(failure);
}

fn healthy_value_extraction_preserves_reentry_and_shared_channel_disposal() {
    #[derive(Clone)]
    struct Value {
        drops: Arc<AtomicUsize>,
    }
    impl Drop for Value {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::SeqCst);
        }
    }
    struct ReentrantCapture {
        observer: ValueNotifier<Value>,
        id: Arc<Mutex<Option<ListenerId>>>,
        calls: Arc<AtomicUsize>,
    }
    impl Drop for ReentrantCapture {
        fn drop(&mut self) {
            assert!(
                self.observer.is_empty(),
                "channel emptied before captures retire"
            );
            self.observer.remove_listener(
                self.id
                    .lock()
                    .expect("listener id")
                    .expect("registered listener"),
            );
            self.calls.fetch_add(1, Ordering::SeqCst);
        }
    }
    let value_drops = Arc::new(AtomicUsize::new(0));
    let reentries = Arc::new(AtomicUsize::new(0));
    let notifier = ValueNotifier::new(Value {
        drops: Arc::clone(&value_drops),
    });
    let alias = notifier.clone();
    let id = Arc::new(Mutex::new(None));
    let capture = ReentrantCapture {
        observer: alias.clone(),
        id: Arc::clone(&id),
        calls: Arc::clone(&reentries),
    };
    let installed = notifier.add_listener(Arc::new(move || {
        let _capture = &capture;
    }));
    *id.lock().expect("listener id") = Some(installed);
    let extracted = notifier.into_value();
    assert_eq!(reentries.load(Ordering::SeqCst), 1);
    assert_eq!(
        value_drops.load(Ordering::SeqCst),
        1,
        "capture's independent clone retired"
    );
    assert!(alias.is_empty(), "extraction disposes the shared channel");
    assert!(Arc::ptr_eq(&alias.value().drops, &extracted.drops));
    drop(alias);
    assert_eq!(value_drops.load(Ordering::SeqCst), 2);
    drop(extracted);
    assert_eq!(
        value_drops.load(Ordering::SeqCst),
        3,
        "returned value retires under caller ownership"
    );

    let normal = ValueNotifier::new(Value {
        drops: Arc::clone(&value_drops),
    });
    drop(normal);
    assert_eq!(
        value_drops.load(Ordering::SeqCst),
        4,
        "healthy final owner still destroys T"
    );
}

#[test]
fn notifier_ownership_and_recovery() {
    const CHILD: &str = "FLUI_NOTIFIER_RECOVERY_CASE";
    if let Ok(case) = std::env::var(CHILD) {
        match case.as_str() {
            "non_clone_argument" => non_clone_argument(),
            "borrowed_argument_type_accepts_a_non_static_reference" => {
                borrowed_argument_type_accepts_a_non_static_reference();
            }
            "non_clone_owned_value" => non_clone_owned_value(),
            "cloneable_values_keep_independent_values_and_shared_listeners" => {
                cloneable_values_keep_independent_values_and_shared_listeners();
            }
            "single_payload" => contained_failure(false, false),
            "aggregate_payload" => contained_failure(true, false),
            "capture_and_payload" => contained_failure(true, true),
            "telemetry_competition" => telemetry_competition(),
            "retirement_competition" => retirement_competition(),
            "terminal_retirement_competition" => terminal_retirement_competition(),
            "terminal_retirement_preserves_incoming_unwind" => {
                terminal_retirement_preserves_incoming_unwind();
            }
            "healthy_terminal_retirement_waits_for_the_last_owner" => {
                healthy_terminal_retirement_waits_for_the_last_owner();
            }
            "value_terminal_retirement_preserves_the_first_failure" => {
                value_terminal_retirement_preserves_the_first_failure();
            }
            "value_retirement_failure_releases_the_shared_channel" => {
                value_retirement_failure_releases_the_shared_channel();
            }
            "value_terminal_retirement_preserves_incoming_unwind" => {
                value_terminal_retirement_preserves_incoming_unwind();
            }
            "healthy_value_extraction_preserves_reentry_and_shared_channel_disposal" => {
                healthy_value_extraction_preserves_reentry_and_shared_channel_disposal();
            }
            _ => panic!("unknown child case"),
        }
        return;
    }
    let mut failures = Vec::new();
    for case in [
        "non_clone_argument",
        "borrowed_argument_type_accepts_a_non_static_reference",
        "non_clone_owned_value",
        "cloneable_values_keep_independent_values_and_shared_listeners",
        "single_payload",
        "aggregate_payload",
        "capture_and_payload",
        "telemetry_competition",
        "retirement_competition",
        "terminal_retirement_competition",
        "terminal_retirement_preserves_incoming_unwind",
        "healthy_terminal_retirement_waits_for_the_last_owner",
        "value_terminal_retirement_preserves_the_first_failure",
        "value_retirement_failure_releases_the_shared_channel",
        "value_terminal_retirement_preserves_incoming_unwind",
        "healthy_value_extraction_preserves_reentry_and_shared_channel_disposal",
    ] {
        let mut child =
            std::process::Command::new(std::env::current_exe().expect("test executable"))
                .args([
                    "--exact",
                    "notifier::notifier_ownership_and_recovery",
                    "--nocapture",
                ])
                .env(CHILD, case)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .expect("start isolated ownership case");
        // Drain both pipes while the child runs: panic backtraces must not fill
        // a pipe and turn the parent's timeout wait into a writer deadlock.
        let mut child_stdout = child.stdout.take().expect("piped child stdout");
        let stdout_reader = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            child_stdout
                .read_to_end(&mut bytes)
                .expect("read child stdout");
            bytes
        });
        let mut child_stderr = child.stderr.take().expect("piped child stderr");
        let stderr_reader = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            child_stderr
                .read_to_end(&mut bytes)
                .expect("read child stderr");
            bytes
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        let mut timed_out = false;
        let status = loop {
            if let Some(status) = child.try_wait().expect("poll ownership child") {
                break status;
            }
            if std::time::Instant::now() >= deadline {
                timed_out = true;
                // The child may have exited between the poll and kill. Reap
                // either outcome before joining readers or advancing the table.
                let _ = child.kill();
                break child.wait().expect("reap timed-out ownership child");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        let stdout_bytes = stdout_reader.join().expect("join child stdout reader");
        let stderr_bytes = stderr_reader.join().expect("join child stderr reader");
        let stdout = String::from_utf8_lossy(&stdout_bytes);
        if timed_out || !status.success() || !stdout.contains("1 passed; 0 failed") {
            failures.push(format!(
                "{case}: {status}, timed_out={timed_out}\n{stdout}\n{}",
                String::from_utf8_lossy(&stderr_bytes)
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
