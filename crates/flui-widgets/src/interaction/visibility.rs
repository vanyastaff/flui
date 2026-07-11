//! [`Visibility`] — show or hide a child, with optional state preservation.

use std::fmt;

use flui_view::prelude::StatelessView;
use flui_view::{BoxedView, BuildContext, IntoView, ViewExt};

use crate::interaction::Offstage;
use crate::layout::SizedBox;

/// Controls whether its child is shown, hidden, or hidden while keeping its
/// state alive in the element tree.
///
/// The three operating modes, in ascending cost:
///
/// 1. **Default (`maintain_state = false`):** when `visible` is `true` the
///    child is present in the tree; when `false` the child is replaced by
///    `replacement` (default [`SizedBox::shrink`]). The child's state is
///    discarded when it transitions from visible to invisible.
///
/// 2. **State-preserving (`maintain_state = true`):** the child is always in
///    the tree, wrapped in an [`Offstage`] widget whose `offstage` flag is the
///    inverse of `visible`. This keeps the child's state alive and allows it
///    to snap back to its last state when made visible again. Paint and
///    hit-testing are suppressed by `Offstage` when `offstage = true`.
///
/// 3. **Interactive-while-hidden (`maintain_interactivity = true`, requires
///    `maintain_state = true`):** deferred — full support requires
///    `maintainSize` and a `TickerMode` widget, neither of which exist in FLUI
///    yet. Setting `maintain_interactivity = true` is accepted but has no
///    additional effect beyond `maintain_state` behaviour; see the divergence
///    note below.
///
/// Flutter parity: `widgets/indexed_stack.dart` `Visibility`.
///
/// **Divergences from Flutter:**
/// - `maintainAnimation` — deferred. `TickerMode` now exists (2026-07-11), so
///   this is a straight wiring job whenever a consumer wants it: gate the
///   hidden subtree's `TickerMode` on the flag instead of always muting.
/// - `maintainSize` (requires `maintainAnimation`) — deferred.
/// - `maintainInteractivity` (requires `maintainSize` in Flutter) — accepted
///   but currently a no-op beyond `maintain_state`; full semantics deferred
///   with `maintainSize`.
/// - Flutter also wraps the result in `_VisibilityScope`; FLUI omits that
///   scope widget (no equivalent query API yet).
#[derive(Clone, StatelessView)]
pub struct Visibility {
    visible: bool,
    maintain_state: bool,
    maintain_interactivity: bool,
    replacement: BoxedView,
    child: BoxedView,
}

impl Visibility {
    /// A `Visibility` that shows `child` by default.
    ///
    /// Toggle visibility with [`.visible()`](Self::visible).
    pub fn new(child: impl IntoView) -> Self {
        Self {
            visible: true,
            maintain_state: false,
            maintain_interactivity: false,
            replacement: SizedBox::shrink().boxed(),
            child: child.into_view().boxed(),
        }
    }

    /// Whether the child is currently visible (default `true`).
    #[must_use]
    pub fn visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    /// Keep the child in the element tree when it is not visible, using
    /// [`Offstage`] to suppress paint and hit-testing (default `false`).
    ///
    /// When `false` (the default), the child is replaced by `replacement`
    /// while not visible, discarding its state.
    #[must_use]
    pub fn maintain_state(mut self, maintain_state: bool) -> Self {
        self.maintain_state = maintain_state;
        self
    }

    /// Allow pointer events to reach the child even when it is not visible.
    ///
    /// Requires `maintain_state = true`. Full implementation is deferred until
    /// `maintainSize` lands; this flag is accepted but currently has no
    /// additional effect beyond `maintain_state`.
    #[must_use]
    pub fn maintain_interactivity(mut self, maintain_interactivity: bool) -> Self {
        self.maintain_interactivity = maintain_interactivity;
        self
    }

    /// The widget to show when `visible` is `false` and `maintain_state` is
    /// `false` (default [`SizedBox::shrink`]).
    #[must_use]
    pub fn replacement(mut self, replacement: impl IntoView) -> Self {
        self.replacement = replacement.into_view().boxed();
        self
    }
}

impl fmt::Debug for Visibility {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Visibility")
            .field("visible", &self.visible)
            .field("maintain_state", &self.maintain_state)
            .field("maintain_interactivity", &self.maintain_interactivity)
            .finish_non_exhaustive()
    }
}

impl StatelessView for Visibility {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        // Flutter oracle: `indexed_stack.dart` `Visibility.build` lines 452–473.
        //
        // Non-maintainSize path (maintainAnimation/maintainSize deferred):
        //   maintainState=true  → Offstage(offstage: !visible, child)
        //   maintainState=false → visible ? child : replacement
        //
        // `maintain_interactivity` is accepted but has no additional effect
        // until `maintainSize` is implemented (documented divergence above).
        let result: BoxedView = if self.maintain_state {
            // `Offstage` is a fresh struct (not yet boxed), so `.boxed()` wraps
            // it once — correct.
            Offstage::new()
                .offstage(!self.visible)
                .child(self.child.clone())
                .boxed()
        } else if self.visible {
            // `self.child` is already a `BoxedView`; do NOT call `.boxed()` again
            // — doing so double-wraps (`BoxedView(BoxedView(inner))`) and
            // corrupts element identity on rebuild.
            self.child.clone()
        } else {
            // Same reasoning: `self.replacement` is already `BoxedView`.
            self.replacement.clone()
        };
        result
    }
}
