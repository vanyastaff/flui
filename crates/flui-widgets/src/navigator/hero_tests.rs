//! ADR-0021 U4: the `Hero` view, its per-route registry, and `HeroHandle`.
//!
//! No flight is started here. These tests pin the three things a flight will stand
//! on: a hero can be *found* by tag from outside the tree, it can be *measured*, and
//! it can be *told* to show a placeholder.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_foundation::ValueKey;
use flui_types::Size;
use flui_types::geometry::px;
use flui_view::ViewExt;
use flui_view::prelude::*;
use parking_lot::Mutex;

use super::hero::{Hero, HeroHandle, HeroRegistry, HeroScope, HeroTag};
use crate::test_harness::{Harness, mount};
use crate::{Center, Column, MainAxisSize, SizedBox, Text};

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
    /// Two heroes, tags `"a"` and `"b"`.
    TwoTags,
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
            Content::TwoTags => Column::new(vec![
                Hero::new(ValueKey::new("a"), SizedBox::new(30.0, 20.0)).boxed(),
                Hero::new(ValueKey::new("b"), SizedBox::new(11.0, 12.0)).boxed(),
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
/// release `result[tag] = heroState` silently keeps the *last*. ADR-0021 D8 chose
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
        .read()
        .box_size(winner.render_id().expect("attached"))
        .expect("laid out");
    assert_eq!(
        (winner_size.width.0, winner_size.height.0),
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

/// A hero's `RenderId` comes from `RenderBox::attach` and goes at `detach` — the
/// ADR-0021 U2 anchor, reused. `BuildContext::find_render_object` walks strict
/// *ancestors* and can never answer this.
///
/// Red-check: delete `fn detach` from `RenderSubtreeAnchor` — the id survives the
/// hero and a controller measures a disposed node.
#[test]
fn hero_render_id_is_none_before_attach_and_cleared_after_dispose() {
    let registry = HeroRegistry::new();
    let mut harness = mount_stage(&registry, Content::OneHero);

    let hero = registry.get(&tag("a")).expect("registered");
    let render_id = hero.render_id().expect("attached during mount");
    assert!(
        harness
            .pipeline_owner()
            .read()
            .box_size(render_id)
            .is_some(),
        "and laid out by the first frame"
    );

    harness.swap_root(stage(&registry, Content::Empty));
    assert_eq!(
        hero.render_id(),
        None,
        "a handle kept past dispose names no render node"
    );
}

// ============================================================================
// Flight state — placeholder building
// ============================================================================

/// The size of the one `RenderConstrainedBox` under the hero's anchor, i.e. what the
/// hero's `SizedBox` placeholder resolved to.
fn hero_box_size(harness: &Harness, hero: &HeroHandle) -> Size {
    let owner = harness.pipeline_owner();
    let owner = owner.read();
    owner
        .box_size(hero.render_id().expect("attached"))
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
    assert_eq!((before.width.0, before.height.0), (30.0, 20.0));

    let captured = hero.start_flight(true).expect("committed layout to freeze");
    assert_eq!(captured, before);
    assert_eq!(hero.placeholder_size(), Some(before));

    harness.tick();
    let after = hero_box_size(&harness, &hero);
    assert_eq!(after, before, "the placeholder holds the hero's old size");
}

/// `shouldIncludeChildInPlaceholder` (`heroes.dart:370-380`) — `true` for the *from*
/// hero of a push: the original subtree is kept, offstage, so its state survives.
///
/// Observable as the child's render object still being in the tree.
///
/// Red-check: drop the `.child(Offstage…)` from `HeroState::build`'s include-child
/// arm — the child's render object disappears.
#[test]
fn start_flight_with_include_child_preserves_child_offstage() {
    let registry = HeroRegistry::new();
    let mut harness = mount_stage(&registry, Content::OneHero);
    let hero = registry.get(&tag("a")).expect("registered");

    hero.start_flight(true).expect("measured");
    harness.tick();

    assert!(hero.includes_child());
    let names = harness.render_debug_names();
    assert!(
        names.iter().any(|name| name.ends_with("RenderOffstage")),
        "the child is kept, offstage: {names:?}"
    );
    assert_eq!(
        names
            .iter()
            .filter(|name| name.ends_with("RenderConstrainedBox"))
            .count(),
        2,
        "the placeholder SizedBox *and* the original child's SizedBox: {names:?}"
    );
}

/// `if (showPlaceholder && !_shouldIncludeChild) return SizedBox(width:…, height:…);`
/// (`heroes.dart:423-425`) — the destination hero's child is not needed while the
/// shuttle flies, so it is dropped entirely.
///
/// Red-check: ignore `include_child` in `HeroState::build` and always keep the child
/// — the `RenderOffstage` reappears.
#[test]
fn start_flight_without_include_child_drops_the_child_from_the_placeholder() {
    let registry = HeroRegistry::new();
    let mut harness = mount_stage(&registry, Content::OneHero);
    let hero = registry.get(&tag("a")).expect("registered");

    hero.start_flight(false).expect("measured");
    harness.tick();

    assert!(!hero.includes_child());
    let names = harness.render_debug_names();
    assert!(
        !names.iter().any(|name| name.ends_with("RenderOffstage")),
        "no child is preserved: {names:?}"
    );
    assert_eq!(
        hero_box_size(&harness, &hero),
        Size::new(px(30.0), px(20.0)),
        "but the hole is still the hero's old size"
    );
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
    // The fixed chain (ADR-0021 §7k) keeps a transparent `Offstage(offstage: false)`
    // around the child even out of flight, so its *presence* is no longer the signal.
    // What matters is that the child is **visible**: an `Offstage` that were still
    // hiding it would zero the anchor, so the real size is the honest check.
    assert_eq!(
        hero_box_size(&harness, &hero),
        Size::new(px(30.0), px(20.0)),
        "the child is back, at its own size — the Offstage is off, not hiding it"
    );
}

/// `if (keepPlaceholder || _placeholderSize == null) return;` (`heroes.dart:398-400`)
/// — a diverted flight ends without un-freezing the hero, because the next flight is
/// about to freeze it again.
///
/// Also pins the `_placeholderSize == null` half: `end_flight` on a hero that never
/// flew is a no-op, not a spurious rebuild.
///
/// Red-check: ignore `keep_placeholder` in `HeroHandle::end_flight`.
#[test]
fn end_flight_keep_placeholder_keeps_placeholder() {
    let registry = HeroRegistry::new();
    let mut harness = mount_stage(&registry, Content::OneHero);
    let hero = registry.get(&tag("a")).expect("registered");

    hero.end_flight(false);
    assert_eq!(hero.placeholder_size(), None, "not in flight: a no-op");

    let frozen = hero.start_flight(true).expect("measured");
    harness.tick();

    hero.end_flight(true);
    harness.tick();

    assert_eq!(
        hero.placeholder_size(),
        Some(frozen),
        "a kept placeholder outlives the flight that made it"
    );
}

// ============================================================================
// Measurement
// ============================================================================

/// `_HeroFlightManifest._boundingBoxFor` (`heroes.dart:501-509`): the hero's box in an
/// ancestor's coordinate space, `transformRect(getTransformTo(ancestor), Offset.zero &
/// box.size)`.
///
/// Here the ancestor is the `HeroScope`'s own subtree root; the offset is what the
/// `Column` gave the second hero.
///
/// Red-check: use `transform_to(ancestor, hero)` (arguments swapped) in
/// `HeroHandle::bounding_box_in` — the rect lands at the origin.
#[test]
fn hero_bounding_box_is_taken_in_the_ancestors_coordinate_space() {
    let registry = HeroRegistry::new();
    let harness = mount_stage(&registry, Content::TwoTags);

    let first = registry.get(&tag("a")).expect("registered");
    let second = registry.get(&tag("b")).expect("registered");

    let root = harness
        .pipeline_owner()
        .read()
        .root_id()
        .expect("a render root");

    let first_rect = first.bounding_box_in(root).expect("measurable");
    let second_rect = second.bounding_box_in(root).expect("measurable");

    // Relative, because `Center` puts the Column wherever it likes on an 800x600 root.
    assert_eq!(
        (second_rect.min.y - first_rect.min.y).0,
        20.0,
        "the second hero sits below the first, past its 20px height"
    );
    assert_eq!((first_rect.width().0, first_rect.height().0), (30.0, 20.0));
    assert_eq!(
        (second_rect.width().0, second_rect.height().0),
        (11.0, 12.0)
    );
    assert!(first_rect.is_finite() && second_rect.is_finite());
}

/// A hero that has left the tree has no render node, so it has no rect and nothing to
/// freeze. It says so rather than guessing. Flutter asserts `box.hasSize` here and
/// crashes.
///
/// (The *other* `None` — attached but not yet laid out — is pinned by
/// `a_hero_bounding_box_is_none_before_layout_commits`; here `render_id()` is already
/// `None`, so `box_size` is never consulted.)
///
/// Red-check: return a zero `Rect` from `HeroHandle::bounding_box_in` when
/// `render_id()` is `None`.
#[test]
fn an_unmounted_hero_measures_to_none() {
    let registry = HeroRegistry::new();
    let mut harness = mount_stage(&registry, Content::OneHero);
    let hero = registry.get(&tag("a")).expect("registered");
    let root = harness
        .pipeline_owner()
        .read()
        .root_id()
        .expect("a render root");
    assert!(hero.bounding_box_in(root).is_some());

    harness.swap_root(stage(&registry, Content::Empty));

    assert_eq!(
        hero.bounding_box_in(root),
        None,
        "no render node, so no rect — not a zero rect"
    );
    assert_eq!(hero.start_flight(true), None, "and nothing to freeze");
}

/// A hero's `RenderId` exists from `attach`, which runs during **build**; its geometry
/// does not exist until layout commits. `bounding_box_in` must answer `None` in that
/// window rather than guess — the same two-stage rule ADR-0021 U2 established for
/// `RouteSubtree`.
///
/// The probe reads the hero through the registry from inside the hero's own child
/// build, i.e. mid-frame: registration and `attach` have both happened, layout has not.
///
/// Red-check: `owner.box_size(render_id).unwrap_or(Size::ZERO)` in `bounding_box_in`
/// — the probe then sees a zero rect instead of `None`.
#[test]
fn a_hero_bounding_box_is_none_before_layout_commits() {
    #[derive(Clone)]
    struct Probe {
        registry: HeroRegistry,
        seen: Arc<Mutex<Vec<(bool, bool)>>>,
    }
    impl View for Probe {
        fn create_element(&self) -> flui_view::element::ElementKind {
            flui_view::element::ElementKind::stateless(self)
        }
    }
    impl StatelessView for Probe {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            if let Some(hero) = self.registry.get(&tag("probe")) {
                // Measured against the hero's *own* node: `transform_to(id, id)` is the
                // identity, so the only thing that can answer `None` is `box_size` —
                // which is exactly the guard under test. (The render root does not
                // exist yet during the mount `build_scope`, so it cannot be the
                // ancestor here.)
                let rect = hero.render_id().and_then(|own| hero.bounding_box_in(own));
                self.seen
                    .lock()
                    .push((hero.render_id().is_some(), rect.is_some()));
            }
            SizedBox::new(8.0, 6.0)
        }
    }

    #[derive(Clone)]
    struct ProbeStage {
        registry: HeroRegistry,
        seen: Arc<Mutex<Vec<(bool, bool)>>>,
    }
    impl View for ProbeStage {
        fn create_element(&self) -> flui_view::element::ElementKind {
            flui_view::element::ElementKind::stateless(self)
        }
    }
    impl StatelessView for ProbeStage {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            HeroScope::new(
                self.registry.clone(),
                Center::new().child(Hero::new(
                    ValueKey::new("probe"),
                    Probe {
                        registry: self.registry.clone(),
                        seen: Arc::clone(&self.seen),
                    },
                )),
            )
        }
    }

    let registry = HeroRegistry::new();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let harness = mount(ProbeStage {
        registry: registry.clone(),
        seen: Arc::clone(&seen),
    });

    let seen = seen.lock().clone();
    assert_eq!(seen.len(), 1, "the hero's child built once");
    assert_eq!(
        seen[0],
        (true, false),
        "the anchor had published its RenderId, but layout had not committed"
    );

    // …and after the frame, the very same hero measures.
    let hero = registry.get(&tag("probe")).expect("registered");
    let root = harness
        .pipeline_owner()
        .read()
        .root_id()
        .expect("a render root");
    assert!(hero.bounding_box_in(root).is_some());
}

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
// U5.2 — the placeholder preserves the child element without a GlobalKey
// ============================================================================

/// **The D2 decision, made concrete.** A stateful hero child must keep its state when
/// the hero enters and leaves a flight — Flutter guarantees this with `_HeroState._key`
/// (`heroes.dart:363`, `:434`), a `GlobalKey`.
///
/// FLUI needs no key: `HeroState::build` emits the fixed chain
/// `SizedBox(size?) → Offstage(show) → child` in **both** the not-in-flight and the
/// in-flight-keep-child cases (ADR-0021 §7k). The child sits at the same depth under
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

/// A `HeroHandle` kept past its hero's unmount is inert: it names no render node, has
/// no committed size to freeze, and every driver call is a harmless no-op — the same
/// stale-capability contract every owned handle in this crate honours.
///
/// Red-check: delete `fn detach` from `RenderSubtreeAnchor` — the stale handle keeps a
/// live `render_id` and `start_flight` on a gone hero returns `Some`.
#[test]
fn a_stale_hero_handle_is_inert_after_unmount() {
    let registry = HeroRegistry::new();
    let mut harness = mount_stage(&registry, Content::OneHero);
    let hero = registry.get(&tag("a")).expect("registered");
    assert!(hero.render_id().is_some());

    harness.swap_root(stage(&registry, Content::Empty));

    assert_eq!(hero.render_id(), None, "no render node");
    assert_eq!(hero.start_flight(true), None, "nothing to freeze");
    assert_eq!(hero.placeholder_size(), None, "and it never entered flight");
    hero.end_flight(false); // must not panic
}
