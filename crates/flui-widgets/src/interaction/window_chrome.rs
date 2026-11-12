//! The parts of a title bar an application draws itself (ADR-0286): [`WindowTitleBar`]
//! moves the window, [`WindowCaptionButton`] minimizes, maximizes or closes it,
//! [`WindowResizeArea`] resizes it, and [`WindowMovePassThrough`] keeps a subtree's presses
//! moving it.
//!
//! Each marks its box as an input region of a window-chrome kind, which the window's backend
//! answers the system's hit test from, so the system moves, resizes and snaps the window as
//! it does for its own title bar. They mark nothing while the system draws the window's
//! title bar ([`WindowFrame::System`]): the same tree runs on every frame and host. Inside a
//! title bar, every widget that takes presses (a button, a field, a gesture detector) is cut
//! out of the move area by itself; hover-only listeners are not.

use std::cell::Cell;
use std::rc::Rc;

use flui_objects::{RenderInputRegion, RenderPressPassThrough};
use flui_platform::{WindowFrame, WindowMode, WindowState};
use flui_rendering::input_regions::{CaptionButton, InputRegionKind, ResizeEdge};
use flui_rendering::protocol::BoxProtocol;
use flui_view::prelude::*;
use flui_view::{
    Child, EventCx, RebuildReason, RenderView, WindowHandle, WindowSubscription, impl_render_view,
};

use crate::localization::{
    BoxedWidgetsLocalizations, DefaultWidgetsLocalizations, WidgetsLocalizations,
};
use crate::{RawButton, Semantics};

/// What of the window's state the chrome widgets draw from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Chrome {
    /// Whether the application draws the title bar: a frame of the application's own or
    /// none at all.
    app_draws: bool,
    mode: WindowMode,
}

impl Chrome {
    /// What a widget outside any window draws from: the system's frame.
    const SYSTEM: Self = Self {
        app_draws: false,
        mode: WindowMode::Normal,
    };

    fn of(state: &WindowState) -> Self {
        Self {
            app_draws: matches!(state.frame, WindowFrame::AppTitleBar(_) | WindowFrame::None),
            mode: state.mode,
        }
    }
}

/// A chrome widget's view of its window: the handle its buttons act through and the state
/// it draws from, kept current by a subscription that rebuilds the widget when it changes.
#[derive(Default)]
pub struct WindowChromeState {
    /// The window, what of its state the widget draws from, and the subscription that
    /// keeps that current; `None` outside a window.
    watching: Option<Watching>,
}

struct Watching {
    window: WindowHandle,
    chrome: Rc<Cell<Chrome>>,
    _subscription: WindowSubscription,
}

impl std::fmt::Debug for WindowChromeState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowChromeState")
            .field("chrome", &self.chrome())
            .finish_non_exhaustive()
    }
}

impl WindowChromeState {
    fn watch(&mut self, ctx: &dyn LifecycleContext) {
        let Some(window) = ctx.window() else {
            return;
        };
        let chrome = Rc::new(Cell::new(Chrome::SYSTEM));
        let seen = Rc::downgrade(&chrome);
        let rebuild = ctx.rebuild_handle();
        // The callback runs on the owner's turn after a publication, never inside this call.
        let subscribed = window.subscribe(move |state| {
            let Some(seen) = seen.upgrade() else { return };
            let now = Chrome::of(state);
            if seen.replace(now) != now {
                rebuild.schedule(RebuildReason::StateChange);
            }
        });
        match subscribed {
            Ok((state, subscription)) => {
                chrome.set(Chrome::of(&state));
                self.watching = Some(Watching {
                    window,
                    chrome,
                    _subscription: subscription,
                });
            }
            Err(closed) => tracing::debug!(%closed, "window chrome: the window is gone"),
        }
    }

    fn chrome(&self) -> Chrome {
        self.watching
            .as_ref()
            .map_or(Chrome::SYSTEM, |watching| watching.chrome.get())
    }
}

/// Marks its child's box as the window's title bar: a press there moves the window as the
/// system's own title bar does, with the system's snapping, double-click action and window
/// menu.
///
/// Widgets in the bar that take presses (buttons, fields, anything with a tap or drag
/// recognizer) are cut out of the move area and keep their presses; wrap one in
/// [`WindowMovePassThrough`] to have it move the window instead. Marks nothing while the
/// system draws the window's title bar.
///
/// ```ignore
/// WindowTitleBar::new().child(Row::new().children([title, search_field, buttons]))
/// ```
#[derive(Clone, Debug, StatefulView)]
pub struct WindowTitleBar {
    child: Child,
}

impl Default for WindowTitleBar {
    fn default() -> Self {
        Self::new()
    }
}

impl WindowTitleBar {
    /// A title bar around nothing yet.
    #[must_use]
    pub fn new() -> Self {
        Self {
            child: Child::empty(),
        }
    }

    /// Set the bar's content.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl StatefulView for WindowTitleBar {
    type State = WindowChromeState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        WindowChromeState::default()
    }
}

impl ViewState<WindowTitleBar> for WindowChromeState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.watch(ctx);
    }

    fn build(&self, view: &WindowTitleBar, _ctx: &dyn BuildContext) -> impl IntoView {
        ChromeRegion {
            kind: InputRegionKind::WindowMove,
            enabled: self.chrome().app_draws,
            child: view.child.clone(),
        }
    }
}

/// Marks its child's box as a border the window is resized from at `edge`, for a window
/// without the system's frame ([`WindowFrame::None`]) that draws its own border, or a band
/// inside an application's title bar. Marks nothing while the system draws the title bar,
/// or while the window is maximized, minimized or fullscreen.
#[derive(Clone, Debug, StatefulView)]
pub struct WindowResizeArea {
    edge: ResizeEdge,
    child: Child,
}

impl WindowResizeArea {
    /// A border the window is resized from at `edge`.
    #[must_use]
    pub fn new(edge: ResizeEdge) -> Self {
        Self {
            edge,
            child: Child::empty(),
        }
    }

    /// Set what draws the border.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl StatefulView for WindowResizeArea {
    type State = WindowChromeState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        WindowChromeState::default()
    }
}

impl ViewState<WindowResizeArea> for WindowChromeState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.watch(ctx);
    }

    fn build(&self, view: &WindowResizeArea, _ctx: &dyn BuildContext) -> impl IntoView {
        let chrome = self.chrome();
        ChromeRegion {
            kind: InputRegionKind::WindowResize(view.edge),
            enabled: chrome.app_draws && chrome.mode == WindowMode::Normal,
            child: view.child.clone(),
        }
    }
}

/// A caption button the application draws: its child is the button's look, and a press
/// minimizes, maximizes or restores, or asks to close the window.
///
/// Its box is marked as that caption button, so the system treats it as its own: on Windows
/// a maximize button shows the snap layouts over it. It is a button to assistive technology,
/// named after what it does now (a maximize button of a maximized window restores it).
/// Its region is marked only while the application draws the title bar; the button works
/// either way.
#[derive(Clone, Debug, StatefulView)]
pub struct WindowCaptionButton {
    button: CaptionButton,
    child: BoxedView,
}

impl WindowCaptionButton {
    /// A button that minimizes the window.
    #[must_use]
    pub fn minimize(child: impl IntoView) -> Self {
        Self::of(CaptionButton::Minimize, child)
    }

    /// A button that maximizes the window, or restores it while maximized.
    #[must_use]
    pub fn maximize(child: impl IntoView) -> Self {
        Self::of(CaptionButton::Maximize, child)
    }

    /// A button that asks to close the window; the application's close-request handler
    /// decides.
    #[must_use]
    pub fn close(child: impl IntoView) -> Self {
        Self::of(CaptionButton::Close, child)
    }

    fn of(button: CaptionButton, child: impl IntoView) -> Self {
        Self {
            button,
            child: child.into_view().boxed(),
        }
    }
}

impl StatefulView for WindowCaptionButton {
    type State = WindowChromeState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        WindowChromeState::default()
    }
}

impl ViewState<WindowCaptionButton> for WindowChromeState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.watch(ctx);
    }

    fn build(&self, view: &WindowCaptionButton, ctx: &dyn BuildContext) -> impl IntoView {
        let chrome = self.chrome();
        let maximized = chrome.mode == WindowMode::Maximized;
        let localizations = BoxedWidgetsLocalizations::maybe_of(ctx);
        let labels: &dyn WidgetsLocalizations = match &localizations {
            Some(localizations) => &***localizations,
            None => &DefaultWidgetsLocalizations,
        };
        let label = match view.button {
            CaptionButton::Minimize => labels.minimize_window_label(),
            CaptionButton::Maximize if maximized => labels.restore_window_label(),
            CaptionButton::Maximize => labels.maximize_window_label(),
            CaptionButton::Close | _ => labels.close_window_label(),
        };
        let content = Semantics::new().label(label).child(view.child.clone());
        let button = match self
            .watching
            .as_ref()
            .map(|watching| watching.window.clone())
        {
            Some(window) => {
                let pressed = view.button;
                RawButton::new(content).on_pressed(move |_cx: &mut EventCx<'_>| {
                    press(&window, pressed);
                })
            }
            None => RawButton::new(content),
        };
        ChromeRegion {
            kind: InputRegionKind::CaptionButton(view.button),
            enabled: chrome.app_draws,
            child: Child::some(button.into_view()),
        }
    }
}

/// What a press on `button` asks of the window.
fn press(window: &WindowHandle, button: CaptionButton) {
    let refused = match button {
        CaptionButton::Minimize => window
            .set_mode(WindowMode::Minimized)
            .err()
            .map(|e| e.to_string()),
        CaptionButton::Maximize => {
            // The mode now, not the one the button was built with: a press can land
            // after a publication and before the rebuild it schedules.
            let maximized = window
                .state()
                .is_ok_and(|state| state.mode == WindowMode::Maximized);
            let mode = if maximized {
                WindowMode::Normal
            } else {
                WindowMode::Maximized
            };
            window.set_mode(mode).err().map(|e| e.to_string())
        }
        CaptionButton::Close | _ => window.request_close().err().map(|e| e.to_string()),
    };
    if let Some(refused) = refused {
        tracing::warn!(?button, %refused, "window caption button: the window refused");
    }
}

/// Keeps presses anywhere in its child moving the window as the title bar around it does:
/// the widgets below it that take presses are not cut out of the bar's move area. For a
/// decoration that only reacts to a press, never for a control the user operates.
#[derive(Clone, Debug)]
pub struct WindowMovePassThrough {
    passing: bool,
    child: Child,
}

impl Default for WindowMovePassThrough {
    fn default() -> Self {
        Self::new()
    }
}

impl WindowMovePassThrough {
    /// Passes the child's presses through to the move area.
    #[must_use]
    pub fn new() -> Self {
        Self {
            passing: true,
            child: Child::empty(),
        }
    }

    /// Whether presses pass through; `false` keeps the child mounted and cuts its press
    /// takers out again.
    #[must_use]
    pub fn passing(mut self, passing: bool) -> Self {
        self.passing = passing;
        self
    }

    /// Set the child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for WindowMovePassThrough {
    type Protocol = BoxProtocol;
    type RenderObject = RenderPressPassThrough;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        let mut pass_through = RenderPressPassThrough::new();
        let _ = pass_through.set_enabled(self.passing);
        pass_through
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        render_object.set_enabled(self.passing)
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(WindowMovePassThrough);

/// A box marked as one chrome region while `enabled`.
#[derive(Clone, Debug)]
struct ChromeRegion {
    kind: InputRegionKind,
    enabled: bool,
    child: Child,
}

impl RenderView for ChromeRegion {
    type Protocol = BoxProtocol;
    type RenderObject = RenderInputRegion;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        let mut region = RenderInputRegion::new(self.kind);
        let _ = region.set_enabled(self.enabled);
        region
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        render_object.set_kind(self.kind) | render_object.set_enabled(self.enabled)
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(ChromeRegion);
