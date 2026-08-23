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
//! Not by claiming the process-global default subscriber — that would evict a
//! binary's own logging setup and swallow every event emitted outside a
//! capture. The fix works one level down, on how that cache is computed.
//!
//! `Rebuilder::JustOne` — the branch that asks only the emitting thread — is
//! taken exactly while **at most one** dispatcher is registered:
//!
//! ```text
//! fn rebuilder(&self) -> Rebuilder<'_> {
//!     if self.has_just_one.load(SeqCst) { return Rebuilder::JustOne; }
//!     Rebuilder::Read(LOCKED_DISPATCHERS.read().unwrap())
//! }
//! fn register_dispatch(&self, d: &Dispatch) -> Rebuilder<'_> {
//!     dispatchers.retain(|d| d.upgrade().is_some());
//!     dispatchers.push(d.registrar());
//!     self.has_just_one.store(dispatchers.len() <= 1, SeqCst);
//! }
//! ```
//!
//! So this module registers **two permissive sentinel dispatchers** and keeps
//! them alive for the process. `has_just_one` is then permanently false, every
//! interest rebuild takes `Rebuilder::Read` over *all* live dispatchers, and
//! their opinions are combined with
//! [`Interest::and`](tracing::subscriber::Interest), which resolves a
//! disagreement to `sometimes`:
//!
//! ```text
//! fn and(self, rhs: Interest) -> Self {
//!     if self.0 == rhs.0 { self } else { Interest::sometimes() }
//! }
//! ```
//!
//! A sentinel that answers `sometimes` for every callsite therefore makes
//! `never` unreachable, whichever thread registers the callsite first. With the
//! cache disarmed, [`capture`] can install its subscriber the ordinary,
//! composable way — thread-locally, for one closure — and everything else about
//! the process is untouched:
//!
//! - the sentinels are **never** anyone's default dispatcher, so they receive no
//!   events; they only vote on interest;
//! - a binary keeps its own global subscriber, and events outside a capture
//!   still reach it;
//! - two threads can capture at once without seeing each other's events, and
//!   without a lock between them.
//!
//! The cost is that no callsite is ever cached as `never` in a process that has
//! captured, so a disabled event pays one `enabled()` call instead of an atomic
//! load. That is the price of the cache being unable to lie.
//!
//! # Limits
//!
//! - Span *fields* are not recorded — only events. Assertions here are about
//!   what was logged, not about span structure.
//! - A crate at or below `flui-interaction` in the layer DAG cannot depend on
//!   this crate, so its in-`src` capture tests keep their own technique and
//!   their own caveat.

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

/// Installed thread-locally by [`capture`]; records into that thread's sink.
struct CaptureSubscriber;

impl Subscriber for CaptureSubscriber {
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

/// Votes `sometimes` on every callsite and is never anyone's default, so it
/// receives no events and only ever influences the interest cache.
struct InterestSentinel;

impl Subscriber for InterestSentinel {
    /// The whole point. See this module's docs: with two of these registered,
    /// `Interest::and` can no longer resolve any callsite to `never`.
    fn register_callsite(&self, _metadata: &'static Metadata<'static>) -> Interest {
        Interest::sometimes()
    }

    /// Unreachable in practice — a sentinel is never a default dispatcher — and
    /// `false` regardless, so a future path that did consult it records nothing.
    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        false
    }

    fn new_span(&self, _span: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _span: &Id, _values: &Record<'_>) {}

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn event(&self, _event: &Event<'_>) {}

    fn enter(&self, _span: &Id) {}

    fn exit(&self, _span: &Id) {}
}

/// Disarm `tracing`'s per-callsite interest cache for this process, once.
///
/// Constructing a [`Dispatch`](tracing::Dispatch) registers it, and the second
/// registration is what flips `has_just_one` false for good — the registry only
/// ever drops dispatchers whose `Weak` no longer upgrades, and these two are
/// held for the life of the process. From then on every interest rebuild
/// consults all live dispatchers instead of just the emitting thread's.
fn disarm_interest_cache() {
    static SENTINELS: OnceLock<[tracing::Dispatch; 2]> = OnceLock::new();
    SENTINELS.get_or_init(|| {
        [
            tracing::Dispatch::new(InterestSentinel),
            tracing::Dispatch::new(InterestSentinel),
        ]
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
    disarm_interest_cache();
    tracing::subscriber::with_default(CaptureSubscriber, || capture_into_sink(body))
}

/// The sink half of [`capture`], with the subscriber already installed for this
/// thread.
fn capture_into_sink<R>(body: impl FnOnce() -> R) -> (R, CapturedLog) {
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
