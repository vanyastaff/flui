//! Notification ownership and recovery, exercised through the public API.

use flui_foundation::{ListenerId, ListenerRegistry, Notifier};
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
    let registry = ListenerRegistry::<Argument>::new();
    let subscription = registry.add_status_listener(Arc::new(|argument| {
        assert_eq!(argument.0, "borrowed value");
    }));
    registry.notify_status(&argument);
    drop(subscription);
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
            "single_payload" => contained_failure(false, false),
            "aggregate_payload" => contained_failure(true, false),
            "capture_and_payload" => contained_failure(true, true),
            "retirement_competition" => retirement_competition(),
            _ => panic!("unknown child case"),
        }
        return;
    }
    let mut failures = Vec::new();
    for case in [
        "non_clone_argument",
        "single_payload",
        "aggregate_payload",
        "capture_and_payload",
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
