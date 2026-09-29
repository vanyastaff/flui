//! [`GestureArenaScope`] — provides a shared, clock-bound [`GestureArena`] to a
//! subtree so overlapping `GestureDetector`s compete in one arena and a binding
//! can drive their deadlines.
//!
//! Flutter parity: Flutter has a single ambient `GestureArenaManager` on
//! `GestureBinding`; every recognizer reaches it through the binding. FLUI is
//! non-singleton, so the arena is handed down explicitly as inherited data — the
//! analogue of that ambient binding, scoped to a subtree.

use flui_interaction::arena::{GestureArena, SweepModel};
use flui_view::prelude::*;
use flui_view::{BoxedView, InheritedView, impl_inherited_view};

/// Provides a shared [`GestureArena`] to its descendant gesture detectors.
///
/// A binding (or a test harness) wraps the application subtree in
/// `GestureArenaScope::new(binding.arena().clone(), child)`. Every
/// gesture consumer below reads this arena ambiently in `init_state` through
/// [`GestureArenaScope::of`] and builds all of its recognizers
/// against it. Two consequences follow:
///
/// 1. **Competition for free** — overlapping detectors along a hit-test path add
///    their recognizers to the *same* arena entry for one contact, so the
///    standard Flutter disambiguation (front-member-wins, reject-on-loss) plays
///    out across detectors, not just within one.
/// 2. **Deadline polling** — because [`GestureArena`] is `Arc`-backed, the clone
///    the scope hands down and the one the binding holds are the same arena and
///    the same clock. The binding's `pump_frame` polls that arena's deadlines,
///    so clock-driven gestures (long-press hold, double-tap give-up) resolve on
///    the virtual timeline.
///
/// Gesture consumers require this scope. Missing presentation ownership is an
/// invariant violation during `init_state`; consumers never create a private
/// arena or take over the binding's close/sweep lifecycle.
///
/// The provided data — the arena handle — never changes for a given scope, so
/// [`update_should_notify`](InheritedView::update_should_notify) is always
/// `false`: a descendant reads the handle once in `init_state` and never needs a
/// dependency-driven rebuild on it.
#[derive(Clone)]
pub struct GestureArenaScope {
    /// The shared arena handed to descendants. Cloning the scope clones this
    /// `Arc`-backed handle, so all clones observe the same arena state + clock.
    arena: GestureArena,
    /// The wrapped subtree the arena is provided to.
    child: BoxedView,
}

impl GestureArenaScope {
    /// Wrap `child` in a scope that provides `arena` to its descendants.
    ///
    /// # Panics
    ///
    /// Panics when `arena` is not binding-driven. A presentation scope is the
    /// binding's lifecycle boundary; admitting a self-driven arena would create
    /// a second close/sweep owner below that boundary.
    #[must_use]
    pub fn new(arena: GestureArena, child: impl IntoView) -> Self {
        assert_eq!(
            arena.sweep_model(),
            SweepModel::BindingDriven,
            "BUG: GestureArenaScope requires a BindingDriven arena owned by the presentation \
             binding; SelfDriven arenas cannot back a presentation scope",
        );
        Self {
            arena,
            child: BoxedView(Box::new(child.into_view())),
        }
    }

    /// Resolve the presentation's exact shared arena without registering an
    /// inherited dependency.
    ///
    /// Gesture recognizers capture their arena for their whole lifetime, so
    /// consumers call this once from `init_state`; the scope handle itself is
    /// immutable and never triggers dependency-driven rebuilds.
    ///
    /// # Panics
    ///
    /// Panics when the consumer is mounted outside a presentation
    /// [`GestureArenaScope`].
    #[must_use]
    pub fn of(ctx: &dyn BuildContext) -> GestureArena {
        ctx.get::<Self, _>(|scope| scope.arena.clone()).expect(
            "BUG: gesture consumers must be mounted beneath GestureArenaScope; \
             the presentation binding is the sole GestureArena lifecycle owner",
        )
    }

    /// The shared arena this scope provides — what a descendant detector reads
    /// in `init_state` to build its recognizers against.
    #[must_use]
    pub fn arena(&self) -> &GestureArena {
        &self.arena
    }
}

impl std::fmt::Debug for GestureArenaScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GestureArenaScope")
            .field("arena", &self.arena)
            .finish_non_exhaustive()
    }
}

impl InheritedView for GestureArenaScope {
    type Data = GestureArena;

    fn data(&self) -> &Self::Data {
        &self.arena
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, _old: &Self) -> bool {
        // The arena handle is fixed for a scope's lifetime; descendants read it
        // once in `init_state` and never depend on it for rebuilds.
        false
    }
}

impl_inherited_view!(GestureArenaScope);
