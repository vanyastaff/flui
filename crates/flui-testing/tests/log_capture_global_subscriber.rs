//! `capture` composes with a binary that owns the process-global subscriber.
//!
//! Its own `[[test]]` target, per this crate's convention: it installs a
//! process-global default subscriber, which is exactly the state a test must
//! not leave behind for its neighbours.
//!
//! The property under test is what makes this capture usable at all in a binary
//! that also exercises application logging: [`capture`] must not claim the
//! global slot. It disarms `tracing`'s per-callsite interest cache with
//! registered-but-never-default sentinel dispatchers and then installs its own
//! subscriber only for the current thread, so the binary's subscriber keeps
//! receiving everything emitted outside a capture.

use std::sync::{Mutex, OnceLock};

use flui_testing::log_capture::capture;
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};

/// Messages the binary's own global subscriber received.
fn global_lines() -> &'static Mutex<Vec<String>> {
    static LINES: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    LINES.get_or_init(|| Mutex::new(Vec::new()))
}

/// Stands in for whatever subscriber a binary installs for itself.
struct BinarySubscriber;

impl Subscriber for BinarySubscriber {
    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _span: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _span: &Id, _values: &Record<'_>) {}

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn event(&self, event: &Event<'_>) {
        struct Message(String);
        impl tracing::field::Visit for Message {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                if field.name() == "message" {
                    self.0 = format!("{value:?}");
                }
            }
        }
        let mut message = Message(String::new());
        event.record(&mut message);
        global_lines()
            .lock()
            .expect("global-line mutex")
            .push(message.0);
    }

    fn enter(&self, _span: &Id) {}

    fn exit(&self, _span: &Id) {}
}

fn global_saw(needle: &str) -> bool {
    global_lines()
        .lock()
        .expect("global-line mutex")
        .iter()
        .any(|line| line.contains(needle))
}

#[test]
fn capture_leaves_the_global_slot_to_the_binary() {
    // The binary claims the global slot FIRST. Under the previous design —
    // `capture` calling `set_global_default` itself — this ordering made the
    // first capture panic, and the reverse ordering made this call panic.
    tracing::subscriber::set_global_default(BinarySubscriber)
        .expect("this target owns the global slot; capture must not have taken it");

    tracing::warn!("before the capture");
    assert!(
        global_saw("before the capture"),
        "precondition: the binary's own subscriber is receiving events",
    );

    let ((), log) = capture(|| tracing::warn!("inside the capture"));

    assert!(
        log.contains("inside the capture"),
        "capture must still record on its own thread; captured:\n{log}",
    );
    assert!(
        !global_saw("inside the capture"),
        "a thread-local capture overrides the global subscriber for its own \
         extent — that is what capturing means",
    );

    // The decisive one: the binary's subscriber is intact afterwards. A capture
    // that had claimed the global slot would swallow this.
    tracing::warn!("after the capture");
    assert!(
        global_saw("after the capture"),
        "events outside a capture must keep reaching the binary's subscriber",
    );
}
