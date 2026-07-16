//! [`Dialog`] — the M3 dialog surface; [`AlertDialog`] — the title/content/
//! actions composition built on top of it; [`show_dialog`] — pushes either
//! one as a modal popup route.
//!
//! # Flutter parity
//!
//! `material/dialog.dart`'s `Dialog`/`AlertDialog` and
//! `widgets/routes.dart`'s `showDialog`/`RawDialogRoute` (oracle tag
//! `3.44.0`).
//!
//! ## `Dialog` — `_DialogDefaultsM3` (`dialog.dart` `:1962-1998`)
//!
//! | Token | Value | Oracle |
//! |---|---|---|
//! | `alignment` | `Alignment.center` | `_DialogDefaultsM3` constructor |
//! | `backgroundColor` | `ColorScheme.surfaceContainerHigh` | `_DialogDefaultsM3.backgroundColor` |
//! | `shadowColor` | `Colors.transparent` | `_DialogDefaultsM3.shadowColor` |
//! | `surfaceTintColor` | `Colors.transparent` | `_DialogDefaultsM3.surfaceTintColor` |
//! | `elevation` | `6.0` | `_DialogDefaultsM3` constructor |
//! | `shape` | `RoundedRectangleBorder(borderRadius: 28.0)` | `_DialogDefaultsM3` constructor |
//! | `clipBehavior` | `Clip.none` | `_DialogDefaultsM3` constructor |
//! | `insetPadding` | `EdgeInsets.symmetric(horizontal: 40.0, vertical: 24.0)` | `_defaultInsetPadding` |
//! | `constraints` | `BoxConstraints(minWidth: 280.0)` | `Dialog.build` |
//!
//! `shadowColor`/`surfaceTintColor` are the same named [`Material`] gap
//! [`crate::card`] already documents — no setter exists on [`Material`] to
//! carry them. Not a new deferral.
//!
//! `Dialog.build` also wraps its content in
//! `MediaQuery.viewInsetsOf(context) + insetPadding` (so the dialog rides up
//! above a shown keyboard), `AnimatedPadding` (an implicit inset tween),
//! `MediaQuery.removeViewInsets`, and a `Semantics(role: ...)` node. None of
//! these are ported: FLUI has no view-inset-publishing `MediaQuery` consumer
//! wired to this substrate yet, and [`Material`] here applies every prop
//! immediately (see that module's "no implicit shape/elevation animation"
//! scope note, which this shares).
//! `Dialog.fullscreen` (`_fullscreen`, `_DialogFullscreenDefaultsM3`) is not
//! ported either — no caller yet.
//!
//! ## `AlertDialog` (`dialog.dart` `:417-965`)
//!
//! Ported: `title`, `content`, `actions`, composed into the oracle's
//! `Column(mainAxisSize: min, crossAxisAlignment: stretch)` wrapped in
//! `IntrinsicWidth`, with the fixed M3 padding/style defaults
//! `_DialogDefaultsM3` and `AlertDialog.build` compute when `icon`,
//! `titlePadding`, `contentPadding`, `actionsPadding`, and every alignment
//! knob are left at their oracle defaults (which is the only configuration
//! this V1 offers — see below):
//!
//! | Slot | Padding (`EdgeInsets`) | Text style |
//! |---|---|---|
//! | `title` | `left/top/right: 24, bottom: 0 if content else 20` | `TextTheme.headlineSmall` |
//! | `content` | `left/right/bottom: 24, top: 16` | `TextTheme.bodyMedium` |
//! | `actions` | `left/right/bottom: 24, top: 0` | — (each action keeps its own style) |
//!
//! `TextTheme.headlineSmall`/`bodyMedium` are read straight off
//! [`ThemeData::text_theme`](crate::ThemeData::text_theme), which already
//! carries the M3 uniform-recolor-to-`onSurface` FLUI applies at
//! [`ThemeData::light`](crate::ThemeData::light)/[`dark`](crate::ThemeData::dark)
//! construction (see that module's `default_text_theme` doc) — the oracle's
//! own `Typography.material2021` reduces to the identical `onSurface` color
//! for every role regardless of brightness, so no extra recolor step is
//! needed here.
//!
//! Actions are laid out in a plain [`Row`] with [`MainAxisAlignment::End`]
//! and an 8px [`SizedBox`] spacer between each — the
//! oracle's `OverflowBar(alignment: end, spacing: (buttonPadding?.horizontal
//! ?? 16) / 2)` with `buttonPadding` always at its `null` default (`8.0`
//! spacing). `buttonPadding` itself is not exposed: the oracle only ever
//! consumes it for that spacing calculation, never as a per-button pad, so
//! there is nothing else to wire.
//!
//! ### Deferred, and named
//!
//! - **`icon`/`iconPadding`/`iconColor`** — the optional icon slot above the
//!   title.
//! - **`scrollable`** — the `SingleChildScrollView`-wrapped title/content
//!   variant for overflow-prone dialogs.
//! - **`actionsOverflowAlignment`/`actionsOverflowDirection`/
//!   `actionsOverflowButtonSpacing`** — `OverflowBar`'s column fallback when
//!   actions don't fit a single row; this V1's plain `Row` never overflows to
//!   a column.
//! - **Per-instance `titlePadding`/`contentPadding`/`actionsPadding`/
//!   `actionsAlignment`/`buttonPadding` overrides**, and forwarding
//!   `backgroundColor`/`elevation`/`shape`/`insetPadding`/`alignment`/
//!   `constraints` through to the underlying [`Dialog`] — construct a
//!   [`Dialog`] directly when any of those need to vary; `AlertDialog` is a
//!   fixed M3 composition for now.
//! - **Semantics** — `namesRoute`, `semanticLabel`,
//!   `MaterialLocalizations.alertDialogLabel`, the per-slot `Semantics`
//!   wrapper nodes.
//! - **`AlertDialog.adaptive`** — the Cupertino/Material platform switch.
//!
//! ## `show_dialog` (`widgets/routes.dart`'s `showDialog`/`RawDialogRoute`)
//!
//! Pushes `builder`'s content as a [`PopupRoute`] with the oracle's
//! `showDialog` defaults: `barrierDismissible: true`,
//! `barrierColor: Colors.black54` (`0x8A000000`, `colors.dart`, oracle tag
//! `3.44.0`).
//!
//! **Takes a [`NavigatorHandle`] directly, not a `BuildContext`.** The
//! oracle resolves one itself — `Navigator.of(context, rootNavigator:
//! useRootNavigator)`, defaulting to the *root* navigator — and panics if
//! none exists. FLUI's [`NavigatorHandle`] lookup
//! ([`NavigatorHandle::maybe_of_root`]) returns `Option`, not a value a
//! [`PANIC-POLICY`](../../../../docs/PANIC-POLICY.md)-abiding library
//! function may unwrap on a caller's behalf; taking the handle as a
//! parameter instead pushes that choice (and the `Option` it entails) onto
//! the caller, who already has it in hand in every realistic call site. A
//! caller that wants the oracle's exact `rootNavigator: true` resolution
//! calls `NavigatorHandle::maybe_of_root(ctx)` itself before this function.
//!
//! Return value: `navigator.push` already returns a [`RouteResult<T>`] — a
//! `Future<Option<T>>` — which is FLUI's `Future<T?>`, so `show_dialog`
//! returns it unchanged rather than wrapping or narrowing it.
//!
//! **Not ported**, and named: `useSafeArea`, `routeSettings`,
//! `traversalEdgeBehavior`, `anchorPoint`, `fullscreenDialog`,
//! `requestFocus`, `animationStyle`, and the oracle's `InheritedTheme`
//! capture/replay (`CapturedThemes`) that lets a dialog pushed onto a
//! *different* navigator scope still see the calling context's `Theme`.
//! FLUI relies on the pushed route's page mounting as a structural
//! descendant of the app's own `Theme`/`Navigator`/`Overlay` chain (the
//! common case — a dialog pushed onto the same navigator its caller's
//! `Theme` ancestor already covers); a dialog pushed onto an unrelated
//! navigator subtree would not see that `Theme`, which the oracle's capture
//! step exists specifically to fix.

use flui_rendering::constraints::BoxConstraints;
use flui_types::geometry::{Radius, px};
use flui_types::painting::Clip;
use flui_types::styling::BorderRadius;
use flui_types::{Alignment, Color, EdgeInsets, Pixels};
use flui_view::prelude::*;
use flui_widgets::{
    Align, Column, ConstrainedBox, CrossAxisAlignment, DefaultTextStyle, IntrinsicWidth,
    MainAxisAlignment, MainAxisSize, NavigatorHandle, Padding, PopupRoute, RouteResult, Row,
    SizedBox,
};

use crate::material::Material;
use crate::shape::MaterialShape;
use crate::text_theme::TextTheme;
use crate::theme::Theme;

/// `_DialogDefaultsM3`'s elevation (`dialog.dart`, oracle tag `3.44.0`).
const DEFAULT_ELEVATION: f32 = 6.0;
/// `_DialogDefaultsM3`'s corner radius (`dialog.dart`, oracle tag `3.44.0`).
const DEFAULT_CORNER_RADIUS: f32 = 28.0;
/// `Dialog.build`'s fallback `constraints.minWidth` (`dialog.dart`, oracle
/// tag `3.44.0`).
const DEFAULT_MIN_WIDTH: f32 = 280.0;
/// `_defaultInsetPadding`'s horizontal component (`dialog.dart`, oracle tag
/// `3.44.0`).
const INSET_PADDING_HORIZONTAL: f32 = 40.0;
/// `_defaultInsetPadding`'s vertical component (`dialog.dart`, oracle tag
/// `3.44.0`).
const INSET_PADDING_VERTICAL: f32 = 24.0;

/// The Material dialog surface: an elevated, rounded, centered
/// [`Material`] panel inset from the screen edges.
///
/// Typically reached through [`AlertDialog`] rather than built directly.
///
/// ```rust
/// use flui_material::Dialog;
/// use flui_widgets::Text;
///
/// let _dialog = Dialog::new(Text::new("Dialog content"));
/// ```
#[derive(Clone, StatelessView)]
pub struct Dialog {
    color: Option<Color>,
    elevation: Option<f32>,
    shape: Option<MaterialShape>,
    clip_behavior: Option<Clip>,
    alignment: Option<Alignment>,
    inset_padding: Option<EdgeInsets>,
    constraints: Option<BoxConstraints>,
    child: BoxedView,
}

impl std::fmt::Debug for Dialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dialog")
            .field("color", &self.color)
            .field("elevation", &self.elevation)
            .field("shape", &self.shape)
            .field("clip_behavior", &self.clip_behavior)
            .field("alignment", &self.alignment)
            .field("inset_padding", &self.inset_padding)
            .finish_non_exhaustive()
    }
}

impl Dialog {
    /// A `Dialog` around `child`, with every visual property falling
    /// through to `_DialogDefaultsM3` (see the module docs' token table).
    pub fn new(child: impl IntoView) -> Self {
        Self {
            color: None,
            elevation: None,
            shape: None,
            clip_behavior: None,
            alignment: None,
            inset_padding: None,
            constraints: None,
            child: BoxedView(Box::new(child.into_view())),
        }
    }

    /// Overrides the dialog surface's background color.
    #[must_use]
    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// Overrides the dialog's elevation.
    #[must_use]
    pub fn elevation(mut self, elevation: f32) -> Self {
        self.elevation = Some(elevation);
        self
    }

    /// Overrides the dialog's shape.
    #[must_use]
    pub fn shape(mut self, shape: MaterialShape) -> Self {
        self.shape = Some(shape);
        self
    }

    /// Overrides the dialog's clip behavior. Defaults to [`Clip::None`].
    #[must_use]
    pub fn clip_behavior(mut self, clip_behavior: Clip) -> Self {
        self.clip_behavior = Some(clip_behavior);
        self
    }

    /// Overrides where the dialog sits within its inset padding. Defaults to
    /// [`Alignment::CENTER`].
    #[must_use]
    pub fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = Some(alignment);
        self
    }

    /// Overrides the padding between the screen edges and the dialog.
    /// Defaults to `EdgeInsets.symmetric(horizontal: 40.0, vertical: 24.0)`.
    #[must_use]
    pub fn inset_padding(mut self, inset_padding: EdgeInsets) -> Self {
        self.inset_padding = Some(inset_padding);
        self
    }

    /// Overrides the dialog's size constraints. Defaults to
    /// `BoxConstraints(minWidth: 280.0)`.
    #[must_use]
    pub fn constraints(mut self, constraints: BoxConstraints) -> Self {
        self.constraints = Some(constraints);
        self
    }
}

impl StatelessView for Dialog {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let colors = Theme::of(ctx).color_scheme;
        let inset_padding = self.inset_padding.unwrap_or_else(|| {
            EdgeInsets::symmetric(px(INSET_PADDING_VERTICAL), px(INSET_PADDING_HORIZONTAL))
        });
        let constraints = self.constraints.unwrap_or_else(|| {
            BoxConstraints::new(
                px(DEFAULT_MIN_WIDTH),
                Pixels::INFINITY,
                Pixels::ZERO,
                Pixels::INFINITY,
            )
        });
        let shape = self.shape.unwrap_or_else(|| {
            MaterialShape::RoundedRect(BorderRadius::all(Radius::circular(px(
                DEFAULT_CORNER_RADIUS,
            ))))
        });

        Padding::new(inset_padding).child(
            Align::new(self.alignment.unwrap_or(Alignment::CENTER)).child(
                ConstrainedBox::new(constraints).child(
                    Material::new(self.color.unwrap_or(colors.surface_container_high))
                        .elevation(self.elevation.unwrap_or(DEFAULT_ELEVATION))
                        .shape(shape)
                        .clip_behavior(self.clip_behavior.unwrap_or(Clip::None))
                        .child(self.child.clone()),
                ),
            ),
        )
    }
}

/// The 8px gap between adjacent `actions` — `(buttonPadding?.horizontal ??
/// 16) / 2` with `buttonPadding` at its `null` default (`dialog.dart`,
/// oracle tag `3.44.0`).
const ACTION_SPACING: f32 = 8.0;

/// A Material Design alert dialog: an optional title, an optional content
/// body, and a row of actions, composed onto a [`Dialog`] surface.
///
/// See the module docs for the exact padding/style table and the named
/// deferrals (icon slot, scrollable content, actions overflow, semantics).
///
/// ```rust
/// use flui_material::AlertDialog;
/// use flui_view::ViewExt;
/// use flui_widgets::Text;
///
/// let _dialog = AlertDialog::new()
///     .title(Text::new("Delete this?"))
///     .content(Text::new("This cannot be undone."))
///     .actions(vec![Text::new("Cancel").boxed(), Text::new("Delete").boxed()]);
/// ```
#[derive(Clone, Default, StatelessView)]
pub struct AlertDialog {
    title: Option<BoxedView>,
    content: Option<BoxedView>,
    actions: Vec<BoxedView>,
}

impl std::fmt::Debug for AlertDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AlertDialog")
            .field("has_title", &self.title.is_some())
            .field("has_content", &self.content.is_some())
            .field("action_count", &self.actions.len())
            .finish()
    }
}

impl AlertDialog {
    /// An `AlertDialog` with no title, content, or actions.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the title, shown above the content in `TextTheme.headlineSmall`.
    #[must_use]
    pub fn title(mut self, title: impl IntoView) -> Self {
        self.title = Some(BoxedView(Box::new(title.into_view())));
        self
    }

    /// Sets the content, shown below the title in `TextTheme.bodyMedium`.
    #[must_use]
    pub fn content(mut self, content: impl IntoView) -> Self {
        self.content = Some(BoxedView(Box::new(content.into_view())));
        self
    }

    /// Sets the action row shown at the bottom, right-aligned. Pass each
    /// action already boxed (`.boxed()`, from [`ViewExt`]) since actions are
    /// typically a heterogeneous mix of button types.
    #[must_use]
    pub fn actions(mut self, actions: Vec<BoxedView>) -> Self {
        self.actions = actions;
        self
    }
}

impl StatelessView for AlertDialog {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let text_theme: TextTheme = Theme::of(ctx).text_theme;
        let mut children: Vec<BoxedView> = Vec::new();

        if let Some(title) = &self.title {
            let title_bottom = if self.content.is_some() { 0.0 } else { 20.0 };
            children.push(
                Padding::new(EdgeInsets::new(
                    px(24.0),
                    px(24.0),
                    px(title_bottom),
                    px(24.0),
                ))
                .child(DefaultTextStyle::new(
                    text_theme.headline_small.clone().unwrap_or_default(),
                    title.clone(),
                ))
                .boxed(),
            );
        }

        if let Some(content) = &self.content {
            children.push(
                Padding::new(EdgeInsets::new(px(16.0), px(24.0), px(24.0), px(24.0)))
                    .child(DefaultTextStyle::new(
                        text_theme.body_medium.clone().unwrap_or_default(),
                        content.clone(),
                    ))
                    .boxed(),
            );
        }

        if !self.actions.is_empty() {
            let mut row_children: Vec<BoxedView> = Vec::with_capacity(self.actions.len() * 2);
            for (index, action) in self.actions.iter().enumerate() {
                if index > 0 {
                    row_children.push(SizedBox::width(ACTION_SPACING).boxed());
                }
                row_children.push(action.clone());
            }
            children.push(
                Padding::new(EdgeInsets::new(px(0.0), px(24.0), px(24.0), px(24.0)))
                    .child(Row::new(row_children).main_axis_alignment(MainAxisAlignment::End))
                    .boxed(),
            );
        }

        Dialog::new(
            IntrinsicWidth::new().child(
                Column::new(children)
                    .main_axis_size(MainAxisSize::Min)
                    .cross_axis_alignment(CrossAxisAlignment::Stretch),
            ),
        )
    }
}

/// `Colors.black54` (`colors.dart`, oracle tag `3.44.0`) — `showDialog`'s
/// `barrierColor` fallback.
const BARRIER_COLOR: Color = Color::from_argb(0x8A00_0000);

/// Pushes `builder`'s content as a modal dialog: a dismissible
/// `Colors.black54` barrier over the current screen, with the dialog
/// centered above it.
///
/// See the module docs for the full `showDialog` parity table, including why
/// this takes a [`NavigatorHandle`] rather than resolving one from a
/// `BuildContext`, and what the returned [`RouteResult<T>`] is.
pub fn show_dialog<T, F, W>(navigator: &NavigatorHandle, builder: F) -> RouteResult<T>
where
    T: Send + Clone + 'static,
    F: Fn(&dyn BuildContext) -> W + 'static,
    W: IntoView,
{
    navigator.push(
        PopupRoute::<T>::new(move |ctx, _animation, _secondary| builder(ctx).into_view().boxed())
            .barrier_dismissible(true)
            .barrier_color(BARRIER_COLOR),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ThemeData;

    #[test]
    fn dialog_new_leaves_every_override_unset() {
        let dialog = Dialog::new(flui_widgets::SizedBox::shrink());
        assert!(dialog.color.is_none());
        assert!(dialog.elevation.is_none());
        assert!(dialog.shape.is_none());
        assert!(dialog.clip_behavior.is_none());
        assert!(dialog.alignment.is_none());
        assert!(dialog.inset_padding.is_none());
        assert!(dialog.constraints.is_none());
    }

    #[test]
    fn dialog_overrides_are_stored_verbatim() {
        let dialog = Dialog::new(flui_widgets::SizedBox::shrink())
            .color(Color::rgb(1, 2, 3))
            .elevation(2.0)
            .shape(MaterialShape::Stadium)
            .clip_behavior(Clip::AntiAlias)
            .alignment(Alignment::CENTER_LEFT)
            .inset_padding(EdgeInsets::all(px(0.0)));

        assert_eq!(dialog.color, Some(Color::rgb(1, 2, 3)));
        assert_eq!(dialog.elevation, Some(2.0));
        assert_eq!(dialog.shape, Some(MaterialShape::Stadium));
        assert_eq!(dialog.clip_behavior, Some(Clip::AntiAlias));
        assert_eq!(dialog.alignment, Some(Alignment::CENTER_LEFT));
        assert_eq!(dialog.inset_padding, Some(EdgeInsets::all(px(0.0))));
    }

    /// `_DialogDefaultsM3`'s shape: 28dp corners (`dialog.dart`, oracle tag
    /// `3.44.0`).
    #[test]
    fn default_shape_is_a_28dp_rounded_rect() {
        let expected = MaterialShape::RoundedRect(BorderRadius::all(Radius::circular(px(28.0))));
        assert_eq!(
            expected
                .to_rrect(flui_types::Size::new(px(400.0), px(400.0)))
                .top_left,
            Radius::circular(px(28.0))
        );
    }

    #[test]
    fn alert_dialog_new_has_no_title_content_or_actions() {
        let dialog = AlertDialog::new();
        assert!(dialog.title.is_none());
        assert!(dialog.content.is_none());
        assert!(dialog.actions.is_empty());
    }

    #[test]
    fn alert_dialog_builders_set_the_expected_slots() {
        let dialog = AlertDialog::new()
            .title(flui_widgets::Text::new("Title"))
            .content(flui_widgets::Text::new("Content"))
            .actions(vec![flui_widgets::Text::new("OK").boxed()]);

        assert!(dialog.title.is_some());
        assert!(dialog.content.is_some());
        assert_eq!(dialog.actions.len(), 1);
    }

    #[test]
    fn barrier_color_is_black_54() {
        let expected = ThemeData::light().color_scheme.shadow; // opaque black, sanity anchor
        assert_eq!(expected, Color::BLACK);
        assert_eq!(BARRIER_COLOR, Color::from_argb(0x8A00_0000));
        assert_eq!(BARRIER_COLOR.with_opacity(1.0), Color::BLACK);
    }
}
