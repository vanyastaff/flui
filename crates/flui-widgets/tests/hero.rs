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
    #[derive(Clone)]
    struct KeyedHero {
        key: flui_view::GlobalKey<KeyedHeroState>,
        inits: std::rc::Rc<std::cell::Cell<usize>>,
    }
    struct KeyedHeroState(std::rc::Rc<std::cell::Cell<usize>>);
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
            KeyedHeroState(std::rc::Rc::clone(&self.inits))
        }
    }
    impl ViewState<KeyedHero> for KeyedHeroState {
        fn init_state(&mut self, _ctx: &dyn LifecycleContext) {
            self.0.set(self.0.get() + 1);
        }
        fn build(&self, _view: &KeyedHero, _ctx: &dyn BuildContext) -> impl IntoView {
            Hero::new(ValueKey::new("shared"), SizedBox::new(30.0, 20.0))
        }
    }
    let left = HeroRegistry::new();
    let right = HeroRegistry::new();
    let key = flui_view::GlobalKey::new();
    let inits = std::rc::Rc::new(std::cell::Cell::new(0));
    let tree = |moved| {
        let child = KeyedHero {
            key: key.clone(),
            inits: std::rc::Rc::clone(&inits),
        }
        .boxed();
        let empty = SizedBox::shrink().boxed();
        let (left_child, right_child) = if moved {
            (empty, child)
        } else {
            (child, empty)
        };
        flui_widgets::Row::new((
            HeroScope::new(left.clone(), left_child),
            HeroScope::new(right.clone(), right_child),
        ))
    };
    let mut laid = crate::common::lay_out(tree(false), crate::common::tight(400.0, 100.0));
    let original = left
        .get(&tag("shared"))
        .expect("the left scope registered the Hero");
    laid.pump_widget(tree(true));
    assert!(
        left.get(&tag("shared")).is_none(),
        "the departed scope cannot invite the Hero"
    );
    let moved = right
        .get(&tag("shared"))
        .expect("the right scope registered the reparented Hero");
    assert!(
        moved.is_same(&original),
        "GlobalKey retakes the existing Hero subtree"
    );
    moved
        .start_flight(true)
        .expect("the moved Hero remains measurable");
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
