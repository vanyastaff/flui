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
//! `window_handle()` hands wgpu the content `UIView` through
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

use crate::display::apple::link_rate::LinkPeriod;
use crate::display::apple::screen::change_screen;
use crate::host;
use crate::host::Host;
use crate::host_window::HostWindow;
use crate::input::apple::ios::TouchInputState;
use crate::owner::OpenWindowError;
use crate::owner::apple::IOSLoop;
use crate::owner::apple::ios::{HostDisplays, NativeOwner, ScreenIdentity};
use crate::owner::apple::ios::{
    IOSSceneAttachmentId, IOSSceneSessionId, LifecycleObservation, SceneConsumer,
    SceneInstallation, SceneWindow, SceneWindows,
};
use crate::owner::call_point::install;
use crate::owner::handlers::WindowCallbacks;
use crate::owner::panic_boundary::{AfterPanic, os_entry};
use crate::owner::signal::OwnerSignal;
use crate::platform::HostPlatform;
use crate::window::installation::{PendingWindowInstallation, WindowInstallation};
use crate::window::state::WindowStateCell;
use crate::{BootstrapError, PlatformError, PlatformProxy};
use flui_foundation::geometry::{EdgeInsets, Size};
use flui_platform::{
    CursorError, DisplayId, PlatformWindow, WindowEvent, WindowExecutionState, WindowId,
    WindowOptions,
};
use flui_platform::{
    WindowAppearance, WindowCapabilities, WindowCommand, WindowCommandError, WindowGeometry,
    WindowState,
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
    callbacks: Arc<WindowCallbacks>,
    text_sizing: RefCell<Option<crate::CapturedTextSizing>>,
    text_sizing_authority: RefCell<Arc<AtomicBool>>,
    owner_signal: std::sync::Weak<crate::owner::signal::OwnerSignal>,
    input: RefCell<TouchInputState>,
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
    /// The cadence the link's ticks show, and the period it last published.
    link_period: RefCell<LinkPeriod>,
    /// The platform's display source, which holds the period the link reports.
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
                    self.callbacks().dispatch_input(event);
                }
            });
        }

        #[unsafe(method(touchesBegan:withEvent:))]
        fn touches_began(&self, touches: &NSSet<UITouch>, _event: Option<&AnyObject>) {
            os_entry("touchesBegan:withEvent:", (), AfterPanic::Answer(|()| ()), || {
                self.pin_for_native_callback();
                self.dispatch_touches(touches, TouchPhase::Began);
            });
        }

        #[unsafe(method(touchesMoved:withEvent:))]
        fn touches_moved(&self, touches: &NSSet<UITouch>, _event: Option<&AnyObject>) {
            os_entry("touchesMoved:withEvent:", (), AfterPanic::Answer(|()| ()), || {
                self.pin_for_native_callback();
                self.dispatch_touches(touches, TouchPhase::Moved);
            });
        }

        #[unsafe(method(touchesEnded:withEvent:))]
        fn touches_ended(&self, touches: &NSSet<UITouch>, _event: Option<&AnyObject>) {
            os_entry("touchesEnded:withEvent:", (), AfterPanic::Answer(|()| ()), || {
                self.pin_for_native_callback();
                self.dispatch_touches(touches, TouchPhase::Ended);
            });
        }

        #[unsafe(method(touchesCancelled:withEvent:))]
        fn touches_cancelled(&self, touches: &NSSet<UITouch>, _event: Option<&AnyObject>) {
            os_entry("touchesCancelled:withEvent:", (), AfterPanic::Answer(|()| ()), || {
                self.pin_for_native_callback();
                self.dispatch_touches(touches, TouchPhase::Cancelled);
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
                    self.callbacks().dispatch_request_frame();
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

/// Which touch callback produced a batch, mapped to the pointer phase the
/// owned pointer vocabulary uses.
#[derive(Clone, Copy)]
enum TouchPhase {
    Began,
    Moved,
    Ended,
    Cancelled,
}

impl FluiView {
    /// Build the content view for one window, sized to the screen's bounds.
    fn new(
        mtm: MainThreadMarker,
        callbacks: Arc<WindowCallbacks>,
        metrics: Arc<parking_lot::Mutex<WindowMetrics>>,
        dark: Arc<AtomicBool>,
        owner_signal: std::sync::Weak<crate::owner::signal::OwnerSignal>,
        display: ViewDisplay,
    ) -> Retained<Self> {
        let bounds = objc2_ui_kit::UIScreen::mainScreen(mtm).bounds();
        let this = mtm.alloc::<Self>();
        let this = this.set_ivars(FluiViewIvars {
            callbacks,
            text_sizing: RefCell::new(None),
            text_sizing_authority: RefCell::new(Arc::new(AtomicBool::new(true))),
            owner_signal,
            input: RefCell::new(TouchInputState::default()),
            sampling: std::cell::Cell::new(SamplingAdmission::Detached),
            sampling_active: std::cell::Cell::new(false),
            sampling_pending: std::cell::Cell::new(false),
            display_link: RefCell::new(None),
            link_period: RefCell::new(LinkPeriod::default()),
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
            self.callbacks().dispatch_appearance_changed();
        }
    }

    fn invalidate_text_sizing(&self) {
        let authority = self
            .ivars()
            .text_sizing_authority
            .replace(Arc::new(AtomicBool::new(true)));
        authority.store(false, Ordering::Release);
        let retired = self.ivars().text_sizing.borrow_mut().take();
        drop(retired);
        if let Some(signal) = self.ivars().owner_signal.upgrade() {
            let _ = signal.wake();
        }
    }

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
        let cached = self.ivars().text_sizing.borrow().clone();
        if let Some(cached) = cached.filter(crate::CapturedTextSizing::is_current) {
            return Ok(crate::TextSizingCaptureState::Ready(cached));
        }
        let authority = Arc::clone(&self.ivars().text_sizing_authority.borrow());
        let traits =
            objc2::exception::catch(std::panic::AssertUnwindSafe(|| self.traitCollection()))
                .map_err(|exception| {
                    drop(exception);
                    crate::TextSizingCaptureError::Native {
                        message: "UIKit trait capture failed".into(),
                    }
                })?;
        let attached = self
            .window()
            .is_some_and(|window| std::ptr::from_ref::<UIWindow>(&window) as usize == identity);
        if !authority.load(Ordering::Acquire)
            || self.ivars().sampling.get() != admission
            || !attached
        {
            return Err(crate::TextSizingCaptureError::Unavailable);
        }
        let capture = crate::CapturedTextSizing::new(
            Arc::new(crate::text::apple::ios::Snapshot { traits }),
            authority,
            std::thread::current().id(),
        );
        let retired = self.ivars().text_sizing.replace(Some(capture.clone()));
        drop(retired);
        Ok(crate::TextSizingCaptureState::Ready(capture))
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
        self.ivars().displays.request_frame_rate(&link, screen);
        self.ivars().display_link.replace(Some(link));
    }

    /// Take `screen` as the one this window is on. A change of screen resets
    /// what the link measured, asks the link for the new screen's best rate,
    /// and tells the owner the display may have changed.
    fn adopt_screen(&self, screen: &UIScreen) {
        let identity = self
            .ivars()
            .displays
            .identify(MainThreadMarker::from(self), screen);
        let changed = change_screen(&mut *self.ivars().screen.lock(), identity);
        let Some(previous) = changed else {
            return;
        };
        if let Some(number) = previous.number {
            self.ivars()
                .displays
                .forget_link_period(number, self.ivars().window_id);
        }
        self.ivars().link_period.borrow_mut().forget();
        let link = self.ivars().display_link.borrow().clone();
        if let Some(link) = link {
            self.ivars().displays.request_frame_rate(&link, screen);
        }
        self.ivars().displays.invalidate();
        self.callbacks().dispatch_display_changed();
    }

    /// The scene is gone: this window names no screen until it is attached
    /// again. Forgets the window's link period on the old screen and, if the
    /// identity was set, tells the owner the display changed. No lock or
    /// borrow is held across the callback.
    fn detach_screen(&self) {
        let changed = change_screen(&mut *self.ivars().screen.lock(), ScreenIdentity::default());
        let Some(previous) = changed else {
            return;
        };
        if let Some(number) = previous.number {
            self.ivars()
                .displays
                .forget_link_period(number, self.ivars().window_id);
        }
        self.ivars().link_period.borrow_mut().forget();
        self.ivars().displays.invalidate();
        self.callbacks().dispatch_display_changed();
    }

    /// Read the cadence one tick shows (the time to the frame it targets) and
    /// publish it when it leaves the band around the last published period.
    fn observe_link_cadence(&self, link: &CADisplayLink) {
        let interval = link.targetTimestamp() - link.timestamp();
        let published = self.ivars().link_period.borrow_mut().observe(interval);
        let number = self.ivars().screen.lock().number;
        let (Some(period), Some(number)) = (published, number) else {
            return;
        };
        self.ivars()
            .displays
            .report_link_period(number, self.ivars().window_id, period);
        if let Some(signal) = self.ivars().owner_signal.upgrade() {
            let _ = signal.wake();
        }
        self.callbacks().dispatch_display_changed();
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
            self.ivars().link_period.borrow_mut().forget();
            let number = self.ivars().screen.lock().number;
            if let Some(number) = number {
                self.ivars()
                    .displays
                    .forget_link_period(number, self.ivars().window_id);
            }
        }
    }

    fn callbacks(&self) -> &Arc<WindowCallbacks> {
        &self.ivars().callbacks
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
            let events = self
                .ivars()
                .input
                .borrow_mut()
                .convert(&touch, phase.as_pointer_phase());
            for event in events {
                let result = self.callbacks().dispatch_input(event);
                any |= result.default_prevented;
            }
        }
        if any {
            self.callbacks().dispatch_request_frame();
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
            self.adopt_screen(&scene.screen());
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
        if previous.size != next.size || previous.scale != next.scale {
            self.callbacks()
                .dispatch_metrics_resize(next.size, next.scale);
            // The published state follows the metrics: `state()` reads them
            // again when the owner asks.
            self.callbacks().dispatch_state_changed();
        }
        if self.ivars().sampling.get() != admission {
            // The resize dispatch above retired this view, so this snapshot's
            // safe-area half must not be announced either. Roll the snapshot
            // back before returning: leaving un-announced insets committed as
            // the baseline would make every later sample compare equal and
            // never report them at all, whereas rolled back they are still
            // seen by a later sample — or by the reattach's own
            // `sample_metrics`. The equality guard keeps the rollback to this
            // very snapshot, so a newer one committed by a callback that
            // re-entered sampling during the dispatch above is left alone.
            let mut metrics = self.ivars().metrics.lock();
            if *metrics == next {
                *metrics = previous;
            }
            return;
        }
        if previous.safe_area != next.safe_area {
            self.callbacks().dispatch_safe_area_change(next.safe_area);
        }
    }
}

impl TouchPhase {
    fn as_pointer_phase(self) -> crate::input::apple::ios::TouchPhase {
        match self {
            TouchPhase::Began => crate::input::apple::ios::TouchPhase::Down,
            TouchPhase::Moved => crate::input::apple::ios::TouchPhase::Move,
            TouchPhase::Ended => crate::input::apple::ios::TouchPhase::Up,
            TouchPhase::Cancelled => crate::input::apple::ios::TouchPhase::Cancel,
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
    callbacks: Arc<WindowCallbacks>,
    closed: AtomicBool,
    lifecycle: parking_lot::Mutex<IOSLifecycle>,
    metrics: Arc<parking_lot::Mutex<WindowMetrics>>,
    /// Shared with the view, which samples it on the main thread.
    dark: Arc<AtomicBool>,
    /// The screen the scene is on, kept by the view; read by `display_id`.
    screen: Arc<parking_lot::Mutex<ScreenIdentity>>,
    id: WindowId,
    /// The input regions last projected, and the screen edges they reach.
    input_regions: parking_lot::Mutex<(
        flui_platform::input_regions::ProjectedInputRegions,
        crate::window::apple::geometry::screen_edges::DeferredEdges,
    )>,
    /// The haptic actuator, made on first use and shared, so its Core
    /// Haptics engine lives as long as the window.
    haptics: std::sync::OnceLock<Arc<crate::haptics::apple::IOSHaptics>>,
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
    ) -> Self {
        let screen = Arc::new(parking_lot::Mutex::new(ScreenIdentity::default()));
        let callbacks = Arc::new(WindowCallbacks::new());
        callbacks.set_input_capabilities(crate::input::availability::ios());
        if let Some(owner) = signal.upgrade() {
            callbacks.set_input_wake(crate::owner::signal::OwnerSignal::callbacks_wake(
                &owner, &callbacks,
            ));
        }
        let metrics = Arc::new(parking_lot::Mutex::new(WindowMetrics {
            size: Size::new(0.0, 0.0),
            scale: 1.0,
            safe_area: EdgeInsets::ZERO,
        }));
        let dark = Arc::new(AtomicBool::new(false));
        let view = FluiView::new(
            mtm,
            Arc::clone(&callbacks),
            Arc::clone(&metrics),
            Arc::clone(&dark),
            signal,
            ViewDisplay {
                displays,
                screen: Arc::clone(&screen),
                window: id,
            },
        );
        Self {
            native: NativeOwner::new(
                NativeWindow {
                    view,
                    attachment: RefCell::new(None),
                },
                mtm,
            ),
            status_bar: status_bar::StatusBar::new(mtm),
            callbacks,
            metrics,
            dark,
            screen,
            id,
            input_regions: parking_lot::Mutex::default(),
            haptics: std::sync::OnceLock::new(),
            snapshot: WindowStateCell::new(WindowState::new(
                WindowGeometry::new(None, Size::new(0.0, 0.0)),
                Size::new(0.0, 0.0),
            )),
            closed: AtomicBool::new(false),
            lifecycle: parking_lot::Mutex::new(IOSLifecycle {
                execution: WindowExecutionState::Detached,
                focused: false,
                visible: false,
                generation: 0,
                pending: VecDeque::new(),
                dispatching: false,
            }),
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
                && self.execution_state() == WindowExecutionState::Running
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
        use crate::owner::handlers::LifecycleEvent;
        self.callbacks
            .dispatch_lifecycle_immediate(LifecycleEvent::Execution(
                WindowExecutionState::Detached,
            ));
        self.callbacks
            .dispatch_lifecycle_immediate(LifecycleEvent::Surface(false));
        crate::owner::panic_boundary::contain_owner_callback(|| self.callbacks.dispatch_close());
        crate::owner::panic_boundary::contain_owner_callback(|| self.callbacks.clear());
        if let Some(marker) = MainThreadMarker::new() {
            self.detach_native(marker);
        }
    }
}

impl IOSWindow {
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
        use crate::owner::handlers::LifecycleEvent;
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
        // Visibility and focus are part of the published state.
        self.callbacks.dispatch_state_changed();
        let current = || {
            !self.closed.load(Ordering::SeqCst) && self.lifecycle.lock().generation == generation
        };
        let emit = |event| self.callbacks.dispatch_lifecycle_immediate(event);
        match observation {
            Inactive => emit(LifecycleEvent::Focus(false)),
            Active => {
                emit(LifecycleEvent::Focus(true));
                if current() {
                    self.set_frame_tick_paused(false);
                    self.request_redraw();
                }
            }
            Background | Detached => {
                emit(LifecycleEvent::Execution(self.execution_state()));
                if !current() {
                    return;
                }
                emit(LifecycleEvent::Focus(false));
                if !current() {
                    return;
                }
                emit(LifecycleEvent::Visibility(false));
                if !current() {
                    return;
                }
                emit(LifecycleEvent::Surface(false));
            }
            Foreground => {
                emit(LifecycleEvent::Surface(true));
                if !current() {
                    return;
                }
                let focused = {
                    let mut state = self.lifecycle.lock();
                    state.execution = WindowExecutionState::Running;
                    state.focused
                };
                emit(LifecycleEvent::Focus(focused));
                if !current() {
                    return;
                }
                emit(LifecycleEvent::Visibility(true));
                if !current() {
                    return;
                }
                emit(LifecycleEvent::Execution(WindowExecutionState::Running));
                if current() {
                    self.set_frame_tick_paused(false);
                    self.request_redraw();
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
/// attaches. It holds nothing; the loop keeps the windows it made.
#[derive(Debug, Default)]
pub struct IOSWindows;

impl SceneWindows for IOSWindows {
    fn create(
        &self,
        marker: MainThreadMarker,
        id: WindowId,
        signal: std::sync::Weak<OwnerSignal>,
        displays: Arc<dyn HostDisplays>,
    ) -> Arc<dyn SceneWindow> {
        Arc::new(IOSWindow::new(marker, id, signal, displays))
    }
}

impl host::Window<IOSLoop> for IOSWindow {
    type Shared = IOSWindows;

    fn open(
        _: &IOSLoop,
        _: &Arc<IOSWindows>,
        _options: WindowOptions,
    ) -> Result<Arc<dyn HostWindow>, OpenWindowError> {
        Err(OpenWindowError::Unavailable { message: "UIKit scene connection supplies the window through on_scene_event; arbitrary open_window is unsupported".into() })
    }

    fn active(owner: &IOSLoop, _: &IOSWindows) -> Option<WindowId> {
        MainThreadMarker::new().and_then(|_| owner.active().map(|window| window.id()))
    }

    fn capabilities(_: &IOSLoop, _: &IOSWindows) -> WindowCapabilities {
        WindowCapabilities::NONE
    }

    fn frame_inset(_: &IOSLoop, _: &IOSWindows, _: f64) -> Size<f64> {
        // A UIKit window has no non-client area.
        Size::new(0.0, 0.0)
    }

    fn on_event(owner: &IOSLoop, callback: Box<dyn FnMut(WindowEvent) + Send>) {
        install(
            &owner.handler(|handlers| &mut handlers.window_event),
            callback,
        );
    }
}

impl crate::host_window::CoreHostWindow for IOSWindow {}

impl crate::host_window::HostWindow for IOSWindow {
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
}

impl PlatformWindow for IOSWindow {
    fn close(&self) {
        crate::owner::apple::ios::request_close(self.id);
    }

    fn id(&self) -> WindowId {
        self.id
    }

    fn physical_size(&self) -> Size<i32> {
        let metrics = *self.metrics.lock();
        Size::new(
            (metrics.size.width * metrics.scale).round() as i32,
            (metrics.size.height * metrics.scale).round() as i32,
        )
    }

    fn logical_size(&self) -> Size<f64> {
        self.metrics.lock().size
    }
    fn scale_factor(&self) -> f64 {
        self.metrics.lock().scale
    }

    fn display_id(&self) -> Option<DisplayId> {
        self.screen.lock().id
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
    ///   through `request_redraw` again), so the `WindowCallbacks` drain
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
    /// this backend has.
    fn request_redraw(&self) {
        tracing::trace!("request_redraw: demand recorded; the CADisplayLink delivers the frame");
    }

    fn safe_area_insets(&self) -> EdgeInsets {
        self.metrics.lock().safe_area
    }

    fn haptics(&self) -> Option<Arc<dyn flui_platform::PlatformHaptics>> {
        let marker = MainThreadMarker::new()?;
        let haptics = self.haptics.get_or_init(|| {
            let view: Retained<objc2_ui_kit::UIView> =
                Retained::into_super(self.native.get(marker).view.clone());
            Arc::new(crate::haptics::apple::IOSHaptics::new(view, marker))
        });
        Some(Arc::clone(haptics) as Arc<dyn flui_platform::PlatformHaptics>)
    }

    /// UIKit defers system gestures per screen edge, not per rectangle: the window
    /// projects the regions, and when the projection changed defers each edge an
    /// exclusion region reaches (`preferredScreenEdgesDeferringSystemGestures`). A region
    /// clear of every edge has no UIKit equivalent and defers nothing.
    fn set_input_regions(&self, regions: &[flui_platform::input_regions::InputRegion]) {
        let Some(marker) = MainThreadMarker::new() else {
            return;
        };
        let size = self.physical_size();
        let scale = self.scale_factor();
        let edges = {
            let mut state = self.input_regions.lock();
            let (projection, edges) = &mut *state;
            let Some(sent) = projection.update(regions, scale, None) else {
                return;
            };
            *edges = crate::window::apple::geometry::screen_edges::DeferredEdges::of(sent, size);
            *edges
        };
        let controller = self
            .native
            .get(marker)
            .attachment
            .borrow()
            .as_ref()
            .map(|attachment| attachment.controller.clone());
        if let Some(controller) = controller {
            controller.defer_edges(rect_edge(edges));
        }
    }

    fn system_chrome(&self) -> Option<Arc<dyn flui_platform::PlatformSystemChrome>> {
        Some(Arc::new(status_bar::IOSSystemChrome(Arc::clone(
            &self.status_bar,
        ))))
    }

    fn appearance(&self) -> WindowAppearance {
        if self.dark.load(Ordering::Relaxed) {
            WindowAppearance::Dark
        } else {
            WindowAppearance::Light
        }
    }

    fn execution_state(&self) -> WindowExecutionState {
        if self.closed.load(Ordering::SeqCst) {
            WindowExecutionState::Detached
        } else {
            self.lifecycle.lock().execution
        }
    }

    fn is_focused(&self) -> bool {
        self.lifecycle.lock().focused && !self.closed.load(Ordering::SeqCst)
    }

    fn is_visible(&self) -> bool {
        self.lifecycle.lock().visible && !self.closed.load(Ordering::SeqCst)
    }

    fn set_cursor(&self, _cursor: CursorIcon) -> Result<(), CursorError> {
        // iOS has no pointer cursor on a touch device.
        Err(CursorError::Unsupported)
    }

    /// `UIWindow` has no title, frame, position or commands, so a scene's
    /// window honours none and observes nothing.
    fn capabilities(&self) -> WindowCapabilities {
        WindowCapabilities::NONE
    }

    fn apply(&self, command: WindowCommand) -> Result<(), WindowCommandError> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(WindowCommandError::Closed);
        }
        // The system owns the surface: every command is refused, except the one
        // that asks for the mode every window has.
        command.admit(&PlatformWindow::capabilities(self))?;
        Ok(())
    }

    /// The scene's size, visibility and focus. The metrics and lifecycle live
    /// apart from this cell, so each call reads them again. `on_state_changed`
    /// is raised where they change: when the view's size or scale does, and on
    /// each lifecycle step.
    fn state(&self) -> WindowState {
        self.snapshot.invalidate();
        let published = self.snapshot.state_with(|| {
            let size = self.metrics.lock().size;
            WindowState::new(WindowGeometry::new(None, size), size)
                .with_visible(self.is_visible())
                .with_focused(self.is_focused())
                .with_resizable(false)
                .with_decorated(false)
                .with_display(self.screen.lock().id)
        });
        if published.changed {
            self.callbacks.dispatch_state_changed();
        }
        published.state
    }

    crate::owner::handlers::impl_window_callback_setters!(callbacks);

    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        // Refuse once closed, so a retained `Arc<dyn PlatformWindow>` cannot
        // hand out a handle to a detached window (the macOS backend's
        // `closed` flag exists for the same reason).
        if self.closed.load(Ordering::SeqCst) {
            return Err(raw_window_handle::HandleError::Unavailable);
        }
        let marker = MainThreadMarker::new().ok_or(raw_window_handle::HandleError::Unavailable)?;
        if self.execution_state() == WindowExecutionState::Detached {
            return Err(raw_window_handle::HandleError::Unavailable);
        }
        let view_ptr = Retained::as_ptr(&self.native.get(marker).view);
        let handle = raw_window_handle::UiKitWindowHandle::new(
            std::ptr::NonNull::new(view_ptr as *mut std::ffi::c_void)
                .ok_or(raw_window_handle::HandleError::Unavailable)?,
        );
        // SAFETY: the `UIView` is retained by `self.view`, which outlives the
        // returned handle's borrow of `self`.
        Ok(unsafe {
            raw_window_handle::WindowHandle::borrow_raw(raw_window_handle::RawWindowHandle::UiKit(
                handle,
            ))
        })
    }

    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        let handle = raw_window_handle::UiKitDisplayHandle::new();
        // SAFETY: the UIKit display handle carries no pointer.
        Ok(unsafe {
            raw_window_handle::DisplayHandle::borrow_raw(
                raw_window_handle::RawDisplayHandle::UiKit(handle),
            )
        })
    }
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
        window: Arc<dyn HostWindow>,
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

/// The application's handler as the loop calls it: each call becomes the
/// [`IOSSceneEvent`] it describes, and a connection returns the loop the
/// pending side of the installation it hands the application.
struct EventConsumer(SceneHandler);

impl SceneConsumer for EventConsumer {
    fn connected(
        &mut self,
        session: IOSSceneSessionId,
        attachment: IOSSceneAttachmentId,
        window: Arc<dyn HostWindow>,
        reconnect: bool,
        wake: PlatformProxy,
    ) -> Result<Box<dyn SceneInstallation>, BootstrapError> {
        let (installation, watched) = WindowInstallation::channel(wake);
        (self.0)(IOSSceneEvent::Connected {
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
        (self.0)(IOSSceneEvent::Disconnected {
            session,
            attachment,
        })
    }

    fn installation_aborted(
        &mut self,
        session: IOSSceneSessionId,
        attachment: IOSSceneAttachmentId,
    ) -> Result<(), BootstrapError> {
        (self.0)(IOSSceneEvent::InstallationAborted {
            session,
            attachment,
        })
    }

    fn discarded(&mut self, session: IOSSceneSessionId) -> Result<(), BootstrapError> {
        (self.0)(IOSSceneEvent::Discarded { session })
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
    /// # Errors
    /// [`PlatformError::Init`] off the main thread, or once the owner stopped.
    pub fn on_scene_event(&self, callback: SceneHandler) -> Result<(), PlatformError> {
        self.owner()
            .install_scene_consumer(Box::new(EventConsumer(callback)))
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
        atomic::{AtomicU8, Ordering},
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

    /// A window's requested status-bar style and the controller reporting it.
    pub(super) struct StatusBar {
        style: Arc<AtomicU8>,
        controller: NativeOwner<RefCell<Option<Retained<FluiViewController>>>>,
    }

    impl StatusBar {
        pub(super) fn new(mtm: MainThreadMarker) -> Arc<Self> {
            Arc::new(Self {
                style: Arc::new(AtomicU8::new(UNSET)),
                controller: NativeOwner::new(RefCell::new(None), mtm),
            })
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
