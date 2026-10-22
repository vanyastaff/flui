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

    impl FluiViewController {
        #[unsafe(method(preferredStatusBarStyle))]
        fn preferred_status_bar_style(&self) -> UIStatusBarStyle {
            status_bar_style(self.ivars().style.load(Ordering::Acquire))
        }

        #[unsafe(method(preferredScreenEdgesDeferringSystemGestures))]
        fn preferred_screen_edges_deferring_system_gestures(&self) -> UIRectEdge {
            self.ivars().deferred_edges.get()
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
            let mtm = MainThreadMarker::new().expect("BUG: the main queue runs on the main thread");
            crate::owner::panic_boundary::contain_owner_callback(|| status_bar.refresh(mtm));
        });
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
