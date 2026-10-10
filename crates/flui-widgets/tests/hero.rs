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
