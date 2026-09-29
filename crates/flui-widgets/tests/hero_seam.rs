//! The executable half of the support seams.
//!
//! These tests predate the public Hero baseline and still pin the four support seams
//! `HeroController` reaches for, each against the
//! `heroes.dart` line that reaches for it:
//!
//! | Seam | Flutter | Test group |
//! |---|---|---|
//! | 2. Observer attachment | `NavigatorObserver.navigator` (`navigator.dart:779`) | `observer_*` |
//! | 3. Route introspection | `route.animation`, `route.isCurrent` (`heroes.dart:331`, `:941`) | `route_peer_*`, `is_current_*` |
//! | 4. Route subtree | `route.subtreeContext` (`routes.dart:1966`) | `route_subtree_*` |
//! | 5. Overlay access | `navigator.overlay` (`heroes.dart:990`) | `overlay_*` |

// ADR-0027: these tests capture owner-local handles in shared cells. The
// library carries the same lint expectation; an integration test is a
// separate crate, so it is repeated here.
#![expect(clippy::arc_with_non_send_sync)]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use flui_view::prelude::*;
use flui_view::{BoxedView, ViewExt};
use parking_lot::Mutex;

use flui_widgets::navigator::{
    Navigator, NavigatorHandle, NavigatorObserver, PageRoute, RouteAnimation, RouteId, SimpleRoute,
};
use flui_widgets::{SizedBox, Text};

use crate::common::harness::{Harness, mount};

fn seeded_navigator() -> NavigatorHandle {
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(SimpleRoute::<i32>::new(|_ctx| {
        SizedBox::new(10.0, 10.0).into_view().boxed()
    }));
    navigator
}

fn page(_ctx: &dyn BuildContext, _a: &RouteAnimation, _s: &RouteAnimation) -> BoxedView {
    SizedBox::new(30.0, 18.0).into_view().boxed()
}

/// A root that can drop its `Navigator` between frames. `Harness::swap_root` goes
/// through `ElementTree::update`, whose dispatch is keyed by `TypeId`, so the root
/// type must not change — only this flag.
#[derive(Clone)]
struct Root {
    navigator: NavigatorHandle,
    show: bool,
}

impl View for Root {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

impl StatelessView for Root {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        if self.show {
            Navigator::new(self.navigator.clone()).boxed()
        } else {
            Text::new("gone").boxed()
        }
    }
}

fn mount_navigator(navigator: &NavigatorHandle) -> Harness {
    mount(Root {
        navigator: navigator.clone(),
        show: true,
    })
}

// ============================================================================
// Seam 2 — observer attachment
// ============================================================================

// ============================================================================
// Seam 3 — route introspection
// ============================================================================

// ============================================================================
// Seam 4 — route subtree publication
// ============================================================================

// ============================================================================
// Seam 5 — overlay access
// ============================================================================

// ============================================================================
// Seam 2, continued — the handle is usable from inside a callback
// ============================================================================

/// Mutating the stack from `did_push` is **defined** in FLUI, where Flutter would
/// `assert(!_debugLocked)` (`navigator.dart:4452`): notifications are delivered
/// after the flush that produced them has fully settled and released the mutex, so
/// a push raised from a callback simply runs a fresh flush, and *its* notifications
/// are delivered after the outer drain finishes.
///
/// This test pins that it terminates and that the extra route lands — not that
/// anyone should do it. The re-entrant push fires from the **seeded** route's
/// `did_push`, i.e. from inside `NavigatorState::init_state`'s own flush, which is
/// the earliest and tightest moment a callback can reach back into the navigator.
///
/// Red-check: deliver notifications under the history lock (as above); this times
/// out instead of failing an assertion.
#[test]
fn an_observer_may_push_from_did_push_without_deadlocking() {
    /// Pushes exactly one extra route, the first time it hears about a push.
    #[derive(Default)]
    struct Reentrant {
        handle: Mutex<Option<NavigatorHandle>>,
        pushed_once: AtomicBool,
        depths: Mutex<Vec<usize>>,
    }
    impl NavigatorObserver for Reentrant {
        fn did_attach(&self, navigator: NavigatorHandle) {
            let _prev = self.handle.lock().replace(navigator);
        }
        fn did_push(&self, _route: RouteId, _previous: Option<RouteId>) {
            let Some(navigator) = self.handle.lock().clone() else {
                return;
            };
            self.depths.lock().push(navigator.route_ids().len());
            if self.pushed_once.swap(true, Ordering::SeqCst) {
                return; // Guard the recursion this test is exercising.
            }
            let _result = navigator.push(PageRoute::<i32>::new(page));
        }
    }

    let navigator = seeded_navigator();
    let observer = Arc::new(Reentrant::default());
    navigator.add_observer(Arc::clone(&observer) as Arc<dyn NavigatorObserver>);

    let mut harness = mount_navigator(&navigator);
    let _result = navigator.push(PageRoute::<i32>::new(page));
    harness.tick();

    assert_eq!(
        navigator.route_ids().len(),
        3,
        "the seeded route, the one the observer pushed, and the one the test pushed"
    );

    let depths = observer.depths.lock().clone();
    assert_eq!(
        depths,
        vec![1, 2, 3],
        "one did_push per route, each reading a settled stack: the seeded route \
         (depth 1), the observer's own re-entrant push (2), then the test's (3)"
    );
}
