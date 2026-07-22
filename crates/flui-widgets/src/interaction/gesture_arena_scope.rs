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

#[cfg(test)]
mod tests {
    use flui_interaction::arena::GestureArena;
    use flui_interaction::ids::PointerId;
    use flui_interaction::recognizers::tap::TapGestureRecognizer;

    use super::*;
    use crate::SizedBox;

    /// Registers a real arena member on `pointer` via a `TapGestureRecognizer`
    /// (the arena member trait is sealed, so a concrete recognizer is the only
    /// way to prove two `GestureArena` handles observe the same underlying
    /// state from outside `flui-interaction`).
    fn register_member(arena: &GestureArena, pointer: PointerId) {
        let recognizer = TapGestureRecognizer::new(arena.clone());
        arena.add(pointer, recognizer);
    }

    #[test]
    fn arena_returns_the_provided_handle() {
        let arena = GestureArena::new();
        let scope = GestureArenaScope::new(arena.clone(), SizedBox::shrink());

        register_member(&arena, PointerId::PRIMARY);

        assert!(
            scope.arena().contains(PointerId::PRIMARY),
            "arena() must return the exact shared handle passed to new(), not \
             an independent clone -- a member added via the original handle \
             must be visible through arena()'s handle",
        );
    }

    #[test]
    fn data_and_child_expose_the_arena_and_wrapped_subtree() {
        let arena = GestureArena::new();
        let scope = GestureArenaScope::new(arena.clone(), SizedBox::shrink());

        register_member(&arena, PointerId::PRIMARY);

        assert!(
            InheritedView::data(&scope).contains(PointerId::PRIMARY),
            "InheritedView::data() must expose the same shared arena as arena()",
        );
        // `child()` returns a `&dyn View` over the wrapped subtree; reaching
        // it without panicking proves the child was stored, not dropped.
        let _child: &dyn View = InheritedView::child(&scope);
    }

    #[test]
    fn update_should_notify_is_always_false() {
        let scope_a = GestureArenaScope::new(GestureArena::new(), SizedBox::shrink());
        let scope_b = GestureArenaScope::new(GestureArena::new(), SizedBox::shrink());
        assert!(
            !scope_a.update_should_notify(&scope_b),
            "the arena handle is fixed for a scope's lifetime; descendants must \
             never be told to rebuild off of it",
        );
    }

    #[test]
    fn debug_reports_the_arena() {
        let scope = GestureArenaScope::new(GestureArena::new(), SizedBox::shrink());
        let debug = format!("{scope:?}");
        assert!(
            debug.starts_with("GestureArenaScope"),
            "Debug output must name the type, got: {debug}",
        );
    }
}
