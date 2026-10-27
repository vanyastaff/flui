//! iOS window implementation (UIKit), with its system chrome: the status-bar
//! style its content asks for ([`status_bar`]).
//!
//! `IOSWindow` wraps a `UIWindow` whose root view controller hosts a
//! [`FluiView`] — an `NSObject`-rooted `UIView` subclass that receives the
//! touch events and forwards them as `PlatformInput`. The window's own
//! metrics come from the view (points) and the screen (scale), so a rotation
//! or split-view resize is just a new `bounds` read.
//!
//! # Raw window handle
//!
//! The window's surface target (`IOSSurface`) hands wgpu the content `UIView` through
//! `raw_window_handle::UiKitWindowHandle`; `wgpu-hal`'s Metal backend derives
//! a `CAMetalLayer` from it (`raw_window_metal::Layer::from_ui_view`), so no
//! layer is created here — the same shape macOS uses with `NSView`.

// Some screen and geometry APIs the window reads remain deprecated in SDK 26.2.
#![expect(deprecated)]
// Direct UIKit Objective-C calls have no safe wrapper: the window is one of the
// crate's `unsafe` FFI islands, opted out at its module boundary.
#![expect(unsafe_code)]

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use cursor_icon::CursorIcon;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_foundation::{
    NSArray, NSDefaultRunLoopMode, NSNumber, NSObjectProtocol, NSRunLoop, NSSet,
};
use objc2_quartz_core::CADisplayLink;
use objc2_ui_kit::{
    UIHoverGestureRecognizer, UIRectEdge, UIScreen, UITouch, UITouchType, UITraitCollection,
    UITraitEnvironment, UIUserInterfaceStyle, UIView, UIWindow, UIWindowScene,
};

use crate::host;
use crate::host::{Host, WindowFacets, WindowPositions};
use crate::host_window::HostWindow;

use crate::owner::OpenWindowError;
use crate::owner::apple::IOSLoop;
use crate::owner::apple::ios::{
    HostDisplays, NativeOwner, ScreenIdentity, TouchPhase, ViewRef, ViewTouches,
};
use crate::owner::apple::ios::{
    IOSSceneAttachmentId, IOSSceneSessionId, LifecycleObservation, SceneConsumer,
    SceneInstallation, SceneWindow, SceneWindows,
};
use crate::owner::panic_boundary::{AfterPanic, os_entry};
use crate::owner::signal::OwnerSignal;
use crate::owner::window_delivery::{Retirement, WindowDelivery};
use crate::platform::HostPlatform;
use crate::text::TextInput;
use crate::window::installation::{PendingWindowInstallation, WindowInstallation};
use crate::window::state::{ReportedFacts, WindowStateCell};
use crate::{BootstrapError, PlatformError, PlatformProxy};
use flui_foundation::geometry::{EdgeInsets, Size};
use flui_platform::{
    AdmittedCommand, ApplyError, WindowAppearance, WindowCapabilities, WindowCommand,
    WindowGeometry, WindowState,
};
use flui_platform::{
    CursorError, PlatformWindow, RedrawRequester, SurfaceChange, SurfaceTarget,
    WindowExecutionState, WindowId, WindowOptions,
};
use status_bar::FluiViewController;
pub(crate) use status_bar::IOSSystemChrome;

/// The content view: a `UIView` subclass that forwards touches.
///
/// `objc2`'s `define_class!` requires the ivars be declared on the class; the
/// callback storage is reached through a `Retained` held in an ivar rather
/// than a raw pointer ivar as the macOS backend does, so the view owns its
/// callbacks and there is no separate deallocation step.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SamplingAdmission {
    Detached,
    Constructing,
    Active {
        token: IOSSceneAttachmentId,
        window: usize,
    },
    Retiring,
}

pub struct FluiViewIvars {
    delivery: Arc<WindowDelivery>,
    /// The window's text input, which keeps the text size captured from the
    /// view's traits; set once, as the window is made after its view.
    text: std::cell::OnceCell<Arc<dyn TextInput>>,
    owner_signal: std::sync::Weak<crate::owner::signal::OwnerSignal>,
    /// The input's part of the view: its touches, as the table made it.
    input: RefCell<Box<dyn ViewTouches>>,
    /// Where this view stands in the attach/detach lifecycle: whether it may
    /// report metrics at all, and which window a report belongs to, so a
    /// callback arriving after a detach is dropped instead of announcing the
    /// metrics of a window this view no longer serves.
    sampling: std::cell::Cell<SamplingAdmission>,
    sampling_active: std::cell::Cell<bool>,
    sampling_pending: std::cell::Cell<bool>,
    /// The per-frame tick source, created once and paused/resumed around the
    /// app's background transitions. `RefCell` because the view's methods
    /// reach it through a shared `&self`.
    display_link: RefCell<Option<Retained<CADisplayLink>>>,
    /// The rate the window last asked for; the link asks for it whenever it
    /// starts, changes screens or the hint changes.
    frame_rate_hint: std::cell::Cell<flui_platform::FrameRateHint>,
    /// The platform's display source, which measures the cadence the link's
    /// ticks show and holds the period it publishes.
    displays: Arc<dyn HostDisplays>,
    /// The screen this window's scene is on; shared with the window, whose
    /// `display_id` any thread may call.
    screen: Arc<parking_lot::Mutex<ScreenIdentity>>,
    /// The window this view belongs to; owns the link periods it reports.
    window_id: WindowId,
    metrics: Arc<parking_lot::Mutex<WindowMetrics>>,
    /// Whether the view's `userInterfaceStyle` was dark when last sampled.
    /// Apart from `metrics`: the theme is the system's, so it needs neither a
    /// laid-out size nor an attached window to be reported.
    dark: Arc<AtomicBool>,
}

define_class!(
    // SAFETY:
    // - `UIView` does not have any subclassing requirements beyond those the
    //   superclass already satisfies.
    // - The class does not implement `Drop`, so `define_class!`'s
    //   `dealloc` story is the superclass's.
    //
    /// The `UIView` subclass that forwards touches to the window's callbacks.
    #[unsafe(super(UIView))]
    #[thread_kind = MainThreadOnly]
    #[name = "FluiView"]
    #[ivars = FluiViewIvars]
    struct FluiView;

    // Every method runs its body in the panic boundary: a panic answers UIKit
    // as if the event had been handled, rather than unwinding through it.
    impl FluiView {
        #[unsafe(method(traitCollectionDidChange:))]
        fn trait_collection_did_change(&self, previous: Option<&UITraitCollection>) {
            os_entry("traitCollectionDidChange:", (), AfterPanic::Answer(|()| ()), || {
                self.pin_for_native_callback();
                // SAFETY: UIView's implementation receives the borrowed old traits.
                let _: () = unsafe { msg_send![super(self), traitCollectionDidChange: previous] };
                crate::owner::panic_boundary::contain_owner_callback(|| self.invalidate_text_sizing());
                crate::owner::panic_boundary::contain_owner_callback(|| self.sample_appearance());
            });
        }

        #[unsafe(method(didMoveToWindow))]
        fn did_move_to_window(&self) {
            os_entry("didMoveToWindow", (), AfterPanic::Answer(|()| ()), || {
                self.pin_for_native_callback();
                // SAFETY: UIView declares this selector and the receiver is pinned.
                let _: () = unsafe { msg_send![super(self), didMoveToWindow] };
                crate::owner::panic_boundary::contain_owner_callback(|| self.invalidate_text_sizing());
                crate::owner::panic_boundary::contain_owner_callback(|| self.sample_appearance());
            });
        }

        #[unsafe(method(onPencilHover:))]
        fn on_pencil_hover(&self, recognizer: &UIHoverGestureRecognizer) {
            os_entry("onPencilHover:", (), AfterPanic::Answer(|()| ()), || {
                self.pin_for_native_callback();
                if self.window().is_none() { return; }
                let events = self.ivars().input.borrow_mut().hover(recognizer, self);
                for event in events {
                    if self.window().is_none() { break; }
                    self.delivery().dispatch_input(event);
                }
            });
        }

        #[unsafe(method(touchesBegan:withEvent:))]
        fn touches_began(&self, touches: &NSSet<UITouch>, _event: Option<&AnyObject>) {
            os_entry("touchesBegan:withEvent:", (), AfterPanic::Answer(|()| ()), || {
                self.pin_for_native_callback();
                self.dispatch_touches(touches, TouchPhase::Down);
            });
        }

        #[unsafe(method(touchesMoved:withEvent:))]
        fn touches_moved(&self, touches: &NSSet<UITouch>, _event: Option<&AnyObject>) {
            os_entry("touchesMoved:withEvent:", (), AfterPanic::Answer(|()| ()), || {
                self.pin_for_native_callback();
                self.dispatch_touches(touches, TouchPhase::Move);
            });
        }

        #[unsafe(method(touchesEnded:withEvent:))]
        fn touches_ended(&self, touches: &NSSet<UITouch>, _event: Option<&AnyObject>) {
            os_entry("touchesEnded:withEvent:", (), AfterPanic::Answer(|()| ()), || {
                self.pin_for_native_callback();
                self.dispatch_touches(touches, TouchPhase::Up);
            });
        }

        #[unsafe(method(touchesCancelled:withEvent:))]
        fn touches_cancelled(&self, touches: &NSSet<UITouch>, _event: Option<&AnyObject>) {
            os_entry("touchesCancelled:withEvent:", (), AfterPanic::Answer(|()| ()), || {
                self.pin_for_native_callback();
                self.dispatch_touches(touches, TouchPhase::Cancel);
            });
        }

        #[unsafe(method(layoutSubviews))]
        fn layout_subviews(&self) {
            os_entry("layoutSubviews", (), AfterPanic::Answer(|()| ()), || {
                self.pin_for_native_callback();
                // SAFETY: `super` is the `UIView` implementation of
                // `layoutSubviews`; calling it is what a correct override does.
                // The annotation pins the return type, which `msg_send!` cannot
                // infer for a `void` super call.
                let _: () = unsafe { msg_send![super(self), layoutSubviews] };
                crate::owner::panic_boundary::contain_owner_callback(|| self.sample_metrics());
            });
        }

        #[unsafe(method(safeAreaInsetsDidChange))]
        fn safe_area_insets_did_change(&self) {
            os_entry("safeAreaInsetsDidChange", (), AfterPanic::Answer(|()| ()), || {
                self.pin_for_native_callback();
                // SAFETY: `super` resolves to the superclass named in the
                // `#[unsafe(super(UIView))]` declaration above at compile time, so
                // this runs UIKit's implementation rather than re-entering this
                // override — which a plain `self` send would do without bound. The
                // receiver is a live `&FluiView` because the pinning call above
                // retains it first; the annotation pins the `void` return that
                // `msg_send!` cannot infer. `UIView` gained this selector in
                // iOS 11, and no lower deployment target is configured here.
                let _: () = unsafe { msg_send![super(self), safeAreaInsetsDidChange] };
                crate::owner::panic_boundary::contain_owner_callback(|| self.sample_metrics());
            });
        }

        /// The `CADisplayLink` target: one tick per refresh requests a frame.
        #[unsafe(method(onDisplayLink:))]
        fn on_display_link(&self, link: &CADisplayLink) {
            os_entry("onDisplayLink:", (), AfterPanic::Answer(|()| ()), || {
                self.pin_for_native_callback();
                let current = self.ivars().display_link.borrow().as_ref()
                    .is_some_and(|active| std::ptr::eq::<CADisplayLink>(&raw const **active, link));
                if current && !link.isPaused() {
                    self.observe_link_cadence(link);
                    self.delivery().dispatch_request_frame();
                }
            });
        }
    }

    unsafe impl NSObjectProtocol for FluiView {}
);

/// What a view needs to name its display: the platform's source, the screen
/// slot it shares with its window, and the window that owns its link periods.
struct ViewDisplay {
    displays: Arc<dyn HostDisplays>,
    screen: Arc<parking_lot::Mutex<ScreenIdentity>>,
    window: WindowId,
}

impl FluiView {
    /// Build the content view for one window, sized to the screen's bounds,
    /// which hands its touches to `touches`.
    fn new(
        mtm: MainThreadMarker,
        delivery: Arc<WindowDelivery>,
        touches: Box<dyn ViewTouches>,
        metrics: Arc<parking_lot::Mutex<WindowMetrics>>,
        dark: Arc<AtomicBool>,
        owner_signal: std::sync::Weak<crate::owner::signal::OwnerSignal>,
        display: ViewDisplay,
    ) -> Retained<Self> {
        let bounds = objc2_ui_kit::UIScreen::mainScreen(mtm).bounds();
        let this = mtm.alloc::<Self>();
        let this = this.set_ivars(FluiViewIvars {
            delivery,
            text: std::cell::OnceCell::new(),
            owner_signal,
            input: RefCell::new(touches),
            sampling: std::cell::Cell::new(SamplingAdmission::Detached),
            sampling_active: std::cell::Cell::new(false),
            sampling_pending: std::cell::Cell::new(false),
            display_link: RefCell::new(None),
            frame_rate_hint: std::cell::Cell::new(flui_platform::FrameRateHint::Default),
            displays: display.displays,
            screen: display.screen,
            window_id: display.window,
            metrics,
            dark,
        });
        // SAFETY: `initWithFrame:` is `UIView`'s designated initializer; the
        // `super(this)` receiver is the partially-initialized allocation with
        // its ivars already set above, which is the `define_class!` idiom.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: bounds] };
        // Multi-touch is what a gesture layer needs; without it UIKit reports
        // only the first finger of a chord.
        this.setMultipleTouchEnabled(true);
        // SAFETY: the live view declares onPencilHover: above. UIKit retains
        // the recognizer when installed; its target does not retain the view.
        let hover = unsafe {
            UIHoverGestureRecognizer::initWithTarget_action(
                mtm.alloc(),
                Some(&this),
                Some(sel!(onPencilHover:)),
            )
        };
        hover.setAllowedTouchTypes(&NSArray::from_retained_slice(&[NSNumber::new_isize(
            UITouchType::Stylus.0,
        )]));
        hover.setCancelsTouchesInView(false);
        this.addGestureRecognizer(&hover);
        // A GPU-rendered surface is opaque, so tell UIKit not to composite
        // anything behind it.
        this.setOpaque(true);
        this.sample_appearance();
        this
    }

    /// Record the view's light/dark style, reporting it when it changed.
    fn sample_appearance(&self) {
        let traits = self.traitCollection();
        // SAFETY: a plain getter on the immutable trait collection UIKit just
        // returned; `FluiView` is main-thread-only, so this runs on the main
        // thread UIKit requires.
        let dark = unsafe { traits.userInterfaceStyle() } == UIUserInterfaceStyle::Dark;
        if self.ivars().dark.swap(dark, Ordering::Relaxed) != dark {
            self.delivery().publish_state();
        }
    }

    /// The view's traits changed, or it moved to another window: what the
    /// window's text input captured of them no longer holds.
    fn invalidate_text_sizing(&self) {
        if let Some(text) = self.ivars().text.get() {
            text.invalidate_sizing();
        }
    }

    /// The window's text size, captured by its text input from the view's
    /// traits while the view is attached to the window it serves; a capture
    /// that saw the view move on meanwhile is withdrawn.
    fn capture_text_sizing(
        &self,
    ) -> Result<crate::TextSizingCaptureState, crate::TextSizingCaptureError> {
        let admission = self.ivars().sampling.get();
        let SamplingAdmission::Active {
            window: identity, ..
        } = admission
        else {
            return Err(crate::TextSizingCaptureError::Unavailable);
        };
        let window = self
            .window()
            .ok_or(crate::TextSizingCaptureError::Unavailable)?;
        if std::ptr::from_ref::<UIWindow>(&window) as usize != identity {
            self.invalidate_text_sizing();
            return Err(crate::TextSizingCaptureError::Unavailable);
        }
        let text = self
            .ivars()
            .text
            .get()
            .map(Arc::clone)
            .ok_or(crate::TextSizingCaptureError::Unavailable)?;
        let captured = text.capture_sizing()?;
        let attached = self
            .window()
            .is_some_and(|window| std::ptr::from_ref::<UIWindow>(&window) as usize == identity);
        if self.ivars().sampling.get() != admission || !attached {
            text.invalidate_sizing();
            return Err(crate::TextSizingCaptureError::Unavailable);
        }
        Ok(captured)
    }

    /// Create the per-frame tick, targeting this view's `onDisplayLink:`.
    ///
    /// The link is added to the main run loop in the default mode — the run
    /// loop `UIApplicationMain` drives — so it fires on the main thread and
    /// therefore on the owner lane.
    ///
    /// `screen` is the one the window's scene is on: the link is asked to run
    /// at that screen's best rate, so a ProMotion panel is not held at 60 Hz.
    fn start_display_link(&self, screen: &UIScreen) {
        if self.ivars().display_link.borrow().is_some() {
            return;
        }
        // SAFETY: `displayLinkWithTarget:selector:` is the documented
        // constructor; `self` is a live `FluiView` and `onDisplayLink:` is a
        // method this class declares above. `addToRunLoop:forMode:` attaches
        // it to the main run loop.
        let link =
            unsafe { CADisplayLink::displayLinkWithTarget_selector(self, sel!(onDisplayLink:)) };
        // SAFETY: the main run loop is live and `NSDefaultRunLoopMode` is the
        // run loop's own default mode constant.
        unsafe {
            link.addToRunLoop_forMode(&NSRunLoop::mainRunLoop(), NSDefaultRunLoopMode);
        }
        self.ivars()
            .displays
            .request_frame_rate(&link, screen, self.ivars().frame_rate_hint.get());
        self.ivars().display_link.replace(Some(link));
    }

    /// Take the window's frame-rate `hint`, and ask the link for it on the
    /// screen the window's scene is on. A view with no link yet, or in no
    /// scene, keeps the hint for the link it starts.
    fn take_frame_rate_hint(&self, hint: flui_platform::FrameRateHint) {
        self.ivars().frame_rate_hint.set(hint);
        let link = self.ivars().display_link.borrow().clone();
        let screen = self
            .window()
            .and_then(|window| window.windowScene())
            .map(|scene| scene.screen());
        if let (Some(link), Some(screen)) = (link, screen) {
            self.ivars()
                .displays
                .request_frame_rate(&link, &screen, hint);
        }
    }

    /// Take `screen` as the one this window is on. A change of screen resets
    /// what the link measured, asks the link for the new screen's best rate,
    /// and tells the owner the display may have changed.
    fn adopt_screen(&self, screen: &UIScreen) {
        let identity = self
            .ivars()
            .displays
            .identify(MainThreadMarker::from(self), screen);
        let moved = self.ivars().displays.move_window(
            self.ivars().window_id,
            &self.ivars().screen,
            identity,
        );
        if !moved {
            return;
        }
        let link = self.ivars().display_link.borrow().clone();
        if let Some(link) = link {
            self.ivars().displays.request_frame_rate(
                &link,
                screen,
                self.ivars().frame_rate_hint.get(),
            );
        }
        self.delivery().publish_state();
    }

    /// The scene is gone: this window names no screen until it is attached
    /// again. Its link's measurement on the old screen is forgotten and, if
    /// the identity was set, the owner is told the display changed. No lock or
    /// borrow is held across the callback.
    fn detach_screen(&self) {
        let moved = self.ivars().displays.move_window(
            self.ivars().window_id,
            &self.ivars().screen,
            ScreenIdentity::default(),
        );
        if moved {
            self.delivery().publish_state();
        }
    }

    /// Hand the cadence one tick shows (the time to the frame it targets) to
    /// the displays, which publish it when it leaves the band around the last
    /// published period.
    fn observe_link_cadence(&self, link: &CADisplayLink) {
        let interval = link.targetTimestamp() - link.timestamp();
        let number = self.ivars().screen.lock().number;
        let published =
            self.ivars()
                .displays
                .observe_link(self.ivars().window_id, number, interval);
        if !published {
            return;
        }
        if let Some(signal) = self.ivars().owner_signal.upgrade() {
            let _ = signal.wake();
        }
    }

    /// Pause or resume the tick — driven by the app's background/foreground
    /// transitions so a suspended app does no work.
    pub(super) fn set_display_link_paused(&self, paused: bool) {
        let link = self.ivars().display_link.borrow().clone();
        if let Some(link) = link {
            link.setPaused(paused);
        }
    }

    fn invalidate_display_link(&self) {
        let link = self.ivars().display_link.borrow_mut().take();
        if let Some(link) = link {
            link.invalidate();
            // A link that is gone measures nothing: the screen's period is
            // unknown until its replacement ticks.
            let number = self.ivars().screen.lock().number;
            self.ivars()
                .displays
                .forget_link(self.ivars().window_id, number);
        }
    }

    fn delivery(&self) -> &Arc<WindowDelivery> {
        &self.ivars().delivery
    }

    fn pin_for_native_callback(&self) {
        // UIKit can keep using the receiver after a Rust observer terminally
        // closes its logical window. Hold it through the outer autorelease pool.
        let _ = Retained::autorelease_ptr(self.retain());
        if let Some(window) = self.window() {
            if let Some(controller) = window.rootViewController() {
                let _ = Retained::autorelease_ptr(controller);
            }
            let _ = Retained::autorelease_ptr(window);
        }
    }

    /// Convert a UIKit touch batch and dispatch each event, then request a
    /// frame — the same "input dirties the presentation" step Android's
    /// motion-event path takes.
    fn dispatch_touches(&self, touches: &NSSet<UITouch>, phase: TouchPhase) {
        let mut any = false;
        for touch in touches {
            let Some(current_window) = self.window() else {
                break;
            };
            if !touch.window().as_ref().is_some_and(|origin| {
                std::ptr::eq::<UIWindow>(&raw const **origin, &raw const *current_window)
            }) {
                continue;
            }
            let events = self.ivars().input.borrow_mut().touch(&touch, phase);
            for event in events {
                let result = self.delivery().dispatch_input(event);
                any |= result.default_prevented;
            }
        }
        if any {
            self.delivery().dispatch_request_frame();
        }
    }

    /// Commit one coherent native snapshot, then emit changed fields without
    /// holding storage. Every emission rechecks admission after foreign callbacks.
    fn sample_metrics(&self) {
        self.ivars().sampling_pending.set(true);
        if self.ivars().sampling_active.replace(true) {
            return;
        }
        struct Lease<'a>(&'a std::cell::Cell<bool>);
        impl Drop for Lease<'_> {
            fn drop(&mut self) {
                self.0.set(false);
            }
        }
        let _lease = Lease(&self.ivars().sampling_active);
        while self.ivars().sampling_pending.replace(false) {
            self.sample_metrics_once();
        }
    }

    fn sample_metrics_once(&self) {
        let admission = self.ivars().sampling.get();
        // Only `window` is destructured out: the attachment `token` rides
        // along in `admission` and is validated by the whole-variant equality
        // re-checks below (before the commit, and again after the resize
        // dispatch), which is what makes an attachment swap mid-sample drop
        // this snapshot. The address compare that follows covers a different
        // case the token cannot see — UIKit re-hosting this view into another
        // window without going through `attach`, which leaves `admission`
        // untouched while `self.window()` no longer agrees with it.
        let SamplingAdmission::Active {
            window: identity, ..
        } = admission
        else {
            return;
        };
        let Some(window) = self.window() else {
            return;
        };
        if std::ptr::from_ref::<UIWindow>(&window) as usize != identity {
            return;
        }
        // The scene can be shown on another screen without a new attachment;
        // layout and trait changes are where this view learns of it. A change
        // dispatches a callback, so admission is rechecked below as for any
        // other emission.
        if let Some(scene) = window.windowScene() {
            let screen = scene.screen();
            self.adopt_screen(&screen);
            // A view without `drawRect:` keeps a content scale of 1.0 on any
            // screen, and the Metal layer wgpu adds follows this view's layer
            // scale, so a 3x screen would get a 1x drawable stretched three
            // times and a snapshot claiming a pixel ratio of 1. The screen's
            // scale is the one to render and report at.
            if self.contentScaleFactor() != screen.scale() {
                self.setContentScaleFactor(screen.scale());
            }
        }
        let bounds = self.bounds();
        let insets = self.safeAreaInsets();
        let next = WindowMetrics {
            size: Size::new(bounds.size.width, bounds.size.height),
            scale: self.contentScaleFactor(),
            safe_area: EdgeInsets::new(insets.top, insets.right, insets.bottom, insets.left),
        };
        if self.ivars().sampling.get() != admission
            || next.size.width <= 0.0
            || next.size.height <= 0.0
        {
            return;
        }
        let previous = {
            let mut metrics = self.ivars().metrics.lock();
            std::mem::replace(&mut *metrics, next)
        };
        if previous != next {
            // The size, scale and safe area go out in one revision.
            self.delivery().publish_state();
        }
    }
}

struct IOSLifecycle {
    execution: WindowExecutionState,
    focused: bool,
    visible: bool,
    generation: u64,
    pending: VecDeque<(LifecycleObservation, u64)>,
    dispatching: bool,
}

#[derive(Clone, Copy, PartialEq)]
struct WindowMetrics {
    size: Size<f64>,
    scale: f64,
    safe_area: EdgeInsets,
}

/// UIKit's edge set for the edges the exclusion regions reach.
fn rect_edge(edges: crate::window::apple::geometry::screen_edges::DeferredEdges) -> UIRectEdge {
    [
        (edges.top, UIRectEdge::Top),
        (edges.left, UIRectEdge::Left),
        (edges.bottom, UIRectEdge::Bottom),
        (edges.right, UIRectEdge::Right),
    ]
    .into_iter()
    .filter(|(reached, _)| *reached)
    .fold(UIRectEdge::None, |set, (_, edge)| set | edge)
}

struct NativeAttachment {
    window: Retained<UIWindow>,
    controller: Retained<FluiViewController>,
    token: IOSSceneAttachmentId,
    published: bool,
    retiring: bool,
}

struct NativeWindow {
    view: Retained<FluiView>,
    attachment: RefCell<Option<NativeAttachment>>,
}

impl Drop for NativeWindow {
    fn drop(&mut self) {
        self.view.ivars().sampling.set(SamplingAdmission::Retiring);
        self.view.invalidate_text_sizing();
        self.view.invalidate_display_link();
        self.view.removeFromSuperview();
        if let Some(attachment) = self.attachment.get_mut().take() {
            attachment.window.setHidden(true);
            attachment.window.setRootViewController(None);
        }
    }
}

/// One logical iOS window. Its stable view outlives scene attachments and any
/// raw handle borrowed by a retained renderer. Native access/release is main-only.
pub struct IOSWindow {
    native: NativeOwner<NativeWindow>,
    /// The status-bar style its content asks for, reported by the root
    /// controller of each attachment.
    status_bar: Arc<status_bar::StatusBar>,
    delivery: Arc<WindowDelivery>,
    /// Set once the window closes for good, and by its [`Drop`] before the
    /// view is released; shared with its surface targets.
    closed: Arc<AtomicBool>,
    /// Shared with the window's surface targets, which hand out no handle
    /// while the window is detached from a scene.
    lifecycle: Arc<parking_lot::Mutex<IOSLifecycle>>,
    metrics: Arc<parking_lot::Mutex<WindowMetrics>>,
    /// Shared with the view, which samples it on the main thread.
    dark: Arc<AtomicBool>,
    /// The screen the scene is on, kept by the view; read by `display_id`.
    screen: Arc<parking_lot::Mutex<ScreenIdentity>>,
    id: WindowId,
    /// The input regions last projected, and the screen edges they reach.
    input_regions: parking_lot::Mutex<(
        crate::input::regions::ProjectedInputRegions,
        crate::window::apple::geometry::screen_edges::DeferredEdges,
    )>,
    /// What the table's positions made for this window: its actuator, shared
    /// so its Core Haptics engine lives as long as the window, its system
    /// chrome, its text input and its bridge.
    facets: WindowFacets,
    /// The published window state (ADR-0237).
    snapshot: WindowStateCell,
}

impl IOSWindow {
    /// Allocate a stable view; a scene connection supplies its native container.
    fn new(
        mtm: MainThreadMarker,
        id: WindowId,
        signal: std::sync::Weak<crate::owner::signal::OwnerSignal>,
        displays: Arc<dyn HostDisplays>,
        touches: Box<dyn ViewTouches>,
        positions: &WindowPositions<ViewRef>,
        handler: Rc<dyn flui_platform::WindowHandler>,
    ) -> Self {
        let screen = Arc::new(parking_lot::Mutex::new(ScreenIdentity::default()));
        let delivery = Arc::new(WindowDelivery::new(
            handler,
            crate::input::availability::ios(),
        ));
        // What another thread raises for the window waits for an owner turn.
        if let Some(signal) = signal.upgrade() {
            delivery.resume_through_signal(&signal);
        }
        let metrics = Arc::new(parking_lot::Mutex::new(WindowMetrics {
            size: Size::new(0.0, 0.0),
            scale: 1.0,
            safe_area: EdgeInsets::ZERO,
        }));
        let dark = Arc::new(AtomicBool::new(false));
        let view = FluiView::new(
            mtm,
            Arc::clone(&delivery),
            touches,
            Arc::clone(&metrics),
            Arc::clone(&dark),
            signal.clone(),
            ViewDisplay {
                displays,
                screen: Arc::clone(&screen),
                window: id,
            },
        );
        let status_bar = status_bar::StatusBar::new(mtm);
        let this = ViewRef {
            view: Retained::into_super(view.clone()),
            marker: mtm,
            signal,
            chrome: Arc::new(IOSSystemChrome(Arc::clone(&status_bar))),
        };
        let facets = positions.for_window(&this);
        let _ = view.ivars().text.set(Arc::clone(&facets.text));
        Self {
            facets,
            native: NativeOwner::new(
                NativeWindow {
                    view,
                    attachment: RefCell::new(None),
                },
                mtm,
            ),
            status_bar,
            delivery,
            metrics,
            dark,
            screen,
            id,
            input_regions: parking_lot::Mutex::default(),
            snapshot: WindowStateCell::new(WindowState::new(
                WindowGeometry::new(None, Size::new(0.0, 0.0)),
                Size::new(0.0, 0.0),
            )),
            closed: Arc::new(AtomicBool::new(false)),
            lifecycle: Arc::new(parking_lot::Mutex::new(IOSLifecycle {
                execution: WindowExecutionState::Detached,
                focused: false,
                visible: false,
                generation: 0,
                pending: VecDeque::new(),
                dispatching: false,
            })),
        }
    }
}

impl SceneWindow for IOSWindow {
    fn attach(&self, scene: &UIWindowScene, token: IOSSceneAttachmentId, marker: MainThreadMarker) {
        let native = self.native.get(marker);
        self.detach_native(marker);
        native
            .view
            .ivars()
            .sampling
            .set(SamplingAdmission::Constructing);
        let controller = self
            .status_bar
            .attach_controller(marker, rect_edge(self.input_regions.lock().1));
        controller.setView(Some(&native.view));
        let window = UIWindow::initWithWindowScene(marker.alloc(), scene);
        window.setRootViewController(Some(&controller));
        *native.attachment.borrow_mut() = Some(NativeAttachment {
            window: window.clone(),
            controller,
            token,
            published: false,
            retiring: false,
        });
        native.view.ivars().sampling.set(SamplingAdmission::Active {
            token,
            window: std::ptr::from_ref::<UIWindow>(&window) as usize,
        });
        window.layoutIfNeeded();
        native.view.sample_metrics();
        self.lifecycle.lock().execution = WindowExecutionState::Suspended;
        native.view.start_display_link(&scene.screen());
        native.view.set_display_link_paused(true);
    }

    fn publish(&self, token: IOSSceneAttachmentId, marker: MainThreadMarker) {
        let window = self
            .native
            .get(marker)
            .attachment
            .borrow_mut()
            .as_mut()
            .filter(|attachment| {
                attachment.token == token && !attachment.published && !attachment.retiring
            })
            .map(|attachment| {
                attachment.published = true;
                attachment.window.clone()
            });
        if let Some(window) = window {
            window.makeKeyAndVisible();
            window.layoutIfNeeded();
            self.native.get(marker).view.sample_metrics();
            if self.attachment_matches(token, marker)
                && self.execution_now() == WindowExecutionState::Running
            {
                self.set_frame_tick_paused(false);
            }
        }
    }

    fn resource_generation(&self) -> u64 {
        self.lifecycle.lock().generation
    }

    fn attachment_matches(&self, token: IOSSceneAttachmentId, marker: MainThreadMarker) -> bool {
        !self.closed.load(Ordering::SeqCst)
            && self
                .native
                .get(marker)
                .attachment
                .borrow()
                .as_ref()
                .is_some_and(|attachment| attachment.token == token)
    }

    /// Pause or resume the per-frame tick (delegates to the content view's
    /// `CADisplayLink`). Driven by the app's background/foreground edges.
    fn set_frame_tick_paused(&self, paused: bool) {
        if let Some(marker) = MainThreadMarker::new() {
            let native = self.native.get(marker);
            let published = native
                .attachment
                .borrow()
                .as_ref()
                .is_some_and(|attachment| attachment.published && !attachment.retiring);
            native.view.set_display_link_paused(paused || !published);
        }
    }

    /// Serialize native observations independently of the input/frame FIFO.
    fn observe_lifecycle(&self, observation: LifecycleObservation) {
        use LifecycleObservation::{Background, Detached, Foreground};
        if self.closed.load(Ordering::SeqCst) {
            return;
        }
        // A callback may pump a nested UIKit run loop before our queue drains.
        // Suspend the physical link immediately, even for a queued observation.
        if matches!(observation, Background | Detached) {
            self.set_frame_tick_paused(true);
        }
        {
            let mut state = self.lifecycle.lock();
            if matches!(observation, Background | Foreground | Detached) {
                state.generation = state.generation.wrapping_add(1);
            }
            let generation = state.generation;
            state.pending.push_back((observation, generation));
            if state.dispatching {
                return;
            }
            state.dispatching = true;
        }
        struct Drain<'a>(&'a parking_lot::Mutex<IOSLifecycle>);
        impl Drop for Drain<'_> {
            fn drop(&mut self) {
                self.0.lock().dispatching = false;
            }
        }
        let _drain = Drain(&self.lifecycle);
        loop {
            let next = self.lifecycle.lock().pending.pop_front();
            let Some((observation, generation)) = next else {
                break;
            };
            if self.closed.load(Ordering::SeqCst) {
                self.lifecycle.lock().pending.clear();
                break;
            }
            self.apply_lifecycle(observation, generation);
        }
    }

    /// Fence the physical link before a queued ownership retirement can run.
    /// A callback may pump UIKit or the remaining lifecycle queue in between.
    fn begin_retirement(&self) {
        let marker = MainThreadMarker::new().expect("BUG: scene retirement runs on main");
        self.native
            .get(marker)
            .view
            .ivars()
            .sampling
            .set(SamplingAdmission::Retiring);
        if let Some(attachment) = self.native.get(marker).attachment.borrow_mut().as_mut() {
            attachment.retiring = true;
        }
        self.native.get(marker).view.invalidate_text_sizing();
        self.set_frame_tick_paused(true);
    }

    fn disconnect(&self, marker: MainThreadMarker) {
        self.observe_lifecycle(LifecycleObservation::Detached);
        self.detach_native(marker);
    }

    fn close_for_good(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        self.begin_retirement();
        // A closed window's snapshot is detached, hidden and unfocused.
        self.delivery.publish_state();
        let failure = self
            .delivery
            .dispatch_surface_immediate(SurfaceChange::Release);
        crate::owner::panic_boundary::contain_owner_callback(|| self.delivery.dispatch_close());
        crate::owner::panic_boundary::contain_owner_callback(|| self.delivery.clear());
        self.facets.text.detach();
        self.facets.accessibility.detach();
        if let Some(marker) = MainThreadMarker::new() {
            self.detach_native(marker);
        }
        // The release's panic is the first failure; the close finished first.
        if let Some(failure) = failure {
            failure.resume();
        }
    }
}

impl IOSWindow {
    /// What the scene's lifecycle lets the window run now; a closed window is
    /// detached. Read without publishing, as the snapshot reads it.
    fn execution_now(&self) -> WindowExecutionState {
        if self.closed.load(Ordering::SeqCst) {
            WindowExecutionState::Detached
        } else {
            self.lifecycle.lock().execution
        }
    }

    /// The scene's size, visibility, focus, metrics and lifecycle, read again.
    /// A closed scene is hidden, unfocused and detached.
    fn sample_state(&self) -> WindowState {
        let metrics = *self.metrics.lock();
        let closed = self.closed.load(Ordering::SeqCst);
        let (focused, visible) = {
            let lifecycle = self.lifecycle.lock();
            (lifecycle.focused && !closed, lifecycle.visible && !closed)
        };
        let state = WindowState::new(WindowGeometry::new(None, metrics.size), metrics.size)
            .with_visible(visible)
            .with_focused(focused)
            .with_resizable(false)
            .with_decorated(false)
            .with_display(self.screen.lock().id);
        ReportedFacts {
            metrics: flui_foundation::geometry::WindowMetrics::from_logical(
                metrics.size,
                metrics.scale,
            )
            .ok(),
            appearance: if self.dark.load(Ordering::Relaxed) {
                WindowAppearance::Dark
            } else {
                WindowAppearance::Light
            },
            safe_area: metrics.safe_area,
            execution: self.execution_now(),
            hovered: self.delivery.hovered(),
            // A scene has no activation apart from its focus.
            active: focused,
            capabilities: scene_capabilities(),
        }
        .onto(state)
    }

    /// Read the scene again for its state publisher, announcing nothing.
    fn read_state(&self) -> crate::owner::window_delivery::Publication {
        self.snapshot.invalidate();
        self.snapshot.state_with(|| self.sample_state()).outcome()
    }

    fn detach_native(&self, marker: MainThreadMarker) {
        let native = self.native.get(marker);
        native
            .view
            .ivars()
            .sampling
            .set(SamplingAdmission::Retiring);
        native.view.invalidate_text_sizing();
        native.view.invalidate_display_link();
        native.view.removeFromSuperview();
        let retired = native.attachment.borrow_mut().take();
        if let Some(retired) = retired {
            retired.window.setHidden(true);
            retired.window.setRootViewController(None);
            drop(retired.controller);
        }
        self.status_bar.detach_controller(marker);
        native
            .view
            .ivars()
            .sampling
            .set(SamplingAdmission::Detached);
        native.view.detach_screen();
    }
    fn apply_lifecycle(&self, observation: LifecycleObservation, generation: u64) {
        use LifecycleObservation::{Active, Background, Detached, Foreground, Inactive};
        {
            let mut state = self.lifecycle.lock();
            if matches!(observation, Background | Foreground | Detached)
                && state.generation != generation
            {
                return;
            }
            match observation {
                Active => {
                    if state.execution != WindowExecutionState::Running {
                        return;
                    }
                    state.focused = true;
                }
                Inactive => state.focused = false,
                Background | Detached => {
                    state.execution = if matches!(observation, Detached) {
                        WindowExecutionState::Detached
                    } else {
                        WindowExecutionState::Suspended
                    };
                    state.visible = false;
                    state.focused = false;
                }
                Foreground => {
                    state.visible = true;
                    if state.execution != WindowExecutionState::Running {
                        state.focused = false;
                    }
                }
            }
        }
        // Execution, visibility and focus are published in one revision.
        self.delivery.publish_state();
        let current = || {
            !self.closed.load(Ordering::SeqCst) && self.lifecycle.lock().generation == generation
        };
        match observation {
            Inactive => {}
            Active => {
                if current() {
                    self.set_frame_tick_paused(false);
                    self.request_redraw();
                }
            }
            // The surface goes after the snapshot that stops the frames.
            Background | Detached => {
                if current()
                    && let Some(failure) = self
                        .delivery
                        .dispatch_surface_immediate(SurfaceChange::Release)
                {
                    failure.resume();
                }
            }
            // The surface comes back before the snapshot that runs the frames
            // again.
            Foreground => {
                // A panicking rebuild still lets the frames run again; its
                // panic resumes once they do.
                let failure = self
                    .delivery
                    .dispatch_surface_immediate(SurfaceChange::Rebuild);
                if current() {
                    self.lifecycle.lock().execution = WindowExecutionState::Running;
                    self.delivery.publish_state();
                    if current() {
                        self.set_frame_tick_paused(false);
                        self.request_redraw();
                    }
                }
                if let Some(failure) = failure {
                    failure.resume();
                }
            }
        }
    }
}

impl std::fmt::Debug for IOSWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IOSWindow")
            .field("id", &self.id)
            .field("closed", &self.closed.load(Ordering::SeqCst))
            .finish_non_exhaustive()
    }
}

/// Makes the iOS windows: one per scene session, which a scene connection
/// attaches, each with what the table's window-level positions make for it.
/// The loop keeps the windows it made.
#[derive(Debug)]
pub struct IOSWindows {
    positions: WindowPositions<ViewRef>,
}

impl IOSWindows {
    /// The windows of a platform whose table's window-level positions are
    /// `positions`.
    pub(crate) fn new(positions: WindowPositions<ViewRef>) -> Self {
        Self { positions }
    }
}

impl SceneWindows for IOSWindows {
    fn create(
        &self,
        marker: MainThreadMarker,
        id: WindowId,
        signal: std::sync::Weak<OwnerSignal>,
        displays: Arc<dyn HostDisplays>,
        touches: Box<dyn ViewTouches>,
        handler: Rc<dyn flui_platform::WindowHandler>,
    ) -> Rc<dyn SceneWindow> {
        let window = Rc::new(IOSWindow::new(
            marker,
            id,
            signal,
            displays,
            touches,
            &self.positions,
            handler,
        ));
        window
            .delivery
            .publish_through_owner(&window, IOSWindow::read_state);
        // The loop holds no borrow while it makes the window.
        window.delivery.deliver_capability_snapshot();
        window
    }
}

impl host::Window<IOSLoop> for IOSWindow {
    type Shared = IOSWindows;
    type Ref = ViewRef;

    fn open(
        _: &IOSLoop,
        _: &Arc<IOSWindows>,
        _options: WindowOptions,
        _handler: Rc<dyn flui_platform::WindowHandler>,
    ) -> Result<Rc<dyn HostWindow>, OpenWindowError> {
        Err(OpenWindowError::Unavailable { message: "UIKit scene connection supplies the window through on_scene_event; arbitrary open_window is unsupported".into() })
    }

    fn active(owner: &IOSLoop, _: &IOSWindows) -> Option<WindowId> {
        MainThreadMarker::new().and_then(|_| owner.active().map(|window| window.id()))
    }

    fn capabilities(_: &IOSLoop, _: &IOSWindows) -> WindowCapabilities {
        scene_capabilities()
    }

    fn frame_inset(_: &IOSLoop, _: &IOSWindows, _: f64) -> Size<f64> {
        // A UIKit window has no non-client area.
        Size::new(0.0, 0.0)
    }
}

/// What a scene's window honours: `UIWindow` has no title, frame, position or
/// commands, and observes nothing; the app may only ask the system to destroy
/// its scene session.
const fn scene_capabilities() -> WindowCapabilities {
    let mut capabilities = WindowCapabilities::NONE;
    capabilities.closes = flui_platform::Availability::Native;
    capabilities.frame_rate_hint = flui_platform::Availability::Native;
    capabilities
}

impl crate::host_window::CoreHostWindow for IOSWindow {
    fn window_delivery(&self) -> &WindowDelivery {
        &self.delivery
    }
}

impl crate::host_window::HostWindow for IOSWindow {
    fn accessibility(&self) -> Option<Arc<dyn flui_semantics::platform::PlatformAccessibility>> {
        Some(Arc::clone(&self.facets.accessibility) as _)
    }

    fn capture_text_sizing(
        &self,
        _owner: crate::host_window::OwnerThreadToken,
    ) -> Result<crate::TextSizingCaptureState, crate::TextSizingCaptureError> {
        let marker = MainThreadMarker::new().ok_or(crate::TextSizingCaptureError::WrongThread)?;
        if self.closed.load(Ordering::Acquire) {
            return Err(crate::TextSizingCaptureError::Unavailable);
        }
        self.native.get(marker).view.capture_text_sizing()
    }

    /// What the table's text position answers: no store host on UIKit yet.
    fn text_store_host(
        &self,
        _owner: crate::host_window::OwnerThreadToken,
    ) -> Option<std::rc::Rc<dyn flui_platform::text_store::TextStoreHost>> {
        self.facets
            .text
            .window_text_input()
            .and_then(crate::text::WindowTextInput::store_host)
    }
}

impl PlatformWindow for IOSWindow {
    fn id(&self) -> WindowId {
        self.id
    }

    /// Ask for a frame.
    ///
    /// A demand signal only — it neither dispatches a frame nor marks the
    /// view dirty. Both were wrong, and the animated demo proved it on a real
    /// simulator:
    ///
    /// - **No `dispatch_request_frame()`.** Dispatching here runs the frame
    ///   synchronously on the caller. A static tree runs one frame and
    ///   returns, which is why the Material and ColoredBox demos looked fine;
    ///   an *animated* tree re-arms from inside that frame (its ticker wakes
    ///   through `request_redraw` again), so the `WindowDelivery` drain
    ///   never empties and `didFinishLaunching` never returns to UIKit. iOS's
    ///   scene-create watchdog then kills the app at ~19.6 s
    ///   (`0x8BADF00D`) with a blank window. The `CADisplayLink` this view
    ///   installs is the frame source (the iOS counterpart of Android's poll
    ///   loop and macOS's display pass) and delivers the frame on the next
    ///   refresh, off the caller's stack.
    /// - **No `setNeedsDisplay()`.** That asks UIKit to repaint the `UIView`'s
    ///   *own* layer, which is opaque and empty, so UIKit draws white over the
    ///   `CAMetalLayer` sublayer the renderer presents into — the frames
    ///   arrived and the screen stayed white. Metal presents a drawable to the
    ///   layer directly; the view's display machinery is not part of this
    ///   path.
    ///
    /// The `needs_redraw` flag the caller set before reaching here is what the
    /// next tick's `wake_action` reads, so the request is not lost — it is
    /// deferred to the display's own cadence, which is the pacing contract
    /// this backend has. The window's `IOSRedraw` is the same request from
    /// any thread.
    fn request_redraw(&self) {
        IOSRedraw.request_redraw();
    }

    fn redraw_requester(&self) -> Arc<dyn RedrawRequester> {
        Arc::new(IOSRedraw)
    }

    fn surface_target(&self) -> Arc<dyn SurfaceTarget> {
        let marker = MainThreadMarker::new()
            .expect("BUG: a UIKit window lives on the main thread, where it is used");
        Arc::new(IOSSurface {
            view: Retained::as_ptr(&self.native.get(marker).view)
                .cast_mut()
                .cast(),
            closed: Arc::clone(&self.closed),
            lifecycle: Arc::clone(&self.lifecycle),
            retirement: self.delivery.retirement(),
        })
    }

    fn haptics(&self) -> Option<Arc<dyn flui_platform::PlatformHaptics>> {
        Some(Arc::clone(&self.facets.haptics))
    }

    fn text_input(&self) -> Option<Arc<dyn flui_platform::PlatformTextInput>> {
        self.facets
            .text
            .window_text_input()
            .and_then(crate::text::WindowTextInput::ime)
    }

    /// UIKit defers system gestures per screen edge, not per rectangle: the window
    /// projects the regions, and when the projection changed defers each edge an
    /// exclusion region reaches (`preferredScreenEdgesDeferringSystemGestures`). A region
    /// clear of every edge has no UIKit equivalent and defers nothing.
    ///
    /// Taken from any thread, as the capability says: off the main thread the
    /// edges reach the root controller through the main queue
    /// (`StatusBar::defer_edges`). A window not yet in a scene keeps them for
    /// the controller it attaches. Only a closed window refuses, with `NoWindow`.
    fn set_input_regions(
        &self,
        regions: &[flui_platform::input_regions::InputRegion],
    ) -> Result<(), flui_platform::Unsupported> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(flui_platform::Unsupported::new(
                flui_platform::UnsupportedReason::NoWindow,
            ));
        }
        let (size, scale) = {
            let metrics = *self.metrics.lock();
            let size = Size::new(
                (metrics.size.width * metrics.scale).round() as i32,
                (metrics.size.height * metrics.scale).round() as i32,
            );
            (size, metrics.scale)
        };
        let edges = {
            let mut state = self.input_regions.lock();
            let (projection, edges) = &mut *state;
            let Some(sent) = projection.update(regions, scale, None) else {
                return Ok(());
            };
            *edges = crate::window::apple::geometry::screen_edges::DeferredEdges::of(sent, size);
            *edges
        };
        self.status_bar.defer_edges(rect_edge(edges));
        Ok(())
    }

    /// Each window's `CADisplayLink` asks for the rate the hint names
    /// (`preferredFrameRateRange` from iOS 15, `preferredFramesPerSecond`
    /// before). A closed window answers `NoWindow`. The owner lane is the main
    /// thread: a call from another thread is a caller's mistake, answered
    /// `Unavailable` while the capability stays `Native`, which the owner's
    /// next hint makes good.
    fn set_frame_rate_hint(
        &self,
        hint: flui_platform::FrameRateHint,
    ) -> Result<(), flui_platform::Unsupported> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(flui_platform::Unsupported::new(
                flui_platform::UnsupportedReason::NoWindow,
            ));
        }
        let Some(marker) = MainThreadMarker::new() else {
            return Err(flui_platform::Unsupported::new(
                flui_platform::UnsupportedReason::Unavailable,
            ));
        };
        self.native.get(marker).view.take_frame_rate_hint(hint);
        Ok(())
    }

    fn system_chrome(&self) -> Option<Arc<dyn flui_platform::PlatformSystemChrome>> {
        Some(Arc::clone(&self.facets.chrome))
    }

    /// UIKit starts no OS drag session from this window yet; its input
    /// capabilities say so.
    fn drag_source(&self) -> Option<Arc<dyn flui_platform::drag::PlatformDragSource>> {
        None
    }

    fn set_cursor(&self, _cursor: CursorIcon) -> Result<(), CursorError> {
        // iOS has no pointer cursor on a touch device.
        Err(
            flui_platform::Unsupported::new(flui_platform::UnsupportedReason::NotOnThisPlatform)
                .into(),
        )
    }

    fn apply(&self, command: AdmittedCommand) -> Result<(), ApplyError> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(ApplyError::Closed);
        }
        // The system owns the surface: every command is refused, except the
        // ones every window takes. A scene cannot bring itself to the
        // foreground: the user and the system decide which app is shown.
        let command = command.against(&self.snapshot.published_capabilities())?;
        if command == WindowCommand::Close {
            crate::owner::apple::ios::request_close(self.id);
        }
        Ok(())
    }

    /// The scene's size, visibility and focus. The metrics and lifecycle live
    /// apart from this cell, so each call reads them again. `WindowHandler::state_changed`
    /// is raised where they change: when the view's size or scale does, and on
    /// each lifecycle step. `UIWindow` has no title, frame, position or
    /// commands, so a scene's window honours none and observes nothing.
    fn state(&self) -> WindowState {
        self.snapshot.invalidate();
        let published = self.snapshot.state_with(|| self.sample_state());
        if published.changed {
            self.delivery.dispatch_state_changed();
        }
        published.state
    }
}

impl Drop for IOSWindow {
    /// The surface targets outlive the window: they hand out nothing from
    /// here on, before the view the window holds is released.
    fn drop(&mut self) {
        self.closed.store(true, Ordering::SeqCst);
    }
}

/// A UIKit window's redraw requester (ADR-0254 §5).
///
/// A demand signal only, like `IOSWindow`'s `request_redraw`: the
/// `needs_redraw` flag its caller set is what the view's `CADisplayLink` reads
/// on its next tick (ADR-0071 §5), so a request from any thread records
/// nothing more here, holds no window and reaches none.
#[derive(Debug, Clone, Copy)]
struct IOSRedraw;

impl RedrawRequester for IOSRedraw {
    fn request_redraw(&self) {
        tracing::trace!("request_redraw: demand recorded; the CADisplayLink delivers the frame");
    }
}

/// A UIKit window's surface target (ADR-0254 §5): the window's content
/// `UIView`, as a `UiKitWindowHandle`, while the window is attached to a
/// scene and has not closed.
struct IOSSurface {
    /// The window's `FluiView`. Never messaged, retained or dereferenced
    /// here.
    view: *mut std::ffi::c_void,
    /// The window's closed flag.
    closed: Arc<AtomicBool>,
    /// The window's lifecycle: a detached window hands out no handle.
    lifecycle: Arc<parking_lot::Mutex<IOSLifecycle>>,
    /// Whether the window's native retirement has started.
    retirement: Retirement,
}

// SAFETY: the target holds the view's address, the window's `closed` flag, its
// lifecycle behind a mutex and its `Retirement`. It never messages, retains or
// dereferences the view: the address goes into a `UiKitWindowHandle` only
// while `closed` and the retirement are unset and the window is attached. The
// window keeps the view for its whole life and releases it on the main thread
// when it drops, after its `Drop` has set `closed`; and a Metal layer is made
// from the handle on the main thread alone (raw-window-metal's
// `Layer::from_ui_view` checks it), where the handle query and its use run with
// no release of the view between them.
unsafe impl Send for IOSSurface {}
// SAFETY: see `Send` above: a shared reference reads the flags, locks the
// lifecycle and copies the address, and nothing else.
unsafe impl Sync for IOSSurface {}

impl IOSSurface {
    /// Whether the window has no surface to hand out: closed, retired, or
    /// detached from its scene.
    fn is_unavailable(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
            || self.retirement.is_retired()
            || self.lifecycle.lock().execution == WindowExecutionState::Detached
    }
}

impl raw_window_handle::HasWindowHandle for IOSSurface {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        if self.is_unavailable() {
            return Err(raw_window_handle::HandleError::Unavailable);
        }
        let view =
            std::ptr::NonNull::new(self.view).ok_or(raw_window_handle::HandleError::Unavailable)?;
        let handle = raw_window_handle::UiKitWindowHandle::new(view);
        // SAFETY: the window has not closed, so it still holds the view; the
        // handle stops being handed out once it closes.
        Ok(unsafe {
            raw_window_handle::WindowHandle::borrow_raw(raw_window_handle::RawWindowHandle::UiKit(
                handle,
            ))
        })
    }
}

impl raw_window_handle::HasDisplayHandle for IOSSurface {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        if self.is_unavailable() {
            return Err(raw_window_handle::HandleError::Unavailable);
        }
        let handle = raw_window_handle::UiKitDisplayHandle::new();
        // SAFETY: the UIKit display handle carries no pointer.
        Ok(unsafe {
            raw_window_handle::DisplayHandle::borrow_raw(
                raw_window_handle::RawDisplayHandle::UiKit(handle),
            )
        })
    }
}

impl SurfaceTarget for IOSSurface {
    /// UIKit has no frame callback to arm before a present: the view's
    /// `CADisplayLink` paces the frames. It returns at once.
    fn pre_present_notify(&self) {}
}

/// Owner-thread session notification. A connection carries explicit completion
/// authority for renderer and tree installation; returning from the handler does
/// not publish the attachment. Reconnect keeps the previously installed logical UI.
pub enum IOSSceneEvent {
    /// A native attachment is ready for installation, with display ticks paused.
    Connected {
        /// Persistent session identity.
        session: IOSSceneSessionId,
        /// Origin of this native connection.
        attachment: IOSSceneAttachmentId,
        /// Stable logical window, also retained across reconnects.
        window: Rc<dyn HostWindow>,
        /// Whether this session already installed its application tree.
        reconnect: bool,
        /// Complete only after logical state and native drivers are installed.
        /// Until then the attachment remains unpublished; dropping this aborts it.
        installation: WindowInstallation,
    },
    /// Reversible loss of a native attachment; retain logical application state.
    Disconnected {
        /// Persistent session identity.
        session: IOSSceneSessionId,
        /// The attachment being retired.
        attachment: IOSSceneAttachmentId,
    },
    /// Failed provisional installation was disposed; the same session may retry.
    InstallationAborted {
        /// Persistent session identity, still eligible for a later connection.
        session: IOSSceneSessionId,
        /// Failed attachment, whose native resources have been retired.
        attachment: IOSSceneAttachmentId,
    },
    /// Terminal session disposal. This persistent identity cannot reconnect.
    Discarded {
        /// Persistent session identity.
        session: IOSSceneSessionId,
    },
}
impl std::fmt::Debug for IOSSceneEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Connected {
                session,
                attachment,
                reconnect,
                ..
            } => f
                .debug_struct("Connected")
                .field("session", session)
                .field("attachment", attachment)
                .field("reconnect", reconnect)
                .finish_non_exhaustive(),
            Self::Disconnected {
                session,
                attachment,
            } => f
                .debug_struct("Disconnected")
                .field("session", session)
                .field("attachment", attachment)
                .finish(),
            Self::InstallationAborted {
                session,
                attachment,
            } => f
                .debug_struct("InstallationAborted")
                .field("session", session)
                .field("attachment", attachment)
                .finish(),
            Self::Discarded { session } => f
                .debug_struct("Discarded")
                .field("session", session)
                .finish(),
        }
    }
}

/// The application's handler of scene sessions.
pub(crate) type SceneHandler = Box<dyn FnMut(IOSSceneEvent) -> Result<(), BootstrapError>>;

/// What makes the handler of a session's window, before the window exists
/// (ADR-0254 §4).
pub(crate) type SceneWindowHandlers =
    Box<dyn FnMut(&IOSSceneSessionId) -> Rc<dyn flui_platform::WindowHandler>>;

/// The application's handlers as the loop calls them: each call becomes the
/// [`IOSSceneEvent`] it describes, and a connection returns the loop the
/// pending side of the installation it hands the application.
struct EventConsumer {
    events: SceneHandler,
    windows: SceneWindowHandlers,
}

impl SceneConsumer for EventConsumer {
    fn window_handler(
        &mut self,
        session: &IOSSceneSessionId,
    ) -> Rc<dyn flui_platform::WindowHandler> {
        (self.windows)(session)
    }

    fn connected(
        &mut self,
        session: IOSSceneSessionId,
        attachment: IOSSceneAttachmentId,
        window: Rc<dyn HostWindow>,
        reconnect: bool,
        wake: PlatformProxy,
    ) -> Result<Box<dyn SceneInstallation>, BootstrapError> {
        let (installation, watched) = WindowInstallation::channel(wake);
        (self.events)(IOSSceneEvent::Connected {
            session,
            attachment,
            window,
            reconnect,
            installation,
        })?;
        Ok(Box::new(watched))
    }

    fn disconnected(
        &mut self,
        session: IOSSceneSessionId,
        attachment: IOSSceneAttachmentId,
    ) -> Result<(), BootstrapError> {
        (self.events)(IOSSceneEvent::Disconnected {
            session,
            attachment,
        })
    }

    fn installation_aborted(
        &mut self,
        session: IOSSceneSessionId,
        attachment: IOSSceneAttachmentId,
    ) -> Result<(), BootstrapError> {
        (self.events)(IOSSceneEvent::InstallationAborted {
            session,
            attachment,
        })
    }

    fn discarded(&mut self, session: IOSSceneSessionId) -> Result<(), BootstrapError> {
        (self.events)(IOSSceneEvent::Discarded { session })
    }
}

impl SceneInstallation for PendingWindowInstallation {
    fn installed(&self) -> Option<bool> {
        self.outcome().map(|outcome| outcome.is_ok())
    }
}

/// What the iOS platform offers beyond [`Platform`](crate::Platform).
impl<H: Host<Loop = IOSLoop>> HostPlatform<H> {
    /// Register the session consumer before entering UIKit. Native connection
    /// invokes it on the owner thread, outside storage borrows. An initial
    /// installation failure retires that session rather than publishing it.
    /// The accepted consumer is invoked and retired on main and may capture
    /// owner-local state such as `Rc`; no `Send` bound is required.
    ///
    /// `window_handler` makes the handler of a session's window, once per
    /// window, before UIKit's scene gets it: the window delivers to it from
    /// its first event on and keeps it across reconnections (ADR-0254 §4).
    ///
    /// # Errors
    /// [`PlatformError::Init`] off the main thread, or once the owner stopped.
    pub fn on_scene_event(
        &self,
        callback: SceneHandler,
        window_handler: SceneWindowHandlers,
    ) -> Result<(), PlatformError> {
        self.owner().install_scene_consumer(Box::new(EventConsumer {
            events: callback,
            windows: window_handler,
        }))
    }
}

pub(crate) mod status_bar {
    //! The status-bar style a window's content asks for.
    //!
    //! UIKit asks the window's root view controller for its
    //! `preferredStatusBarStyle` (view-controller-based appearance, the
    //! Info.plist default). The root controller is a [`FluiViewController`] that
    //! answers from a shared atomic; [`IOSSystemChrome`] stores a new style from
    //! any thread and asks UIKit, on the main thread, to read it again.

    use std::cell::{Cell, RefCell};
    use std::sync::{
        Arc,
        atomic::{AtomicU8, AtomicUsize, Ordering},
    };

    use dispatch2::DispatchQueue;
    use objc2::rc::Retained;
    use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
    use objc2_foundation::NSObjectProtocol;
    use objc2_ui_kit::{UIRectEdge, UIStatusBarStyle, UIViewController};

    use flui_platform::{PlatformSystemChrome, SystemUiOverlayStyle};

    use crate::owner::panic_boundary::{AfterPanic, os_entry};

    use crate::owner::apple::ios::NativeOwner;

    /// No style delivered yet: UIKit's own default stands.
    const UNSET: u8 = 0;
    const LIGHT_ICONS: u8 = 1;
    const DARK_ICONS: u8 = 2;

    fn encode(style: SystemUiOverlayStyle) -> u8 {
        match style {
            SystemUiOverlayStyle::Dark => DARK_ICONS,
            _ => LIGHT_ICONS,
        }
    }

    fn status_bar_style(code: u8) -> UIStatusBarStyle {
        match code {
            LIGHT_ICONS => UIStatusBarStyle::LightContent,
            // `DarkContent` arrived in iOS 13, the floor.
            DARK_ICONS => UIStatusBarStyle::DarkContent,
            // The unset state: what a plain `UIViewController` reports, so an
            // unannotated window keeps the system's style.
            _ => UIStatusBarStyle::Default,
        }
    }

    /// The root view controller's ivars: the status-bar style it reports and the
    /// screen edges whose system gestures the application's exclusion regions
    /// reach.
    pub struct FluiViewControllerIvars {
        style: Arc<AtomicU8>,
        deferred_edges: Cell<UIRectEdge>,
    }

    define_class!(
        // SAFETY:
        // - `UIViewController` has no subclassing requirements beyond those the
        //   superclass already satisfies.
        // - The class does not implement `Drop`; its ivars are an `Arc` and a
        //   `Cell` of a plain integer, so the superclass's `dealloc` is the whole
        //   story.
        //
        /// The root view controller, reporting the status-bar style the content
        /// asked for and answering `preferredScreenEdgesDeferringSystemGestures`
        /// from its ivar. One class serves both: UIKit asks the root controller
        /// for each, and an Objective-C class name registers once per process.
        #[unsafe(super(UIViewController))]
        #[thread_kind = MainThreadOnly]
        #[name = "FluiViewController"]
        #[ivars = FluiViewControllerIvars]
        pub(super) struct FluiViewController;

        // A panic answers what a plain `UIViewController` would: the default
        // style, and no deferred edges.
        impl FluiViewController {
            #[unsafe(method(preferredStatusBarStyle))]
            fn preferred_status_bar_style(&self) -> UIStatusBarStyle {
                os_entry(
                    "preferredStatusBarStyle",
                    (), AfterPanic::Answer(|()| UIStatusBarStyle::Default),
                    || status_bar_style(self.ivars().style.load(Ordering::Acquire)),
                )
            }

            #[unsafe(method(preferredScreenEdgesDeferringSystemGestures))]
            fn preferred_screen_edges_deferring_system_gestures(&self) -> UIRectEdge {
                os_entry(
                    "preferredScreenEdgesDeferringSystemGestures",
                    (), AfterPanic::Answer(|()| UIRectEdge::empty()),
                    || self.ivars().deferred_edges.get(),
                )
            }
        }

        unsafe impl NSObjectProtocol for FluiViewController {}
    );

    impl FluiViewController {
        fn new(
            mtm: MainThreadMarker,
            style: Arc<AtomicU8>,
            deferred_edges: UIRectEdge,
        ) -> Retained<Self> {
            let this = mtm.alloc::<Self>().set_ivars(FluiViewControllerIvars {
                style,
                deferred_edges: Cell::new(deferred_edges),
            });
            // SAFETY: `init` reaches UIViewController's designated initializer
            // with no nib; the receiver is the allocation whose ivars were set
            // above, which is the `define_class!` idiom.
            unsafe { msg_send![super(this), init] }
        }

        /// Defers `edges` from now on, and asks UIKit to read them again.
        pub(super) fn defer_edges(&self, edges: UIRectEdge) {
            if self.ivars().deferred_edges.replace(edges) != edges {
                self.setNeedsUpdateOfScreenEdgesDeferringSystemGestures();
            }
        }
    }

    /// A window's requested status-bar style, the screen edges it defers, and
    /// the controller reporting both.
    pub(super) struct StatusBar {
        style: Arc<AtomicU8>,
        /// The `UIRectEdge` bits the window last asked to defer; the
        /// controller reads the newest whenever it is refreshed.
        deferred_edges: AtomicUsize,
        controller: NativeOwner<RefCell<Option<Retained<FluiViewController>>>>,
    }

    impl StatusBar {
        pub(super) fn new(mtm: MainThreadMarker) -> Arc<Self> {
            Arc::new(Self {
                style: Arc::new(AtomicU8::new(UNSET)),
                deferred_edges: AtomicUsize::new(UIRectEdge::empty().bits()),
                controller: NativeOwner::new(RefCell::new(None), mtm),
            })
        }

        /// Defer `edges` from now on. On the main thread the controller hears
        /// it at once; from any other thread the main queue refreshes it, and
        /// reads the newest edges then, so a refresh queued earlier never
        /// brings back edges a later call replaced.
        pub(super) fn defer_edges(self: &Arc<Self>, edges: UIRectEdge) {
            self.deferred_edges.store(edges.bits(), Ordering::Release);
            if let Some(mtm) = MainThreadMarker::new() {
                self.refresh_edges(mtm);
                return;
            }
            let status_bar = Arc::clone(self);
            DispatchQueue::main().exec_async(move || {
                os_entry(
                    "the deferred screen edges' refresh",
                    (),
                    AfterPanic::Answer(|()| ()),
                    move || {
                        let mtm = MainThreadMarker::new()
                            .expect("BUG: the main queue runs on the main thread");
                        status_bar.refresh_edges(mtm);
                    },
                );
            });
        }

        fn refresh_edges(&self, mtm: MainThreadMarker) {
            let edges = UIRectEdge::from_bits_retain(self.deferred_edges.load(Ordering::Acquire));
            let controller = self.controller.get(mtm).borrow().clone();
            if let Some(controller) = controller {
                controller.defer_edges(edges);
            }
        }

        /// A root view controller reporting this window's style and deferring
        /// `deferred_edges`; it replaces the previous attachment's controller as
        /// the one to refresh.
        pub(super) fn attach_controller(
            &self,
            mtm: MainThreadMarker,
            deferred_edges: UIRectEdge,
        ) -> Retained<FluiViewController> {
            let controller = FluiViewController::new(mtm, Arc::clone(&self.style), deferred_edges);
            *self.controller.get(mtm).borrow_mut() = Some(controller.clone());
            controller
        }

        /// Forget the detached controller.
        pub(super) fn detach_controller(&self, mtm: MainThreadMarker) {
            let retired = self.controller.get(mtm).borrow_mut().take();
            drop(retired);
        }

        fn refresh(&self, mtm: MainThreadMarker) {
            let controller = self.controller.get(mtm).borrow().clone();
            if let Some(controller) = controller {
                controller.setNeedsStatusBarAppearanceUpdate();
            }
        }
    }

    /// `PlatformSystemChrome` for one iOS window.
    pub struct IOSSystemChrome(pub(super) Arc<StatusBar>);

    impl std::fmt::Debug for IOSSystemChrome {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("IOSSystemChrome").finish_non_exhaustive()
        }
    }

    /// UIKit asks a window's root controller for its status bar, so the
    /// chrome is the window's own: the position hands out what the window
    /// keeps.
    impl crate::host::Chrome<crate::owner::apple::ios::ViewRef> for IOSSystemChrome {
        fn for_window(window: &crate::owner::apple::ios::ViewRef) -> Arc<dyn PlatformSystemChrome> {
            Arc::clone(&window.chrome)
        }
    }

    impl PlatformSystemChrome for IOSSystemChrome {
        fn set_overlay_style(&self, style: SystemUiOverlayStyle) {
            self.0.style.store(encode(style), Ordering::Release);
            if let Some(mtm) = MainThreadMarker::new() {
                self.0.refresh(mtm);
                return;
            }
            let status_bar = Arc::clone(&self.0);
            DispatchQueue::main().exec_async(move || {
                os_entry(
                    "the status bar's refresh",
                    (),
                    AfterPanic::Answer(|()| ()),
                    move || {
                        let mtm = MainThreadMarker::new()
                            .expect("BUG: the main queue runs on the main thread");
                        status_bar.refresh(mtm);
                    },
                );
            });
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }
}
