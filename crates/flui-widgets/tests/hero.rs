//! The `Hero` view, its per-route registry, and `HeroHandle`.
//!
//! No flight is started here. These tests pin the three things a flight will stand
//! on: a hero can be *found* by tag from outside the tree, it can be *measured*, and
//! it can be *told* to show a placeholder.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_foundation::ValueKey;
use flui_view::prelude::*;

use flui_widgets::__test_access::{HeroRegistry, HeroScope, HeroTag};
use flui_widgets::navigator::Hero;
use flui_widgets::{Center, SizedBox};

use crate::common::harness::mount;

fn tag(name: &'static str) -> HeroTag {
    HeroTag::new(ValueKey::new(name))
}

mod registry_reentry {
    use super::*;
    use flui_foundation::ViewKey;
    use flui_widgets::__test_access::{FlightManager, HeroControllerProbe as _};
    use std::any::Any;
    use std::cell::{Cell, RefCell};
    use std::fmt;
    use std::rc::Rc;

    thread_local! {
        static REGISTRY: RefCell<Option<HeroRegistry>> = const { RefCell::new(None) };
        static CALLBACK: Cell<Option<&'static str>> = const { Cell::new(None) };
        static READS: Cell<usize> = const { Cell::new(0) };
        static MANAGER: RefCell<Option<Rc<FlightManager>>> = const { RefCell::new(None) };
    }

    fn inspect(kind: &'static str) {
        if CALLBACK.get() != Some(kind) {
            return;
        }
        eprintln!("entered Hero tag {kind} reentry");
        let registry = REGISTRY.with(|slot| {
            slot.borrow()
                .as_ref()
                .expect("the registry is saved")
                .clone()
        });
        let _count = registry.len();
        let manager = MANAGER.with(|slot| slot.borrow().clone());
        if let Some(manager) = manager {
            eprintln!("entered Hero flight tag {kind} reentry");
            let _count = manager.len();
        }
        READS.set(READS.get() + 1);
    }

    #[derive(Clone)]
    struct Key(u64);

    impl ViewKey for Key {
        fn as_any(&self) -> &dyn Any {
            self
        }
        fn key_hash(&self) -> u64 {
            inspect("hash");
            7
        }
        fn key_eq(&self, other: &dyn ViewKey) -> bool {
            inspect("equality");
            other
                .as_any()
                .downcast_ref::<Self>()
                .is_some_and(|other| self.0 == other.0)
        }
        fn clone_key(&self) -> Box<dyn ViewKey> {
            Box::new(self.clone())
        }
        fn debug_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "reentrant-hero-{}", self.0)
        }
    }

    pub(super) fn run() {
        let Some(case) = crate::common::child_process::selected_case() else {
            crate::common::child_process::run_rows(
                "contracts::hero_flights",
                &["hash", "equality", "flight_hash", "flight_equality"],
            );
            return;
        };
        let registry = HeroRegistry::new();
        REGISTRY.with(|slot| *slot.borrow_mut() = Some(registry.clone()));
        if case == "hash" {
            CALLBACK.set(Some("hash"));
        }
        let mut laid = crate::common::lay_out(
            HeroScope::new(
                registry.clone(),
                Center::new().child(Hero::new(Key(1), SizedBox::new(30.0, 20.0))),
            ),
            crate::common::tight(400.0, 400.0),
        );
        assert_eq!(registry.len(), 1);
        if case == "equality" {
            CALLBACK.set(Some("equality"));
        }
        let hero = registry
            .get(&HeroTag::new(Key(1)))
            .expect("reentrant lookup finds the mounted Hero");
        assert!(hero.render_id().is_some());
        assert!(
            registry.get(&HeroTag::new(Key(2))).is_none(),
            "hash collision does not alias identity"
        );
        laid.pump_widget(SizedBox::shrink());
        assert_eq!(registry.len(), 0, "the mounted registration is withdrawn");
        CALLBACK.set(None);
        if case.starts_with("flight_") {
            fly(case.trim_start_matches("flight_"));
        }
        assert!(READS.get() > 0, "the authored callback read its registry");
        REGISTRY.with(|slot| slot.borrow_mut().take());
        crate::common::child_process::pass();
    }

    fn fly(operation: &str) {
        use flui_widgets::VsyncScope;
        use flui_widgets::navigator::{
            HeroController, Navigator, NavigatorHandle, NavigatorObserver, PageRoute,
        };
        let page = || {
            PageRoute::<i32>::new(|_ctx, _primary, _secondary| {
                Center::new()
                    .child(Hero::new(Key(1), SizedBox::new(30.0, 20.0)))
                    .boxed()
            })
            .transition_duration(std::time::Duration::from_millis(100))
        };
        let vsync = flui_animation::Vsync::new();
        let navigator = NavigatorHandle::new();
        navigator.seed_initial(page());
        let mut laid = crate::common::lay_out_animated(
            VsyncScope::new(vsync.clone(), Navigator::new(navigator.clone())),
            crate::common::tight(400.0, 400.0),
            vsync,
        );
        let controller = HeroController::new();
        navigator.add_observer(Arc::clone(&controller) as Arc<dyn NavigatorObserver>);
        MANAGER.with(|slot| *slot.borrow_mut() = Some(Rc::clone(controller.flights())));
        CALLBACK.set(Some(if operation == "hash" {
            "hash"
        } else {
            "equality"
        }));
        let _push = laid.enter_owner_scope(|| navigator.push(page()));
        for _ in 0..2 {
            laid.pump_for(std::time::Duration::from_millis(16));
        }
        let manager = controller.flights();
        assert_eq!(
            manager.len(),
            1,
            "a real route transition starts one flight"
        );
        let flight = manager
            .get(&HeroTag::new(Key(1)))
            .expect("the in-flight key remains readable");
        assert!(manager.get(&HeroTag::new(Key(2))).is_none());
        let owner = laid.pipeline_owner();
        assert!(
            owner.with(|owner| owner
                .render_tree()
                .iter()
                .any(|(_, node)| node.debug_name().ends_with("RenderIgnorePointer"))),
            "the accepted flight builds its shuttle"
        );
        for _ in 0..16 {
            laid.pump_for(std::time::Duration::from_millis(16));
        }
        assert_eq!(manager.len(), 0, "the flight lands with reentrant keys");
        assert!(
            !owner.with(|owner| owner
                .render_tree()
                .iter()
                .any(|(_, node)| node.debug_name().ends_with("RenderIgnorePointer"))),
            "landing removes the real shuttle"
        );
        drop(flight);
        laid.pump_widget(SizedBox::shrink());
        CALLBACK.set(None);
        MANAGER.with(|slot| slot.borrow_mut().take());
    }
}

pub(crate) fn registry_key_callbacks_can_read_their_registry() {
    registry_reentry::run();
}

pub(crate) fn changing_a_mounted_hero_tag_moves_its_registration() {
    let registry = HeroRegistry::new();
    let tree = |name, width| {
        HeroScope::new(
            registry.clone(),
            Center::new().child(Hero::new(ValueKey::new(name), SizedBox::new(width, 20.0))),
        )
    };
    let mut laid = crate::common::lay_out(tree("old", 30.0), crate::common::tight(400.0, 400.0));
    let original = registry
        .get(&tag("old"))
        .expect("the original Hero mounted");
    laid.pump_widget(tree("new", 45.0));
    assert!(
        registry.get(&tag("old")).is_none(),
        "retagging withdraws the old match key"
    );
    let updated = registry
        .get(&tag("new"))
        .expect("the new tag names the mounted Hero");
    assert!(
        updated.is_same(&original),
        "a match tag change preserves mounted identity"
    );
    let render = updated.render_id().expect("the updated Hero is laid out");
    assert_eq!(
        laid.pipeline_owner().with(|owner| owner.box_size(render)),
        Some(flui_foundation::geometry::Size::new(45.0, 20.0))
    );
}

pub(crate) fn retagging_to_a_duplicate_preserves_the_existing_winner() {
    let registry = HeroRegistry::new();
    let tree = |name: Option<&'static str>| {
        let second = name.map_or_else(
            || SizedBox::shrink().boxed(),
            |name| Hero::new(ValueKey::new(name), SizedBox::new(50.0, 20.0)).boxed(),
        );
        HeroScope::new(
            registry.clone(),
            flui_widgets::Row::new((
                Hero::new(ValueKey::new("winner"), SizedBox::new(30.0, 20.0)),
                second,
            )),
        )
    };
    let mut laid = crate::common::lay_out(tree(Some("old")), crate::common::tight(400.0, 100.0));
    let winner = registry
        .get(&tag("winner"))
        .expect("the first Hero mounted");
    let moved = registry.get(&tag("old")).expect("the second Hero mounted");
    laid.pump_widget(tree(Some("winner")));
    assert!(registry.get(&tag("old")).is_none());
    assert!(
        registry
            .get(&tag("winner"))
            .expect("the winner remains")
            .is_same(&winner)
    );
    assert!(!winner.is_same(&moved));
    assert_eq!(registry.len(), 1);
    laid.pump_widget(tree(None));
    assert!(
        registry
            .get(&tag("winner"))
            .expect("rejected duplicate cleanup preserves the winner")
            .is_same(&winner)
    );
    assert!(winner.start_flight(true).is_some());
}

pub(crate) fn replacing_a_hero_scope_moves_the_existing_hero() {
    let old = HeroRegistry::new();
    let next = HeroRegistry::new();
    let tree = |registry| {
        HeroScope::new(
            registry,
            Hero::new(ValueKey::new("shared"), SizedBox::new(30.0, 20.0)),
        )
    };
    let mut laid = crate::common::lay_out(tree(old.clone()), crate::common::tight(400.0, 400.0));
    let original = old
        .get(&tag("shared"))
        .expect("the original scope registered its Hero");
    laid.pump_widget(tree(next.clone()));
    assert!(
        old.get(&tag("shared")).is_none(),
        "the former scope cannot invite the moved Hero"
    );
    let moved = next
        .get(&tag("shared"))
        .expect("the new scope receives the Hero");
    assert!(moved.is_same(&original));
    assert!(
        moved.start_flight(true).is_some(),
        "the migrated handle still measures its mounted node"
    );
}

pub(crate) fn reparenting_a_hero_moves_registration_without_recreating_it() {
    reparent_hero(true, false, false);
}

pub(crate) fn reparenting_an_unscoped_hero_adopts_its_first_route() {
    reparent_hero(false, false, false);
}

pub(crate) fn reparenting_refreshes_a_build_time_miss_but_prunes_an_unread_one() {
    for stop_reading in [false, true] {
        reparent_hero(false, true, stop_reading);
    }
}

fn reparent_hero(initially_scoped: bool, read_scope: bool, stop_reading: bool) {
    #[derive(Clone)]
    struct KeyedHero {
        key: flui_view::GlobalKey<KeyedHeroState>,
        inits: std::rc::Rc<std::cell::Cell<usize>>,
        changes: std::rc::Rc<std::cell::Cell<usize>>,
        read_scope: bool,
    }
    struct KeyedHeroState {
        inits: std::rc::Rc<std::cell::Cell<usize>>,
        changes: std::rc::Rc<std::cell::Cell<usize>>,
    }
    impl View for KeyedHero {
        fn create_element(&self) -> flui_view::element::ElementKind {
            flui_view::element::ElementKind::stateful(self)
        }
        fn key(&self) -> Option<&dyn flui_foundation::ViewKey> {
            Some(&self.key)
        }
    }
    impl StatefulView for KeyedHero {
        type State = KeyedHeroState;
        fn create_state(&self) -> Self::State {
            KeyedHeroState {
                inits: std::rc::Rc::clone(&self.inits),
                changes: std::rc::Rc::clone(&self.changes),
            }
        }
    }
    impl ViewState<KeyedHero> for KeyedHeroState {
        fn init_state(&mut self, _ctx: &dyn LifecycleContext) {
            self.inits.set(self.inits.get() + 1);
        }
        fn did_change_dependencies(&mut self, _ctx: &dyn LifecycleContext) {
            self.changes.set(self.changes.get() + 1);
        }
        fn build(&self, view: &KeyedHero, ctx: &dyn BuildContext) -> impl IntoView {
            let scoped = view.read_scope && ctx.depend_on::<HeroScope, _>(|_| ()).is_some();
            Hero::new(
                ValueKey::new("shared"),
                SizedBox::new(if scoped { 45.0 } else { 30.0 }, 20.0),
            )
        }
    }
    let left = HeroRegistry::new();
    let right = HeroRegistry::new();
    let key = flui_view::GlobalKey::new();
    let inits = std::rc::Rc::new(std::cell::Cell::new(0));
    let changes = std::rc::Rc::new(std::cell::Cell::new(0));
    let tree = |moved, read_scope| {
        let child = KeyedHero {
            key: key.clone(),
            inits: std::rc::Rc::clone(&inits),
            changes: std::rc::Rc::clone(&changes),
            read_scope,
        }
        .boxed();
        let empty = SizedBox::shrink().boxed();
        let (left_child, right_child) = if moved {
            (empty, child)
        } else {
            (child, empty)
        };
        let left_child = if initially_scoped {
            HeroScope::new(left.clone(), left_child).boxed()
        } else {
            Center::new().child(left_child).boxed()
        };
        flui_widgets::Row::new((left_child, HeroScope::new(right.clone(), right_child)))
    };
    let mut laid =
        crate::common::lay_out(tree(false, read_scope), crate::common::tight(400.0, 100.0));
    let original = left.get(&tag("shared"));
    assert_eq!(original.is_some(), initially_scoped);
    if stop_reading {
        laid.pump_widget(tree(false, false));
    }
    laid.pump_widget(tree(true, read_scope && !stop_reading));
    assert_eq!(
        changes.get(),
        usize::from(read_scope && !stop_reading),
        "a missing build dependency survives reparenting only while it is still read"
    );
    assert!(
        left.get(&tag("shared")).is_none(),
        "the departed scope cannot invite the Hero"
    );
    let moved = right
        .get(&tag("shared"))
        .expect("the right scope registered the reparented Hero");
    if let Some(original) = original {
        assert!(
            moved.is_same(&original),
            "GlobalKey retakes the existing Hero subtree"
        );
    }
    let size = moved
        .start_flight(true)
        .expect("the moved Hero remains measurable");
    assert_eq!(
        size,
        flui_foundation::geometry::Size::new(
            if read_scope && !stop_reading {
                45.0
            } else {
                30.0
            },
            20.0
        )
    );
    laid.pump();
    assert_eq!(
        inits.get(),
        1,
        "GlobalKey does not recreate the mounted subtree"
    );
}

pub(crate) fn a_retained_hero_handle_does_not_keep_its_presentation_alive() {
    for flying in [false, true] {
        let registry = HeroRegistry::new();
        let mut laid = crate::common::lay_out(
            HeroScope::new(
                registry.clone(),
                Center::new().child(Hero::new(
                    ValueKey::new("retained"),
                    SizedBox::new(30.0, 20.0),
                )),
            ),
            crate::common::tight(400.0, 400.0),
        );
        let handle = registry.get(&tag("retained")).expect("the hero mounted");
        let owner = laid.pipeline_owner().downgrade();
        if flying {
            assert_eq!(
                handle.start_flight(true),
                Some(flui_foundation::geometry::Size::new(30.0, 20.0))
            );
            laid.pump();
        }
        laid.pump_widget(SizedBox::new(5.0, 5.0));
        assert!(registry.get(&tag("retained")).is_none());
        assert_eq!(handle.render_id(), None);
        assert_eq!(handle.start_flight(false), None);
        assert_eq!(handle.placeholder_size(), None);
        laid.pump_widget(HeroScope::new(
            registry.clone(),
            Center::new().child(Hero::new(
                ValueKey::new("retained"),
                SizedBox::new(60.0, 40.0),
            )),
        ));
        let replacement = registry
            .get(&tag("retained"))
            .expect("the same tag can mount again");
        assert!(!replacement.is_same(&handle));
        assert_eq!(
            replacement.start_flight(true),
            Some(flui_foundation::geometry::Size::new(60.0, 40.0))
        );
        handle.end_flight(false);
        assert_eq!(
            replacement.placeholder_size(),
            Some(flui_foundation::geometry::Size::new(60.0, 40.0)),
            "the stale handle cannot retire its replacement"
        );
        drop(laid);
        assert!(
            owner.upgrade().is_none(),
            "a retained Hero handle cannot own the closed presentation"
        );
        assert_eq!(
            handle.placeholder_size(),
            None,
            "unmount withdraws the frozen placeholder"
        );
        handle.end_flight(false);
    }
}

pub(crate) fn unmount_releases_hero_configuration_despite_a_retained_handle() {
    struct Capture {
        drops: std::rc::Rc<std::cell::Cell<usize>>,
        observe: Box<dyn Fn()>,
    }
    impl Drop for Capture {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
            (self.observe)();
        }
    }
    let registry = HeroRegistry::new();
    let retained = std::rc::Rc::new(std::cell::RefCell::new(
        None::<flui_widgets::__test_access::HeroHandle>,
    ));
    let drops = std::rc::Rc::new(std::cell::Cell::new(0));
    let capture = std::rc::Rc::new(Capture {
        drops: std::rc::Rc::clone(&drops),
        observe: Box::new({
            let retained = std::rc::Rc::downgrade(&retained);
            let registry = registry.clone();
            move || {
                let Some(retained) = retained.upgrade() else {
                    return;
                };
                let handle = retained
                    .borrow()
                    .as_ref()
                    .expect("the mounted handle was retained")
                    .clone();
                assert!(
                    registry.get(&tag("retained")).is_none(),
                    "registration is withdrawn before authored destruction"
                );
                assert_eq!(handle.placeholder_size(), None);
                assert_eq!(
                    handle.start_flight(true),
                    None,
                    "destruction observes an inert handle"
                );
                handle.end_flight(false);
            }
        }),
    });
    let released = std::rc::Rc::downgrade(&capture);
    let mut laid = crate::common::lay_out(
        HeroScope::new(
            registry.clone(),
            Hero::new(ValueKey::new("retained"), SizedBox::new(30.0, 20.0)).create_rect_tween(
                move |begin, end| {
                    let _capture = &capture;
                    flui_animation::RectTween::new(begin, end)
                },
            ),
        ),
        crate::common::tight(400.0, 400.0),
    );
    let handle = registry.get(&tag("retained")).expect("the hero mounted");
    *retained.borrow_mut() = Some(handle.clone());
    laid.pump_widget(SizedBox::new(5.0, 5.0));
    assert_eq!(handle.render_id(), None);
    assert!(
        released.upgrade().is_none(),
        "unmount releases the authored configuration while the handle remains alive"
    );
    assert_eq!(handle.start_flight(false), None);
    assert_eq!(
        drops.get(),
        1,
        "authored retirement ran once while the handle remained alive"
    );
}

// ============================================================================
// Registration
// ============================================================================

// ============================================================================
// Flight state — placeholder building
// ============================================================================

// ============================================================================
// Measurement
// ============================================================================

// ============================================================================
// The placeholder preserves the child element without a GlobalKey
// ============================================================================

/// **Preserving child state without a `GlobalKey`.** A stateful hero child must keep its state when
/// the hero enters and leaves a flight; a framework could guarantee this with a
/// `GlobalKey`.
///
/// FLUI needs no key: `HeroState::build` emits the fixed chain
/// `SizedBox(size?) → Offstage(show) → child` in **both** the not-in-flight and the
/// in-flight-keep-child cases. The child sits at the same depth under
/// the same two view types throughout, so reconciliation migrates its element in place
/// rather than rebuilding it. `create_state` therefore runs exactly once across a whole
/// flight.
///
/// Red-check: revert `HeroState::build` to the old toggling shape (pass-through out of
/// flight, `SizedBox → Offstage → child` in flight) — the child's depth changes, its
/// element is rebuilt, and `create_state` runs again.
pub(crate) fn a_hero_child_keeps_its_state_across_a_flight_without_a_global_key() {
    /// Counts how many times its state is created.
    #[derive(Clone)]
    struct Counter(Arc<AtomicUsize>);
    impl View for Counter {
        fn create_element(&self) -> flui_view::element::ElementKind {
            flui_view::element::ElementKind::stateful(self)
        }
    }
    impl StatefulView for Counter {
        type State = CounterState;
        fn create_state(&self) -> Self::State {
            self.0.fetch_add(1, Ordering::SeqCst);
            CounterState
        }
    }
    struct CounterState;
    impl ViewState<Counter> for CounterState {
        fn build(&self, _view: &Counter, _ctx: &dyn BuildContext) -> impl IntoView {
            SizedBox::new(30.0, 20.0)
        }
    }

    #[derive(Clone)]
    struct Stage {
        registry: HeroRegistry,
        creations: Arc<AtomicUsize>,
    }
    impl View for Stage {
        fn create_element(&self) -> flui_view::element::ElementKind {
            flui_view::element::ElementKind::stateless(self)
        }
    }
    impl StatelessView for Stage {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            HeroScope::new(
                self.registry.clone(),
                Center::new().child(Hero::new(
                    ValueKey::new("a"),
                    Counter(Arc::clone(&self.creations)),
                )),
            )
        }
    }

    let registry = HeroRegistry::new();
    let creations = Arc::new(AtomicUsize::new(0));
    let mut harness = mount(Stage {
        registry: registry.clone(),
        creations: Arc::clone(&creations),
    });
    assert_eq!(creations.load(Ordering::SeqCst), 1, "built once on mount");

    let hero = registry.get(&tag("a")).expect("registered");

    // Into flight, keeping the child (the *from* hero of a push).
    hero.start_flight(true).expect("measured");
    harness.tick();
    assert_eq!(
        creations.load(Ordering::SeqCst),
        1,
        "the child's element migrated into the placeholder — not rebuilt"
    );

    // And back out.
    hero.end_flight(false);
    harness.tick();
    assert_eq!(
        creations.load(Ordering::SeqCst),
        1,
        "…and migrated back out again, state intact, with no GlobalKey"
    );
}
