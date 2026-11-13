//! Public-API tests for `Navigator`.
//!
//! Driven through the real `flui_widgets::prelude` surface and a real
//! `HeadlessBinding` frame — the path `UiRuntime::draw_frame` takes. If a name
//! were not exported, this file would not compile.
//!
//! # Scenarios
//!
//! Navigating to and from a stateful widget, a graceful failure when no
//! navigator is in context, finding the root navigator, push/pop/replace in
//! sequence, `remove_route`, and removing a route whose value is awaited.

// ADR-0027: these public API tests capture owner-local `NavigatorHandle`s in
// test cells. The production crate has the same lint allowance; integration
// tests are separate crates, so repeat it here.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::common::{child_process, lay_out, loose};
use flui_widgets::RouteParseError;
use parking_lot::Mutex;

// Exercise the public prelude import path.
use flui_widgets::__test_access::{Navigator, NavigatorHandle, NavigatorRoute, SimpleRoute};
use flui_widgets::prelude::*;
use flui_widgets::{Route, RouteContentBuilder, RouteSettings};

/// A prefix parser failure in the public default producer destroys the routes
/// it already accepted (ADR-0281), the parser's failure is the one raised, and
/// the next parse succeeds. The counters are process-global, so the case runs
/// in a child process.
#[test]
fn default_back_stack_parser_failure_destroys_accepted_routes() {
    use std::sync::atomic::AtomicBool;

    if let Some(case) = child_process::selected_case() {
        static PARSER_FAILS: AtomicBool = AtomicBool::new(true);
        static FULL_DROPS: AtomicUsize = AtomicUsize::new(0);
        static PREFIX_DROPS: AtomicUsize = AtomicUsize::new(0);

        #[derive(Clone, PartialEq)]
        struct ParsedRoute(&'static str);
        impl Drop for ParsedRoute {
            fn drop(&mut self) {
                let drops = if self.0 == "/a/b" {
                    &FULL_DROPS
                } else {
                    &PREFIX_DROPS
                };
                drops.fetch_add(1, Ordering::Relaxed);
            }
        }
        impl Routable for ParsedRoute {
            fn to_path(&self) -> RoutePath {
                RoutePath::parse(self.0).expect("fixture route path")
            }
            fn from_path(path: &RoutePath) -> Result<Self, RouteParseError> {
                match path.as_str() {
                    "/a/b" => Ok(Self("/a/b")),
                    "/" => Ok(Self("/")),
                    "/a" => {
                        assert!(
                            !PARSER_FAILS.load(Ordering::Relaxed),
                            "first prefix parser failure"
                        );
                        Ok(Self("/a"))
                    }
                    _ => Err(RouteParseError::NoMatch { path: path.clone() }),
                }
            }
        }

        assert_eq!(case, "parser");
        let path = RoutePath::parse("/a/b").expect("fixture path");
        let failure = std::panic::catch_unwind(|| {
            let _ = ParsedRoute::back_stack(&path);
        })
        .expect_err("later prefix parser failure propagates");
        assert_eq!(
            flui_foundation::panic::payload_text(failure.as_ref()),
            Some("first prefix parser failure")
        );
        assert_eq!(
            FULL_DROPS.load(Ordering::Relaxed),
            1,
            "the accepted full route is destroyed by the parser unwind"
        );
        assert_eq!(
            PREFIX_DROPS.load(Ordering::Relaxed),
            1,
            "the accepted root prefix is destroyed by the parser unwind"
        );

        PARSER_FAILS.store(false, Ordering::Relaxed);
        let stack = ParsedRoute::back_stack(&path).expect("next healthy default parse");
        assert_eq!(
            stack.iter().map(|route| route.0).collect::<Vec<_>>(),
            ["/", "/a", "/a/b"]
        );
        drop(stack);
        assert_eq!(
            FULL_DROPS.load(Ordering::Relaxed),
            2,
            "healthy full route retires normally"
        );
        assert_eq!(
            PREFIX_DROPS.load(Ordering::Relaxed),
            3,
            "healthy prefix routes retire normally"
        );
        child_process::pass();
    }

    child_process::run_rows(
        "navigator_public::default_back_stack_parser_failure_destroys_accepted_routes",
        &["parser"],
    );
}

pub(crate) fn delivered_route_results_remain_completed() {
    use std::future::Future as _;
    use std::pin::Pin;
    use std::task::{Context, Poll, Waker};

    for (asynchronous, expected) in [
        (false, Some(42)),
        (true, Some(42)),
        (false, None),
        (true, None),
    ] {
        let navigator = NavigatorHandle::new(flui_foundation::BuildPhase::default());
        let built = Built::default();
        navigator.push(page(&built, "root"));
        let mut result = navigator.push(page(&built, "details"));
        assert!(!result.is_completed());
        assert!(expected.map_or_else(|| navigator.pop(), |value| navigator.pop_with(value)));
        assert!(result.is_completed());
        if asynchronous {
            let mut context = Context::from_waker(Waker::noop());
            assert_eq!(
                Pin::new(&mut result).poll(&mut context),
                Poll::Ready(Ok(expected))
            );
        } else {
            assert_eq!(result.try_take(), Some(Ok(expected)));
        }
        assert!(result.is_completed(), "delivery must preserve completion");
        assert_eq!(result.try_take(), None, "a value is delivered once");
        let next = navigator.push(page(&built, "next"));
        assert!(navigator.pop_with(43));
        assert_eq!(next.try_take(), Some(Ok(Some(43))));
    }
}

// ============================================================================
// PROBES
// ============================================================================

/// Records which route contents were built.
#[derive(Clone, Default)]
struct Built(Arc<Mutex<Vec<&'static str>>>);

fn page(built: &Built, name: &'static str) -> SimpleRoute<i32> {
    let built = built.clone();
    SimpleRoute::new(move |_ctx| {
        built.0.lock().push(name);
        SizedBox::new(10.0, 10.0).into_view().boxed()
    })
    .named(name)
}

/// A pop result whose `Drop` is observable.
///
/// Several undelivered-result paths run under the history mutex, where dropping
/// a caller's value inline runs user code with a lock held. A counter proves the
/// drop happened once *and* — because the navigator is still usable after it —
/// that it happened outside the guard.
struct DropCounter(Arc<AtomicUsize>);

impl DropCounter {
    fn new(counter: &Arc<AtomicUsize>) -> Self {
        Self(Arc::clone(counter))
    }
}

impl Drop for DropCounter {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

/// The `operation` field of every undeliverable-result report in `log`, in order.
///
/// Discriminating per row on the operation name, rather than counting matches of
/// a message substring: a count cannot tell "the path I meant reported" from
/// "some other path reported instead", which is the difference between an
/// assertion and a coincidence.
fn undelivered_operations(log: &flui_testing::log_capture::CapturedLog) -> Vec<&str> {
    log.records()
        .iter()
        .filter(|record| {
            record.contains("reached no route") || record.contains("could not be delivered")
        })
        .map(|record| record.field("operation").unwrap_or("<no operation field>"))
        .collect()
}

// ============================================================================
// TESTS
// ============================================================================

/// A leaf a route can show, with no probe attached.
fn leaf(_ctx: &dyn BuildContext) -> BoxedView {
    SizedBox::new(10.0, 10.0).into_view().boxed()
}

/// An application finishes its authored page entrance without canceling the
/// navigator's original completion future or leaving the old page alive.
pub(crate) fn explicit_finish_commits_an_authored_page_and_retires_the_old_route() {
    use std::cell::Cell;
    use std::time::Duration;

    use flui_animation::{Animation, AnimationController, AnimationError, DrivenController, Vsync};
    use flui_painting::{display_list::DrawOp, styling::Color};
    use flui_rendering::layer::Layer;
    use flui_widgets::{AnimationRunFuture, FadeTransition, PushCompletion};

    struct AuthoredPage {
        settings: RouteSettings,
        builder: RouteContentBuilder,
        owner: Option<DrivenController>,
        run: Rc<RefCell<Option<AnimationRunFuture>>>,
        disposals: Rc<Cell<usize>>,
    }

    impl Route for AuthoredPage {
        type Output = ();

        fn settings(&self) -> &RouteSettings {
            &self.settings
        }

        fn did_push(&mut self) -> PushCompletion {
            let Some(owner) = &self.owner else {
                return PushCompletion::Immediate;
            };
            let run = owner.controller().forward().expect("page entrance starts");
            *self.run.borrow_mut() = Some(run.clone());
            PushCompletion::Animating(run)
        }

        fn dispose(&mut self) {
            drop(self.owner.take());
            self.disposals.set(self.disposals.get() + 1);
        }
    }

    impl NavigatorRoute for AuthoredPage {
        fn content_builder(&self) -> RouteContentBuilder {
            Rc::clone(&self.builder)
        }
    }

    fn scene_alpha(app: &crate::common::LaidOut) -> f64 {
        let scene = app.layer_tree().expect("page scene submitted");
        let mut alpha = 1.0;
        let mut rectangles = 0;
        for (_, node) in scene.iter() {
            match node.layer() {
                Layer::Opacity(layer) => alpha *= layer.alpha(),
                Layer::Picture(picture) => {
                    for command in picture.picture() {
                        if let DrawOp::Rect { rect, paint } = &command.op
                            && paint.color == Color::RED
                        {
                            assert!(rect.width() > 0.0 && rect.height() > 0.0);
                            rectangles += 1;
                        }
                    }
                }
                _ => {}
            }
        }
        assert_eq!(rectangles, 1, "authored page actually paints");
        alpha
    }

    let vsync = Vsync::new();
    let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&vsync));
    let controller = owner.controller().clone();
    let original = Rc::new(RefCell::new(None));
    let old_disposals = Rc::new(Cell::new(0));
    let new_disposals = Rc::new(Cell::new(0));
    let handle = NavigatorHandle::new(flui_foundation::BuildPhase::default());
    handle.seed_initial(AuthoredPage {
        settings: RouteSettings::named("/old"),
        builder: Rc::new(leaf),
        owner: None,
        run: Rc::clone(&original),
        disposals: Rc::clone(&old_disposals),
    });
    let mut app = crate::common::lay_out_animated(
        Navigator::new(handle.clone()),
        loose(400.0),
        vsync.clone(),
    );
    let opacity: Rc<dyn Animation<f64>> = Rc::new(controller.clone());
    let _popped = handle.push_and_remove_until(
        AuthoredPage {
            settings: RouteSettings::named("/new"),
            builder: Rc::new(move |_| {
                FadeTransition::new(Rc::clone(&opacity))
                    .child(ColoredBox::new(Color::RED).child(SizedBox::new(80.0, 40.0)))
                    .into_view()
                    .boxed()
            }),
            owner: Some(owner),
            run: Rc::clone(&original),
            disposals: Rc::clone(&new_disposals),
        },
        |_| false,
    );
    app.tick();
    app.pump_for(Duration::from_millis(400));
    let fade = app.find_by_render_type("RenderAnimatedOpacity");
    let interior = app.opacity(fade);
    assert!(
        interior > 0.0 && interior < 1.0,
        "entrance is painted midway"
    );
    assert_eq!(scene_alpha(&app), interior);
    assert_eq!(old_disposals.get(), 0, "old page waits for the entrance");
    let run = original
        .borrow()
        .as_ref()
        .expect("original push run")
        .clone();
    assert!(run.is_pending());
    let outcomes = Rc::new(Cell::new(0));
    run.when_complete_or_cancel({
        let outcomes = Rc::clone(&outcomes);
        move |outcome| {
            assert!(
                outcome.is_ok(),
                "explicit finish succeeds rather than cancels"
            );
            outcomes.set(outcomes.get() + 1);
        }
    });

    controller
        .finish()
        .expect("application finishes the entrance");
    app.tick();
    assert!(run.is_complete());
    assert_eq!(outcomes.get(), 1);
    assert_eq!(app.opacity(fade), 1.0);
    assert_eq!(scene_alpha(&app), 1.0);
    assert_eq!(old_disposals.get(), 1);
    app.pump_for(Duration::from_secs(2));
    assert_eq!(outcomes.get(), 1);
    assert_eq!(old_disposals.get(), 1);

    let id = handle.route_ids()[0];
    handle.remove_route(id);
    app.tick();
    assert_eq!(new_disposals.get(), 1, "route releases its animation owner");
    assert_eq!(controller.finish(), Err(AnimationError::Disposed));
    assert!(!vsync.has_running());
    assert!(
        vsync.is_empty(),
        "disposed route withdraws its registration"
    );
    let _next = handle.push(SimpleRoute::<()>::new(leaf).named("/next"));
    app.tick();
    assert_eq!(handle.route_ids().len(), 1, "navigation remains usable");
}

// ----------------------------------------------------------------------------
// A route whose pop does NOT finalise it — the state every production
// PageRoute/PopupRoute occupies for the length of its exit transition.
// ----------------------------------------------------------------------------

/// A route that answers `finished_when_popped() == false`, like every
/// `TransitionRoute` descendant.
///
/// Every other fixture in this file is a `SimpleRoute`, which takes
/// the `Route` default `true` and is finalised inside the same flush as its pop.
/// That made the `Popping`/`Removing` window — where a real `PageRoute` lives for
/// the whole of its exit transition — unreachable from this suite, and it is
/// where three of the defects this PR fixed were hiding.
///
/// It carries no `RouteBindingSlot`, so nothing finalises it: once popped it
/// stays in `Popping` indefinitely. That is deliberate — it is the *widest*
/// version of the window, so anything that reads the stack while a pop is in
/// flight has to be correct against it.
struct DeferredExitRoute {
    settings: RouteSettings,
    builder: RouteContentBuilder,
    current_result: i32,
}

impl DeferredExitRoute {
    fn new(name: &'static str, current_result: i32) -> Self {
        Self {
            settings: RouteSettings::named(name),
            builder: Rc::new(leaf),
            current_result,
        }
    }
}

impl Route for DeferredExitRoute {
    type Output = i32;

    fn settings(&self) -> &RouteSettings {
        &self.settings
    }

    fn current_result(&mut self) -> Option<i32> {
        Some(self.current_result)
    }

    /// The whole point of this fixture.
    fn finished_when_popped(&self) -> bool {
        false
    }
}

impl NavigatorRoute for DeferredExitRoute {
    fn content_builder(&self) -> RouteContentBuilder {
        Rc::clone(&self.builder)
    }
}

/// A mismatched pop result is reported and dropped **outside** the history lock.
///
/// `RouteRecord::did_complete` runs inside the flush, under the history mutex. It
/// used to emit `tracing::error!` and drop the mismatched box right there — and
/// both are user code: a subscriber is user-written, and the box wraps a value the
/// caller supplied, so its `Drop` is theirs. Either can call back into the
/// navigator, and the mutex is not reentrant.
///
/// # Reaching it, given `pop_with`'s `Send` bound
///
/// The payload cannot capture a `NavigatorHandle` — `pop_with<T: Send>` forbids
/// it, since the handle is deliberately `!Send`. Its `Drop` reaches the navigator
/// through an owner-thread `thread_local`, as a payload that finds the navigator
/// through ambient owner state would.
///
/// # Expected failure shape
///
/// Under the defect this **hangs**, it does not assert: `timeout 60` → exit 124,
/// with no output after the start line. The drop counter is therefore asserted
/// inside the capture, as the test proceeds — a final assertion never runs in a
/// deadlock.
///
/// The subscriber half of the same hazard is real but not demonstrated here:
/// `flui_testing::log_capture`'s subscriber is inert, which is exactly why an
/// existing test drives this path and passes. This covers the value's `Drop`.
///
/// Red-check: emit the `tracing::error!` and drop the box inside
/// `RouteRecord::did_complete` again, and this deadlocks the owner thread.
pub(crate) fn a_mismatched_pop_result_is_reported_and_dropped_outside_the_history_lock() {
    thread_local! {
        static OWNER_NAVIGATOR: RefCell<Option<NavigatorHandle>> = const { RefCell::new(None) };
    }

    /// A payload whose `Drop` drives the navigator from the owner thread.
    struct CommandsNavigatorOnDrop {
        drops: Arc<AtomicUsize>,
    }

    impl Drop for CommandsNavigatorOnDrop {
        fn drop(&mut self) {
            // Takes the history lock. Deadlocks if this drop happens under it.
            let navigator = OWNER_NAVIGATOR.with(|cell| cell.borrow().clone());
            if let Some(navigator) = navigator {
                let _ = navigator.maybe_pop();
            }
            self.drops.fetch_add(1, Ordering::Relaxed);
        }
    }

    let built = Built::default();
    let drops = Arc::new(AtomicUsize::new(0));
    let handle = NavigatorHandle::new(flui_foundation::BuildPhase::default());
    handle.seed_initial(page(&built, "/"));
    OWNER_NAVIGATOR.with(|cell| *cell.borrow_mut() = Some(handle.clone()));
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));

    // `page` is a `SimpleRoute<i32>`, so this payload is the wrong type for it and
    // takes the mismatch path.
    let route = handle.push(page(&built, "target"));
    laid.tick();

    let ((), log) = flui_testing::log_capture::capture(|| {
        assert!(handle.pop_with(CommandsNavigatorOnDrop {
            drops: Arc::clone(&drops),
        }));
        // Reached only if the drop did not deadlock.
        assert_eq!(
            drops.load(Ordering::Relaxed),
            1,
            "the mismatched payload was dropped, and its Drop reached the navigator \
             without deadlocking"
        );
    });
    laid.tick();

    assert_eq!(
        log.count_containing("pop result has the wrong type"),
        1,
        "and the mismatch was reported once, from outside the lock; captured:\n{}",
        log.render_at_least(tracing::Level::WARN)
    );
    assert_eq!(
        route.try_take(),
        Some(Err(flui_widgets::RouteResultError::TypeMismatch {
            expected: std::any::type_name::<i32>(),
        })),
        "the route completed with the typed mismatch, not a silent `None`"
    );
}

/// Every operation that cannot deliver a caller's result **reports** it,
/// including the paths that used to drop it silently.
///
/// Several of them ran **under the history guard**, so the value's `Drop` — user
/// code — could have deadlocked, not merely vanished. And one of them,
/// `maybe_pop`'s `Bubble` arm, is not an edge case at all: `popDisposition` is
/// `isFirst ? bubble : pop`, so a lone route bubbles *by design* and
/// `maybe_pop_with` on a one-route navigator took it every time.
///
/// Asserted on the **captured log**, not a counter: a counter would pin a parallel
/// predicate rather than the emission, which is the trap this module already
/// documents for the registration-conflict warning. And asserted per row on the
/// report's `operation` field rather than on a count of matching messages — a
/// count cannot distinguish "the path I meant reported" from "a different path
/// reported instead", which for the two `maybe_pop` arms below is the whole
/// question.
///
/// One site is deliberately **not** a row here: the `pending_result` displacement
/// in `RouteEntry::arm_pop` / `arm_complete`. Reaching it needs one entry armed
/// twice before its flush, and every public mutation flushes before returning
/// while `debug_assert!(!flushing)` rejects re-entry from inside a flush — so it
/// is a defence with no public caller, not an untested path. If a future
/// operation arms without flushing, that is when it becomes reachable.
///
/// Red-check: restore any one early return to dropping `result` inline and that
/// row's operation list goes empty.
pub(crate) fn every_operation_that_cannot_deliver_a_result_reports_it() {
    /// Drive one scenario and report how many "no route" warnings it emitted.
    fn warnings_from(drive: impl FnOnce(&NavigatorHandle)) -> Vec<String> {
        let ((), log) = flui_testing::log_capture::capture(|| {
            let handle = NavigatorHandle::new(flui_foundation::BuildPhase::default());
            drive(&handle);
        });
        undelivered_operations(&log)
            .into_iter()
            .map(str::to_owned)
            .collect()
    }

    // 1. `maybe_pop_with` while unmounted.
    assert_eq!(
        warnings_from(|handle| {
            assert!(
                handle.maybe_pop_with(7_i32),
                "an unmounted navigator swallows the pop"
            );
        }),
        ["maybe_pop"],
        "the unmounted early return reports too"
    );

    // 2. `pop_with` on an empty stack.
    assert_eq!(
        warnings_from(|handle| {
            assert!(!handle.pop_with(7_i32), "nothing to pop");
        }),
        ["pop"],
        "the plainest case of all"
    );

    // 3. `remove_route_with` for an id that is not there. The id comes from a
    //    *different* navigator, since `RouteId::next` is crate-private — which
    //    also makes it a genuinely absent id rather than a fabricated one.
    let stranger = NavigatorHandle::new(flui_foundation::BuildPhase::default());
    stranger.seed_initial(DeferredExitRoute::new("/elsewhere", 0));
    let absent = stranger.route_ids()[0];
    assert_eq!(
        warnings_from(move |handle| {
            assert!(
                !handle.remove_route_with(absent, 7_i32),
                "that route belongs to another navigator"
            );
        }),
        ["remove_route"],
        "a missing removal target reports too"
    );

    // 4. `maybe_pop_with` on a lone route, which bubbles BY DESIGN — the one that
    //    is not an edge case.
    let bubbled = flui_testing::log_capture::capture(|| {
        let built = Built::default();
        let handle = NavigatorHandle::new(flui_foundation::BuildPhase::default());
        handle.seed_initial(page(&built, "/"));
        let _laid = lay_out(Navigator::new(handle.clone()), loose(400.0));
        assert!(
            !handle.maybe_pop_with(7_i32),
            "a lone route bubbles: popDisposition is `isFirst ? bubble : pop`"
        );
    })
    .1;
    assert_eq!(
        undelivered_operations(&bubbled),
        ["maybe_pop"],
        "the Bubble arm reports, and it used to drop under the history guard; \
         captured:\n{}",
        bubbled.render_at_least(tracing::Level::WARN)
    );

    // 5. `DoNotPop` — the arm that reports *and* answers "handled". A vetoing
    //    route keeps the stack and the caller's result has nowhere to go, so this
    //    is the one arm where "the request was dealt with" and "your value was
    //    discarded" are true at once. It is also the row carrying an observable
    //    `Drop`: the arm runs under the history guard, so the proof that matters
    //    is that the value's `Drop` ran and the navigator still works afterwards.
    let vetoed_drops = Arc::new(AtomicUsize::new(0));
    let vetoed = flui_testing::log_capture::capture(|| {
        let built = Built::default();
        let handle = NavigatorHandle::new(flui_foundation::BuildPhase::default());
        handle.seed_initial(page(&built, "/"));
        let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));
        handle.push(VetoingRoute::new());
        laid.tick();
        assert!(
            handle.maybe_pop_with(DropCounter::new(&vetoed_drops)),
            "a veto HANDLES the request — it just does not pop"
        );
        laid.tick();
        assert_eq!(
            handle.route_ids().len(),
            2,
            "and the vetoing route is still there"
        );
        assert_eq!(
            vetoed_drops.load(Ordering::Relaxed),
            1,
            "the value's Drop ran once, after the guard released — the navigator \
             below still answers, which it could not do from inside its own lock"
        );
        assert!(handle.can_pop(), "still usable");
    })
    .1;
    assert_eq!(
        undelivered_operations(&vetoed),
        ["maybe_pop"],
        "the DoNotPop arm reports; captured:\n{}",
        vetoed.render_at_least(tracing::Level::WARN)
    );

    // 6. `pop_disposition_of_top() == None` on a **mounted** navigator — distinct
    //    from row 1 (unmounted) and row 2 (`pop`, which never asks for a
    //    disposition). Reached by removing the only route: a removed entry is not
    //    `is_present`, so the disposition query has no top to ask.
    let dispositionless = flui_testing::log_capture::capture(|| {
        let built = Built::default();
        let handle = NavigatorHandle::new(flui_foundation::BuildPhase::default());
        handle.seed_initial(page(&built, "/"));
        let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));
        let root = handle.current().expect("seeded");
        assert!(handle.remove_route(root), "removed");
        laid.tick();
        assert_eq!(handle.current(), None, "nothing is present any more");
        assert!(
            handle.is_mounted(),
            "and yet the navigator is mounted — this is not row 1"
        );
        assert!(
            !handle.maybe_pop_with(7_i32),
            "no disposition means nothing handled it"
        );
    })
    .1;
    assert_eq!(
        undelivered_operations(&dispositionless),
        ["maybe_pop"],
        "the no-disposition arm reports; captured:\n{}",
        dispositionless.render_at_least(tracing::Level::WARN)
    );
}

/// A route whose `popDisposition` is `DoNotPop` — the third arm of
/// `maybe_pop`'s match, and the only one that reports *and* returns "handled".
struct VetoingRoute {
    settings: RouteSettings,
    builder: RouteContentBuilder,
}

impl VetoingRoute {
    fn new() -> Self {
        Self {
            settings: RouteSettings::named("vetoing"),
            builder: Rc::new(|_ctx| SizedBox::new(10.0, 10.0).into_view().boxed()),
        }
    }
}

impl Route for VetoingRoute {
    type Output = i32;

    fn settings(&self) -> &RouteSettings {
        &self.settings
    }

    fn vetoes_pop(&self) -> bool {
        true
    }
}

impl NavigatorRoute for VetoingRoute {
    fn content_builder(&self) -> RouteContentBuilder {
        Rc::clone(&self.builder)
    }
}
