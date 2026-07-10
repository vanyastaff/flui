//! ADR-0021 U2 seams 2–5, the executable half.
//!
//! There is no `Hero` here, no `HeroController`, and no flight. These tests pin
//! the four things a future `HeroController` reaches for, each against the
//! `heroes.dart` line that reaches for it:
//!
//! | Seam | Flutter | Test group |
//! |---|---|---|
//! | 2. Observer attachment | `NavigatorObserver.navigator` (`navigator.dart:779`) | `observer_*` |
//! | 3. Route introspection | `route.animation`, `route.isCurrent` (`heroes.dart:331`, `:941`) | `route_peer_*`, `is_current_*` |
//! | 4. Route subtree | `route.subtreeContext` (`routes.dart:1966`) | `route_subtree_*` |
//! | 5. Overlay access | `navigator.overlay` (`heroes.dart:990`) | `overlay_*` |

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_foundation::RenderId;
use flui_rendering::pipeline::PipelineOwner;
use flui_types::Size;
use flui_view::prelude::*;
use flui_view::{BoxedView, ViewExt};
use parking_lot::{Mutex, RwLock};

use super::binding::TransitionGroup;
use super::navigator::{Navigator, NavigatorHandle};
use super::observer::NavigatorObserver;
use super::overlay_route::{RouteAnimation, SimpleRoute};
use super::page_route::{PageRoute, PopupRoute};
use super::route::RouteId;
use super::subtree::RouteSubtree;
use crate::overlay::{InsertPosition, OverlayEntry};
use crate::test_harness::{Harness, mount};
use crate::{Opacity, SizedBox, Text};

/// `Harness::mount` roots the tree at tight 800x600, and a `ModalRoute`'s page sits
/// under `Stack(fit: expand)` (`routes.dart:2350-2356`, merged into one entry here).
/// So a route's page **fills the screen**, and its size cannot distinguish the
/// anchor from the `RenderTheater` above it — the render-tree position does.
const SCREEN: Size = Size::new(
    flui_types::geometry::px(800.0),
    flui_types::geometry::px(600.0),
);

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

fn unmount_navigator(harness: &mut Harness, navigator: &NavigatorHandle) {
    harness.swap_root(Root {
        navigator: navigator.clone(),
        show: false,
    });
}

// ============================================================================
// Seam 2 — observer attachment
// ============================================================================

/// Records the attach/detach protocol, and keeps whatever handle it was given.
#[derive(Default)]
struct AttachSpy {
    name: &'static str,
    log: Arc<Mutex<Vec<String>>>,
    handle: Mutex<Option<NavigatorHandle>>,
}

impl AttachSpy {
    fn new(name: &'static str, log: &Arc<Mutex<Vec<String>>>) -> Arc<Self> {
        Arc::new(Self {
            name,
            log: Arc::clone(log),
            handle: Mutex::new(None),
        })
    }

    fn handle(&self) -> Option<NavigatorHandle> {
        self.handle.lock().clone()
    }
}

impl NavigatorObserver for AttachSpy {
    fn did_attach(&self, navigator: NavigatorHandle) {
        self.log.lock().push(format!("attach:{}", self.name));
        *self.handle.lock() = Some(navigator);
    }

    fn did_detach(&self) {
        self.log.lock().push(format!("detach:{}", self.name));
    }
}

/// **Seam 2.** Flutter's `initState` writes `NavigatorObserver._navigators[observer]
/// = this` (`navigator.dart:3836`), which is what `observer.navigator` reads.
///
/// Red-check: delete `self.shared.attach_observers(&self.handle())` from
/// `NavigatorState::init_state`.
#[test]
fn an_observer_registered_before_mount_is_attached_when_the_navigator_mounts() {
    let navigator = seeded_navigator();
    let log = Arc::new(Mutex::new(Vec::new()));
    let spy = AttachSpy::new("a", &log);
    navigator.add_observer(Arc::clone(&spy) as Arc<dyn NavigatorObserver>);

    assert!(
        spy.handle().is_none(),
        "an observer on an unmounted navigator holds nothing — Flutter's Expando \
         entry is null until initState"
    );

    let _harness = mount_navigator(&navigator);

    let handle = spy
        .handle()
        .expect("the mounted navigator attached its observer");
    assert!(handle.is_mounted());
    assert_eq!(
        handle.route_ids().len(),
        1,
        "the handle names this navigator"
    );
}

/// The attach must precede the seeded flush, or the first `did_push` an observer
/// sees arrives before it has anything to act on. Flutter attaches at `:3834-3837`
/// and only then calls `restoreState` → `_flushHistoryUpdates` (`:3922-3934`).
///
/// Red-check: move `attach_observers` below the `self.shared.mutate(…)` flush in
/// `NavigatorState::init_state`.
#[test]
fn observers_are_attached_before_the_seeded_flush_notifies_them() {
    /// Pushes `"push"` only if it already holds a handle.
    #[derive(Default)]
    struct OrderSpy {
        log: Mutex<Vec<&'static str>>,
        attached: Mutex<bool>,
    }
    impl NavigatorObserver for OrderSpy {
        fn did_attach(&self, _navigator: NavigatorHandle) {
            *self.attached.lock() = true;
            self.log.lock().push("attach");
        }
        fn did_push(&self, _route: RouteId, _previous: Option<RouteId>) {
            self.log.lock().push(if *self.attached.lock() {
                "push"
            } else {
                "push-unattached"
            });
        }
    }

    let navigator = seeded_navigator();
    let spy = Arc::new(OrderSpy::default());
    navigator.add_observer(Arc::clone(&spy) as Arc<dyn NavigatorObserver>);
    let _harness = mount_navigator(&navigator);

    assert_eq!(*spy.log.lock(), vec!["attach", "push"]);
}

/// Flutter's `didUpdateWidget` path (`navigator.dart:4058-4061`): an observer added
/// to a live navigator is attached at once, not at the next mount.
///
/// Red-check: drop the `if observers_attached { observer.did_attach(…) }` arm from
/// `NavigatorHandle::add_observer`.
#[test]
fn an_observer_registered_after_mount_is_attached_immediately() {
    let navigator = seeded_navigator();
    let _harness = mount_navigator(&navigator);

    let log = Arc::new(Mutex::new(Vec::new()));
    let spy = AttachSpy::new("late", &log);
    navigator.add_observer(Arc::clone(&spy) as Arc<dyn NavigatorObserver>);

    assert!(spy.handle().is_some());
    assert_eq!(*log.lock(), vec!["attach:late"]);
}

/// Attachment order is registration order, and so is detachment — Flutter iterates
/// `_effectiveObservers` forwards in both loops (`:3834`, `:4106`).
///
/// Red-check: `for observer in self.observers().into_iter().rev()` in
/// `NavigatorShared::attach_observers`.
#[test]
fn observers_attach_and_detach_in_registration_order() {
    let navigator = seeded_navigator();
    let log = Arc::new(Mutex::new(Vec::new()));
    for name in ["a", "b", "c"] {
        navigator.add_observer(AttachSpy::new(name, &log) as Arc<dyn NavigatorObserver>);
    }

    let mut harness = mount_navigator(&navigator);
    assert_eq!(*log.lock(), vec!["attach:a", "attach:b", "attach:c"]);

    log.lock().clear();
    unmount_navigator(&mut harness, &navigator);
    assert_eq!(*log.lock(), vec!["detach:a", "detach:b", "detach:c"]);
}

/// **Seam 2, the other half.** Flutter nulls the Expando entry in `deactivate`
/// (`navigator.dart:4108`) and asserts it stayed null through `dispose` (`:4133`).
/// FLUI's `ElementBase::unmount` reaches `ViewState::dispose` without necessarily
/// passing `deactivate`, so both hooks detach — and the observer must still hear it
/// exactly once.
///
/// Red-check: delete `fn dispose` (and `fn deactivate`) from
/// `impl ViewState<Navigator> for NavigatorState`.
#[test]
fn unmounting_the_navigator_detaches_its_observers_exactly_once() {
    let navigator = seeded_navigator();
    let log = Arc::new(Mutex::new(Vec::new()));
    let spy = AttachSpy::new("a", &log);
    navigator.add_observer(Arc::clone(&spy) as Arc<dyn NavigatorObserver>);

    let mut harness = mount_navigator(&navigator);
    log.lock().clear();

    unmount_navigator(&mut harness, &navigator);

    assert_eq!(
        *log.lock(),
        vec!["detach:a"],
        "detach fires once even though deactivate and dispose both call it"
    );
}

/// A handle an observer kept past its detach is **inert, not wrong**: it still
/// resolves, and it truthfully reports that the navigator has left the tree —
/// exactly the `navigator == null` check `_startHeroTransition` performs before
/// touching anything (`heroes.dart:970-972`, `:995-997`).
///
/// Red-check: make `NavigatorHandle::is_mounted` return `true` unconditionally.
#[test]
fn a_handle_kept_past_detach_reports_the_navigator_as_unmounted() {
    let navigator = seeded_navigator();
    let log = Arc::new(Mutex::new(Vec::new()));
    let spy = AttachSpy::new("a", &log);
    navigator.add_observer(Arc::clone(&spy) as Arc<dyn NavigatorObserver>);

    let mut harness = mount_navigator(&navigator);
    let stale = spy.handle().expect("attached");
    assert!(stale.is_mounted());

    unmount_navigator(&mut harness, &navigator);

    assert!(
        !stale.is_mounted(),
        "a detached handle must not claim a mounted navigator"
    );
    assert!(
        !stale.overlay().is_mounted(),
        "and neither may its overlay — the capability outlives the tree, the tree \
         does not outlive itself"
    );
    // The stack itself survives: `NavigatorHandle` owns it, and Flutter's routes
    // likewise outlive a deactivated `NavigatorState`. What a stale handle must not
    // do is *paint* — `inserting_into_a_stale_navigators_overlay_is_inert` pins that.
    assert_eq!(stale.route_ids().len(), 1);
}

// ============================================================================
// Seam 3 — route introspection
// ============================================================================

/// `HeroController` starts a flight only between two `PageRoute`s
/// (`heroes.dart:941-948`, `_maybeStartHeroTransition`'s `is! PageRoute` guard) and
/// reads `route.animation` off each. `route_peer` is both facts.
///
/// Red-check: return `TransitionGroup::Default` from `PageRoute`'s `.group(…)` call.
#[test]
fn route_peer_reports_the_transition_family_and_animation() {
    let navigator = seeded_navigator();
    let mut harness = mount_navigator(&navigator);

    let page_id = {
        let _result = navigator.push(PageRoute::<i32>::new(page));
        navigator.current().expect("pushed")
    };
    let popup_id = {
        let _result = navigator.push(PopupRoute::<i32>::new(page));
        navigator.current().expect("pushed")
    };
    harness.tick();

    let page_peer = navigator
        .route_peer(page_id)
        .expect("a PageRoute publishes a peer");
    assert_eq!(page_peer.group, TransitionGroup::Page);

    let popup_peer = navigator
        .route_peer(popup_id)
        .expect("a PopupRoute publishes a peer");
    assert_eq!(
        popup_peer.group,
        TransitionGroup::Default,
        "a PopupRoute is not a PageRoute: `canTransitionTo(popup) == false`"
    );

    let seeded = navigator.route_ids()[0];
    assert!(
        navigator.route_peer(seeded).is_none(),
        "a SimpleRoute is not a TransitionRoute — `nextRoute is TransitionRoute` is false"
    );
}

/// Flutter's `Route.isCurrent` (`routes.dart:196-201`), the guard in
/// `Hero._allHeroesFor` (`heroes.dart:331`).
///
/// Red-check: `self.current() != Some(id)` in `NavigatorHandle::is_current`.
#[test]
fn is_current_names_only_the_topmost_route() {
    let navigator = seeded_navigator();
    let mut harness = mount_navigator(&navigator);
    let seeded = navigator.route_ids()[0];

    let _result = navigator.push(PageRoute::<i32>::new(page));
    harness.tick();
    let pushed = navigator.current().expect("pushed");

    assert!(navigator.is_current(pushed));
    assert!(!navigator.is_current(seeded));
}

// ============================================================================
// Seam 4 — route subtree publication
// ============================================================================

/// `RenderObject::debug_name` defaults to the full type path; only objects that
/// override it (as `RenderSubtreeAnchor` does) return a bare name. Comparing on the
/// suffix is what the rest of the navigator's render probes do.
fn is_render(owner: &PipelineOwner, id: RenderId, name: &str) -> bool {
    owner
        .render_tree()
        .get(id)
        .expect("the published render id names a live node")
        .debug_name()
        .ends_with(name)
}

/// **Seam 4.** `route.subtreeContext` is `null` until the page mounts
/// (`routes.dart:1966` — `_subtreeKey.currentContext`), and a route that is not a
/// `ModalRoute` has no `_subtreeKey` at all. Registration at `install()` is *not*
/// resolution: the cell exists, and answers `None`.
///
/// Red-check: delete `binding.publish_subtree(…)` from `ModalRoute::install`.
/// (The "`None` before the page builds" halves are pinned by
/// `route_subtree_ids_are_published_before_layout_commits` and
/// `a_stale_handle_resolves_no_route_subtree_after_the_navigator_unmounts`, which
/// red-check the `attach` and `init_state` publications independently.)
#[test]
fn route_subtree_is_none_until_the_routes_page_is_mounted_and_attached() {
    let navigator = seeded_navigator();
    let mut harness = mount_navigator(&navigator);

    let seeded = navigator.route_ids()[0];
    assert!(
        navigator.route_subtree(seeded).is_none(),
        "a SimpleRoute has no page anchor"
    );

    let _result = navigator.push(PageRoute::<i32>::new(page));
    let pushed = navigator.current().expect("pushed");
    assert!(
        navigator.route_subtree(pushed).is_none(),
        "the route is registered at install() but its page has never been built"
    );

    harness.tick();
    assert!(navigator.route_subtree(pushed).is_some());
}

/// The published `RenderId` must name the route's **own page anchor** — inside the
/// transitions, wrapping only `buildPage`'s output. Flutter hangs `_subtreeKey` on
/// the `RepaintBoundary` around `buildPage` and nothing else
/// (`routes.dart:1229-1231`); everything above it belongs to the transition, to the
/// `Offstage`, or to the overlay's `RenderTheater`, and measuring against any of
/// them would give `HeroController` a coordinate space that slides and fades.
///
/// The route carries a **non-identity** transition (`Opacity`) on purpose: with the
/// default jump-cut builder the transitions add no render object, so anchoring the
/// transitions' output and anchoring the page would produce the same tree and this
/// test could not tell them apart.
///
/// Red-check: in `ModalScopeState::build`, anchor `(view.transitions)(…)`'s result
/// instead of `page` — the anchor's parent becomes `RenderOffstage` and its child
/// `RenderOpacity`, and both assertions below flip.
#[test]
fn route_subtree_names_the_page_anchor_not_the_transition_offstage_or_theater() {
    let navigator = seeded_navigator();
    let mut harness = mount_navigator(&navigator);

    let route = PageRoute::<i32>::new(page)
        .transitions(|_ctx, _animation, _secondary, child| Opacity::new(0.5).child(child).boxed());
    let _result = navigator.push(route);
    let pushed = navigator.current().expect("pushed");
    harness.tick();

    let RouteSubtree { render_id, .. } = navigator.route_subtree(pushed).expect("mounted");
    let owner = harness.pipeline_owner();
    let owner = owner.read();
    let tree = owner.render_tree();

    assert!(is_render(&owner, render_id, "RenderSubtreeAnchor"));

    let parent = tree.parent(render_id).expect("the anchor has a parent");
    assert!(
        is_render(&owner, parent, "RenderOpacity"),
        "the anchor sits *below* the transitions, so it never moves with them"
    );

    let children = tree.children(render_id);
    assert_eq!(children.len(), 1);
    assert!(
        is_render(&owner, children[0], "RenderConstrainedBox"),
        "and *above* the page — `SizedBox` builds a RenderConstrainedBox"
    );

    assert_eq!(owner.box_size(render_id), Some(SCREEN));
}

/// **The two-stage contract.** A `RouteSubtree` resolves from `attach`, which
/// happens during **build**. Layout has not run then, and `box_size` says so. Only
/// the post-frame phase of that same frame sees committed geometry (ADR-0021 U1.5).
///
/// This is why `SubtreeAnchor::get()` alone is *not* layout-readiness, and why
/// `HeroController._maybeStartHeroTransition` measures from a post-frame callback
/// (`heroes.dart:964-973`) rather than from `didPush`.
///
/// Red-check (either half fails on its own):
/// * move `RenderSubtreeAnchor`'s publication from `attach` to `perform_layout` —
///   the build-phase `route_subtree(…)` goes `None`;
/// * make `PipelineOwner::box_size` fall back to `Size::ZERO` before layout — the
///   build-phase `box_size` assertion goes `Some(ZERO)`.
#[test]
fn route_subtree_ids_are_published_before_layout_commits() {
    /// What one build of the page saw: the ids, and what geometry they resolved to
    /// *at that moment*.
    type Sighting = (Option<RouteSubtree>, Option<Size>);

    /// Reads the seam from inside the page's own `build`, i.e. mid-frame.
    #[derive(Clone)]
    struct Probe {
        navigator: NavigatorHandle,
        route: RouteId,
        owner: Arc<RwLock<PipelineOwner>>,
        seen: Arc<Mutex<Vec<Sighting>>>,
    }

    impl View for Probe {
        fn create_element(&self) -> flui_view::element::ElementKind {
            flui_view::element::ElementKind::stateless(self)
        }
    }

    impl StatelessView for Probe {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            let subtree = self.navigator.route_subtree(self.route);
            let owner = self
                .owner
                .try_read()
                .expect("BUG: the pipeline owner is not write-locked during build_scope");
            let size = subtree.and_then(|s| owner.box_size(s.render_id));
            self.seen.lock().push((subtree, size));
            SizedBox::new(30.0, 18.0)
        }
    }

    let navigator = seeded_navigator();
    let mut harness = mount_navigator(&navigator);
    let owner = harness.pipeline_owner();
    let seen = Arc::new(Mutex::new(Vec::new()));

    // The id must exist before the page builder can name it, so push a route whose
    // page closure captures it — `RouteId::next()` is what `push` will mint.
    let route_cell: Arc<Mutex<Option<RouteId>>> = Arc::new(Mutex::new(None));
    let probe_parts = (navigator.clone(), Arc::clone(&owner), Arc::clone(&seen));
    let route_for_page = Arc::clone(&route_cell);
    let _result = navigator.push(PageRoute::<i32>::new(move |_ctx, _a, _s| {
        let route = route_for_page
            .lock()
            .expect("the page is built after `push` returns");
        Probe {
            navigator: probe_parts.0.clone(),
            route,
            owner: Arc::clone(&probe_parts.1),
            seen: Arc::clone(&probe_parts.2),
        }
        .boxed()
    }));
    let pushed = navigator.current().expect("pushed");
    *route_cell.lock() = Some(pushed);

    // What the post-frame callback of the very same frame sees.
    let after_layout: Arc<Mutex<Option<Size>>> = Arc::new(Mutex::new(None));
    let after_cb = Arc::clone(&after_layout);
    let owner_cb = Arc::clone(&owner);
    let navigator_cb = navigator.clone();
    harness
        .scheduler()
        .add_post_frame_callback(Box::new(move |_| {
            let subtree = navigator_cb.route_subtree(pushed);
            *after_cb.lock() = subtree.and_then(|s| owner_cb.read().box_size(s.render_id));
        }));

    harness.tick();

    let seen = seen.lock().clone();
    assert_eq!(seen.len(), 1, "the page built once this frame");
    let (subtree, size_during_build) = seen[0];
    assert!(
        subtree.is_some(),
        "the anchor publishes from `attach`, which runs while the subtree mounts — \
         before its own child builds"
    );
    assert_eq!(
        size_during_build, None,
        "…and `attach` is not layout: the anchor has no committed size yet"
    );

    assert_eq!(
        *after_layout.lock(),
        Some(SCREEN),
        "the post-frame callback of that same frame sees committed geometry"
    );
}

/// A disposed route must not be resolvable. Flutter drops the whole `Route` object,
/// so `subtreeContext` goes with it; FLUI withdraws the registry entry in
/// `ModalRoute::dispose`, and the page's own `dispose`/`detach` empty the cell.
///
/// Red-check: delete `binding.withdraw_subtree()` from `ModalRoute::dispose` — the
/// registry then keeps a cell that the *unmounted* page has already emptied, so
/// `resolve()` returns `None` anyway. That is the trap: assert on the registry, not
/// only on the resolution.
#[test]
fn a_disposed_route_is_withdrawn_from_the_subtree_registry() {
    let navigator = seeded_navigator();
    let mut harness = mount_navigator(&navigator);

    let _result = navigator.push(PageRoute::<i32>::new(page));
    let pushed = navigator.current().expect("pushed");
    harness.tick();
    assert!(navigator.route_subtree(pushed).is_some());
    assert_eq!(navigator.tracked_subtree_count(), 1);

    assert!(navigator.pop());
    harness.tick();

    assert!(navigator.route_subtree(pushed).is_none());
    assert_eq!(
        navigator.tracked_subtree_count(),
        0,
        "a disposed route's registry entry leaks forever if `dispose` does not \
         withdraw it — invisible through `route_subtree`, which the emptied cell \
         already answers `None`"
    );
}

/// Unmounting the navigator empties **both halves** of every route's cell: the
/// `RouteSubtreeAnchor` element is disposed, and the `RenderSubtreeAnchor` is
/// detached. A `HeroController` holding a stale `NavigatorHandle` therefore
/// measures nothing.
///
/// Asserted half by half, because `resolve()` is an `AND`: it answers `None` when
/// *either* retraction fires, so a test that only checked `route_subtree(…)` would
/// stay green with one of them deleted.
///
/// Red-check (each half fails on its own):
/// * delete `fn dispose` from `RouteSubtreeAnchorState` — the element half stays;
/// * delete `fn detach` from `RenderSubtreeAnchor` — the render half stays.
#[test]
fn a_stale_handle_resolves_no_route_subtree_after_the_navigator_unmounts() {
    let navigator = seeded_navigator();
    let mut harness = mount_navigator(&navigator);

    let _result = navigator.push(PageRoute::<i32>::new(page));
    let pushed = navigator.current().expect("pushed");
    harness.tick();
    assert!(navigator.route_subtree(pushed).is_some());

    unmount_navigator(&mut harness, &navigator);

    let (element, render) = navigator
        .route_subtree_parts(pushed)
        .expect("the route is still on the stack, so its cell is still registered");
    assert_eq!(element, None, "the anchor element was disposed");
    assert_eq!(render, None, "the anchor render object was detached");
    assert!(
        navigator.route_subtree(pushed).is_none(),
        "so there is nothing to measure"
    );
}

// ============================================================================
// Seam 5 — overlay access
// ============================================================================

/// **Seam 5.** `_startHeroTransition` reaches `navigator.overlay` and inserts the
/// flight's entry above the routes (`heroes.dart:990`, `:1073`). This is that
/// capability, and nothing more: `Overlay` and `OverlayEntry` stay unexported.
///
/// Red-check: `InsertPosition::Below(bottom)` instead of `Top` — the flight would
/// paint under the routes it is flying between.
#[test]
fn the_navigators_overlay_accepts_a_flight_entry_above_every_route() {
    let navigator = seeded_navigator();
    let mut harness = mount_navigator(&navigator);
    let _result = navigator.push(PageRoute::<i32>::new(page));
    harness.tick();

    let builds = Arc::new(AtomicUsize::new(0));
    let builds_for_entry = Arc::clone(&builds);
    let flight = OverlayEntry::new(move |_ctx| {
        builds_for_entry.fetch_add(1, Ordering::SeqCst);
        SizedBox::new(1.0, 1.0).into_view().boxed()
    });

    let before = navigator.overlay().entry_ids();
    navigator.overlay().insert(&flight, &InsertPosition::Top);
    harness.tick();

    let after = navigator.overlay().entry_ids();
    assert_eq!(after.len(), before.len() + 1);
    assert_eq!(
        after.last().copied(),
        Some(flight.id()),
        "the flight entry paints above every route"
    );
    assert_eq!(builds.load(Ordering::SeqCst), 1, "and it was built");

    flight.remove();
    harness.tick();
    assert_eq!(navigator.overlay().entry_ids(), before);
}

/// The overlay half of the stale-handle contract. `OverlayHandle` mutation on an
/// unmounted overlay is defined behaviour (ADR-0019 U1), so a `HeroController` that
/// wakes up after its navigator left the tree cannot corrupt anything — it simply
/// has no overlay to insert into. Flutter's guard is the explicit
/// `if (navigator == null || overlay == null) return;` (`heroes.dart:995-997`), and
/// `is_mounted` is what a FLUI controller will ask instead.
///
/// Red-check: make `OverlayHandle::is_mounted` return `true` unconditionally.
///
/// That mutation reddens the `is_mounted` assertion only. **The `builds == 0`
/// assertion is a regression guard, not a proof**: an unmounted overlay holds no
/// `RebuildHandle`, so `insert` schedules nothing *by construction*, and there is
/// no mutation short of handing an unmounted overlay a live rebuild handle that
/// makes it build. Said plainly rather than dressed up as a red-check.
#[test]
fn inserting_into_a_stale_navigators_overlay_is_inert() {
    let navigator = seeded_navigator();
    let mut harness = mount_navigator(&navigator);
    unmount_navigator(&mut harness, &navigator);

    let overlay = navigator.overlay().clone();
    assert!(!overlay.is_mounted());

    let builds = Arc::new(AtomicUsize::new(0));
    let builds_for_entry = Arc::clone(&builds);
    let flight = OverlayEntry::new(move |_ctx| {
        builds_for_entry.fetch_add(1, Ordering::SeqCst);
        SizedBox::new(1.0, 1.0).into_view().boxed()
    });
    overlay.insert(&flight, &InsertPosition::Top);
    harness.tick();

    assert_eq!(
        builds.load(Ordering::SeqCst),
        0,
        "an unmounted overlay builds nothing"
    );
}
