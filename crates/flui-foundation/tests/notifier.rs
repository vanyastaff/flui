//! Notification ownership and recovery, exercised through the public API.

use flui_foundation::{Listenable, ListenerId, Notifier, ValueListenable, ValueNotifier};
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

#[test]
fn notifier_ownership_and_recovery() {
    const CHILD: &str = "FLUI_NOTIFIER_RECOVERY_CASE";
    if let Ok(case) = std::env::var(CHILD) {
        match case.as_str() {
            "non_clone_argument" => non_clone_argument(),
            "non_clone_owned_value" => non_clone_owned_value(),
            "cloneable_values_keep_independent_values_and_shared_listeners" => {
                cloneable_values_keep_independent_values_and_shared_listeners();
            }
            "single_payload" => contained_failure(false, false),
            "aggregate_payload" => contained_failure(true, false),
            "capture_and_payload" => contained_failure(true, true),
            "telemetry_competition" => telemetry_competition(),
            "retirement_competition" => retirement_competition(),
            _ => panic!("unknown child case"),
        }
        return;
    }
    let mut failures = Vec::new();
    for case in [
        "non_clone_argument",
        "non_clone_owned_value",
        "cloneable_values_keep_independent_values_and_shared_listeners",
        "single_payload",
        "aggregate_payload",
        "capture_and_payload",
        "telemetry_competition",
        "retirement_competition",
    ] {
        let output = std::process::Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "notifier::notifier_ownership_and_recovery",
                "--nocapture",
            ])
            .env(CHILD, case)
            .output()
            .expect("start isolated ownership case");
        let stdout = String::from_utf8_lossy(&output.stdout);
        if !output.status.success() || !stdout.contains("1 passed; 0 failed") {
            failures.push(format!(
                "{case}: {}\n{stdout}\n{}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
