//! iOS window implementation (UIKit).
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

use std::any::Any;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use cursor_icon::CursorIcon;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_foundation::{NSDefaultRunLoopMode, NSObjectProtocol, NSRunLoop, NSSet, NSString};
use objc2_quartz_core::CADisplayLink;
use objc2_ui_kit::{
    UIApplication, UIApplicationState, UITouch, UIView, UIViewController, UIWindow,
};

use flui_types::geometry::{DevicePixels, Pixels, Size, device_px, px};

use super::events::touch_to_pointer_events;
use crate::shared::WindowCallbacks;
use crate::traits::{CursorError, PlatformWindow, WindowExecutionState, WindowId};

/// The content view: a `UIView` subclass that forwards touches.
///
/// `objc2`'s `define_class!` requires the ivars be declared on the class; the
/// callback storage is reached through a `Retained` held in an ivar rather
/// than a raw pointer ivar as the macOS backend does, so the view owns its
/// callbacks and there is no separate deallocation step.
pub struct FluiViewIvars {
    callbacks: Arc<WindowCallbacks>,
    /// The most recent logical size, so a resize dispatch compares against it
    /// and does not re-announce an unchanged size on every layout pass.
    last_size: std::cell::Cell<Size<Pixels>>,
    /// The per-frame tick source, created once and paused/resumed around the
    /// app's background transitions. `RefCell` because the view's methods
    /// reach it through a shared `&self`.
    display_link: RefCell<Option<Retained<CADisplayLink>>>,
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

    impl FluiView {
        #[unsafe(method(touchesBegan:withEvent:))]
        fn touches_began(&self, touches: &NSSet<UITouch>, _event: Option<&AnyObject>) {
            self.dispatch_touches(touches, TouchPhase::Began);
        }

        #[unsafe(method(touchesMoved:withEvent:))]
        fn touches_moved(&self, touches: &NSSet<UITouch>, _event: Option<&AnyObject>) {
            self.dispatch_touches(touches, TouchPhase::Moved);
        }

        #[unsafe(method(touchesEnded:withEvent:))]
        fn touches_ended(&self, touches: &NSSet<UITouch>, _event: Option<&AnyObject>) {
            self.dispatch_touches(touches, TouchPhase::Ended);
        }

        #[unsafe(method(touchesCancelled:withEvent:))]
        fn touches_cancelled(&self, touches: &NSSet<UITouch>, _event: Option<&AnyObject>) {
            self.dispatch_touches(touches, TouchPhase::Cancelled);
        }

        #[unsafe(method(layoutSubviews))]
        fn layout_subviews(&self) {
            // SAFETY: `super` is the `UIView` implementation of
            // `layoutSubviews`; calling it is what a correct override does.
            // The annotation pins the return type, which `msg_send!` cannot
            // infer for a `void` super call.
            let _: () = unsafe { msg_send![super(self), layoutSubviews] };
            self.dispatch_resize_if_changed();
        }

        /// The `CADisplayLink` target: one tick per refresh requests a frame.
        #[unsafe(method(onDisplayLink:))]
        fn on_display_link(&self, _link: &CADisplayLink) {
            self.callbacks().dispatch_request_frame();
        }
    }

    unsafe impl NSObjectProtocol for FluiView {}
);

/// Which touch callback produced a batch, mapped to the pointer phase the
/// `ui-events` vocabulary uses.
#[derive(Clone, Copy)]
enum TouchPhase {
    Began,
    Moved,
    Ended,
    Cancelled,
}

impl FluiView {
    /// Build the content view for one window, sized to the screen's bounds.
    pub(super) fn new(mtm: MainThreadMarker, callbacks: Arc<WindowCallbacks>) -> Retained<Self> {
        let bounds = objc2_ui_kit::UIScreen::mainScreen(mtm).bounds();
        let this = mtm.alloc::<Self>();
        let this = this.set_ivars(FluiViewIvars {
            callbacks,
            last_size: std::cell::Cell::new(Size::new(px(0.0), px(0.0))),
            display_link: RefCell::new(None),
        });
        // SAFETY: `initWithFrame:` is `UIView`'s designated initializer; the
        // `super(this)` receiver is the partially-initialized allocation with
        // its ivars already set above, which is the `define_class!` idiom.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: bounds] };
        // Multi-touch is what a gesture layer needs; without it UIKit reports
        // only the first finger of a chord.
        this.setMultipleTouchEnabled(true);
        // A GPU-rendered surface is opaque, so tell UIKit not to composite
        // anything behind it.
        this.setOpaque(true);
        this.start_display_link();
        this
    }

    /// Create the per-frame tick, targeting this view's `onDisplayLink:`.
    ///
    /// The link is added to the main run loop in the default mode — the run
    /// loop `UIApplicationMain` drives — so it fires on the main thread and
    /// therefore on the owner lane.
    fn start_display_link(&self) {
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
        self.ivars().display_link.replace(Some(link));
    }

    /// Pause or resume the tick — driven by the app's background/foreground
    /// transitions so a suspended app does no work.
    pub(super) fn set_display_link_paused(&self, paused: bool) {
        if let Some(link) = self.ivars().display_link.borrow().as_ref() {
            link.setPaused(paused);
        }
    }

    fn callbacks(&self) -> &Arc<WindowCallbacks> {
        &self.ivars().callbacks
    }

    /// Convert a UIKit touch batch and dispatch each event, then request a
    /// frame — the same "input dirties the presentation" step Android's
    /// motion-event path takes.
    fn dispatch_touches(&self, touches: &NSSet<UITouch>, phase: TouchPhase) {
        let scale = self.contentScaleFactor();
        let mut any = false;
        for touch in touches {
            for event in touch_to_pointer_events(&touch, phase.as_pointer_phase(), scale) {
                let result = self.callbacks().dispatch_input(event);
                any |= result.default_prevented;
            }
        }
        if any {
            self.callbacks().dispatch_request_frame();
        }
    }

    /// Announce a resize when the view's logical size actually changed.
    fn dispatch_resize_if_changed(&self) {
        let bounds = self.bounds();
        let size = Size::new(px(bounds.size.width as f32), px(bounds.size.height as f32));
        if self.ivars().last_size.get() == size || size.width.0 <= 0.0 || size.height.0 <= 0.0 {
            return;
        }
        self.ivars().last_size.set(size);
        self.callbacks()
            .dispatch_resize(size, self.contentScaleFactor() as f32);
    }
}

impl TouchPhase {
    fn as_pointer_phase(self) -> super::events::TouchPhase {
        match self {
            TouchPhase::Began => super::events::TouchPhase::Down,
            TouchPhase::Moved => super::events::TouchPhase::Move,
            TouchPhase::Ended => super::events::TouchPhase::Up,
            TouchPhase::Cancelled => super::events::TouchPhase::Cancel,
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

#[derive(Clone, Copy)]
pub(super) enum LifecycleObservation {
    Active,
    Inactive,
    Background,
    Foreground,
}

/// iOS window wrapping a `UIWindow` and its content view.
pub struct IOSWindow {
    window: Retained<UIWindow>,
    view_controller: Retained<UIViewController>,
    view: Retained<FluiView>,
    callbacks: Arc<WindowCallbacks>,
    closed: Arc<AtomicBool>,
    lifecycle: Arc<parking_lot::Mutex<IOSLifecycle>>,
    /// The window is only meaningfully sized once the screen is attached;
    /// `logical_size` reads the view live, so this is not cached beyond the
    /// resize comparison above.
    id: WindowId,
}

// SAFETY: `retain`/`release` on the underlying Objective-C objects are
// atomic, and every `UIWindow`/`UIView` message this backend sends is routed
// through the main thread by construction: `run` blocks the main thread in
// `UIApplicationMain`, and the platform's `open_window` is only reachable
// from `on_ready`, which runs there. The `Arc<WindowCallbacks>` is itself
// `Send + Sync`. This mirrors `MacOSWindow`'s own reasoning (ADR-0039).
unsafe impl Send for IOSWindow {}
unsafe impl Sync for IOSWindow {}

impl IOSWindow {
    /// Create the window. Must run on the main thread.
    pub(super) fn new(mtm: MainThreadMarker) -> Self {
        let callbacks = Arc::new(WindowCallbacks::new());
        let view = FluiView::new(mtm, Arc::clone(&callbacks));

        let view_controller = UIViewController::new(mtm);
        view_controller.setView(Some(&view));

        let window: Retained<UIWindow> = unsafe {
            msg_send![mtm.alloc::<UIWindow>(), initWithFrame: objc2_ui_kit::UIScreen::mainScreen(mtm).bounds()]
        };
        window.setRootViewController(Some(&view_controller));
        // Order the window in and make it key. Doing this at construction
        // matches what a single-window iPhone app wants: the FLUI window is
        // the application's only window.
        window.makeKeyAndVisible();

        let this = Self {
            window,
            view_controller,
            view,
            callbacks,
            closed: Arc::new(AtomicBool::new(false)),
            lifecycle: Arc::new(parking_lot::Mutex::new(IOSLifecycle {
                execution: if UIApplication::sharedApplication(mtm).applicationState()
                    == UIApplicationState::Background
                {
                    WindowExecutionState::Suspended
                } else {
                    WindowExecutionState::Running
                },
                focused: UIApplication::sharedApplication(mtm).applicationState()
                    == UIApplicationState::Active,
                visible: UIApplication::sharedApplication(mtm).applicationState()
                    != UIApplicationState::Background,
                generation: 0,
                pending: VecDeque::new(),
                dispatching: false,
            })),
            id: WindowId(1),
        };
        this.set_frame_tick_paused(this.execution_state() != WindowExecutionState::Running);
        this
    }

    /// The callback storage, for the platform's input/frame dispatch.
    pub(super) fn callbacks(&self) -> &Arc<WindowCallbacks> {
        &self.callbacks
    }

    /// Pause or resume the per-frame tick (delegates to the content view's
    /// `CADisplayLink`). Driven by the app's background/foreground edges.
    pub(super) fn set_frame_tick_paused(&self, paused: bool) {
        self.view.set_display_link_paused(paused);
    }

    /// Serialize native observations independently of the input/frame FIFO.
    pub(super) fn observe_lifecycle(&self, observation: LifecycleObservation) {
        use LifecycleObservation::{Background, Foreground};
        if self.closed.load(Ordering::SeqCst) {
            return;
        }
        // A callback may pump a nested UIKit run loop before our queue drains.
        // Suspend the physical link immediately, even for a queued observation.
        if matches!(observation, Background) {
            self.set_frame_tick_paused(true);
        }
        {
            let mut state = self.lifecycle.lock();
            if matches!(observation, Background | Foreground) {
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

    fn apply_lifecycle(&self, observation: LifecycleObservation, generation: u64) {
        use crate::shared::LifecycleEvent;
        use LifecycleObservation::{Active, Background, Foreground, Inactive};
        {
            let mut state = self.lifecycle.lock();
            if matches!(observation, Background | Foreground) && state.generation != generation {
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
                Background => {
                    state.execution = WindowExecutionState::Suspended;
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
            Background => {
                emit(LifecycleEvent::Execution(WindowExecutionState::Suspended));
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

    /// Detach the root view controller and hide the window. Idempotent.
    pub(super) fn close_inner(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        self.set_frame_tick_paused(true);
        self.window.setHidden(true);
        self.window.setRootViewController(None);
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

impl PlatformWindow for IOSWindow {
    fn id(&self) -> WindowId {
        self.id
    }

    fn physical_size(&self) -> Size<DevicePixels> {
        let bounds = self.view.bounds();
        let scale = self.view.contentScaleFactor();
        Size::new(
            device_px((bounds.size.width * scale).round() as i32),
            device_px((bounds.size.height * scale).round() as i32),
        )
    }

    fn logical_size(&self) -> Size<Pixels> {
        let bounds = self.view.bounds();
        Size::new(px(bounds.size.width as f32), px(bounds.size.height as f32))
    }

    fn scale_factor(&self) -> f64 {
        self.view.contentScaleFactor()
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

    fn get_title(&self) -> String {
        // `UIWindow` has no title; the bundle display name is the app's name.
        NSString::from_str("FLUI iOS").to_string()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    crate::shared::impl_window_callback_setters!(callbacks);

    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        // Refuse once closed, so a retained `Arc<dyn PlatformWindow>` cannot
        // hand out a handle to a detached window (the macOS backend's
        // `closed` flag exists for the same reason).
        if self.closed.load(Ordering::SeqCst) {
            return Err(raw_window_handle::HandleError::Unavailable);
        }
        let view_ptr = Retained::as_ptr(&self.view);
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

impl Clone for IOSWindow {
    fn clone(&self) -> Self {
        Self {
            window: self.window.clone(),
            view_controller: self.view_controller.clone(),
            view: self.view.clone(),
            callbacks: Arc::clone(&self.callbacks),
            closed: Arc::clone(&self.closed),
            lifecycle: Arc::clone(&self.lifecycle),
            id: self.id,
        }
    }
}

impl Drop for IOSWindow {
    fn drop(&mut self) {
        // Only the last clone closes the native window.
        if Arc::strong_count(&self.callbacks) == 1 && !self.closed.load(Ordering::SeqCst) {
            tracing::debug!("iOS window dropped; hiding last native window");
            self.close_inner();
        }
    }
}
