//! [`AppBar`] — a Material app bar: a leading/title/actions toolbar on a
//! [`Material`] surface.
//!
//! # Flutter parity
//!
//! `material/app_bar.dart`'s `AppBar` (oracle tag `3.44.0`). Implemented
//! subset: `leading`, `title`, `actions`, `toolbar_height`, `bottom`,
//! `background_color`, `foreground_color`, `elevation`, and the M3 token defaults
//! (`_AppBarDefaultsM3`, `app_bar.dart:2521-2570`): `background_color` falls
//! back to `ColorScheme.surface`, `foreground_color` to `ColorScheme.on_surface`,
//! `elevation` to `0.0`, and the title's text style to `TextTheme.title_large`
//! (recolored to the resolved foreground).
//!
//! ## The app bar consumes the top inset itself
//!
//! When `widget.primary` (`app_bar.dart:1189-1191`), the oracle wraps its
//! toolbar in `SafeArea(bottom: false, child: appBar)` — the app bar pads
//! itself against `MediaQuery.paddingOf(context).top`, rather than a parent
//! adding that padding on its behalf. This substrate does the same
//! unconditionally (no `primary` toggle yet — every `AppBar` behaves as
//! `primary: true`), via [`flui_widgets::SafeArea`]. A consequence, matching
//! the oracle: a standalone `AppBar` (mounted with no `Scaffold` at all, just
//! a `MediaQuery` ancestor) already reserves the status-bar inset on its own.
//!
//! ## `centerTitle`: a platform switch, narrowed
//!
//! `_getEffectiveCenterTitle` (`app_bar.dart:805-817`) is a `TargetPlatform`
//! switch: `false` on Android/Fuchsia/Linux/Windows, `true` on iOS/macOS with
//! fewer than two actions. FLUI's desktop targets are Linux and Win32 — both
//! land on the `false` branch — so this substrate always start-aligns the
//! title (no `center_title` override, no `NavigationToolbar`-style toggle
//! yet). **Named divergence**: real macOS parity (the `true` branch) waits
//! for a platform-adaptive seam; today every platform gets the
//! Android/Linux/Windows answer.
//!
//! ## `bottom`: a fixed-height slot below the toolbar
//!
//! [`AppBar::bottom`] accepts anything implementing [`PreferredSizeView`]
//! (typically a [`crate::TabBar`]) and mounts it directly beneath the
//! toolbar, inside the same [`SafeArea`]. Flutter parity: `_AppBarState.build`'s
//! `if (widget.bottom != null)` branch (`app_bar.dart:1164-1183`, oracle tag
//! `3.44.0`) — ported as the identical shape: a `Column` with
//! `mainAxisAlignment: spaceBetween` whose first child is the toolbar wrapped
//! in `Flexible(child: ConstrainedBox(maxHeight: toolbar_height))` and whose
//! second is `bottom` itself, unwrapped. The whole `Column` is forced to
//! `toolbar_height + bottom.preferred_size().height` via an outer
//! [`SizedBox`] (this substrate's equivalent of the oracle's
//! `_PreferredAppBarSize`-driven ambient sizing — see [`AppBar::preferred_size`]
//! below), then handed to the same top-inset-consuming `SafeArea` the toolbar
//! alone already used.
//!
//! **Why the toolbar flexes and `bottom` does not**: `Flexible` (not
//! `Expanded`) with a *loose* fit means the toolbar happily shrinks below
//! `toolbar_height` when the `Column`'s own available height falls short of
//! `toolbar_height + bottom_height` (a caller-imposed cap tighter than this
//! bar's own preferred size, or a `SafeArea` top inset large enough to eat
//! into it) — `bottom` is the `Column`'s other, non-flexible child, so it
//! always gets its own natural height first and the toolbar absorbs the
//! shortfall. This is not a simplification: it's the exact oracle shape,
//! ported so a `TabBar` mounted as `bottom` never gets silently clipped
//! by a tight parent while the toolbar above it holds its full height.
//!
//! [`Scaffold::app_bar`](crate::Scaffold::app_bar)'s own cap math
//! (`max_height = view.app_bar_preferred_height + media_query.padding.top`)
//! is unaffected by `bottom`: `app_bar_preferred_height` already snapshots
//! [`AppBar::preferred_size`]'s `toolbar_height + bottom_height` sum at
//! `Scaffold::app_bar`-builder time (see that method's own doc comment for
//! why the snapshot, not a live re-consult), so the cap this substrate hands
//! back down already has exactly enough room for both slots plus the top
//! inset — the `Flexible` shrink path above only fires when a caller
//! deliberately imposes something tighter than that (or mounts `AppBar`
//! standalone, with no `Scaffold` reserving room for it at all).
//!
//! **Deferred, and named** (this `bottom` slot specifically): `bottomOpacity`
//! (the oracle's `Opacity`/`Interval`-curve fade as a `SliverAppBar` scrolls
//! `bottom` toward its collapsed state — no `scrolledUnder`/sliver-collapse
//! substrate here to drive it) and `PreferredSizeWidget`'s `Scaffold`-side
//! bottom-height re-consult on data change (see [`PreferredSizeView`]'s own
//! "Named divergence" doc — this whole substrate resolves it once, at
//! `.bottom(...)`/`.app_bar(...)` builder time).
//!
//! ## Deferred, and named
//!
//! - `center_title` / a full `NavigationToolbar` port — the title area here
//!   is a plain `Expanded` + `Align(center_left)`, not `NavigationToolbar`'s
//!   overflow-aware middle-widget layout.
//! - `scrolledUnder` — no `ScrollNotification` substrate to observe yet.
//! - `flexibleSpace` — stacked behind the toolbar+bottom in the oracle
//!   (`app_bar.dart`'s trailing `Stack` when `widget.flexibleSpace != null`);
//!   no consumer or substrate for it here yet.
//! - **Named divergence: no shadow suppression at a nonzero elevation.**
//!   `_AppBarDefaultsM3` sets `shadowColor: Colors.transparent` AND
//!   `surfaceTintColor: Colors.transparent` (`app_bar.dart:2541-2545`) — the
//!   oracle's M3 app bar casts no shadow even when `scrolledUnderElevation`/
//!   an explicit `elevation` override raises it above `0`; the surface
//!   communicates elevation through a tonal color shift instead (M3's
//!   `ElevationOverlay`), not a drop shadow. [`crate::Material`] has no
//!   `shadow_color` setter yet (see that module's docs' `surfaceTintColor`
//!   section for the matching gap), so this substrate cannot suppress it —
//!   an `AppBar::new().elevation(4.0)` here casts a real shadow the M3
//!   oracle would not. Revisit once `Material` grows `shadow_color`.
//!
//! ## Implied leading: a `BackButton`, no `DrawerButton`
//!
//! `_AppBarState.build`'s leading resolution (`app_bar.dart:1009-1014`,
//! oracle tag `3.44.0`): when `leading` is unset and
//! `automatically_imply_leading` is set, the oracle synthesizes a
//! `DrawerButton` if the enclosing `Scaffold` has a drawer, else a
//! `BackButton`/`CloseButton` if `parentRoute?.impliesAppBarDismissal ??
//! false` (`willHandlePopInternally || canPop`, from `ModalRoute`). This
//! substrate has no `Drawer`/`Scaffold.hasDrawer` and no `ModalRoute`
//! abstraction (routes are plain [`flui_widgets::Route`]s, not modal-aware
//! ones), so `resolve_leading` narrows the condition to what those two
//! substrates leave reachable: no leading set, `automatically_imply_leading`
//! set, a [`NavigatorHandle`] ancestor
//! exists, and it reports [`NavigatorHandle::can_pop`] — always a
//! [`crate::BackButton`], never a `CloseButton` (no `fullscreenDialog`
//! substrate to pick that branch) or `DrawerButton` (no drawer substrate at
//! all). **Named divergence**, not a silently dropped case.
//!
//! **Second named divergence, worth calling out precisely:**
//! `NavigatorHandle::can_pop` is navigator-global (Flutter's own
//! `NavigatorState.canPop` is too), but the oracle's `parentRoute` is
//! resolved via `ModalRoute.of(context)` — the SPECIFIC route this
//! `AppBar`'s subtree is inside — so a bottom-of-stack route's own
//! `impliesAppBarDismissal` is `false` even while the navigator as a whole
//! can pop (a route above it exists). This substrate has no `ModalRoute`
//! equivalent to ask "which route is this `AppBar` inside, and specifically
//! is IT poppable" — every mounted `AppBar` under the same navigator sees
//! the same global answer. In the common case (one route showing an
//! `AppBar` at a time, which is what an `Overlay`-based navigator is for)
//! this is unobservable; it only diverges when multiple routes carrying
//! their own `AppBar` are simultaneously mounted (see
//! `tests/app_bar.rs`'s `implied_leading_appears_once_the_navigator_can_pop`
//! for exactly that case, documented rather than hidden).
//!
//! ## The leading slot is a fixed `LEADING_WIDTH`, not the leading widget's own intrinsic size
//!
//! Whatever `leading` resolves to (explicit or implied) is wrapped in
//! `ConstrainedBox(BoxConstraints.tightFor(width: LEADING_WIDTH))` around
//! `Center` before it reaches the toolbar `Row` — Flutter parity:
//! `_AppBarState.build`'s own `leading = ConstrainedBox(constraints:
//! BoxConstraints.tightFor(width: widget.leadingWidth ?? appBarTheme.leadingWidth
//! ?? _kLeadingWidth), child: leading)` (`app_bar.dart`, tag `3.44.0`;
//! `_kLeadingWidth = kToolbarHeight`, "so the leading button is square").
//! Simplified from the oracle in one way: Flutter only wraps in `Center`
//! `when leading is IconButton`; this substrate does it unconditionally
//! (harmless for any leading widget that already fills its own bounds).
//! **Without this wrap**, a bare 40×40 `IconButton` (this crate's
//! `_IconButtonDefaultsM3.minimumSize`) would collapse the slot to 40px
//! wide in the `Row` instead of the M3-specified 56px — `LEADING_WIDTH`'s
//! `ConstrainedBox` is what prevents that. No `leadingWidth`/
//! `AppBarTheme.leadingWidth` override exists yet (named V1 deferral), so
//! `LEADING_WIDTH` is the only width this slot ever takes.

use flui_rendering::constraints::BoxConstraints;
use flui_types::geometry::px;
use flui_types::styling::Color;
use flui_types::typography::TextStyle;
use flui_types::{Alignment, Pixels, Size};
use flui_view::prelude::*;
use flui_widgets::{
    Align, Center, Column, ConstrainedBox, CrossAxisAlignment, DefaultTextStyle, Expanded,
    Flexible, IconTheme, IconThemeData, MainAxisAlignment, NavigatorHandle, PreferredSizeView, Row,
    SafeArea, SizedBox,
};

use crate::back_button::BackButton;
use crate::material::Material;
use crate::theme::Theme;
use crate::theme_data::ThemeData;

/// The default toolbar height in logical pixels.
///
/// Flutter parity: `material/constants.dart`'s `kToolbarHeight` (oracle tag
/// `3.44.0`).
pub const DEFAULT_TOOLBAR_HEIGHT: f32 = 56.0;

/// The leading slot's fixed width — Flutter parity: `_AppBarState.build`'s
/// `_kLeadingWidth` (`app_bar.dart:43`, `= kToolbarHeight`, "so the leading
/// button is square"). No `widget.leadingWidth`/`AppBarTheme.leadingWidth`
/// override exists yet in this V1 (see the module docs' deferred list), so
/// this constant is the only width the slot ever takes.
const LEADING_WIDTH: f32 = DEFAULT_TOOLBAR_HEIGHT;

/// A Material app bar: a `leading` / `title` / `actions` toolbar painted on a
/// [`Material`] surface, sized to [`toolbar_height`](Self::toolbar_height) and
/// self-padded against the top safe-area inset.
///
/// See the module docs for the implemented subset, the "consumes the top
/// inset itself" contract, and the deferred list.
///
/// # Examples
///
/// ```rust
/// use flui_material::AppBar;
/// use flui_widgets::Text;
///
/// let _bar = AppBar::new().title(Text::new("FLUI")).toolbar_height(64.0);
/// ```
#[derive(Clone, StatelessView)]
pub struct AppBar {
    leading: Option<BoxedView>,
    automatically_imply_leading: bool,
    title: Option<BoxedView>,
    actions: Vec<BoxedView>,
    toolbar_height: f32,
    background_color: Option<Color>,
    foreground_color: Option<Color>,
    elevation: Option<f32>,
    bottom: Option<BoxedView>,
    /// `bottom`'s [`preferred_size`](PreferredSizeView::preferred_size)
    /// height, snapshotted at [`Self::bottom`]-builder time — see that
    /// method's doc comment and [`PreferredSizeView`]'s own "Named
    /// divergence" note on why this substrate resolves it once rather than
    /// re-consulting it later.
    bottom_preferred_height: f32,
}

impl AppBar {
    /// An `AppBar` with no leading/title/actions, the default toolbar height
    /// ([`DEFAULT_TOOLBAR_HEIGHT`]), `automatically_imply_leading: true`, and
    /// every color/elevation left to the M3 theme defaults.
    #[must_use]
    pub fn new() -> Self {
        Self {
            leading: None,
            automatically_imply_leading: true,
            title: None,
            actions: Vec::new(),
            toolbar_height: DEFAULT_TOOLBAR_HEIGHT,
            background_color: None,
            foreground_color: None,
            elevation: None,
            bottom: None,
            bottom_preferred_height: 0.0,
        }
    }

    /// Sets the widget in the leading slot (before the title), overriding
    /// any implied leading — see [`Self::automatically_imply_leading`].
    #[must_use]
    pub fn leading(mut self, leading: impl IntoView) -> Self {
        self.leading = Some(leading.into_view().boxed());
        self
    }

    /// Whether a [`crate::BackButton`] is synthesized into the leading slot
    /// when [`Self::leading`] is unset and a poppable
    /// [`NavigatorHandle`] ancestor exists.
    /// Defaults to `true`. See the module docs' "Implied leading" section
    /// for the narrowed condition this substrate checks.
    #[must_use]
    pub fn automatically_imply_leading(mut self, automatically_imply_leading: bool) -> Self {
        self.automatically_imply_leading = automatically_imply_leading;
        self
    }

    /// Sets the title widget, start-aligned in the space between `leading`
    /// and `actions` — see the module docs' `centerTitle` note.
    #[must_use]
    pub fn title(mut self, title: impl IntoView) -> Self {
        self.title = Some(title.into_view().boxed());
        self
    }

    /// Sets the trailing action widgets, laid out in a row after the title.
    #[must_use]
    pub fn actions(mut self, actions: Vec<BoxedView>) -> Self {
        self.actions = actions;
        self
    }

    /// Sets the toolbar's height. Defaults to [`DEFAULT_TOOLBAR_HEIGHT`].
    #[must_use]
    pub fn toolbar_height(mut self, toolbar_height: f32) -> Self {
        self.toolbar_height = toolbar_height;
        self
    }

    /// Overrides the surface color. Defaults to `ColorScheme.surface`.
    #[must_use]
    pub fn background_color(mut self, color: Color) -> Self {
        self.background_color = Some(color);
        self
    }

    /// Overrides the icon/title color. Defaults to `ColorScheme.on_surface`.
    #[must_use]
    pub fn foreground_color(mut self, color: Color) -> Self {
        self.foreground_color = Some(color);
        self
    }

    /// Overrides the `Material` elevation. Defaults to `0.0`.
    #[must_use]
    pub fn elevation(mut self, elevation: f32) -> Self {
        self.elevation = Some(elevation);
        self
    }

    /// Sets a slot rendered directly below the toolbar (typically a
    /// [`crate::TabBar`]) — see the module docs' `bottom` section for the
    /// exact Flexible-toolbar/fixed-`bottom` layout this composes and why
    /// the toolbar (not `bottom`) is what shrinks under a height shortfall.
    ///
    /// `bottom`'s [`preferred_size`](PreferredSizeView::preferred_size) is
    /// resolved once, here, and its height captured — matching
    /// [`crate::Scaffold::app_bar`]'s identical snapshot-at-builder-time
    /// contract, for the same reason (see that method's doc comment).
    #[must_use]
    pub fn bottom(mut self, bottom: impl PreferredSizeView) -> Self {
        self.bottom_preferred_height = bottom.preferred_size().height.get();
        self.bottom = Some(bottom.boxed());
        self
    }
}

impl Default for AppBar {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for AppBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppBar")
            .field("has_leading", &self.leading.is_some())
            .field(
                "automatically_imply_leading",
                &self.automatically_imply_leading,
            )
            .field("has_title", &self.title.is_some())
            .field("action_count", &self.actions.len())
            .field("toolbar_height", &self.toolbar_height)
            .field("has_bottom", &self.bottom.is_some())
            .finish_non_exhaustive()
    }
}

/// [`AppBar`]'s theme-resolved colors and text styles — `_AppBarDefaultsM3`
/// (`app_bar.dart:2521-2570`, oracle tag `3.44.0`) applied to the caller's
/// overrides, then coalesced. Factored out of [`AppBar::build`] so the
/// resolution itself (a pure function of a [`ThemeData`] and the three
/// override fields) is directly unit-testable without mounting a widget
/// tree — see this module's tests.
struct ResolvedAppBarStyle {
    background_color: Color,
    foreground_color: Color,
    elevation: f32,
    /// The **toolbar-wide** ambient text style — Flutter parity:
    /// `defaults.toolbarTextStyle?.copyWith(color: foregroundColor)`
    /// (`app_bar.dart`, oracle tag `3.44.0`). Always the M3 default recolored
    /// to `foreground_color`; FLUI has no `toolbarTextStyle` widget/theme
    /// override slot yet (named deferral — nothing reads one). [`AppBar::build`]
    /// wraps the WHOLE toolbar in this, so a bare `Text` in `leading`/`actions`
    /// gets a sane ambient style — this must stay independent of
    /// [`title_style`](Self::title_style) below, or a themed title style
    /// leaks into every other toolbar child (the bug this split fixes).
    toolbar_text_style: TextStyle,
    /// The **title-only** text style — Flutter parity: `widget.titleTextStyle
    /// ?? appBarTheme.titleTextStyle ?? defaults.titleTextStyle?.copyWith(
    /// color: foregroundColor)` (`app_bar.dart`, oracle tag `3.44.0`).
    /// [`AppBar::build`] wraps ONLY `self.title` in this, never the toolbar
    /// at large — matching the oracle, where `titleTextStyle` styles the
    /// title widget specifically, not `leading`/`actions`.
    ///
    /// A **verbatim** theme-tier value, not recolored, when
    /// `app_bar_theme.title_text_style` is set: only the default tier gets
    /// recolored to the resolved `foreground_color`, because a theme-supplied
    /// style already carries its own intended color.
    title_style: TextStyle,
}

/// Resolve `AppBar`'s M3 defaults through the widget → theme → default
/// cascade: `background_color` falls back to `ThemeData.app_bar_theme`'s own
/// `background_color`, then `ColorScheme.surface`; `foreground_color`
/// likewise falls back through `app_bar_theme` to `ColorScheme.on_surface`;
/// `elevation` through `app_bar_theme` to `0.0`. Flutter parity:
/// `widget.backgroundColor ?? appBarTheme.backgroundColor ??
/// defaults.backgroundColor` (and the `foregroundColor`/`elevation`
/// equivalents), `app_bar.dart`, oracle tag `3.44.0`.
///
/// `title_style` and `toolbar_text_style` are deliberately DIFFERENT values
/// once a theme configures `title_text_style` — see [`ResolvedAppBarStyle`]'s
/// own doc comment on why collapsing them back into one shared value would
/// leak the title's style onto every other toolbar child.
fn resolve_style(
    theme: &ThemeData,
    background_color: Option<Color>,
    foreground_color: Option<Color>,
    elevation: Option<f32>,
) -> ResolvedAppBarStyle {
    let app_bar_theme = theme.app_bar_theme.as_ref();

    let background_color = background_color
        .or_else(|| app_bar_theme.and_then(|t| t.background_color))
        .unwrap_or(theme.color_scheme.surface);
    let foreground_color = foreground_color
        .or_else(|| app_bar_theme.and_then(|t| t.foreground_color))
        .unwrap_or(theme.color_scheme.on_surface);
    let elevation = elevation
        .or_else(|| app_bar_theme.and_then(|t| t.elevation))
        .unwrap_or(0.0);
    let toolbar_text_style = theme
        .text_theme
        .title_large
        .clone()
        .unwrap_or_default()
        .with_color(foreground_color);
    let title_style = app_bar_theme
        .and_then(|t| t.title_text_style.clone())
        .unwrap_or_else(|| toolbar_text_style.clone());

    ResolvedAppBarStyle {
        background_color,
        foreground_color,
        elevation,
        toolbar_text_style,
        title_style,
    }
}

/// [`leading_short_circuit`]'s verdict: either the leading slot is already
/// settled with no need to consult a [`NavigatorHandle`] at all, or the
/// caller must look one up. Not `Option<Option<BoxedView>>` — clippy's
/// `option_option` lint rightly rejects that shape as ambiguous; this names
/// the two outcomes instead.
enum LeadingShortCircuit {
    /// Neither the explicit-`leading` nor the `automatically_imply_leading:
    /// false` short-circuit applies — settled state unknown without a
    /// navigator lookup.
    ConsultNavigator,
    /// Already resolved: `Some` (the explicit `leading`) or `None`
    /// (suppressed by `automatically_imply_leading: false`).
    Resolved(Option<BoxedView>),
}

/// The two outcomes `resolve_leading` can settle without ever consulting a
/// [`NavigatorHandle`]: an explicit `leading` always wins, and
/// `automatically_imply_leading: false` always suppresses the implied
/// button. Split out as a pure, `BuildContext`-free function so this half of
/// `resolve_leading`'s logic is unit-testable without a mounted tree; the
/// navigator-consulting half needs a real `BuildContext` and is covered
/// end-to-end by `tests/app_bar.rs`.
fn leading_short_circuit(
    leading: Option<&BoxedView>,
    automatically_imply_leading: bool,
) -> LeadingShortCircuit {
    if let Some(leading) = leading {
        return LeadingShortCircuit::Resolved(Some(leading.clone()));
    }
    if !automatically_imply_leading {
        return LeadingShortCircuit::Resolved(None);
    }
    LeadingShortCircuit::ConsultNavigator
}

/// Resolves the leading slot: `self.leading` verbatim if set, else a
/// synthesized [`BackButton`] when `automatically_imply_leading` is set and
/// a poppable navigator ancestor exists — see the module docs' "Implied
/// leading" section.
fn resolve_leading(
    leading: Option<&BoxedView>,
    automatically_imply_leading: bool,
    ctx: &dyn BuildContext,
) -> Option<BoxedView> {
    match leading_short_circuit(leading, automatically_imply_leading) {
        LeadingShortCircuit::Resolved(result) => return result,
        LeadingShortCircuit::ConsultNavigator => {}
    }
    let navigator = NavigatorHandle::maybe_of(ctx)?;
    navigator.can_pop().then(|| BackButton::new().boxed())
}

impl StatelessView for AppBar {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let theme = Theme::of(ctx);
        let ResolvedAppBarStyle {
            background_color,
            foreground_color,
            elevation,
            toolbar_text_style,
            title_style,
        } = resolve_style(
            &theme,
            self.background_color,
            self.foreground_color,
            self.elevation,
        );

        let leading = resolve_leading(self.leading.as_ref(), self.automatically_imply_leading, ctx);

        let mut toolbar_children: Vec<BoxedView> = Vec::new();
        if let Some(leading) = &leading {
            // Flutter parity: `_AppBarState.build` wraps `leading` in
            // `Center` (when it `is IconButton`; simplified here to
            // unconditional — see the module docs' "Implied leading"
            // section) then `ConstrainedBox(BoxConstraints.tightFor(width:
            // _kLeadingWidth))`, pinning the slot to a fixed 56px width
            // regardless of the leading widget's own intrinsic size —
            // NOT the 40px `IconButton` minimum size a bare, unwrapped
            // leading would otherwise collapse to in this `Row`.
            let leading_constraints = BoxConstraints::new(
                px(LEADING_WIDTH),
                px(LEADING_WIDTH),
                px(0.0),
                Pixels::INFINITY,
            );
            toolbar_children.push(
                ConstrainedBox::new(leading_constraints)
                    .child(Center::new().child(leading.clone()))
                    .boxed(),
            );
        }
        if let Some(title) = &self.title {
            // Always start-aligned — see the module docs' `centerTitle` note.
            // `title_style` is scoped to JUST this slot via its own
            // `DefaultTextStyle` — it must NOT reach the toolbar-wide wrap
            // below (which carries `toolbar_text_style` instead), or a
            // themed `title_text_style` would restyle bare `Text` in
            // `leading`/`actions` too. See `ResolvedAppBarStyle`'s doc
            // comment.
            toolbar_children.push(
                Expanded::new(
                    Align::new(Alignment::CENTER_LEFT)
                        .child(DefaultTextStyle::new(title_style, title.clone())),
                )
                .boxed(),
            );
        }
        if !self.actions.is_empty() {
            toolbar_children.push(Row::new(self.actions.clone()).boxed());
        }

        let toolbar = Row::new(toolbar_children).cross_axis_alignment(CrossAxisAlignment::Center);

        let themed_toolbar = IconTheme::new(
            IconThemeData {
                color: Some(foreground_color),
                ..IconThemeData::default()
            },
            DefaultTextStyle::new(
                toolbar_text_style,
                SizedBox::height(self.toolbar_height).child(toolbar),
            ),
        );

        // With a `bottom` slot, the toolbar+bottom pair replaces the bare
        // toolbar as the thing `SafeArea` pads — see the module docs' `bottom`
        // section for exactly why the toolbar is `Flexible` (shrinks under a
        // height shortfall) while `bottom` is not.
        let toolbar_and_bottom: BoxedView = if let Some(bottom) = &self.bottom {
            let flexible_toolbar = Flexible::new(
                ConstrainedBox::new(BoxConstraints {
                    max_height: px(self.toolbar_height),
                    ..BoxConstraints::UNCONSTRAINED
                })
                .child(themed_toolbar),
            );
            SizedBox::height(self.toolbar_height + self.bottom_preferred_height)
                .child(
                    Column::new(vec![flexible_toolbar.boxed(), bottom.clone()])
                        .main_axis_alignment(MainAxisAlignment::SpaceBetween),
                )
                .boxed()
        } else {
            themed_toolbar.boxed()
        };

        // The app bar pads itself against the top safe-area inset — see the
        // module docs' "consumes the top inset itself" section.
        let safe_toolbar = SafeArea::new().bottom(false).child(toolbar_and_bottom);

        Material::new(background_color)
            .elevation(elevation)
            .child(safe_toolbar)
    }
}

impl PreferredSizeView for AppBar {
    fn preferred_size(&self) -> Size {
        // Flutter oracle: `_PreferredAppBarSize(toolbarHeight, bottom?.preferredSize.height)`
        // (`app_bar.dart:76-81`, oracle tag `3.44.0`) — `toolbar_height` plus
        // `bottom`'s own preferred height, `0.0` when there is no `bottom`.
        Size::new(
            px(f32::INFINITY),
            px(self.toolbar_height + self.bottom_preferred_height),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_toolbar_height_matches_the_oracle_constant() {
        assert_eq!(DEFAULT_TOOLBAR_HEIGHT, 56.0);
    }

    #[test]
    fn preferred_size_reports_the_toolbar_height() {
        let bar = AppBar::new().toolbar_height(64.0);
        assert_eq!(bar.preferred_size().height, px(64.0));
    }

    #[test]
    fn preferred_size_defaults_to_the_default_toolbar_height() {
        let bar = AppBar::new();
        assert_eq!(bar.preferred_size().height, px(DEFAULT_TOOLBAR_HEIGHT));
    }

    /// Flutter parity: `_PreferredAppBarSize(toolbarHeight, bottom?.preferredSize.height)`
    /// — with a `bottom` slot set, `preferred_size` reports `toolbar_height
    /// + bottom.preferred_size().height`, not `toolbar_height` alone.
    ///
    /// Red-check: revert `preferred_size` to `px(self.toolbar_height)` alone
    /// — this assertion fails (`56.0` instead of `104.0`).
    #[test]
    fn preferred_size_adds_the_bottom_slots_height_when_set() {
        use flui_widgets::layout::PreferredSize;

        let bottom_height = 48.0;
        let bar = AppBar::new().bottom(PreferredSize::new(
            Size::new(px(f32::INFINITY), px(bottom_height)),
            SizedBox::shrink(),
        ));

        assert_eq!(
            bar.preferred_size().height,
            px(DEFAULT_TOOLBAR_HEIGHT + bottom_height),
            "preferred_size must be toolbar_height + the bottom slot's own preferred height"
        );
    }

    #[test]
    fn resolve_style_defaults_to_the_m3_token_table() {
        let theme = ThemeData::light();
        let resolved = resolve_style(&theme, None, None, None);

        assert_eq!(
            resolved.background_color, theme.color_scheme.surface,
            "background_color must fall back to ColorScheme.surface"
        );
        assert_eq!(
            resolved.foreground_color, theme.color_scheme.on_surface,
            "foreground_color must fall back to ColorScheme.on_surface"
        );
        assert_eq!(resolved.elevation, 0.0, "elevation must fall back to 0.0");
        assert_eq!(
            resolved.title_style,
            theme
                .text_theme
                .title_large
                .clone()
                .unwrap_or_default()
                .with_color(theme.color_scheme.on_surface),
            "the title style must be TextTheme.title_large recolored to the resolved foreground"
        );
    }

    /// Middle-tier coverage: with no widget-level override, an
    /// `app_bar_theme` slot's fields win over the M3 defaults, per field
    /// (not as an all-or-nothing struct) — a themed `elevation` must not
    /// force a themed `background_color`/`foreground_color` too.
    #[test]
    fn resolve_style_falls_through_to_the_app_bar_theme_when_no_widget_override_is_set() {
        let mut theme = ThemeData::light();
        let themed_background = Color::rgb(9, 8, 7);
        theme.app_bar_theme = Some(crate::theme_data::AppBarThemeData {
            background_color: Some(themed_background),
            elevation: Some(5.0),
            ..Default::default()
        });

        let resolved = resolve_style(&theme, None, None, None);

        assert_eq!(resolved.background_color, themed_background);
        assert_eq!(resolved.elevation, 5.0);
        // `foreground_color` was left unset on the theme slot — falls all
        // the way through to the M3 default, proving the per-field (not
        // whole-struct) fallthrough.
        assert_eq!(resolved.foreground_color, theme.color_scheme.on_surface);
    }

    /// Highest-tier coverage: an explicit widget-level override still wins
    /// over a configured `app_bar_theme`, matching Flutter's
    /// `widget.backgroundColor ?? appBarTheme.backgroundColor ?? …` order.
    #[test]
    fn resolve_style_widget_override_wins_over_the_app_bar_theme() {
        let mut theme = ThemeData::light();
        theme.app_bar_theme = Some(crate::theme_data::AppBarThemeData {
            background_color: Some(Color::rgb(1, 1, 1)),
            ..Default::default()
        });
        let widget_background = Color::rgb(2, 2, 2);

        let resolved = resolve_style(&theme, Some(widget_background), None, None);

        assert_eq!(resolved.background_color, widget_background);
    }

    /// The theme's `title_text_style` is used AS-IS, not recolored to the
    /// resolved `foreground_color` — unlike the default tier (see the next
    /// test) — matching the oracle's own
    /// `titleTextStyle ?? appBarTheme.titleTextStyle ?? defaults….copyWith(…)`
    /// order.
    #[test]
    fn resolve_style_theme_title_text_style_is_used_verbatim_not_recolored() {
        let mut theme = ThemeData::light();
        let themed_title_style =
            flui_types::typography::TextStyle::new().with_color(Color::rgb(3, 3, 3));
        theme.app_bar_theme = Some(crate::theme_data::AppBarThemeData {
            title_text_style: Some(themed_title_style.clone()),
            ..Default::default()
        });
        let widget_foreground = Color::rgb(4, 4, 4);

        let resolved = resolve_style(&theme, None, Some(widget_foreground), None);

        assert_eq!(resolved.title_style, themed_title_style);
        assert_ne!(resolved.title_style.color, Some(widget_foreground));
    }

    #[test]
    fn resolve_style_honors_explicit_overrides() {
        let theme = ThemeData::light();
        let background_override = Color::rgb(1, 2, 3);
        let foreground_override = Color::rgb(4, 5, 6);
        let resolved = resolve_style(
            &theme,
            Some(background_override),
            Some(foreground_override),
            Some(8.0),
        );

        assert_eq!(resolved.background_color, background_override);
        assert_eq!(resolved.foreground_color, foreground_override);
        assert_eq!(resolved.elevation, 8.0);
        assert_eq!(resolved.title_style.color, Some(foreground_override));
    }

    #[test]
    fn builders_set_the_expected_fields() {
        use flui_widgets::layout::PreferredSize;

        let bar = AppBar::new()
            .leading(flui_widgets::SizedBox::shrink())
            .title(flui_widgets::SizedBox::shrink())
            .actions(vec![flui_widgets::SizedBox::shrink().boxed()])
            .background_color(Color::rgb(10, 20, 30))
            .foreground_color(Color::rgb(40, 50, 60))
            .elevation(4.0)
            .bottom(PreferredSize::new(
                Size::new(px(f32::INFINITY), px(48.0)),
                SizedBox::shrink(),
            ));

        assert!(bar.leading.is_some());
        assert!(bar.title.is_some());
        assert_eq!(bar.actions.len(), 1);
        assert_eq!(bar.background_color, Some(Color::rgb(10, 20, 30)));
        assert_eq!(bar.foreground_color, Some(Color::rgb(40, 50, 60)));
        assert_eq!(bar.elevation, Some(4.0));
        assert!(bar.bottom.is_some());
        assert_eq!(bar.bottom_preferred_height, 48.0);
    }

    #[test]
    fn automatically_imply_leading_defaults_to_true() {
        assert!(AppBar::new().automatically_imply_leading);
    }

    #[test]
    fn automatically_imply_leading_builder_overrides_the_default() {
        let bar = AppBar::new().automatically_imply_leading(false);
        assert!(!bar.automatically_imply_leading);
    }

    #[test]
    fn leading_short_circuit_prefers_an_explicit_leading_regardless_of_the_imply_flag() {
        let leading = flui_widgets::SizedBox::shrink().boxed();
        let resolved = leading_short_circuit(Some(&leading), true);
        assert!(
            matches!(resolved, LeadingShortCircuit::Resolved(Some(_))),
            "an explicit leading must short-circuit to itself, never falling through to a \
             navigator lookup",
        );
    }

    #[test]
    fn leading_short_circuit_suppresses_the_implied_leading_when_the_flag_is_false() {
        assert!(matches!(
            leading_short_circuit(None, false),
            LeadingShortCircuit::Resolved(None)
        ));
    }

    #[test]
    fn leading_short_circuit_defers_to_the_navigator_lookup_when_neither_short_circuit_applies() {
        assert!(matches!(
            leading_short_circuit(None, true),
            LeadingShortCircuit::ConsultNavigator
        ));
    }
}
