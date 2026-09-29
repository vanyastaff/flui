//! Unit tests of the `Navigator`'s handle, command channel, export boundary,
//! user-gesture bookkeeping and named-route registry — everything that needs
//! no mounted element tree. The mounted suite lives in
//! `crates/flui-widgets/tests/navigator.rs`.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_view::prelude::*;
use parking_lot::Mutex;

use super::named_route::RouteRequest;
use super::navigator::{
    NavigatorCommand, NavigatorCommandError, NavigatorCommandOutcome, NavigatorCommandTarget,
    NavigatorHandle,
};
use super::overlay_route::SimpleRoute;
use crate::SizedBox;

/// Records every route builder invocation.
#[derive(Clone, Default)]
struct Built(Arc<Mutex<Vec<&'static str>>>);

/// A route whose content is a leaf, recording its name each time it builds.
fn page(built: &Built, name: &'static str) -> SimpleRoute<i32> {
    let built = built.clone();
    SimpleRoute::new(move |_ctx| {
        built.0.lock().push(name);
        SizedBox::new(10.0, 10.0).into_view().boxed()
    })
    .named(name)
}

#[test]
fn navigator_handle_is_owner_affine_but_command_target_is_send_sync() {
    static_assertions::assert_not_impl_any!(NavigatorHandle: Send, Sync);
    static_assertions::assert_impl_all!(NavigatorCommandTarget: Send, Sync);
    static_assertions::assert_impl_all!(NavigatorCommand: Send, Sync);
}

#[test]
fn navigator_command_applies_only_on_owner_thread() {
    let built = Built::default();
    let handle = NavigatorHandle::new();
    handle.seed_initial(page(&built, "/"));
    let route = handle.push(page(&built, "/next"));
    let target = handle.command_target();

    assert_eq!(
        NavigatorCommand::pop(target).apply_on_owner(),
        Ok(NavigatorCommandOutcome::Popped(true))
    );
    assert_eq!(handle.route_ids().len(), 1);
    assert_eq!(
        route.try_take(),
        Some(None),
        "a pop without result completes with None"
    );
}

#[test]
fn navigator_command_dead_target_is_typed_error() {
    let target = {
        let handle = NavigatorHandle::new();
        handle.command_target()
    };

    assert_eq!(
        NavigatorCommand::maybe_pop(target).apply_on_owner(),
        Err(NavigatorCommandError::OwnerGone)
    );
}

// ============================================================================
// User gestures (navigator.dart:5803-5860)
// ============================================================================

mod user_gesture {
    use super::*;
    use crate::navigator::observer::NavigatorObserver;
    use crate::navigator::route::RouteId;

    /// Records every `did_start_user_gesture`/`did_stop_user_gesture` call, in order.
    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Note {
        Start(RouteId, Option<RouteId>),
        Stop,
    }

    #[derive(Default)]
    struct Spy {
        notes: Mutex<Vec<Note>>,
    }

    impl Spy {
        fn notes(&self) -> Vec<Note> {
            self.notes.lock().clone()
        }
    }

    impl NavigatorObserver for Spy {
        fn did_start_user_gesture(&self, route: RouteId, previous: Option<RouteId>) {
            self.notes.lock().push(Note::Start(route, previous));
        }
        fn did_stop_user_gesture(&self) {
            self.notes.lock().push(Note::Stop);
        }
    }

    /// Flutter's `didStartUserGesture` resolves `route`/`previousRoute` from the
    /// live history, so a two-route stack reports the top and the route beneath.
    ///
    /// Red-check: don't release the history lock before firing observers — a
    /// `did_start_user_gesture` observer that reads `handle.route_ids()` back
    /// deadlocks instead of failing this assertion.
    #[test]
    fn did_start_user_gesture_reports_the_current_and_previous_route() {
        let built = Built::default();
        let handle = NavigatorHandle::new();
        handle.seed_initial(page(&built, "/"));
        let root = handle.route_ids()[0];
        let spy = Arc::new(Spy::default());
        handle.add_observer(Arc::clone(&spy) as Arc<dyn NavigatorObserver>);

        let _pushed = handle.push(page(&built, "/next"));
        let top_id = *handle.route_ids().last().expect("pushed route present");

        assert!(!handle.user_gesture_in_progress());
        handle.did_start_user_gesture();
        assert!(handle.user_gesture_in_progress());
        assert_eq!(spy.notes(), vec![Note::Start(top_id, Some(root))]);

        handle.did_stop_user_gesture();
        assert!(!handle.user_gesture_in_progress());
        assert_eq!(
            spy.notes(),
            vec![Note::Start(top_id, Some(root)), Note::Stop]
        );
    }

    /// A `NavigatorObserver::did_start_user_gesture` callback that calls back
    /// into the navigator (e.g. `can_pop`) must not deadlock — the history
    /// lock is released before observers run (the same lock-hazard class the
    /// `PopScope` fan-out and the focus-listener path were fixed for).
    #[test]
    fn an_observer_may_call_back_into_the_navigator_from_did_start_user_gesture() {
        struct ReentrantObserver {
            handle: NavigatorHandle,
            calls_seen: AtomicUsize,
        }
        impl NavigatorObserver for ReentrantObserver {
            fn did_start_user_gesture(&self, _route: RouteId, _previous: Option<RouteId>) {
                let _ = self.handle.can_pop();
                self.calls_seen.fetch_add(1, Ordering::SeqCst);
            }
        }

        let built = Built::default();
        let handle = NavigatorHandle::new();
        handle.seed_initial(page(&built, "/"));
        let observer = Arc::new(ReentrantObserver {
            handle: handle.clone(),
            calls_seen: AtomicUsize::new(0),
        });
        handle.add_observer(Arc::clone(&observer) as Arc<dyn NavigatorObserver>);

        handle.did_start_user_gesture();
        assert_eq!(observer.calls_seen.load(Ordering::SeqCst), 1);
        handle.did_stop_user_gesture();
    }
}

// ============================================================================
// Named-route registry
// ============================================================================

/// The conflict warning is really *emitted*, exactly once, and names the route
/// and both result types.
///
/// The sibling test asserts on `route_conflict_warns()`, a counter this module
/// maintains. That counter is coupled to the emission by construction, but a
/// counter is still not an event: it cannot show that `tracing::warn!` ran, that
/// the message says anything useful, or that the fields carry both type names.
/// This captures the real events instead, through
/// [`flui_testing::log_capture::capture`] — which is race-free in a parallel
/// test binary where a hand-rolled `with_default` is not, because `tracing`
/// caches each callsite's interest process-globally.
///
/// Red-check: delete the `tracing::warn!` call and this fails while every
/// counter assertion in the sibling test still passes — which is exactly the gap
/// a counter-only oracle leaves.
#[test]
fn the_conflict_warning_is_emitted_once_and_names_both_result_types() {
    let (handle, log) = flui_testing::log_capture::capture(|| {
        let handle = NavigatorHandle::new();
        // Same type twice: legitimate wholesale replacement, no warning.
        for _ in 0..2 {
            handle.route("/order", |_request: &RouteRequest<'_>| {
                Some(SimpleRoute::<u32>::new(|_ctx| {
                    SizedBox::new(1.0, 1.0).into_view().boxed()
                }))
            });
        }
        // Then flip the type three times: three conflicts, one warning.
        for _ in 0..3 {
            handle.route("/order", |_request: &RouteRequest<'_>| {
                Some(SimpleRoute::<String>::new(|_ctx| {
                    SizedBox::new(1.0, 1.0).into_view().boxed()
                }))
            });
            handle.route("/order", |_request: &RouteRequest<'_>| {
                Some(SimpleRoute::<u32>::new(|_ctx| {
                    SizedBox::new(1.0, 1.0).into_view().boxed()
                }))
            });
        }
        handle
    });

    let conflicts: Vec<_> = log
        .records()
        .iter()
        .filter(|record| record.contains("re-registered with a different result type"))
        .collect();

    assert_eq!(
        conflicts.len(),
        1,
        "six conflicting re-registrations emit exactly one warning; captured:\n{}",
        log.render_at_least(tracing::Level::WARN)
    );
    let warning = conflicts[0];
    assert_eq!(warning.level, tracing::Level::WARN);
    assert!(
        warning.contains("/order"),
        "the warning names the route: {warning:?}"
    );
    assert!(
        warning.contains("u32") && warning.contains("String"),
        "and both result types, so the reader can find the two disagreeing \
         registration sites: {warning:?}"
    );
    assert_eq!(
        handle.route_conflicts_seen(),
        6,
        "while every conflict is still counted"
    );
}

/// The re-entrancy the latch has to survive: a `tracing` subscriber that
/// registers a conflicting route **while handling the conflict warning**.
///
/// # What this actually catches, measured rather than assumed
///
/// The reported defect was that latch-after-emit lets the re-entrant
/// registration observe `warns_emitted == 0` and emit a **second warning**. That
/// half is *not* reachable, and this test is the evidence: `tracing` suppresses
/// re-entrant event dispatch on the same thread, so the inner `warn!` never
/// reaches a subscriber. Under the defective ordering the captured event count
/// is 1, not 2.
///
/// What *is* reachable is a **counter that drifts from the emission**: both the
/// inner and the outer call increment `warns_emitted`, so it reads 2 while one
/// event was emitted. Measured, with the increment placed after the warn:
/// `events=1 warns_counter=2 conflicts=2`; with the decision and the latch
/// committed together under the lock: `events=1 warns_counter=1 conflicts=2`.
///
/// That drift matters because `warns_emitted` is what the sibling counter test
/// asserts on. A counter that over-reports would make "warned once" pass while
/// the latch was broken in some other way — the counter is only a usable oracle
/// while it cannot diverge from the emission, which is exactly what committing
/// both in one locked step buys.
///
/// Recorded so nobody later observes that the double-warn is unreachable and
/// removes the guard as dead: it is not guarding the warn, it is keeping the
/// counter honest.
///
/// The subscriber is a ZST because `tracing::Subscriber` is `Send + Sync` and
/// `NavigatorHandle` deliberately is not; the handle reaches it through a
/// thread-local, which is sound because `with_default` installs per thread and
/// the warn is emitted on that same thread. `take()` bounds the re-entry to one,
/// so a failure is an assertion rather than a stack overflow.
///
/// Red-check (performed): move the `warns_emitted` increment back after the
/// `tracing::warn!` call and the counter assertion reads 2.
#[test]
fn a_subscriber_that_re_registers_while_handling_the_warning_cannot_skew_the_latch() {
    use std::cell::RefCell;
    use tracing::span::{Attributes, Id, Record};
    use tracing::{Event, Metadata, Subscriber};

    thread_local! {
        /// Taken by the first event, so the re-entry happens exactly once.
        static REENTER_ON: RefCell<Option<NavigatorHandle>> = const { RefCell::new(None) };
    }
    static CONFLICT_EVENTS: AtomicUsize = AtomicUsize::new(0);

    fn register_as_text(handle: &NavigatorHandle) {
        handle.route("/order", |_request: &RouteRequest<'_>| {
            Some(SimpleRoute::<String>::new(|_ctx| {
                SizedBox::new(1.0, 1.0).into_view().boxed()
            }))
        });
    }

    /// The re-entrant registration must flip the type *back*, or it replaces
    /// `String` with `String` and is not a conflict at all — which is what the
    /// precondition below exists to catch.
    fn register_as_number(handle: &NavigatorHandle) {
        handle.route("/order", |_request: &RouteRequest<'_>| {
            Some(SimpleRoute::<u32>::new(|_ctx| {
                SizedBox::new(1.0, 1.0).into_view().boxed()
            }))
        });
    }

    struct ReentrantSubscriber;

    impl Subscriber for ReentrantSubscriber {
        fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _span: &Attributes<'_>) -> Id {
            Id::from_u64(1)
        }
        fn record(&self, _span: &Id, _values: &Record<'_>) {}
        fn record_follows_from(&self, _span: &Id, _follows: &Id) {}
        fn event(&self, event: &Event<'_>) {
            if event.metadata().target().contains("named_route") {
                CONFLICT_EVENTS.fetch_add(1, Ordering::Relaxed);
            }
            // Re-enter registration from inside the warn's own dispatch.
            let target = REENTER_ON.with(|cell| cell.borrow_mut().take());
            if let Some(handle) = target {
                register_as_number(&handle);
            }
        }
        fn enter(&self, _span: &Id) {}
        fn exit(&self, _span: &Id) {}
    }

    flui_testing::log_capture::disarm_interest_cache();
    let handle = NavigatorHandle::new();
    register_as_number(&handle);
    REENTER_ON.with(|cell| {
        let _prev = cell.borrow_mut().replace(handle.clone());
    });

    tracing::subscriber::with_default(ReentrantSubscriber, || register_as_text(&handle));

    assert_eq!(
        handle.route_conflicts_seen(),
        2,
        "precondition: the re-entry really did register a second conflict — \
         without this the test would pass by the re-entry never happening, and \
         it caught exactly that when the re-entrant registration first used the \
         same type as the one it was replacing"
    );
    assert_eq!(
        handle.route_conflict_warns(),
        1,
        "the latch was committed under the lock before the event was dispatched, \
         so the re-entrant registration saw it and did not count itself as first"
    );
    assert_eq!(
        CONFLICT_EVENTS.load(Ordering::Relaxed),
        1,
        "and exactly one warning reached the subscriber — which is 1 under the \
         defective ordering too, because tracing suppresses re-entrant dispatch; \
         see this test's docs for why the counter is the discriminating oracle"
    );
}

/// Displacing a registration drops a **user closure**, and that closure's `Drop`
/// may reach back into the registry. It must not deadlock.
///
/// `parking_lot::Mutex` is not reentrant and there is no compile-time oracle for
/// a self-locking call — it compiles clean and hangs at run time. The path is
/// not exotic here: the closure being dropped by `clear_routes` is, by design,
/// the one that captured a `NavigatorHandle`, so its captured state is exactly
/// what a `Drop` impl would hang off.
///
/// Both directions are covered: re-registering a name (which drops the entry it
/// displaces) and `clear_routes` (which drops every entry at once).
///
/// Red-check: drop the displaced entry *inside* the guard — in
/// `RouteRegistry::register_named`, bind `previous` inside the locked block
/// instead of outside it; in `clear`, call `table.clear()` under the guard
/// rather than moving the map out. This test then hangs rather than failing,
/// which is why it is written to make progress observable (`re_registrations`
/// rises) instead of only asserting at the end.
#[test]
fn a_registration_dropped_while_replacing_or_clearing_may_re_enter_the_registry() {
    /// Registers another route from its own `Drop` — the self-locking shape.
    struct ReRegisterOnDrop {
        navigator: NavigatorHandle,
        ran: Arc<AtomicUsize>,
    }

    impl Drop for ReRegisterOnDrop {
        fn drop(&mut self) {
            self.ran.fetch_add(1, Ordering::Relaxed);
            self.navigator
                .route("/dropped-into", |_request: &RouteRequest<'_>| {
                    Some(SimpleRoute::<u32>::new(|_ctx| {
                        SizedBox::new(1.0, 1.0).into_view().boxed()
                    }))
                });
        }
    }

    let re_registrations = Arc::new(AtomicUsize::new(0));
    let handle = NavigatorHandle::new();

    // A factory whose captured state re-enters the registry when dropped.
    let hook = ReRegisterOnDrop {
        navigator: handle.clone(),
        ran: Arc::clone(&re_registrations),
    };
    handle.route("/replaced", move |_request: &RouteRequest<'_>| {
        let _keeps_the_hook_alive = &hook;
        Some(SimpleRoute::<u32>::new(|_ctx| {
            SizedBox::new(1.0, 1.0).into_view().boxed()
        }))
    });

    // Displacing it drops the hook. Under a guard held across the drop this
    // deadlocks the owner thread.
    handle.route("/replaced", |_request: &RouteRequest<'_>| {
        Some(SimpleRoute::<u32>::new(|_ctx| {
            SizedBox::new(1.0, 1.0).into_view().boxed()
        }))
    });
    assert_eq!(
        re_registrations.load(Ordering::Relaxed),
        1,
        "the displaced closure's Drop ran, and re-entered the registry without \
         deadlocking"
    );
    assert!(
        handle.push_named("/dropped-into").is_ok(),
        "and its re-entrant registration actually landed"
    );

    // The same, through `clear_routes`, which drops every entry at once.
    let hook = ReRegisterOnDrop {
        navigator: handle.clone(),
        ran: Arc::clone(&re_registrations),
    };
    handle.route("/cleared", move |_request: &RouteRequest<'_>| {
        let _keeps_the_hook_alive = &hook;
        Some(SimpleRoute::<u32>::new(|_ctx| {
            SizedBox::new(1.0, 1.0).into_view().boxed()
        }))
    });
    handle.clear_routes();
    assert_eq!(
        re_registrations.load(Ordering::Relaxed),
        2,
        "clear_routes dropped the closure outside its guard too"
    );

    // And the two generator hooks, which displace by assignment rather than by
    // `HashMap::insert` — the same hazard, reached a different way. Written out
    // twice rather than looped: the two methods take `impl Fn`, so there is no
    // one value that installs either.
    let generator_hook = ReRegisterOnDrop {
        navigator: handle.clone(),
        ran: Arc::clone(&re_registrations),
    };
    handle.on_generate_route(move |_request: &RouteRequest<'_>| {
        let _keeps_the_hook_alive = &generator_hook;
        None
    });
    handle.on_generate_route(|_request: &RouteRequest<'_>| None);
    assert_eq!(
        re_registrations.load(Ordering::Relaxed),
        3,
        "a replaced on_generate_route hook is dropped outside the guard too"
    );

    let fallback_hook = ReRegisterOnDrop {
        navigator: handle.clone(),
        ran: Arc::clone(&re_registrations),
    };
    handle.on_unknown_route(move |_request: &RouteRequest<'_>| {
        let _keeps_the_hook_alive = &fallback_hook;
        None
    });
    handle.on_unknown_route(|_request: &RouteRequest<'_>| None);
    assert_eq!(
        re_registrations.load(Ordering::Relaxed),
        4,
        "and so is a replaced on_unknown_route hook"
    );
}
