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
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_foundation::{NSDefaultRunLoopMode, NSObjectProtocol, NSRunLoop, NSSet};
use objc2_quartz_core::CADisplayLink;
use objc2_ui_kit::{UITouch, UIView, UIViewController, UIWindow, UIWindowScene};

use flui_types::geometry::{DevicePixels, Pixels, Size, device_px, px};

use super::events::touch_to_pointer_events;
use super::native_owner::NativeOwner;
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
    metrics: Arc<parking_lot::Mutex<WindowMetrics>>,
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
            self.pin_for_native_callback();
            self.dispatch_touches(touches, TouchPhase::Began);
        }

        #[unsafe(method(touchesMoved:withEvent:))]
        fn touches_moved(&self, touches: &NSSet<UITouch>, _event: Option<&AnyObject>) {
            self.pin_for_native_callback();
            self.dispatch_touches(touches, TouchPhase::Moved);
        }

        #[unsafe(method(touchesEnded:withEvent:))]
        fn touches_ended(&self, touches: &NSSet<UITouch>, _event: Option<&AnyObject>) {
            self.pin_for_native_callback();
            self.dispatch_touches(touches, TouchPhase::Ended);
        }

        #[unsafe(method(touchesCancelled:withEvent:))]
        fn touches_cancelled(&self, touches: &NSSet<UITouch>, _event: Option<&AnyObject>) {
            self.pin_for_native_callback();
            self.dispatch_touches(touches, TouchPhase::Cancelled);
        }

        #[unsafe(method(layoutSubviews))]
        fn layout_subviews(&self) {
            self.pin_for_native_callback();
            // SAFETY: `super` is the `UIView` implementation of
            // `layoutSubviews`; calling it is what a correct override does.
            // The annotation pins the return type, which `msg_send!` cannot
            // infer for a `void` super call.
            let _: () = unsafe { msg_send![super(self), layoutSubviews] };
            self.dispatch_resize_if_changed();
        }

        /// The `CADisplayLink` target: one tick per refresh requests a frame.
        #[unsafe(method(onDisplayLink:))]
        fn on_display_link(&self, link: &CADisplayLink) {
            self.pin_for_native_callback();
            let current = self.ivars().display_link.borrow().as_ref()
                .is_some_and(|active| std::ptr::eq::<CADisplayLink>(&raw const **active, link));
            if current && !link.isPaused() { self.callbacks().dispatch_request_frame(); }
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
    fn new(
        mtm: MainThreadMarker,
        callbacks: Arc<WindowCallbacks>,
        metrics: Arc<parking_lot::Mutex<WindowMetrics>>,
    ) -> Retained<Self> {
        let bounds = objc2_ui_kit::UIScreen::mainScreen(mtm).bounds();
        let this = mtm.alloc::<Self>();
        let this = this.set_ivars(FluiViewIvars {
            callbacks,
            last_size: std::cell::Cell::new(Size::new(px(0.0), px(0.0))),
            display_link: RefCell::new(None),
            metrics,
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
        let link = self.ivars().display_link.borrow().clone();
        if let Some(link) = link {
            link.setPaused(paused);
        }
    }

    fn invalidate_display_link(&self) {
        let link = self.ivars().display_link.borrow_mut().take();
        if let Some(link) = link {
            link.invalidate();
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
        let scale = self.contentScaleFactor();
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
        *self.ivars().metrics.lock() = WindowMetrics {
            size,
            scale: self.contentScaleFactor(),
        };
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
    Detached,
}

#[derive(Clone, Copy)]
struct WindowMetrics {
    size: Size<Pixels>,
    scale: f64,
}

struct NativeAttachment {
    window: Retained<UIWindow>,
    controller: Retained<UIViewController>,
    token: u64,
    published: bool,
    retiring: bool,
}

struct NativeWindow {
    view: Retained<FluiView>,
    attachment: RefCell<Option<NativeAttachment>>,
}

impl Drop for NativeWindow {
    fn drop(&mut self) {
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
    callbacks: Arc<WindowCallbacks>,
    closed: AtomicBool,
    lifecycle: parking_lot::Mutex<IOSLifecycle>,
    metrics: Arc<parking_lot::Mutex<WindowMetrics>>,
    id: WindowId,
}

impl IOSWindow {
    /// Allocate a stable view; a scene connection supplies its native container.
    pub(super) fn new(mtm: MainThreadMarker, id: WindowId) -> Self {
        let callbacks = Arc::new(WindowCallbacks::new());
        let metrics = Arc::new(parking_lot::Mutex::new(WindowMetrics {
            size: Size::new(px(0.0), px(0.0)),
            scale: 1.0,
        }));
        let view = FluiView::new(mtm, Arc::clone(&callbacks), Arc::clone(&metrics));
        Self {
            native: NativeOwner::new(
                NativeWindow {
                    view,
                    attachment: RefCell::new(None),
                },
                mtm,
            ),
            callbacks,
            metrics,
            id,
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

    pub(super) fn attach(&self, scene: &UIWindowScene, token: u64, marker: MainThreadMarker) {
        let native = self.native.get(marker);
        self.detach_native(marker);
        let controller = UIViewController::new(marker);
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
        window.layoutIfNeeded();
        native.view.dispatch_resize_if_changed();
        self.lifecycle.lock().execution = WindowExecutionState::Suspended;
        native.view.start_display_link();
        native.view.set_display_link_paused(true);
    }

    pub(super) fn publish(&self, token: u64, marker: MainThreadMarker) {
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
            if self.attachment_matches(token, marker)
                && self.execution_state() == WindowExecutionState::Running
            {
                self.set_frame_tick_paused(false);
            }
        }
    }

    pub(super) fn resource_generation(&self) -> u64 {
        self.lifecycle.lock().generation
    }

    pub(super) fn attachment_matches(&self, token: u64, marker: MainThreadMarker) -> bool {
        !self.closed.load(Ordering::SeqCst)
            && self
                .native
                .get(marker)
                .attachment
                .borrow()
                .as_ref()
                .is_some_and(|attachment| attachment.token == token)
    }

    fn detach_native(&self, marker: MainThreadMarker) {
        let native = self.native.get(marker);
        native.view.invalidate_display_link();
        native.view.removeFromSuperview();
        let retired = native.attachment.borrow_mut().take();
        if let Some(retired) = retired {
            retired.window.setHidden(true);
            retired.window.setRootViewController(None);
            drop(retired.controller);
        }
    }

    /// Pause or resume the per-frame tick (delegates to the content view's
    /// `CADisplayLink`). Driven by the app's background/foreground edges.
    pub(super) fn set_frame_tick_paused(&self, paused: bool) {
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
    pub(super) fn observe_lifecycle(&self, observation: LifecycleObservation) {
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

    fn apply_lifecycle(&self, observation: LifecycleObservation, generation: u64) {
        use crate::shared::LifecycleEvent;
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

    /// Fence the physical link before a queued ownership retirement can run.
    /// A callback may pump UIKit or the remaining lifecycle queue in between.
    pub(super) fn begin_retirement(&self) {
        let marker = MainThreadMarker::new().expect("BUG: scene retirement runs on main");
        if let Some(attachment) = self.native.get(marker).attachment.borrow_mut().as_mut() {
            attachment.retiring = true;
        }
        self.set_frame_tick_paused(true);
    }

    pub(super) fn disconnect(&self, marker: MainThreadMarker) {
        self.observe_lifecycle(LifecycleObservation::Detached);
        self.detach_native(marker);
    }

    pub(super) fn close_inner(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        self.set_frame_tick_paused(true);
        use crate::shared::LifecycleEvent;
        self.callbacks
            .dispatch_lifecycle_immediate(LifecycleEvent::Execution(
                WindowExecutionState::Detached,
            ));
        self.callbacks
            .dispatch_lifecycle_immediate(LifecycleEvent::Surface(false));
        crate::shared::panic_boundary::contain_owner_callback(|| self.callbacks.dispatch_close());
        crate::shared::panic_boundary::contain_owner_callback(|| self.callbacks.clear());
        if let Some(marker) = MainThreadMarker::new() {
            self.detach_native(marker);
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

impl PlatformWindow for IOSWindow {
    fn close(&self) {
        super::platform::request_close(self.id);
    }

    fn id(&self) -> WindowId {
        self.id
    }

    fn physical_size(&self) -> Size<DevicePixels> {
        let metrics = *self.metrics.lock();
        Size::new(
            device_px((f64::from(metrics.size.width.0) * metrics.scale).round() as i32),
            device_px((f64::from(metrics.size.height.0) * metrics.scale).round() as i32),
        )
    }

    fn logical_size(&self) -> Size<Pixels> {
        self.metrics.lock().size
    }
    fn scale_factor(&self) -> f64 {
        self.metrics.lock().scale
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
        "FLUI iOS".to_owned()
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
