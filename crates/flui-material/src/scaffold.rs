//! [`Scaffold`] — the top-level Material page structure: an app bar, a body,
//! a floating action button, and drawer/end-drawer slots, laid out by the
//! private `ScaffoldLayoutDelegate` defined further down in this module.
//!
//! # Flutter parity
//!
//! `material/scaffold.dart`'s `Scaffold` (oracle tag `3.44.0`). Implemented
//! subset: the `app_bar`, `body`, `floating_action_button`, `drawer`, and
//! `end_drawer` slots of `_ScaffoldLayout.performLayout`
//! (`scaffold.dart:1027-1296`) — the remaining six slots
//! (`bottomNavigationBar`, `persistentFooter`, `materialBanner`, `bodyScrim`,
//! `snackBar`, `bottomSheet`, `statusBar`) are not ported; a future slot
//! extends `ScaffoldLayoutDelegate`, it does not restructure it.
//! `resize_to_avoid_bottom_inset` defaults to `true`, same as the oracle.
//! `background_color` falls back to `ColorScheme.surface` (the oracle's
//! `themeData.scaffoldBackgroundColor`, which this substrate has not ported
//! onto [`crate::ThemeData`] yet — `ColorScheme.surface` is the closer M3
//! analogue in the meantime).
//!
//! ## Drawer wiring
//!
//! `Scaffold` is a [`StatefulView`] specifically to own the two
//! [`GlobalKey`]s and the [`DrawerHandle`] the drawer/end-drawer
//! [`DrawerController`]s need — see `crate::drawer`'s module docs for the
//! `GlobalKey` bridge and why [`DrawerHandle`] is `!Send`. The state tracks
//! each drawer's opened bool (`crate::drawer::DrawerController::on_open_changed`
//! updates it and reschedules this build), which drives BOTH the dynamic
//! child order — Flutter parity: `Scaffold.build`'s `if (_endDrawerOpened.value)
//! { buildDrawer, buildEndDrawer } else { buildEndDrawer, buildDrawer }`
//! (`scaffold.dart:3211-3217`) — added last, an open end-drawer's scrim/panel
//! must paint on top of, and hit-test before, a closed start-drawer's edge
//! strip — and `on_drawer_changed`/`on_end_drawer_changed`'s relay to the
//! app author.
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
//! `bottomNavigationBar`, `persistentFooterButtons`, `MaterialBanner`,
//! `SnackBar` (via `ScaffoldMessenger`), `bottomSheet`,
//! `extendBody`/`extendBodyBehindAppBar`, non-`endFloat`
//! `FloatingActionButtonLocation`s. Also deferred, specific to the drawer
//! slots this update adds: the `AppBar` auto-hamburger (no `AppBar` ↔
//! `Scaffold` coupling exists yet), per-side `enable_open_drag_gesture`
//! (oracle: separate `drawerEnableOpenDragGesture`/
//! `endDrawerEnableOpenDragGesture` — this substrate exposes one flag for
//! both sides), and `drawerDragStartBehavior`/`drawerBarrierDismissible`
//! overrides at the `Scaffold` level (fixed at
//! `DrawerController`'s own defaults — see `crate::drawer`). None of these
//! slots exist in `ScaffoldLayoutDelegate` yet — the five that do
//! (`app_bar`/`body`/`floating_action_button`/`drawer`/`end_drawer`) are
//! copied verbatim from `_ScaffoldLayout`'s branches for those same slots,
//! so adding a slot later is additive, not a rewrite.
//!
//! **Named divergence: no `TextDirection` / RTL.** The oracle's
//! `_ScaffoldLayout` carries a `textDirection` field, both to mirror
//! `MediaQuery`/`Directionality` into `shouldRelayout`'s comparison and
//! because `endFloat` resolves "end" against it (`AxisDirection` — `end` is
//! `left` under RTL). `ScaffoldLayoutDelegate` hardcodes the FAB to the
//! right edge and carries no `text_direction` field at all, so an RTL
//! subtree gets the LTR position and `should_relayout` cannot react to a
//! direction change. `flui_widgets::Directionality` exists, but neither
//! `Scaffold` nor `ScaffoldLayoutDelegate` reads it (see [`crate::AppBar`]'s
//! own centerTitle note for the matching directionality gap there); revisit
//! together.

use std::any::Any;
use std::rc::Rc;
use std::sync::Arc;

use flui_rendering::constraints::BoxConstraints;
use flui_types::geometry::px;
use flui_types::styling::Color;
use flui_types::{EdgeInsets, Offset, Pixels, Size};
use flui_view::prelude::*;
use flui_view::{GlobalKey, impl_inherited_view};
use flui_widgets::{
    ConstrainedBox, CustomMultiChildLayout, LayoutId, MediaQuery, MultiChildLayoutContext,
    MultiChildLayoutDelegate, PreferredSizeView,
};

use crate::drawer::{
    Drawer, DrawerAlignment, DrawerController, DrawerControllerState, DrawerHandle,
};
use crate::material::Material;
use crate::theme::Theme;

/// The `app_bar` slot id — see [`ScaffoldLayoutDelegate`].
const SLOT_APP_BAR: &str = "app_bar";
/// The `body` slot id — see [`ScaffoldLayoutDelegate`].
const SLOT_BODY: &str = "body";
/// The `floating_action_button` slot id — see [`ScaffoldLayoutDelegate`].
const SLOT_FLOATING_ACTION_BUTTON: &str = "floating_action_button";
/// The `drawer` slot id — see [`ScaffoldLayoutDelegate`].
const SLOT_DRAWER: &str = "drawer";
/// The `end_drawer` slot id — see [`ScaffoldLayoutDelegate`].
const SLOT_END_DRAWER: &str = "end_drawer";

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
#[derive(Clone, StatefulView)]
pub struct Scaffold {
    body: Option<BoxedView>,
    app_bar: Option<BoxedView>,
    app_bar_preferred_height: f32,
    floating_action_button: Option<BoxedView>,
    resize_to_avoid_bottom_inset: bool,
    background_color: Option<Color>,
    drawer: Option<Drawer>,
    end_drawer: Option<Drawer>,
    on_drawer_changed: Option<Rc<dyn Fn(bool)>>,
    on_end_drawer_changed: Option<Rc<dyn Fn(bool)>>,
    drawer_scrim_color: Option<Color>,
    drawer_edge_drag_width: Option<f32>,
    enable_open_drag_gesture: bool,
}

impl Scaffold {
    /// An empty `Scaffold`: no app bar, no body, no floating action button,
    /// no drawer/end-drawer, `resize_to_avoid_bottom_inset: true`,
    /// `enable_open_drag_gesture: true`, and a `background_color` falling
    /// back to `ColorScheme.surface`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            body: None,
            app_bar: None,
            app_bar_preferred_height: 0.0,
            floating_action_button: None,
            resize_to_avoid_bottom_inset: true,
            background_color: None,
            drawer: None,
            end_drawer: None,
            on_drawer_changed: None,
            on_end_drawer_changed: None,
            drawer_scrim_color: None,
            drawer_edge_drag_width: None,
            enable_open_drag_gesture: true,
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

    /// Sets the start-side drawer, opened via
    /// [`DrawerHandle::open_drawer`]/edge-swipe from the start edge.
    #[must_use]
    pub fn drawer(mut self, drawer: Drawer) -> Self {
        self.drawer = Some(drawer);
        self
    }

    /// Sets the end-side drawer, opened via
    /// [`DrawerHandle::open_end_drawer`]/edge-swipe from the end edge.
    #[must_use]
    pub fn end_drawer(mut self, end_drawer: Drawer) -> Self {
        self.end_drawer = Some(end_drawer);
        self
    }

    /// Called whenever the start-side drawer opens or closes.
    #[must_use]
    pub fn on_drawer_changed(mut self, callback: impl Fn(bool) + 'static) -> Self {
        self.on_drawer_changed = Some(Rc::new(callback));
        self
    }

    /// Called whenever the end-side drawer opens or closes.
    #[must_use]
    pub fn on_end_drawer_changed(mut self, callback: impl Fn(bool) + 'static) -> Self {
        self.on_end_drawer_changed = Some(Rc::new(callback));
        self
    }

    /// Overrides both drawers' scrim color. Defaults to `Colors.black54`.
    #[must_use]
    pub fn drawer_scrim_color(mut self, color: Color) -> Self {
        self.drawer_scrim_color = Some(color);
        self
    }

    /// Overrides both drawers' closed-state edge-drag detection width.
    #[must_use]
    pub fn drawer_edge_drag_width(mut self, width: f32) -> Self {
        self.drawer_edge_drag_width = Some(width);
        self
    }

    /// Whether either drawer can be opened with an edge-swipe from closed.
    /// Defaults to `true`. Applies to both `drawer` and `end_drawer` — see
    /// the module docs' deferred-and-named note on the per-side split this
    /// substrate does not (yet) expose.
    #[must_use]
    pub fn enable_open_drag_gesture(mut self, enabled: bool) -> Self {
        self.enable_open_drag_gesture = enabled;
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
            .field("has_drawer", &self.drawer.is_some())
            .field("has_end_drawer", &self.end_drawer.is_some())
            .field(
                "resize_to_avoid_bottom_inset",
                &self.resize_to_avoid_bottom_inset,
            )
            .finish_non_exhaustive()
    }
}

/// Publishes a [`DrawerHandle`] to a [`Scaffold`]'s subtree — the runtime
/// capability to open/close its drawer/end-drawer. Flutter parity:
/// `_ScaffoldScope`/`Scaffold.of`, narrowed to the drawer runtime API (this
/// substrate's `ScaffoldState` has no snack-bar/bottom-sheet API yet).
#[derive(Clone, Debug)]
pub struct ScaffoldScope {
    handle: DrawerHandle,
    child: BoxedView,
}

impl ScaffoldScope {
    /// Access the nearest ancestor [`Scaffold`]'s [`DrawerHandle`].
    ///
    /// # Panics
    ///
    /// Panics if there is no [`Scaffold`] ancestor. Use
    /// [`maybe_of`](Self::maybe_of) for a non-panicking variant.
    #[must_use]
    pub fn of(ctx: &dyn BuildContext) -> DrawerHandle {
        Self::maybe_of(ctx).expect(
            "ScaffoldScope::of called with no Scaffold ancestor in the tree — wrap the subtree \
             in a Scaffold, or use ScaffoldScope::maybe_of with a caller-chosen fallback",
        )
    }

    /// Look up the nearest ancestor [`Scaffold`]'s [`DrawerHandle`]. Returns
    /// `None` if there is no [`Scaffold`] ancestor.
    ///
    /// No dependency is registered: the handle is a stable capability object
    /// (its own methods read live state), not a value whose *identity*
    /// changing should trigger a rebuild — same reasoning `crate::theme::Theme`'s
    /// sibling lookups document for other stable ambient handles.
    #[must_use]
    pub fn maybe_of(ctx: &dyn BuildContext) -> Option<DrawerHandle> {
        ctx.get::<Self, _>(|scope| scope.handle.clone())
    }
}

impl InheritedView for ScaffoldScope {
    type Data = DrawerHandle;

    fn data(&self) -> &Self::Data {
        &self.handle
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, _old: &Self) -> bool {
        // The handle is the same stable object across every rebuild (created
        // once in `ScaffoldState::create_state`) — nothing for a dependent
        // to react to.
        false
    }
}

impl_inherited_view!(ScaffoldScope);

/// Persistent state behind [`Scaffold`] — see the module docs' "Drawer
/// wiring" section.
pub struct ScaffoldState {
    handle: DrawerHandle,
}

impl std::fmt::Debug for ScaffoldState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScaffoldState")
            .field("drawer_open", &self.handle.is_drawer_open())
            .field("end_drawer_open", &self.handle.is_end_drawer_open())
            .finish_non_exhaustive()
    }
}

impl StatefulView for Scaffold {
    type State = ScaffoldState;

    fn create_state(&self) -> Self::State {
        ScaffoldState {
            handle: DrawerHandle::new(),
        }
    }
}

impl ViewState<Scaffold> for ScaffoldState {
    fn build(&self, view: &Scaffold, ctx: &dyn BuildContext) -> impl IntoView {
        let theme = Theme::of(ctx);
        let background_color = view.background_color.unwrap_or(theme.color_scheme.surface);
        let media_query = MediaQuery::of(ctx);

        self.handle.set_has_drawer(view.drawer.is_some());
        self.handle.set_has_end_drawer(view.end_drawer.is_some());

        let mut children: Vec<LayoutId> = Vec::new();

        if let Some(body) = &view.body {
            let mut reduced_media_query = media_query.clone();
            if view.app_bar.is_some() {
                reduced_media_query.padding.top = px(0.0);
            }
            if view.resize_to_avoid_bottom_inset {
                reduced_media_query.view_insets.bottom = px(0.0);
            }
            children.push(LayoutId::new(
                SLOT_BODY,
                MediaQuery::new(reduced_media_query, body.clone()),
            ));
        }

        if let Some(app_bar) = &view.app_bar {
            let max_height = px(view.app_bar_preferred_height) + media_query.padding.top;
            let cap = BoxConstraints {
                max_height,
                ..BoxConstraints::UNCONSTRAINED
            };
            children.push(LayoutId::new(
                SLOT_APP_BAR,
                ConstrainedBox::new(cap).child(app_bar.clone()),
            ));
        }

        if let Some(floating_action_button) = &view.floating_action_button {
            children.push(LayoutId::new(
                SLOT_FLOATING_ACTION_BUTTON,
                floating_action_button.clone(),
            ));
        }

        let drawer_slot = view.drawer.as_ref().map(|drawer| {
            LayoutId::new(
                SLOT_DRAWER,
                self.build_drawer_controller(
                    DrawerAlignment::Start,
                    drawer,
                    self.handle.drawer_key(),
                    self.handle.is_drawer_open(),
                    view.on_drawer_changed.clone(),
                    view,
                    ctx,
                ),
            )
        });
        let end_drawer_slot = view.end_drawer.as_ref().map(|end_drawer| {
            LayoutId::new(
                SLOT_END_DRAWER,
                self.build_drawer_controller(
                    DrawerAlignment::End,
                    end_drawer,
                    self.handle.end_drawer_key(),
                    self.handle.is_end_drawer_open(),
                    view.on_end_drawer_changed.clone(),
                    view,
                    ctx,
                ),
            )
        });

        // Flutter parity: `Scaffold.build` (`scaffold.dart:3211-3217`) — with
        // the end drawer open, the start drawer is added first so the open
        // end drawer's scrim/panel paint on top of, and hit-test before, the
        // closed start drawer's edge strip; otherwise the reverse.
        if self.handle.is_end_drawer_open() {
            children.extend(drawer_slot);
            children.extend(end_drawer_slot);
        } else {
            children.extend(end_drawer_slot);
            children.extend(drawer_slot);
        }

        // Flutter oracle: `minInsets = MediaQuery.paddingOf(context).copyWith(
        // bottom: resizeToAvoidBottomInset ? viewInsetsOf(context).bottom : 0.0)`
        // (`scaffold.dart:3220-3222`) — the safe-area padding on every edge,
        // with the bottom edge swapped for the keyboard inset when resizing.
        let min_insets = EdgeInsets::new(
            media_query.padding.top,
            media_query.padding.right,
            if view.resize_to_avoid_bottom_inset {
                media_query.view_insets.bottom
            } else {
                px(0.0)
            },
            media_query.padding.left,
        );

        // Flutter oracle: `minViewPadding = MediaQuery.viewPaddingOf(context)
        // .copyWith(bottom: resizeToAvoidBottomInset && viewInsetsOf(context)
        // .bottom != 0.0 ? 0.0 : null)` (`scaffold.dart:3226-3230`) — the raw
        // safe-area bottom inset (e.g. the home-indicator area on iOS),
        // zeroed only while the keyboard is actually up and being resized
        // around. `MediaQueryData` has no `viewPadding` field distinct from
        // `padding` (both name the same "safe area from the OS" concept
        // here), so `padding.bottom` is this substrate's `viewPadding.bottom`.
        let min_view_padding_bottom =
            if view.resize_to_avoid_bottom_inset && media_query.view_insets.bottom != px(0.0) {
                px(0.0)
            } else {
                media_query.padding.bottom
            };

        let delegate: Arc<dyn MultiChildLayoutDelegate> = Arc::new(ScaffoldLayoutDelegate {
            min_insets,
            min_view_padding_bottom,
        });

        ScaffoldScope {
            handle: self.handle.clone(),
            child: Material::new(background_color)
                .child(CustomMultiChildLayout::new(delegate, children))
                .boxed(),
        }
    }
}

impl ScaffoldState {
    /// Builds the [`DrawerController`] for one drawer slot, wiring its
    /// `on_open_changed` to update [`DrawerHandle`]'s tracked opened-bool,
    /// reschedule this `Scaffold`'s own rebuild (so the dynamic slot order
    /// and `on_drawer_changed` relay both react), and forward to the app
    /// author's callback.
    #[allow(clippy::too_many_arguments, reason = "internal helper, not public API")]
    fn build_drawer_controller(
        &self,
        alignment: DrawerAlignment,
        drawer: &Drawer,
        key: GlobalKey<DrawerControllerState>,
        is_open: bool,
        on_changed: Option<Rc<dyn Fn(bool)>>,
        view: &Scaffold,
        ctx: &dyn BuildContext,
    ) -> DrawerController {
        let handle = self.handle.clone();
        let rebuild = ctx.rebuild_handle();
        let set_opened: fn(&DrawerHandle, bool) = match alignment {
            DrawerAlignment::Start => DrawerHandle::set_drawer_opened,
            DrawerAlignment::End => DrawerHandle::set_end_drawer_opened,
        };

        let mut controller = DrawerController::new(key, alignment, drawer.clone())
            .panel_width(drawer.configured_width())
            .is_open(is_open)
            .enable_open_drag_gesture(view.enable_open_drag_gesture)
            .on_open_changed(move |opened| {
                set_opened(&handle, opened);
                rebuild.schedule();
                if let Some(callback) = &on_changed {
                    callback(opened);
                }
            });
        if let Some(color) = view.drawer_scrim_color {
            controller = controller.scrim_color(color);
        }
        if let Some(width) = view.drawer_edge_drag_width {
            controller = controller.edge_drag_width(width);
        }
        controller
    }
}

/// The layout algorithm for [`Scaffold`]'s `app_bar` / `body` /
/// `floating_action_button` / `drawer` / `end_drawer` slots.
///
/// Flutter parity: `_ScaffoldLayout` (`scaffold.dart:991-1308`), narrowed to
/// the five slots this substrate ports — see the module docs for the full
/// deferred-slot list and the inset contract this delegate enforces.
#[derive(Debug, Clone, PartialEq)]
struct ScaffoldLayoutDelegate {
    /// The safe-area padding on every edge, with the bottom edge swapped for
    /// the keyboard inset when `resize_to_avoid_bottom_inset` is set. See
    /// `Scaffold::build`'s citation of `_ScaffoldLayout`'s `minInsets`.
    min_insets: EdgeInsets,
    /// The raw safe-area bottom inset the floating action button must clear
    /// (e.g. the home-indicator area), zeroed while the keyboard is up and
    /// being resized around. See `Scaffold::build`'s citation of
    /// `_ScaffoldState.build`'s `minViewPadding`.
    min_view_padding_bottom: Pixels,
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
            // 554-581`).
            let fab_x = size.width
                - px(FLOATING_ACTION_BUTTON_MARGIN)
                - self.min_insets.right
                - fab_size.width;

            // `FabFloatOffsetY`: `safeMargin = max(margin,
            // minViewPadding.bottom - bottomContentHeight + margin)`, where
            // `bottomContentHeight = scaffoldSize.height - contentBottom`
            // (`:560-568`). This is NOT a flat `margin` in general — with a
            // nonzero `min_view_padding_bottom` (e.g. a 34px home-indicator
            // area) and no keyboard, `bottomContentHeight` is `0` and
            // `safeMargin` grows past `margin` to keep the button clear of
            // that safe-area edge. It only reduces to the flat `margin` when
            // `min_view_padding_bottom <= bottom_content_height` — the
            // ordinary case once the keyboard (which zeroes
            // `min_view_padding_bottom`, see `Scaffold::build`) is up.
            let bottom_content_height = size.height - content_bottom;
            let safe_margin = (self.min_view_padding_bottom - bottom_content_height
                + px(FLOATING_ACTION_BUTTON_MARGIN))
            .max(px(FLOATING_ACTION_BUTTON_MARGIN));
            let fab_y = content_bottom - fab_size.height - safe_margin;
            ctx.position_child(SLOT_FLOATING_ACTION_BUTTON, Offset::new(fab_x, fab_y));
        }

        // Flutter oracle: `_ScaffoldLayout.performLayout` (`scaffold.dart:1282-1289`)
        // — both drawer slots are laid out tight at the scaffold's full size,
        // pinned to the origin; each `DrawerController` handles its own
        // internal open/closed sizing (an edge strip when closed, the full
        // area when open).
        if ctx.has_child(SLOT_DRAWER) {
            ctx.layout_child(SLOT_DRAWER, BoxConstraints::tight(size));
            ctx.position_child(SLOT_DRAWER, Offset::ZERO);
        }
        if ctx.has_child(SLOT_END_DRAWER) {
            ctx.layout_child(SLOT_END_DRAWER, BoxConstraints::tight(size));
            ctx.position_child(SLOT_END_DRAWER, Offset::ZERO);
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

    fn zero_insets_delegate() -> ScaffoldLayoutDelegate {
        ScaffoldLayoutDelegate {
            min_insets: EdgeInsets::new(px(0.0), px(0.0), px(0.0), px(0.0)),
            min_view_padding_bottom: px(0.0),
        }
    }

    #[test]
    fn should_relayout_is_false_for_equal_delegates() {
        let a = zero_insets_delegate();
        let b = zero_insets_delegate();
        assert!(!MultiChildLayoutDelegate::should_relayout(&a, &b));
    }

    #[test]
    fn should_relayout_is_true_when_bottom_min_inset_changes() {
        let a = zero_insets_delegate();
        let b = ScaffoldLayoutDelegate {
            min_insets: EdgeInsets::new(px(0.0), px(0.0), px(300.0), px(0.0)),
            ..zero_insets_delegate()
        };
        assert!(MultiChildLayoutDelegate::should_relayout(&a, &b));
    }

    #[test]
    fn should_relayout_is_true_when_min_view_padding_bottom_changes() {
        let a = zero_insets_delegate();
        let b = ScaffoldLayoutDelegate {
            min_view_padding_bottom: px(34.0),
            ..zero_insets_delegate()
        };
        assert!(
            MultiChildLayoutDelegate::should_relayout(&a, &b),
            "a min_view_padding_bottom change (e.g. rotating so the home indicator moves) \
             must trigger relayout even when min_insets is unchanged",
        );
    }

    /// Pins `FabFloatOffsetY`'s exact arithmetic
    /// (`floating_action_button_location.dart:554-581`) directly against the
    /// delegate's `perform_layout`, independent of the mounted-harness tests
    /// in `tests/scaffold.rs` — a `MockContext` the way
    /// `flui-rendering`'s own `MultiChildLayoutDelegate` doctest/unit tests
    /// do (`crates/flui-rendering/src/delegates/multi_child_layout_delegate.rs`).
    #[test]
    fn fab_y_grows_the_safe_margin_for_a_nonzero_min_view_padding_bottom() {
        use std::collections::HashMap;

        use flui_types::Offset;

        // Mirrors `flui_rendering::delegates::multi_child_layout_delegate`'s
        // own in-crate `MockContext` test pattern.
        struct MockContext {
            children: HashMap<String, Size>,
            positions: HashMap<String, Offset>,
        }
        impl MultiChildLayoutContext for MockContext {
            fn has_child(&self, child_id: &str) -> bool {
                self.children.contains_key(child_id)
            }
            fn layout_child(&mut self, child_id: &str, _constraints: BoxConstraints) -> Size {
                self.children[child_id]
            }
            fn position_child(&mut self, child_id: &str, offset: Offset) {
                self.positions.insert(child_id.to_string(), offset);
            }
        }

        let delegate = ScaffoldLayoutDelegate {
            min_insets: EdgeInsets::new(px(0.0), px(0.0), px(0.0), px(0.0)),
            min_view_padding_bottom: px(34.0),
        };
        let mut ctx = MockContext {
            children: HashMap::from([(
                SLOT_FLOATING_ACTION_BUTTON.to_string(),
                Size::new(px(56.0), px(56.0)),
            )]),
            positions: HashMap::new(),
        };

        delegate.perform_layout(&mut ctx, Size::new(px(400.0), px(800.0)));

        // bottom_content_height = 800 - content_bottom = 800 - 800 = 0
        // (no bottom min_insets, no bottom widgets).
        // safe_margin = max(16, 34 - 0 + 16) = 50.
        // fab_y = 800 - 56 - 50 = 694.
        assert_eq!(
            ctx.positions[SLOT_FLOATING_ACTION_BUTTON],
            Offset::new(px(400.0 - 16.0 - 56.0), px(694.0)),
            "a nonzero min_view_padding_bottom (e.g. the 34px home-indicator area) with no \
             keyboard must lift the FAB safe_margin above the flat kFloatingActionButtonMargin, \
             not park it at content_bottom - fab_height - 16",
        );
    }
}
