//! [`Visibility`] — show or hide a child, with optional state preservation.

use std::fmt;

use flui_view::prelude::StatelessView;
use flui_view::{BoxedView, BuildContext, IntoView, ViewExt};

use crate::animated::TickerMode;
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
///    inverse of `visible`. By default, an inner [`TickerMode`] also mutes
///    descendant animations while hidden. This keeps the child's state alive
///    and allows it to snap back to its last state when made visible again.
///    Paint and hit-testing are suppressed by `Offstage` when hidden.
///
/// 3. **Interactive-while-hidden (`maintain_interactivity = true`, requires
///    `maintain_state = true`):** deferred — full support requires
///    `maintainSize`. Setting `maintain_interactivity = true` is accepted but
///    has no additional effect beyond `maintain_state` behaviour; see the
///    divergence note below.
///
/// Flutter parity: `widgets/indexed_stack.dart` `Visibility`.
///
/// **Divergences from Flutter:**
/// - `maintainAnimation` controls descendants registered through an ambient
///   `VsyncScope`, as production `AppBinding` roots provide. Without an ambient
///   scope, FLUI's `TickerMode` intentionally passes its child through so an
///   undriven nested registry cannot swallow wall-clock fallback animations.
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
    maintain_animation: bool,
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
            maintain_animation: false,
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

    /// Keep descendant animations running while the child is hidden (default
    /// `false`).
    ///
    /// Requires `maintain_state = true`. Debug builds check this invariant when
    /// the completed widget builds, so either builder method may be called
    /// first. Dynamically changing this flag can change the wrapper shape,
    /// remount the child, and lose its state.
    ///
    /// Animation muting applies to descendants registered through an ambient
    /// `VsyncScope`; production `AppBinding` roots provide that scope.
    #[must_use]
    pub fn maintain_animation(mut self, maintain_animation: bool) -> Self {
        self.maintain_animation = maintain_animation;
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
            .field("maintain_animation", &self.maintain_animation)
            .field("maintain_interactivity", &self.maintain_interactivity)
            .finish_non_exhaustive()
    }
}

impl StatelessView for Visibility {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        debug_assert!(
            self.maintain_state || !self.maintain_animation,
            "maintain_animation requires maintain_state"
        );

        // Flutter oracle: `indexed_stack.dart` `Visibility.build`.
        //
        // Non-maintainSize path:
        //   maintainState=true  → Offstage(offstage: !visible,
        //                           TickerMode(enabled: visible, child))
        //                         unless maintainAnimation=true
        //   maintainState=false → visible ? child : replacement
        //
        // `maintain_interactivity` is accepted but has no additional effect
        // until `maintainSize` is implemented (documented divergence above).
        let result: BoxedView = if self.maintain_state {
            let child = if self.maintain_animation {
                self.child.clone()
            } else {
                TickerMode::new(self.child.clone())
                    .enabled(self.visible)
                    .into_view()
                    .boxed()
            };
            // `Offstage` is a fresh struct (not yet boxed), so `.boxed()` wraps
            // it once — correct.
            Offstage::new().offstage(!self.visible).child(child).boxed()
        } else if self.visible {
            // The stored child is already type-erased for this return path.
            self.child.clone()
        } else {
            // The stored replacement is already type-erased for this return path.
            self.replacement.clone()
        };
        result
    }
}
