//! Deterministic `tracing` capture for tests that assert on what was logged.
//!
//! # Why this exists
//!
//! The obvious way to capture events — build a subscriber and install it with
//! [`tracing::subscriber::with_default`] — is **not** race-free in a
//! thread-parallel test binary, and the failure is silent: the capturing test
//! observes an empty log and fails, or worse, asserts `== 0` and passes
//! vacuously.
//!
//! The reason is in `tracing-core`. A callsite's `Interest` is computed **once**,
//! on whichever thread first reaches it, and then cached **process-globally**:
//!
//! ```text
//! // tracing_core::callsite, std build
//! fn rebuilder(&self) -> Rebuilder<'_> {
//!     if self.has_just_one.load(SeqCst) { return Rebuilder::JustOne; }
//!     Rebuilder::Read(LOCKED_DISPATCHERS.read().unwrap())
//! }
//! // Rebuilder::JustOne::for_each → dispatcher::get_default(f)
//! ```
//!
//! `get_default` reads the *calling thread's* dispatcher. So when one test
//! installs a thread-local capturing subscriber and a **different** test on
//! another thread reaches the same `warn!`/`error!` first, that callsite's
//! interest is computed against `NoSubscriber`, cached as `Interest::never()`,
//! and the capturing test's event is dropped before dispatch ever happens.
//!
//! This was not hypothetical. `flui-widgets`' parity suite carried two
//! hand-rolled copies of the `with_default` technique, both documenting the
//! caveat and neither able to fix it — one of them,
//! `grid_view_builder_unbounded_truncation_warns_once_not_every_frame`, failed
//! **4 times in 25** runs of the `parity` binary while passing 60/60 in
//! isolation, poisoned by a sibling test that mounts an identical tree and
//! reaches the same warning first.
//!
//! # How this fixes it
//!
//! [`capture`] installs **one process-global subscriber**, once, whose
//! [`register_callsite`](Subscriber::register_callsite) unconditionally returns
//! [`Interest::sometimes`]. No callsite can then be cached as `never`, whichever
//! thread reaches it first, because every thread's default dispatcher is this
//! subscriber. Whether an event is actually recorded is decided per event by
//! [`enabled`](Subscriber::enabled), which checks a **thread-local** sink — so
//! two threads can capture concurrently without seeing each other's events, and
//! a thread with no active capture pays one thread-local read and drops the
//! event.
//!
//! Installation also heals a callsite poisoned earlier in the same process:
//! registering a dispatcher rebuilds the interest of every callsite already
//! registered. So there is no ordering requirement on the first [`capture`]
//! call.
//!
//! # Limits
//!
//! - The process-global default slot is claimed on first use. A binary whose
//!   tests install their own global subscriber cannot also use this; [`capture`]
//!   panics with that diagnosis rather than silently capturing nothing.
//! - A crate at or below `flui-interaction` in the layer DAG cannot depend on
//!   this crate, so its in-`src` capture tests keep their own technique and
//!   their own caveat.
//! - Span *fields* are not recorded — only events. Assertions here are about
//!   what was logged, not about span structure.

use std::cell::RefCell;
use std::fmt::Write as _;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::subscriber::Interest;
use tracing::{Event, Level, Metadata, Subscriber};

thread_local! {
    /// The active capture buffer for this thread, if any.
    ///
    /// `Some` exactly for the dynamic extent of a [`capture`] call. Its
    /// presence is what [`CaptureSubscriber::enabled`] answers, so capture is
    /// per-thread even though the subscriber is process-global.
    static SINK: RefCell<Option<Vec<CapturedRecord>>> = const { RefCell::new(None) };
}

/// One captured `tracing` event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedRecord {
    /// The event's level.
    pub level: Level,
    /// The event's target (usually the emitting module path).
    pub target: String,
    /// The `message` field, empty when the event carried none.
    pub message: String,
    /// Every other field, in emission order, formatted with `Debug`.
    pub fields: Vec<(String, String)>,
}

impl CapturedRecord {
    /// Whether the message or any field value contains `needle`.
    #[must_use]
    pub fn contains(&self, needle: &str) -> bool {
        self.message.contains(needle) || self.fields.iter().any(|(_, value)| value.contains(needle))
    }

    /// The value of the field named `name`, if the event carried one.
    #[must_use]
    pub fn field(&self, name: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value.as_str())
    }
}

impl std::fmt::Display for CapturedRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:>5} {}: {}", self.level, self.target, self.message)?;
        for (name, value) in &self.fields {
            write!(f, " {name}={value}")?;
        }
        Ok(())
    }
}

/// The events a [`capture`] call observed, in emission order.
///
/// Assert against the structured accessors rather than substring-matching a
/// rendered dump: [`count_containing`](Self::count_containing) says what a
/// "once, not once per frame" test actually means, and [`Display`] renders the
/// whole log for the failure message when it does not hold.
///
/// [`Display`]: std::fmt::Display
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CapturedLog {
    records: Vec<CapturedRecord>,
}

impl CapturedLog {
    /// Every captured event, in emission order.
    #[must_use]
    pub fn records(&self) -> &[CapturedRecord] {
        &self.records
    }

    /// Number of captured events.
    #[must_use]
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Whether nothing was captured.
    ///
    /// A capturing test should treat this as a **vacuous-pass guard**: an empty
    /// log usually means the code under test never ran, not that it stayed
    /// quiet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// How many captured events mention `needle` in their message or any field.
    #[must_use]
    pub fn count_containing(&self, needle: &str) -> usize {
        self.records
            .iter()
            .filter(|record| record.contains(needle))
            .count()
    }

    /// Whether any captured event mentions `needle`.
    #[must_use]
    pub fn contains(&self, needle: &str) -> bool {
        self.count_containing(needle) > 0
    }

    /// The captured events at exactly `level`.
    pub fn at_level(&self, level: Level) -> impl Iterator<Item = &CapturedRecord> {
        self.records
            .iter()
            .filter(move |record| record.level == level)
    }

    /// The captured events at `level` or more severe, one per line.
    ///
    /// For an assertion message: the full [`Display`] of a captured frame is
    /// mostly `DEBUG` tree-walk chatter, and a failure about a warning or an
    /// error reads better without it.
    ///
    /// [`Display`]: std::fmt::Display
    #[must_use]
    pub fn render_at_least(&self, level: Level) -> String {
        // `tracing::Level` orders ERROR lowest, so "at least as severe" is `<=`.
        let mut out = String::new();
        for record in self.records.iter().filter(|r| r.level <= level) {
            let _ = writeln!(out, "{record}");
        }
        if out.is_empty() {
            out.push_str("<no events at or above ");
            let _ = write!(out, "{level}>");
        }
        out
    }
}

impl std::fmt::Display for CapturedLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.records.is_empty() {
            return f.write_str("<no events captured>");
        }
        for record in &self.records {
            writeln!(f, "{record}")?;
        }
        Ok(())
    }
}

/// Collects an event's fields, separating out the conventional `message`.
struct RecordVisitor {
    message: String,
    fields: Vec<(String, String)>,
}

impl Visit for RecordVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            // The `message` field is formatted, not Debug-quoted, exactly as a
            // human-facing sink renders it.
            let _ = write!(self.message, "{value:?}");
        } else {
            self.fields
                .push((field.name().to_string(), format!("{value:?}")));
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message.push_str(value);
        } else {
            self.fields
                .push((field.name().to_string(), value.to_string()));
        }
    }
}

/// The process-global subscriber. Always *interested*; never *enabled* unless
/// the current thread is inside a [`capture`] call.
struct CaptureSubscriber;

impl Subscriber for CaptureSubscriber {
    /// The whole point: an unconditional `sometimes` keeps every callsite's
    /// cached interest permissive, so the per-event `enabled` check below is
    /// what decides, on the thread that actually emits.
    fn register_callsite(&self, _metadata: &'static Metadata<'static>) -> Interest {
        Interest::sometimes()
    }

    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        // `try_with`: during thread teardown the local is already destroyed,
        // and a late event must not panic in a destructor.
        SINK.try_with(|sink| sink.borrow().is_some())
            .unwrap_or(false)
    }

    fn new_span(&self, _span: &Attributes<'_>) -> Id {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        // Ids must be non-zero; wrapping past u64::MAX is not reachable in a
        // test process, and `max(1)` keeps it total anyway.
        Id::from_u64(NEXT.fetch_add(1, Ordering::Relaxed).max(1))
    }

    fn record(&self, _span: &Id, _values: &Record<'_>) {}

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let mut visitor = RecordVisitor {
            message: String::new(),
            fields: Vec::new(),
        };
        event.record(&mut visitor);
        let metadata = event.metadata();
        let record = CapturedRecord {
            level: *metadata.level(),
            target: metadata.target().to_string(),
            message: visitor.message,
            fields: visitor.fields,
        };
        let _ = SINK.try_with(|sink| {
            if let Some(records) = sink.borrow_mut().as_mut() {
                records.push(record);
            }
        });
    }

    fn enter(&self, _span: &Id) {}

    fn exit(&self, _span: &Id) {}
}

/// Claim the process-global default subscriber slot, once.
fn install() {
    static INSTALLED: OnceLock<()> = OnceLock::new();
    INSTALLED.get_or_init(|| {
        tracing::subscriber::set_global_default(CaptureSubscriber).expect(
            "flui_testing::log_capture needs the process-global default subscriber slot, \
             and something else in this test binary already claimed it. Capture cannot be \
             made race-free without it — see this module's docs — so route that other \
             subscriber through `capture` instead of installing it globally.",
        );
    });
}

/// Restores the previous (absent) sink even if `body` panics.
struct SinkGuard;

impl Drop for SinkGuard {
    fn drop(&mut self) {
        let _ = SINK.try_with(|sink| sink.borrow_mut().take());
    }
}

/// Run `body`, capturing every `tracing` event it emits **on this thread**.
///
/// Race-free in a thread-parallel test binary — see the module docs for why
/// the obvious `with_default` version is not. Two threads may capture at the
/// same time without seeing each other's events.
///
/// # Panics
///
/// - If a capture is already active on this thread. Nesting would silently
///   split one log across two buffers.
/// - If another subscriber already owns the process-global default slot.
///
/// A panic inside `body` propagates, and the sink is torn down on the way out.
///
/// # Example
///
/// ```rust,ignore
/// let (laid, log) = flui_testing::log_capture::capture(|| lay_out(root, constraints));
/// assert!(!log.is_empty(), "vacuous-pass guard: the tree must have logged something");
/// assert_eq!(log.count_containing("unbounded main axis declares"), 1, "{log}");
/// ```
pub fn capture<R>(body: impl FnOnce() -> R) -> (R, CapturedLog) {
    install();

    SINK.with(|sink| {
        let mut sink = sink.borrow_mut();
        assert!(
            sink.is_none(),
            "a tracing capture is already active on this thread; nesting would split \
             one log across two buffers",
        );
        *sink = Some(Vec::new());
    });
    let guard = SinkGuard;

    let result = body();

    let records = SINK
        .with(|sink| sink.borrow_mut().take())
        .unwrap_or_default();
    drop(guard);

    (result, CapturedLog { records })
}
