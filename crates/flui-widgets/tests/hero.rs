//! The `Hero` view, its per-route registry, and `HeroHandle`.
//!
//! No flight is started here. These tests pin the three things a flight will stand
//! on: a hero can be *found* by tag from outside the tree, it can be *measured*, and
//! it can be *told* to show a placeholder.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_foundation::ValueKey;
use flui_foundation::geometry::Size;
use flui_view::ViewExt;
use flui_view::prelude::*;

use flui_widgets::__test_access::{HeroHandle, HeroRegistry, HeroScope, HeroTag};
use flui_widgets::navigator::Hero;
use flui_widgets::{Center, Column, MainAxisSize, SizedBox, Text};

use crate::common::harness::{Harness, mount};

fn tag(name: &'static str) -> HeroTag {
    HeroTag::new(ValueKey::new(name))
}

/// A root that hosts a `HeroScope` — the ambient registry a route provides — and can
/// swap what is inside it, so heroes can be mounted and unmounted at will.
#[derive(Clone)]
struct Stage {
    registry: HeroRegistry,
    content: Content,
}

/// What the stage currently shows. A `TypeId`-stable root: `Harness::swap_root` goes
/// through `ElementTree::update`, so only this flag may change between frames.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Content {
    /// One hero, tag `"a"`.
    OneHero,
    /// Two heroes sharing tag `"a"` — the duplicate case.
    DuplicateTags,
    /// The same `Column`, with only the *first* of the two duplicates left. Swapping
    /// `DuplicateTags` to this unmounts the loser and nothing else.
    DuplicateTagsLoserRemoved,
    /// No heroes at all.
    Empty,
}

impl View for Stage {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

impl StatelessView for Stage {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let inside: BoxedView = match self.content {
            Content::OneHero => Hero::new(ValueKey::new("a"), SizedBox::new(30.0, 20.0)).boxed(),
            Content::DuplicateTags => Column::new(vec![
                Hero::new(ValueKey::new("a"), SizedBox::new(30.0, 20.0)).boxed(),
                Hero::new(ValueKey::new("a"), SizedBox::new(11.0, 12.0)).boxed(),
            ])
            .main_axis_size(MainAxisSize::Min)
            .boxed(),
            Content::DuplicateTagsLoserRemoved => Column::new(vec![
                Hero::new(ValueKey::new("a"), SizedBox::new(30.0, 20.0)).boxed(),
            ])
            .main_axis_size(MainAxisSize::Min)
            .boxed(),
            Content::Empty => Text::new("no heroes").boxed(),
        };
        // `Harness::mount` roots the tree at **tight** 800x600. A `Hero` directly
        // under that root would be forced to fill the screen and every size assertion
        // below would read 800x600. `Center` hands its child loose constraints, which
        // is also what a hero sees inside a real page.
        HeroScope::new(self.registry.clone(), Center::new().child(inside))
    }
}

fn stage(registry: &HeroRegistry, content: Content) -> Stage {
    Stage {
        registry: registry.clone(),
        content,
    }
}

fn mount_stage(registry: &HeroRegistry, content: Content) -> Harness {
    mount(stage(registry, content))
}

// ============================================================================
// Registration
// ============================================================================

/// A `Hero` registers itself with the nearest enclosing `HeroScope` on mount and
/// removes itself on unmount. This is what replaces `_allHeroesFor`'s element walk
/// (`heroes.dart:317-321`) — and the reason no downcast from `&dyn View` is needed.
///
/// Red-check: delete `registry.register(…)` from `HeroState::init_state`, or
/// `registry.deregister(…)` from `HeroState::dispose`.
#[test]
fn hero_registers_its_tag_with_the_enclosing_route_and_deregisters_on_dispose() {
    let registry = HeroRegistry::new();
    assert_eq!(registry.len(), 0);

    let mut harness = mount_stage(&registry, Content::OneHero);
    assert_eq!(registry.len(), 1);
    assert!(registry.get(&tag("a")).is_some());
    assert!(
        registry.get(&tag("b")).is_none(),
        "tags compare by ViewKey value, not by identity"
    );

    harness.swap_root(stage(&registry, Content::Empty));
    assert_eq!(registry.len(), 0, "the hero deregistered on dispose");
}

/// Two heroes sharing a tag inside one route: **log and keep the first**, never panic.
///
/// Flutter throws inside an `assert` (`heroes.dart:287-305`) — debug-only — and in
/// release `result[tag] = heroState` silently keeps the *last*. FLUI chose
/// first-wins-and-log: a duplicate tag is a caller mistake, not a framework invariant
/// (PANIC-POLICY), and last-wins would make the survivor depend on mount order.
///
/// The losing hero must also be **harmless on unmount**: its `dispose` must not evict
/// the hero that won the tag. That is the `Arc::ptr_eq` check in
/// `HeroRegistry::deregister`, and it is the half a naive port gets wrong.
///
/// Red-check (each fails on its own):
/// * make `register` overwrite instead of returning early — the second hero wins, and
///   `winner_size` reads 11x12;
/// * make `deregister` remove by tag alone (drop the `held.is(handle)` test) — the
///   loser's dispose evicts the winner and `registry.len()` is 0.
#[test]
fn duplicate_tags_in_one_route_log_and_drop_the_second() {
    let registry = HeroRegistry::new();
    let mut harness = mount_stage(&registry, Content::DuplicateTags);

    assert_eq!(registry.len(), 1, "one tag survives, and nothing panicked");

    let winner = registry
        .get(&tag("a"))
        .expect("the first hero holds the tag");
    let owner = harness.pipeline_owner();
    let winner_size = owner
        .with(|owner| owner.box_size(winner.render_id().expect("attached")))
        .expect("laid out");
    assert_eq!(
        (winner_size.width, winner_size.height),
        (30.0, 20.0),
        "the FIRST hero kept the tag; last-wins would measure the 11x12 one"
    );

    // Unmount **only the loser**. Its `dispose` must not evict the hero that holds the
    // tag — that is the `Arc::ptr_eq` check, and unmounting both would hide a
    // deregister-by-tag bug behind the winner's own cleanup.
    harness.swap_root(stage(&registry, Content::DuplicateTagsLoserRemoved));
    assert_eq!(
        registry.len(),
        1,
        "the loser's dispose must not evict the winner"
    );
    assert!(
        registry
            .get(&tag("a"))
            .is_some_and(|held| held.is_same(&winner))
    );

    // And the winner still cleans up after itself.
    harness.swap_root(stage(&registry, Content::Empty));
    assert_eq!(registry.len(), 0);
}

// ============================================================================
// Flight state — placeholder building
// ============================================================================

/// The size of the one `RenderConstrainedBox` under the hero's anchor, i.e. what the
/// hero's `SizedBox` placeholder resolved to.
fn hero_box_size(harness: &Harness, hero: &HeroHandle) -> Size {
    harness
        .pipeline_owner()
        .with(|owner| owner.box_size(hero.render_id().expect("attached")))
        .expect("laid out")
}

/// `_HeroState.startFlight` (`heroes.dart:381-389`) freezes the hero at its committed
/// size: `_placeholderSize = box.size`, then `setState`.
///
/// The hero's own box keeps that size across the rebuild — which is the point. A
/// flight replaces the hero's contents with a fixed-size hole so the layout around it
/// does not reflow while the shuttle is in the overlay.
///
/// Red-check: return `Some(Size::ZERO)` from `start_flight` instead of the measured
/// size — the placeholder collapses.
#[test]
fn start_flight_makes_the_hero_show_a_placeholder_of_the_measured_size() {
    let registry = HeroRegistry::new();
    let mut harness = mount_stage(&registry, Content::OneHero);
    let hero = registry.get(&tag("a")).expect("registered");

    assert_eq!(hero.placeholder_size(), None, "not in flight");
    let before = hero_box_size(&harness, &hero);
    assert_eq!((before.width, before.height), (30.0, 20.0));

    let captured = hero.start_flight(true).expect("committed layout to freeze");
    assert_eq!(captured, before);
    assert_eq!(hero.placeholder_size(), Some(before));

    harness.tick();
    let after = hero_box_size(&harness, &hero);
    assert_eq!(after, before, "the placeholder holds the hero's old size");
}

/// `_HeroState.endFlight` (`heroes.dart:397-408`): drop the placeholder, show the
/// child again.
///
/// Red-check: delete `*placeholder = None;` from `HeroHandle::end_flight`.
#[test]
fn end_flight_restores_child() {
    let registry = HeroRegistry::new();
    let mut harness = mount_stage(&registry, Content::OneHero);
    let hero = registry.get(&tag("a")).expect("registered");

    hero.start_flight(false).expect("measured");
    harness.tick();
    assert!(hero.placeholder_size().is_some());

    hero.end_flight(false);
    harness.tick();

    assert_eq!(hero.placeholder_size(), None);
    // The fixed chain keeps a transparent `Offstage(offstage: false)`
    // around the child even out of flight, so its *presence* is no longer the signal.
    // What matters is that the child is **visible**: an `Offstage` that were still
    // hiding it would zero the anchor, so the real size is the honest check.
    assert_eq!(
        hero_box_size(&harness, &hero),
        Size::new(30.0, 20.0),
        "the child is back, at its own size — the Offstage is off, not hiding it"
    );
}

// ============================================================================
// Measurement
// ============================================================================

/// A `Hero` outside any `HeroScope` is inert, not a panic: `_allHeroesFor` simply
/// never visits it, and `ctx.get::<HeroScope, _>` answers `None`.
///
/// Red-check: `expect("BUG: a Hero must be inside a route")` in `HeroState::init_state`.
#[test]
fn a_hero_outside_any_route_registers_with_nothing_and_still_builds() {
    #[derive(Clone)]
    struct Bare;
    impl View for Bare {
        fn create_element(&self) -> flui_view::element::ElementKind {
            flui_view::element::ElementKind::stateless(self)
        }
    }
    impl StatelessView for Bare {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            Center::new().child(Hero::new(ValueKey::new("orphan"), SizedBox::new(5.0, 5.0)))
        }
    }

    let harness = mount(Bare);
    let names = harness.render_debug_names();
    assert!(
        names
            .iter()
            .any(|name| name.ends_with("RenderSubtreeAnchor")),
        "the hero built, anchor and all: {names:?}"
    );
}

// ============================================================================
// The placeholder preserves the child element without a GlobalKey
// ============================================================================

/// **Preserving child state without a `GlobalKey`.** A stateful hero child must keep its state when
/// the hero enters and leaves a flight — Flutter guarantees this with `_HeroState._key`
/// (`heroes.dart:363`, `:434`), a `GlobalKey`.
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
#[test]
fn a_hero_child_keeps_its_state_across_a_flight_without_a_global_key() {
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
