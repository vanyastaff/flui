//! [`Scaffold`] — the top-level Material page structure: an app bar, a body,
//! and a floating action button, laid out by the private
//! `ScaffoldLayoutDelegate` defined further down in this module.
//!
//! # Flutter parity
//!
//! `material/scaffold.dart`'s `Scaffold` (oracle tag `3.44.0`). Implemented
//! subset: the `app_bar`, `body`, and `floating_action_button` slots of
//! `_ScaffoldLayout.performLayout` (`scaffold.dart:1027-1296`) — the other
//! eight slots (`bottomNavigationBar`, `persistentFooter`, `materialBanner`,
//! `bodyScrim`, `snackBar`, `bottomSheet`, drawers, `statusBar`) are not
//! ported; a future slot extends `ScaffoldLayoutDelegate`, it does not
//! restructure it. `resize_to_avoid_bottom_inset` defaults to `true`, same
//! as the oracle. `background_color` falls back to `ColorScheme.surface`
//! (the oracle's `themeData.scaffoldBackgroundColor`, which this substrate
//! has not ported onto [`crate::ThemeData`] yet — `ColorScheme.surface` is
//! the closer M3 analogue in the meantime).
//!
//! ## The inset contract
//!
//! This is the part of the oracle most worth reading closely before touching
//! this file — get it wrong and either the app bar double-shifts the body,
//! or a floating action button hides under the keyboard.
//!
//! - **The app bar pads itself.** [`crate::AppBar`] wraps its toolbar in a
//!   `SafeArea` and consumes `MediaQuery.padding.top` internally (see that
//!   module's docs). This widget's own contribution is only a height *cap*:
//!   the `app_bar` slot is wrapped in `ConstrainedBox(max_height:
//!   preferred_height + padding.top)` (oracle: `_appBarMaxHeight =
//!   AppBar.preferredHeightFor(...) + topPadding`, `scaffold.dart:3048-3051`).
//!   `ScaffoldLayoutDelegate` itself never adds `padding.top` a second
//!   time — `content_top` is simply the app bar's *measured* laid-out
//!   height (oracle: `contentTop = appBarHeight`, `:1043`). Adding the
//!   padding again here would double-shift the body.
//! - **The body's `MediaQuery` is reduced, not passed through.** When an app
//!   bar is present the body's ambient `padding.top` is zeroed (it already
//!   sits below the app bar, which already consumed that inset); when
//!   `resize_to_avoid_bottom_inset` is set the body's `view_insets.bottom`
//!   is zeroed too (oracle: `_addIfNonNull(..., removeTopPadding:
//!   widget.appBar != null, removeBottomInset: _resizeToAvoidBottomInset)`,
//!   `:3019-3035`). Skipping this makes a `SafeArea` nested in the body
//!   double-pad. `MediaQueryData` has no `MediaQuery.removePadding`
//!   equivalent yet (it is a plain pub-field struct), so the reduced copy is
//!   built inline rather than through a helper method.
//! - **The body's constraints are loose**, not tight-width: `max = (width,
//!   content_bottom - content_top)`, `min = 0` on both axes (oracle:
//!   `_BodyBoxConstraints(maxWidth: ..., maxHeight: bodyMaxHeight)` — a
//!   `BoxConstraints` literal leaves `minWidth`/`minHeight` at their `0.0`
//!   default). A body smaller than the available area is legal.
//! - **The floating action button is measured loosely**, then positioned
//!   from `content_bottom` — never from the scaffold's raw `size.height`.
//!   With the keyboard up (`view_insets.bottom > 0`), `content_bottom`
//!   already sits above it; positioning from raw height would hide the
//!   button under the keyboard. See `ScaffoldLayoutDelegate`'s docs (further
//!   down in this module) for the exact formula and its oracle citation.
//!
//! ## Deferred, and named
//!
//! `bottomNavigationBar`, `persistentFooterButtons`, `drawer`/`endDrawer`,
//! `MaterialBanner`, `SnackBar` (via `ScaffoldMessenger`), `bottomSheet`,
//! `extendBody`/`extendBodyBehindAppBar`, non-`endFloat`
//! `FloatingActionButtonLocation`s, and `Scaffold.of`/`ScaffoldState`'s
//! runtime API (opening drawers, showing snack bars). None of these slots
//! exist in `ScaffoldLayoutDelegate` yet — the three that do
//! (`app_bar`/`body`/`floating_action_button`) are copied verbatim from
//! `_ScaffoldLayout`'s branches for those same slots, so adding a slot later
//! is additive, not a rewrite.

use std::any::Any;
use std::sync::Arc;

use flui_rendering::constraints::BoxConstraints;
use flui_types::geometry::px;
use flui_types::styling::Color;
use flui_types::{EdgeInsets, Offset, Size};
use flui_view::prelude::*;
use flui_widgets::{
    ConstrainedBox, CustomMultiChildLayout, LayoutId, MediaQuery, MultiChildLayoutContext,
    MultiChildLayoutDelegate, PreferredSizeView,
};

use crate::material::Material;
use crate::theme::Theme;

/// The `app_bar` slot id — see [`ScaffoldLayoutDelegate`].
const SLOT_APP_BAR: &str = "app_bar";
/// The `body` slot id — see [`ScaffoldLayoutDelegate`].
const SLOT_BODY: &str = "body";
/// The `floating_action_button` slot id — see [`ScaffoldLayoutDelegate`].
const SLOT_FLOATING_ACTION_BUTTON: &str = "floating_action_button";

/// The margin between a floating action button and the scaffold edge it
/// floats near.
///
/// Flutter parity: `floating_action_button_location.dart`'s
/// `kFloatingActionButtonMargin` (oracle tag `3.44.0`).
const FLOATING_ACTION_BUTTON_MARGIN: f32 = 16.0;

/// The top-level Material page structure: an app bar, a body, and a floating
/// action button.
///
/// See the module docs for the implemented slot subset, the inset contract
/// `ScaffoldLayoutDelegate` enforces, and the deferred slot list.
///
/// # Examples
///
/// ```rust
/// use flui_material::{AppBar, Scaffold};
/// use flui_widgets::Text;
///
/// let _page = Scaffold::new()
///     .app_bar(AppBar::new().title(Text::new("FLUI")))
///     .body(Text::new("Hello"));
/// ```
#[derive(Clone, StatelessView)]
pub struct Scaffold {
    body: Option<BoxedView>,
    app_bar: Option<BoxedView>,
    app_bar_preferred_height: f32,
    floating_action_button: Option<BoxedView>,
    resize_to_avoid_bottom_inset: bool,
    background_color: Option<Color>,
}

impl Scaffold {
    /// An empty `Scaffold`: no app bar, no body, no floating action button,
    /// `resize_to_avoid_bottom_inset: true`, and a `background_color`
    /// falling back to `ColorScheme.surface`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            body: None,
            app_bar: None,
            app_bar_preferred_height: 0.0,
            floating_action_button: None,
            resize_to_avoid_bottom_inset: true,
            background_color: None,
        }
    }

    /// Sets the primary content, positioned below the app bar (if any) and
    /// above the keyboard (when `resize_to_avoid_bottom_inset` is set).
    #[must_use]
    pub fn body(mut self, body: impl IntoView) -> Self {
        self.body = Some(body.into_view().boxed());
        self
    }

    /// Sets the app bar, pinned to the top of the scaffold.
    ///
    /// `app_bar`'s [`preferred_size`](PreferredSizeView::preferred_size) is
    /// resolved once, here, and its height captured — see
    /// [`PreferredSizeView`]'s module docs on why this substrate captures the
    /// size at construction rather than re-consulting it later.
    #[must_use]
    pub fn app_bar(mut self, app_bar: impl PreferredSizeView) -> Self {
        self.app_bar_preferred_height = app_bar.preferred_size().height.get();
        self.app_bar = Some(app_bar.boxed());
        self
    }

    /// Sets the floating action button, positioned at the bottom-right of
    /// the content area (Flutter's `FloatingActionButtonLocation.endFloat` —
    /// the only location this substrate implements).
    #[must_use]
    pub fn floating_action_button(mut self, floating_action_button: impl IntoView) -> Self {
        self.floating_action_button = Some(floating_action_button.into_view().boxed());
        self
    }

    /// Whether the body's bottom-view-inset (typically the on-screen
    /// keyboard) shrinks the body's available height. Defaults to `true`.
    #[must_use]
    pub fn resize_to_avoid_bottom_inset(mut self, resize_to_avoid_bottom_inset: bool) -> Self {
        self.resize_to_avoid_bottom_inset = resize_to_avoid_bottom_inset;
        self
    }

    /// Overrides the scaffold's background surface color. Defaults to
    /// `ColorScheme.surface`.
    #[must_use]
    pub fn background_color(mut self, color: Color) -> Self {
        self.background_color = Some(color);
        self
    }
}

impl Default for Scaffold {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Scaffold {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scaffold")
            .field("has_body", &self.body.is_some())
            .field("has_app_bar", &self.app_bar.is_some())
            .field(
                "has_floating_action_button",
                &self.floating_action_button.is_some(),
            )
            .field(
                "resize_to_avoid_bottom_inset",
                &self.resize_to_avoid_bottom_inset,
            )
            .finish_non_exhaustive()
    }
}

impl StatelessView for Scaffold {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let theme = Theme::of(ctx);
        let background_color = self.background_color.unwrap_or(theme.color_scheme.surface);
        let media_query = MediaQuery::of(ctx);

        let mut children: Vec<LayoutId> = Vec::new();

        if let Some(body) = &self.body {
            let mut reduced_media_query = media_query.clone();
            if self.app_bar.is_some() {
                reduced_media_query.padding.top = px(0.0);
            }
            if self.resize_to_avoid_bottom_inset {
                reduced_media_query.view_insets.bottom = px(0.0);
            }
            children.push(LayoutId::new(
                SLOT_BODY,
                MediaQuery::new(reduced_media_query, body.clone()),
            ));
        }

        if let Some(app_bar) = &self.app_bar {
            let max_height = px(self.app_bar_preferred_height) + media_query.padding.top;
            let cap = BoxConstraints {
                max_height,
                ..BoxConstraints::UNCONSTRAINED
            };
            children.push(LayoutId::new(
                SLOT_APP_BAR,
                ConstrainedBox::new(cap).child(app_bar.clone()),
            ));
        }

        if let Some(floating_action_button) = &self.floating_action_button {
            children.push(LayoutId::new(
                SLOT_FLOATING_ACTION_BUTTON,
                floating_action_button.clone(),
            ));
        }

        // Flutter oracle: `minInsets = MediaQuery.paddingOf(context).copyWith(
        // bottom: resizeToAvoidBottomInset ? viewInsetsOf(context).bottom : 0.0)`
        // (`scaffold.dart:3220-3222`) — the safe-area padding on every edge,
        // with the bottom edge swapped for the keyboard inset when resizing.
        let min_insets = EdgeInsets::new(
            media_query.padding.top,
            media_query.padding.right,
            if self.resize_to_avoid_bottom_inset {
                media_query.view_insets.bottom
            } else {
                px(0.0)
            },
            media_query.padding.left,
        );

        let delegate: Arc<dyn MultiChildLayoutDelegate> =
            Arc::new(ScaffoldLayoutDelegate { min_insets });

        Material::new(background_color).child(CustomMultiChildLayout::new(delegate, children))
    }
}

/// The layout algorithm for [`Scaffold`]'s `app_bar` / `body` /
/// `floating_action_button` slots.
///
/// Flutter parity: `_ScaffoldLayout` (`scaffold.dart:991-1308`), narrowed to
/// the three slots this substrate ports — see the module docs for the full
/// deferred-slot list and the inset contract this delegate enforces.
#[derive(Debug, Clone, PartialEq)]
struct ScaffoldLayoutDelegate {
    /// The safe-area padding on every edge, with the bottom edge swapped for
    /// the keyboard inset when `resize_to_avoid_bottom_inset` is set. See
    /// [`Scaffold::build`]'s citation of `_ScaffoldLayout`'s `minInsets`.
    min_insets: EdgeInsets,
}

impl MultiChildLayoutDelegate for ScaffoldLayoutDelegate {
    fn perform_layout(&self, ctx: &mut dyn MultiChildLayoutContext, size: Size) {
        // Tight width, loose height (0..size.height) — the app bar reports
        // its own preferred height within that loose band. Oracle:
        // `fullWidthConstraints = looseConstraints.tighten(width: size.width)`
        // (`:1035`).
        let full_width_loose_height =
            BoxConstraints::new(size.width, size.width, px(0.0), size.height);

        let mut content_top = px(0.0);
        if ctx.has_child(SLOT_APP_BAR) {
            let app_bar_size = ctx.layout_child(SLOT_APP_BAR, full_width_loose_height);
            // `content_top` is the app bar's MEASURED height — already
            // includes whatever top inset the app bar consumed internally
            // (see the module docs). Adding `min_insets.top` again here
            // would double-shift the body.
            content_top = app_bar_size.height;
            ctx.position_child(SLOT_APP_BAR, Offset::ZERO);
        }

        // Oracle: `contentBottom = max(0, bottom - max(minInsets.bottom,
        // bottomWidgetsHeight))` (`:1088-1091`) — `bottomWidgetsHeight` is
        // always `0.0` here (no `bottomNavigationBar`/persistent-footer
        // slot in this substrate), so it reduces to `bottom - min_insets.bottom`.
        let content_bottom = (size.height - self.min_insets.bottom).max(px(0.0));

        if ctx.has_child(SLOT_BODY) {
            // Loose constraints, not tight-width — see the module docs.
            let body_max_height = (content_bottom - content_top).max(px(0.0));
            let body_constraints =
                BoxConstraints::new(px(0.0), size.width, px(0.0), body_max_height);
            ctx.layout_child(SLOT_BODY, body_constraints);
            ctx.position_child(SLOT_BODY, Offset::new(px(0.0), content_top));
        }

        if ctx.has_child(SLOT_FLOATING_ACTION_BUTTON) {
            // Loose on both axes — the button reports its own intrinsic
            // size, never forced from `size` (oracle: `layoutChild(...,
            // looseConstraints)`, `:1160`).
            let fab_size =
                ctx.layout_child(SLOT_FLOATING_ACTION_BUTTON, BoxConstraints::loose(size));

            // `FloatingActionButtonLocation.endFloat` (`FabEndOffsetX` +
            // `FabFloatOffsetY`, `floating_action_button_location.dart:517-528,
            // 554-581`). The `FabFloatOffsetY` `safeMargin` term reduces to
            // the flat `kFloatingActionButtonMargin` here: it is
            // `max(margin, minViewPadding.bottom - bottomContentHeight +
            // margin)`, and `bottomContentHeight` (`scaffoldSize.height -
            // contentBottom`) always equals `min_insets.bottom` in this
            // substrate (no bottom-anchored slots to widen the gap), so the
            // second term is `minViewPadding.bottom - min_insets.bottom +
            // margin <= margin` whenever `minViewPadding.bottom <=
            // min_insets.bottom` — which holds because `MediaQueryData` has
            // no `viewPadding` field to diverge from `padding` (named
            // divergence: the oracle's `minViewPadding` and `minInsets` can
            // differ in principle; this substrate has only one padding
            // source, so they cannot).
            let fab_x = size.width
                - px(FLOATING_ACTION_BUTTON_MARGIN)
                - self.min_insets.right
                - fab_size.width;
            let fab_y = content_bottom - fab_size.height - px(FLOATING_ACTION_BUTTON_MARGIN);
            ctx.position_child(SLOT_FLOATING_ACTION_BUTTON, Offset::new(fab_x, fab_y));
        }
    }

    fn get_size(&self, constraints: BoxConstraints) -> Size {
        debug_assert!(
            constraints.has_bounded_width() && constraints.has_bounded_height(),
            "Scaffold requires bounded constraints from its parent — mount it under a \
             sized route/window, not inside an unbounded scrollable or unconstrained box"
        );
        constraints.biggest()
    }

    fn should_relayout(&self, old_delegate: &dyn MultiChildLayoutDelegate) -> bool {
        // Oracle: `oldDelegate.minInsets != minInsets || ...` (`:1300`) —
        // `min_insets` is the only field this narrowed delegate captures, so
        // comparing it covers every relayout trigger this substrate has
        // (chiefly: the keyboard showing/hiding).
        old_delegate
            .as_any()
            .downcast_ref::<Self>()
            .is_none_or(|old| self != old)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_scaffold_has_no_slots_and_resizes_by_default() {
        let scaffold = Scaffold::new();
        assert!(scaffold.body.is_none());
        assert!(scaffold.app_bar.is_none());
        assert!(scaffold.floating_action_button.is_none());
        assert!(scaffold.resize_to_avoid_bottom_inset);
    }

    #[test]
    fn app_bar_builder_captures_the_preferred_height() {
        let scaffold = Scaffold::new().app_bar(crate::AppBar::new().toolbar_height(72.0));
        assert!(scaffold.app_bar.is_some());
        assert_eq!(scaffold.app_bar_preferred_height, 72.0);
    }

    #[test]
    fn resize_to_avoid_bottom_inset_builder_overrides_the_default() {
        let scaffold = Scaffold::new().resize_to_avoid_bottom_inset(false);
        assert!(!scaffold.resize_to_avoid_bottom_inset);
    }

    #[test]
    fn should_relayout_is_false_for_equal_min_insets() {
        let a = ScaffoldLayoutDelegate {
            min_insets: EdgeInsets::new(px(0.0), px(0.0), px(0.0), px(0.0)),
        };
        let b = ScaffoldLayoutDelegate {
            min_insets: EdgeInsets::new(px(0.0), px(0.0), px(0.0), px(0.0)),
        };
        assert!(!MultiChildLayoutDelegate::should_relayout(&a, &b));
    }

    #[test]
    fn should_relayout_is_true_when_bottom_min_inset_changes() {
        let a = ScaffoldLayoutDelegate {
            min_insets: EdgeInsets::new(px(0.0), px(0.0), px(0.0), px(0.0)),
        };
        let b = ScaffoldLayoutDelegate {
            min_insets: EdgeInsets::new(px(0.0), px(0.0), px(300.0), px(0.0)),
        };
        assert!(MultiChildLayoutDelegate::should_relayout(&a, &b));
    }
}
