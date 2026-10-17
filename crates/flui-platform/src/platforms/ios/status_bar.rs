//! The status-bar style a window's content asks for.
//!
//! UIKit asks the window's root view controller for its
//! `preferredStatusBarStyle` (view-controller-based appearance, the
//! Info.plist default). The root controller is a [`FluiViewController`] that
//! answers from a shared atomic; [`IOSSystemChrome`] stores a new style from
//! any thread and asks UIKit, on the main thread, to read it again.

use std::cell::RefCell;
use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, available, define_class, msg_send};
use objc2_foundation::NSObjectProtocol;
use objc2_ui_kit::{UIStatusBarStyle, UIViewController};

use flui_platform_api::{PlatformSystemChrome, SystemUiOverlayStyle};

use super::native_owner::NativeOwner;

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
        DARK_ICONS if available!(ios = 13.0) => UIStatusBarStyle::DarkContent,
        // Dark icons before iOS 13, where the default is dark content; and
        // the unset state, where it is what a plain `UIViewController`
        // reports, so an unannotated window keeps the system's style.
        _ => UIStatusBarStyle::Default,
    }
}

/// The root view controller's ivars: the style it reports.
pub struct FluiViewControllerIvars {
    style: Arc<AtomicU8>,
}

define_class!(
    // SAFETY:
    // - `UIViewController` has no subclassing requirements beyond those the
    //   superclass already satisfies.
    // - The class does not implement `Drop`, so `define_class!`'s
    //   `dealloc` story is the superclass's.
    //
    /// The root view controller, reporting the status-bar style the content
    /// asked for.
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
    }

    unsafe impl NSObjectProtocol for FluiViewController {}
);

impl FluiViewController {
    fn new(mtm: MainThreadMarker, style: Arc<AtomicU8>) -> Retained<Self> {
        let this = mtm
            .alloc::<Self>()
            .set_ivars(FluiViewControllerIvars { style });
        // SAFETY: `init` reaches UIViewController's designated initializer
        // with no nib; the receiver is the allocation whose ivars were set
        // above, which is the `define_class!` idiom.
        unsafe { msg_send![super(this), init] }
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

    /// A root view controller reporting this window's style; it replaces the
    /// previous attachment's controller as the one to refresh.
    pub(super) fn attach_controller(&self, mtm: MainThreadMarker) -> Retained<UIViewController> {
        let controller = FluiViewController::new(mtm, Arc::clone(&self.style));
        *self.controller.get(mtm).borrow_mut() = Some(controller.clone());
        Retained::into_super(controller)
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
pub(super) struct IOSSystemChrome(pub(super) Arc<StatusBar>);

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
            crate::shared::panic_boundary::contain_owner_callback(|| status_bar.refresh(mtm));
        });
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
