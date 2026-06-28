//! [`GestureArenaScope`] — provides a shared, clock-bound [`GestureArena`] to a
//! subtree so overlapping `GestureDetector`s compete in one arena and a binding
//! can drive their deadlines.
//!
//! Flutter parity: Flutter has a single ambient `GestureArenaManager` on
//! `GestureBinding`; every recognizer reaches it through the binding. FLUI is
//! non-singleton, so the arena is handed down explicitly as inherited data — the
//! analogue of that ambient binding, scoped to a subtree.

use flui_interaction::arena::GestureArena;
use flui_view::prelude::*;
use flui_view::{BoxedView, InheritedView, impl_inherited_view};

/// Provides a shared [`GestureArena`] to its descendant gesture detectors.
///
/// A binding (or a test harness) wraps the application subtree in
/// `GestureArenaScope::new(binding.arena().clone(), child)`. Every
/// `GestureDetector` below reads this arena ambiently in `init_state` (via
/// `ctx.get::<GestureArenaScope, _>(..)`) and builds all of its recognizers
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
/// A `GestureDetector` with no `GestureArenaScope` above it falls back to a
/// private arena it closes itself — preserving the standalone
/// tap/secondary-tap/pan behavior, at the cost of no deadline polling (no binding
/// drives a private arena).
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
    #[must_use]
    pub fn new(arena: GestureArena, child: impl IntoView) -> Self {
        Self {
            arena,
            child: BoxedView(Box::new(child.into_view())),
        }
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
