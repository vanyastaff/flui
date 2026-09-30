//! [`AppBar`] — a Material app bar: a leading/title/actions toolbar on a
//! [`Material`] surface.
//!
//! # Implemented subset
//!
//! `leading`, `title`, `actions`, `toolbar_height`, `bottom`,
//! `background_color`, `foreground_color`, `elevation`, and the M3 token
//! defaults: `background_color` falls back to `ColorScheme.surface`,
//! `foreground_color` to `ColorScheme.on_surface`, `elevation` to `0.0`, and
//! the title's text style to `TextTheme.title_large` (recolored to the
//! resolved foreground).
//!
//! ## The app bar consumes the top inset itself
//!
//! The app bar wraps its toolbar in a top-only `SafeArea` — it pads itself
//! against `MediaQuery`'s top padding, rather than a parent adding that
//! padding on its behalf. This is unconditional (no `primary` toggle yet —
//! every `AppBar` behaves as a primary one), via
//! [`flui_sdk::widgets::SafeArea`]. A consequence: a standalone `AppBar`
//! (mounted with no `Scaffold` at all, just a `MediaQuery` ancestor) already
//! reserves the status-bar inset on its own.
//!
//! ## Title alignment: a platform switch, narrowed
//!
//! The Material spec centers the title on iOS/macOS with fewer than two
//! actions and start-aligns it elsewhere. FLUI's desktop targets are Linux and
//! Win32 — both start-aligned — so this substrate always start-aligns the
//! title (no `center_title` override, no navigation-toolbar-style toggle
//! yet). **Named limitation**: the centered macOS behavior waits for a
//! platform-adaptive seam; today every platform gets the
//! Android/Linux/Windows answer.
//!
//! ## `bottom`: a fixed-height slot below the toolbar
//!
//! [`AppBar::bottom`] accepts anything implementing [`PreferredSizeView`]
//! (typically a [`crate::TabBar`]) and mounts it directly beneath the
//! toolbar, inside the same [`SafeArea`]. The shape is a `Column` with
//! `MainAxisAlignment::SpaceBetween` whose first child is the toolbar wrapped
//! in a flexible `ConstrainedBox` (max height `toolbar_height`) and whose
//! second is `bottom` itself, unwrapped. The whole `Column` is forced to
//! `toolbar_height + bottom.preferred_size().height` via an outer
//! [`SizedBox`] (see [`AppBar::preferred_size`] below), then handed to the
//! same top-inset-consuming `SafeArea` the toolbar alone already used.
//!
//! **Why the toolbar flexes and `bottom` does not**: `Flexible` (not
//! `Expanded`) with a *loose* fit means the toolbar happily shrinks below
//! `toolbar_height` when the `Column`'s own available height falls short of
//! `toolbar_height + bottom_height` (a caller-imposed cap tighter than this
//! bar's own preferred size, or a `SafeArea` top inset large enough to eat
//! into it) — `bottom` is the `Column`'s other, non-flexible child, so it
//! always gets its own natural height first and the toolbar absorbs the
//! shortfall. This ensures a `TabBar` mounted as `bottom` never gets silently
//! clipped by a tight parent while the toolbar above it holds its full
//! height.
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
//! (fading `bottom` as a sliver app bar scrolls toward its collapsed state —
//! no scrolled-under/sliver-collapse substrate here to drive it) and a
//! `Scaffold`-side bottom-height re-consult on data change (see
//! [`PreferredSizeView`]'s own limitation note — this whole substrate
//! resolves it once, at `.bottom(...)`/`.app_bar(...)` builder time).
//!
//! ## Deferred, and named
//!
//! - `center_title` / a full navigation-toolbar layout — the title area here
//!   is a plain `Expanded` + `Align(center_left)`, not an overflow-aware
//!   middle-widget layout.
//! - Scrolled-under styling — no scroll-notification substrate to observe yet.
//! - `flexibleSpace` — stacked behind the toolbar+bottom; no consumer or
//!   substrate for it here yet.
//! - **Named limitation: no shadow suppression at a nonzero elevation.**
//!   The M3 app bar casts no shadow even when an explicit `elevation`
//!   override raises it above `0`: its shadow and surface-tint colors are
//!   transparent, and the surface communicates elevation through a tonal
//!   color shift instead (M3's elevation overlay), not a drop shadow.
//!   [`crate::Material`] has no `shadow_color` setter yet (see that module's
//!   docs' `surfaceTintColor` section for the matching gap), so this
//!   substrate cannot suppress it — an `AppBar::new().elevation(4.0)` here
//!   casts a real shadow the M3 spec would not. Revisit once `Material` grows
//!   `shadow_color`.
//!
//! ## Implied leading: a `BackButton`, no `DrawerButton`
//!
//! When `leading` is unset and `automatically_imply_leading` is set, the
//! Material spec synthesizes a drawer button if the enclosing `Scaffold` has
//! a drawer, else a back/close button if the enclosing route can be
//! dismissed. This substrate has no drawer and no modal-route abstraction
//! (routes are plain [`flui_sdk::widgets::Route`]s, not modal-aware ones), so
//! `resolve_leading` narrows the condition to what is reachable: no leading
//! set, `automatically_imply_leading` set, a [`NavigatorHandle`] ancestor
//! exists, and it reports [`NavigatorHandle::can_pop`] — always a
//! [`crate::BackButton`], never a close button (no fullscreen-dialog
//! substrate to pick that branch) or drawer button (no drawer substrate at
//! all). **Named limitation**, not a silently dropped case.
//!
//! **Second named limitation, worth calling out precisely:**
//! `NavigatorHandle::can_pop` is navigator-global, but the right question is
//! about the SPECIFIC route this `AppBar`'s subtree is inside — a
//! bottom-of-stack route's own app bar should not imply a back button even
//! while the navigator as a whole can pop (a route above it exists). This
//! substrate has no modal-route equivalent to ask "which route is this
//! `AppBar` inside, and specifically is IT poppable" — every mounted
//! `AppBar` under the same navigator sees the same global answer. In the
//! common case (one route showing an `AppBar` at a time, which is what an
//! `Overlay`-based navigator is for) this is unobservable; it only differs
//! when multiple routes carrying their own `AppBar` are simultaneously
//! mounted (see `tests/app_bar.rs`'s
//! `implied_leading_appears_once_the_navigator_can_pop` for exactly that
//! case, documented rather than hidden).
//!
//! ## The leading slot is a fixed `LEADING_WIDTH`, not the leading widget's own intrinsic size
//!
//! Whatever `leading` resolves to (explicit or implied) is wrapped in a
//! `ConstrainedBox` with a tight `LEADING_WIDTH` around a `Center` before it
//! reaches the toolbar `Row` (`LEADING_WIDTH` equals the toolbar height, "so
//! the leading button is square"). The `Center` wrap is unconditional
//! (harmless for any leading widget that already fills its own bounds).
//! **Without this wrap**, a bare 40×40 `IconButton` (this crate's M3
//! minimum size) would collapse the slot to 40px wide in the `Row` instead of
//! the M3-specified 56px — `LEADING_WIDTH`'s `ConstrainedBox` is what prevents
//! that. No `leading_width`/`AppBarTheme` override exists yet (named V1
//! deferral), so `LEADING_WIDTH` is the only width this slot ever takes.

use flui_sdk::painting::Color;
use flui_sdk::painting::TextStyle;
use flui_sdk::rendering::BoxConstraints;
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{
    Align, Center, Column, ConstrainedBox, CrossAxisAlignment, DefaultTextStyle, Expanded,
    Flexible, IconTheme, IconThemeData, MainAxisAlignment, NavigatorHandle, Positioned,
    PreferredSizeView, Row, SafeArea, SizedBox, Stack,
};
use flui_sdk::{geometry::Size, painting::Alignment};

use crate::back_button::BackButton;
use crate::material::Material;
use crate::theme::Theme;
use crate::theme_data::ThemeData;

/// The default toolbar height in logical pixels.
pub const DEFAULT_TOOLBAR_HEIGHT: f64 = 56.0;

/// The leading slot's fixed width — the toolbar height, "so the leading
/// button is square". No `leading_width`/`AppBarTheme` override exists yet in
/// this V1 (see the module docs' deferred list), so this constant is the only
/// width the slot ever takes.
const LEADING_WIDTH: f64 = DEFAULT_TOOLBAR_HEIGHT;

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
/// use flui_sdk::widgets::Text;
///
/// let _bar = AppBar::new().title(Text::new("FLUI")).toolbar_height(64.0);
/// ```
#[derive(Clone, StatelessView)]
pub struct AppBar {
    leading: Option<BoxedView>,
    automatically_imply_leading: bool,
    title: Option<BoxedView>,
    actions: Vec<BoxedView>,
    toolbar_height: f64,
    background_color: Option<Color>,
    foreground_color: Option<Color>,
    elevation: Option<f64>,
    bottom: Option<BoxedView>,
    /// A widget painted behind the toolbar and above this bar's own
    /// [`Material`] — the flexible-space slot. Inert at the
    /// bar's own preferred size; it earns its name inside a `SliverAppBar`,
    /// where the bar's box expands and collapses around it.
    flexible_space: Option<BoxedView>,
    /// `bottom`'s [`preferred_size`](PreferredSizeView::preferred_size)
    /// height, snapshotted at [`Self::bottom`]-builder time — see that
    /// method's doc comment and [`PreferredSizeView`]'s own "Named
    /// divergence" note on why this substrate resolves it once rather than
    /// re-consulting it later.
    bottom_preferred_height: f64,
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
            flexible_space: None,
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
    pub fn toolbar_height(mut self, toolbar_height: f64) -> Self {
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
    pub fn elevation(mut self, elevation: f64) -> Self {
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
        self.bottom_preferred_height = bottom.preferred_size().height;
        self.bottom = Some(bottom.boxed());
        self
    }

    /// Sets the widget painted behind the toolbar, above this bar's own
    /// [`Material`]. Mostly useful
    /// through `SliverAppBar`, where the bar's box expands around it.
    #[must_use]
    pub fn flexible_space(mut self, flexible_space: impl IntoView) -> Self {
        self.flexible_space = Some(flexible_space.into_view().boxed());
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

/// [`AppBar`]'s theme-resolved colors and text styles — the M3 defaults
/// applied to the caller's overrides, then coalesced. Factored out of [`AppBar::build`] so the
/// resolution itself (a pure function of a [`ThemeData`] and the three
/// override fields) is directly unit-testable without mounting a widget
/// tree — see this module's tests.
struct ResolvedAppBarStyle {
    background_color: Color,
    foreground_color: Color,
    elevation: f64,
    /// The **toolbar-wide** ambient text style. Always the M3 default recolored
    /// to `foreground_color`; FLUI has no toolbar-text-style widget/theme
    /// override slot yet (named deferral — nothing reads one). [`AppBar::build`]
    /// wraps the WHOLE toolbar in this, so a bare `Text` in `leading`/`actions`
    /// gets a sane ambient style — this must stay independent of
    /// [`title_style`](Self::title_style) below, or a themed title style
    /// leaks into every other toolbar child (the bug this split fixes).
    toolbar_text_style: TextStyle,
    /// The **title-only** text style: the widget's own, else the theme's, else
    /// the M3 default recolored to the foreground color.
    /// [`AppBar::build`] wraps ONLY `self.title` in this, never the toolbar
    /// at large — it styles the title widget specifically, not
    /// `leading`/`actions`.
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
/// `elevation` through `app_bar_theme` to `0.0`. The widget's own value wins
/// over the theme's, which wins over the M3 default.
///
/// `title_style` and `toolbar_text_style` are deliberately DIFFERENT values
/// once a theme configures `title_text_style` — see [`ResolvedAppBarStyle`]'s
/// own doc comment on why collapsing them back into one shared value would
/// leak the title's style onto every other toolbar child.
fn resolve_style(
    theme: &ThemeData,
    background_color: Option<Color>,
    foreground_color: Option<Color>,
    elevation: Option<f64>,
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
            // `leading` is wrapped in a `Center` (unconditionally — see the
            // module docs' leading-slot section) then a tight-width
            // `ConstrainedBox`, pinning the slot to a fixed 56px width
            // regardless of the leading widget's own intrinsic size —
            // NOT the 40px `IconButton` minimum size a bare, unwrapped
            // leading would otherwise collapse to in this `Row`.
            let leading_constraints =
                BoxConstraints::new(LEADING_WIDTH, LEADING_WIDTH, 0.0, f64::INFINITY);
            toolbar_children.push(
                ConstrainedBox::new(leading_constraints)
                    .child(Center::new().child(leading.clone()))
                    .boxed(),
            );
        }
        if let Some(title) = &self.title {
            // Always start-aligned — see the module docs' title-alignment note.
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
                    max_height: self.toolbar_height,
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

        // The flexible space paints ABOVE this bar's own Material and BELOW
        // the toolbar. Outside that slot order the bar's opaque surface either
        // hides the flexible content or fails to back it.
        let surface_content: BoxedView = match &self.flexible_space {
            Some(flexible_space) => {
                Stack::new((Positioned::fill(flexible_space.clone()), safe_toolbar)).boxed()
            }
            None => safe_toolbar.boxed(),
        };

        Material::new(background_color)
            .elevation(elevation)
            .child(surface_content)
    }
}

impl PreferredSizeView for AppBar {
    fn preferred_size(&self) -> Size {
        // `toolbar_height` plus `bottom`'s own preferred height, `0.0` when
        // there is no `bottom`.
        Size::new(
            f64::INFINITY,
            self.toolbar_height + self.bottom_preferred_height,
        )
    }
}
