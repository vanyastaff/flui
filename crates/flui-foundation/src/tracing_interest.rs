//! Disarm `tracing`'s per-callsite interest cache, so a thread-local
//! subscriber cannot silently lose events in a parallel test binary.
//!
//! # The problem
//!
//! A callsite's [`Interest`] is computed **once**, on whichever thread first
//! reaches it, and cached **process-globally**. In `tracing-core`'s `std`
//! build:
//!
//! ```text
//! fn rebuilder(&self) -> Rebuilder<'_> {
//!     if self.has_just_one.load(SeqCst) { return Rebuilder::JustOne; }
//!     Rebuilder::Read(LOCKED_DISPATCHERS.read().unwrap())
//! }
//! // Rebuilder::JustOne::for_each → dispatcher::get_default(f)
//! ```
//!
//! `get_default` reads the **calling thread's** dispatcher. So when one test
//! installs a capturing subscriber with
//! [`tracing::subscriber::with_default`] — which is thread-local — and a
//! *different* test on another thread reaches the same `warn!`/`error!` first
//! with no subscriber installed, that callsite's interest is computed against
//! `NoSubscriber`, cached as `Interest::never`, and the capturing test's
//! event is dropped before dispatch ever happens.
//!
//! The failure is silent and looks like a flake. It made a `flui-widgets`
//! parity test fail 4 times in 25 runs of its binary while passing 60/60 in
//! isolation, poisoned by a sibling test that mounts an identical tree and
//! reaches the same warning first. Serialising the capturing tests against
//! each other does not help: the poisoner is any other test in the binary.
//!
//! # The fix
//!
//! `disarm_interest_cache` registers two permissive sentinel dispatchers and
//! keeps them alive for the process. `has_just_one` is then permanently false,
//! so every interest rebuild takes the `Rebuilder::Read` branch over *all* live
//! dispatchers and combines their opinions with `Interest::and`, which resolves
//! a disagreement to `sometimes`:
//!
//! ```text
//! fn and(self, rhs: Interest) -> Self {
//!     if self.0 == rhs.0 { self } else { Interest::sometimes() }
//! }
//! ```
//!
//! A sentinel that answers `sometimes` for every callsite therefore makes
//! `never` unreachable, whichever thread registers the callsite first.
//!
//! The sentinels are **never** anyone's default dispatcher, so they receive no
//! events and never occupy the process-global default subscriber slot: a
//! binary keeps whatever subscriber it installs, and a `with_default` capture
//! keeps working exactly as written. Call this once before installing one.
//!
//! # Cost
//!
//! No callsite is ever cached as `never` in a process that has called this, so
//! a disabled event pays one `enabled()` call instead of an atomic load. That
//! is the price of the cache being unable to lie, and it is why this lives
//! behind the `testing` feature rather than always being on.
//!
//! # Why here
//!
//! Every crate in the workspace already depends on `flui-foundation`, so the
//! primitive reaches all of them with no new dependency edge. The *capture
//! API* built on it lives in `flui-testing` (`flui_testing::log_capture`);
//! this module is only the cache disarm, which a crate too low in the DAG to
//! reach `flui-testing` can still use with its own subscriber.

use std::sync::OnceLock;

use tracing::span::{Attributes, Id, Record};
use tracing::subscriber::Interest;
use tracing::{Event, Metadata, Subscriber};

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

/// Make `tracing`'s per-callsite interest cache unable to resolve any callsite
/// to `never` for the rest of this process. Idempotent.
///
/// Call it **before** installing a capturing subscriber — typically as the
/// first line of a test's capture helper. It takes no subscriber slot and
/// changes no dispatch, so it composes with whatever the caller installs.
///
/// Constructing a [`tracing::Dispatch`] registers it, and the *second*
/// registration is what flips `has_just_one` false for good: the registry only
/// ever drops dispatchers whose `Weak` no longer upgrades, and these two are
/// held for the life of the process.
///
/// Registering also rebuilds the interest of every callsite already registered,
/// so a callsite poisoned earlier in the same process is healed — there is no
/// ordering requirement on the first call.
///
/// # Example
///
/// ```
/// # use flui_foundation::tracing_interest::disarm_interest_cache;
/// disarm_interest_cache();
/// // …now install a thread-local subscriber and capture without racing…
/// ```
pub fn disarm_interest_cache() {
    static SENTINELS: OnceLock<[tracing::Dispatch; 2]> = OnceLock::new();
    SENTINELS.get_or_init(|| {
        [
            tracing::Dispatch::new(InterestSentinel),
            tracing::Dispatch::new(InterestSentinel),
        ]
    });
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier, Mutex};

    use tracing::span::{Attributes, Id, Record};
    use tracing::{Event, Metadata, Subscriber};

    use super::disarm_interest_cache;

    /// One callsite, reachable from two threads.
    ///
    /// It must be a function: two `warn!` invocations at different source
    /// locations are different callsites, and the race is about a *single* one.
    fn emit_probe(tag: &str) {
        tracing::warn!(tag, "interest-cache probe");
    }

    /// The kind of thread-local capturing subscriber every consumer of this
    /// module installs for itself.
    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Vec<String>>>);

    impl Subscriber for Capture {
        fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
            true
        }

        fn new_span(&self, _span: &Attributes<'_>) -> Id {
            Id::from_u64(1)
        }

        fn record(&self, _span: &Id, _values: &Record<'_>) {}

        fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

        fn event(&self, event: &Event<'_>) {
            struct Tag(String);
            impl tracing::field::Visit for Tag {
                fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
                    if field.name() == "tag" {
                        self.0 = value.to_owned();
                    }
                }

                fn record_debug(
                    &mut self,
                    _field: &tracing::field::Field,
                    _value: &dyn std::fmt::Debug,
                ) {
                }
            }
            let mut tag = Tag(String::new());
            event.record(&mut tag);
            self.0.lock().expect("capture mutex").push(tag.0);
        }

        fn enter(&self, _span: &Id) {}

        fn exit(&self, _span: &Id) {}
    }

    /// A callsite another thread reaches mid-capture is still recorded.
    ///
    /// Without [`disarm_interest_cache`], the other thread — which has no
    /// subscriber — makes `tracing` compute this callsite's interest against
    /// `NoSubscriber` and cache it as `never` for the whole process, after
    /// which the capturing thread's own event is dropped before dispatch.
    ///
    /// Two rendezvous points force that interleaving rather than hoping for it:
    /// the other thread emits strictly INSIDE the capture, after the capturing
    /// subscriber's own dispatcher registration has already happened and so
    /// cannot save it.
    ///
    /// **Verifying it by hand needs `--exact`.** The sentinels are process-wide
    /// and idempotent, so any other test in this binary that calls
    /// [`disarm_interest_cache`] — `diagnostics`' own capture helper does —
    /// arms them first and the probe passes even with the call below deleted.
    /// Run this test alone to see it fail (it reports `saw []`).
    #[test]
    fn a_callsite_another_thread_reaches_mid_capture_is_still_recorded() {
        disarm_interest_cache();

        let barrier = Barrier::new(2);
        let capture = Capture::default();

        std::thread::scope(|scope| {
            scope.spawn(|| {
                barrier.wait();
                emit_probe("other-thread");
                barrier.wait();
            });

            tracing::subscriber::with_default(capture.clone(), || {
                barrier.wait();
                barrier.wait();
                emit_probe("capturing-thread");
            });
        });

        let seen = capture.0.lock().expect("capture mutex").clone();
        assert!(
            seen.iter().any(|tag| tag == "capturing-thread"),
            "the capturing thread's own event must survive a callsite another \
             thread reached first with no subscriber; saw {seen:?}",
        );
        assert!(
            !seen.iter().any(|tag| tag == "other-thread"),
            "a thread-local subscriber must not receive another thread's \
             events; saw {seen:?}",
        );
    }

    #[test]
    fn calling_it_twice_is_a_no_op() {
        disarm_interest_cache();
        disarm_interest_cache();
    }
}
