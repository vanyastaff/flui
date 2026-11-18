//! Where a picker, an alert or a share sheet is presented from: the
//! requesting window's root view controller, or the controller it presents
//! on top, as deep as the chain goes.
//!
//! UIKit presents one controller over another only from a controller that
//! presents nothing: asked of one that already presents, or of one whose
//! presented controller is still leaving, it logs a warning, presents
//! nothing and calls nobody. So the presenter is the last controller of the
//! chain, a chain with a controller still being dismissed has none (the
//! request answers that the window is busy), and every presentation is
//! checked right after it was asked for: a controller that is not the
//! presenter's presented one was not shown, and the request answers at
//! once instead of waiting for an end that never comes.
//!
//! An iPad shows a share sheet and an action sheet in a popover, which
//! raises an exception when it has no anchor. A request names no control to
//! point at, so whatever is presented with a popover gets it anchored in
//! the middle of the presenter's view, with no arrow; a phone presents the
//! same controller as a sheet and reads none of it.
//!
//! UIKit reports a controller it cannot make or present by raising an
//! Objective-C exception, which would end the process from inside a
//! widget's call. Each call that makes, presents or dismisses a controller
//! runs where the exception is caught (`guarded`), and answers a typed
//! refusal ([`Refusal::Raised`]) instead.

use flui_platform::capability::Capability;
use flui_platform::{SystemFailure, Unsupported, UnsupportedReason};

use crate::dialog::DialogError;
use crate::share::{ShareCapability, ShareError};

/// Why nothing was presented on a window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Refusal {
    /// The window has no UIKit window, or that window no root view
    /// controller: a headless window, a view out of any window.
    NoWindow,
    /// A controller the window presents is still being dismissed, or UIKit
    /// took nothing when asked; asking again later may succeed.
    Busy,
    /// UIKit raised an exception making, presenting or dismissing a
    /// controller.
    Raised,
}

impl Refusal {
    /// What a dialog of the capability `C` answers for this refusal: no
    /// window, a window that is busy, or a system that answered outside its
    /// contract.
    pub(super) fn dialog_error<C: Capability>(self) -> DialogError {
        match self {
            Self::NoWindow => {
                DialogError::Unsupported(Unsupported::of::<C>(UnsupportedReason::NoWindow))
            }
            Self::Busy => DialogError::Busy,
            Self::Raised => DialogError::Failed(SystemFailure::MalformedAnswer),
        }
    }

    /// What a share answers for this refusal. A share has no busy answer of
    /// its own: a window that cannot present now is `Unavailable`.
    pub(super) fn share_error(self) -> ShareError {
        let reason = match self {
            Self::NoWindow => UnsupportedReason::NoWindow,
            Self::Busy => UnsupportedReason::Unavailable,
            Self::Raised => return ShareError::Failed(SystemFailure::MalformedAnswer),
        };
        ShareError::Unsupported(Unsupported::of::<ShareCapability>(reason))
    }
}

/// The controller to present from, starting at `root`: the last of the
/// chain `presented` follows, or [`Refusal::Busy`] when a controller of the
/// chain is `leaving` (being dismissed), since UIKit presents over neither
/// it nor the controller that presented it.
pub(super) fn topmost<C>(
    root: C,
    presented: impl Fn(&C) -> Option<C>,
    leaving: impl Fn(&C) -> bool,
) -> Result<C, Refusal> {
    let mut presenter = root;
    while let Some(next) = presented(&presenter) {
        if leaving(&next) {
            return Err(Refusal::Busy);
        }
        presenter = next;
    }
    Ok(presenter)
}

/// The middle of a view whose bounds start at `origin` and span `size`, as
/// the one-point rectangle a popover is anchored to: `(x, y, width,
/// height)`. A view with no extent, or one whose geometry is not finite,
/// is anchored at its origin, or at zero.
pub(super) fn middle(origin: (f64, f64), size: (f64, f64)) -> (f64, f64, f64, f64) {
    let along = |start: f64, extent: f64| {
        let start = if start.is_finite() { start } else { 0.0 };
        let half = extent / 2.0;
        if half.is_finite() && half > 0.0 && (start + half).is_finite() {
            start + half
        } else {
            start
        }
    };
    (along(origin.0, size.0), along(origin.1, size.1), 1.0, 1.0)
}

#[cfg(target_os = "ios")]
pub(super) use uikit::{dismiss, guarded, present, presenter};

#[cfg(target_os = "ios")]
#[expect(unsafe_code, reason = "a raw window handle's view")]
mod uikit {
    use flui_platform::capability::WindowRef;
    use objc2::MainThreadMarker;
    use objc2::rc::Retained;
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};
    use objc2_ui_kit::{UIPopoverArrowDirection, UIView, UIViewController};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    use super::{Refusal, middle, topmost};

    /// Run UIKit calls that may raise an Objective-C exception:
    /// [`Refusal::Raised`] when one did. A Rust panic in `calls` passes
    /// through unchanged.
    pub(in crate::ios) fn guarded<R>(calls: impl FnOnce() -> R) -> Result<R, Refusal> {
        // The exception is released here; nothing of it is a code the
        // contract could carry.
        objc2::exception::catch(std::panic::AssertUnwindSafe(calls)).map_err(|_| Refusal::Raised)
    }

    /// The root view controller of the UIKit window `window` is in, when
    /// its handle names a view in one.
    fn root_view_controller(
        window: &WindowRef,
        _mtm: MainThreadMarker,
    ) -> Option<Retained<UIViewController>> {
        let RawWindowHandle::UiKit(handle) = window.window_handle().ok()?.as_raw() else {
            return None;
        };
        // SAFETY: a UIKit handle names a live `UIView` of this process, which
        // the window's surface target keeps alive while the handle is
        // borrowed, and it is read on the main thread, where the view is
        // used.
        let view: &UIView = unsafe { handle.ui_view.cast::<UIView>().as_ref() };
        view.window()?.rootViewController()
    }

    /// The controller to present from on `window` (see the module).
    pub(in crate::ios) fn presenter(
        window: &WindowRef,
        mtm: MainThreadMarker,
    ) -> Result<Retained<UIViewController>, Refusal> {
        let root = root_view_controller(window, mtm).ok_or(Refusal::NoWindow)?;
        topmost(
            root,
            |controller| controller.presentedViewController(),
            |controller| controller.isBeingDismissed(),
        )
    }

    /// Anchor the popover UIKit gives `controller`, when it gives one, in
    /// the middle of `presenter`'s view with no arrow (see the module).
    fn anchor_popover(controller: &UIViewController, presenter: &UIViewController) {
        let (Some(popover), Some(view)) =
            (controller.popoverPresentationController(), presenter.view())
        else {
            return;
        };
        let bounds = view.bounds();
        let (x, y, width, height) = middle(
            (bounds.origin.x, bounds.origin.y),
            (bounds.size.width, bounds.size.height),
        );
        popover.setSourceView(Some(&view));
        popover.setSourceRect(CGRect::new(CGPoint::new(x, y), CGSize::new(width, height)));
        popover.setPermittedArrowDirections(UIPopoverArrowDirection::empty());
    }

    /// Present `controller` from `presenter`, animated, its popover
    /// anchored where it has one. [`Refusal::Busy`] when UIKit took nothing
    /// (see the module), [`Refusal::Raised`] when it raised.
    pub(in crate::ios) fn present(
        presenter: &UIViewController,
        controller: &UIViewController,
    ) -> Result<(), Refusal> {
        let taken = guarded(|| {
            anchor_popover(controller, presenter);
            presenter.presentViewController_animated_completion(controller, true, None);
            presenter
                .presentedViewController()
                .is_some_and(|presented| std::ptr::eq(&*presented, controller))
        })?;
        if taken { Ok(()) } else { Err(Refusal::Busy) }
    }

    /// Dismiss `controller` and whatever it presents, when it is still
    /// presented and not already leaving: a controller that answered has
    /// dismissed itself, and dismissing it again would close whatever its
    /// presenter shows next. A dismissal UIKit raises on dismisses nothing.
    pub(in crate::ios) fn dismiss(controller: &UIViewController) {
        // Nobody is left to tell of a refusal: the dialog has ended for its
        // caller.
        let _ = guarded(|| {
            if controller.presentingViewController().is_some() && !controller.isBeingDismissed() {
                controller.dismissViewControllerAnimated_completion(true, None);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use flui_platform::{SystemFailure, Unsupported, UnsupportedReason};

    use super::{Refusal, middle, topmost};
    use crate::dialog::{DialogError, FileDialogCapability, MessageBoxCapability};
    use crate::share::{ShareCapability, ShareError};

    /// A chain of controllers by index: each presents the next, and those
    /// in `leaving` are being dismissed.
    fn walk(chain: usize, leaving: &[usize]) -> Result<usize, Refusal> {
        topmost(
            0,
            |&controller| (controller + 1 < chain).then_some(controller + 1),
            |controller| leaving.contains(controller),
        )
    }

    #[test]
    fn a_presentation_starts_from_the_last_controller_and_waits_for_one_leaving() {
        assert_eq!(walk(1, &[]), Ok(0), "the root presents nothing");
        assert_eq!(walk(3, &[]), Ok(2), "the last of the chain presents");
        assert_eq!(
            walk(3, &[2]),
            Err(Refusal::Busy),
            "the last one is leaving: neither it nor its presenter takes one"
        );
        assert_eq!(walk(3, &[1]), Err(Refusal::Busy));
        assert_eq!(
            walk(1, &[0]),
            Ok(0),
            "the root is never being dismissed; only a presented one is asked"
        );
    }

    #[test]
    fn a_popover_is_anchored_at_a_finite_point_in_the_middle_of_the_view() {
        assert_eq!(
            middle((0.0, 0.0), (768.0, 1024.0)),
            (384.0, 512.0, 1.0, 1.0)
        );
        assert_eq!(middle((10.0, -20.0), (100.0, 40.0)), (60.0, 0.0, 1.0, 1.0));
        assert_eq!(
            middle((5.0, 7.0), (0.0, -3.0)),
            (5.0, 7.0, 1.0, 1.0),
            "a view with no extent is anchored at its origin"
        );
        let anchored = middle((f64::NAN, f64::MAX), (f64::INFINITY, f64::MAX));
        assert_eq!(
            anchored,
            (0.0, f64::MAX, 1.0, 1.0),
            "no non-finite geometry reaches UIKit"
        );
    }

    #[test]
    fn a_controller_uikit_cannot_show_answers_a_typed_refusal() {
        let no_window =
            |unsupported: Unsupported| unsupported.reason == UnsupportedReason::NoWindow;
        assert!(matches!(
            Refusal::NoWindow.dialog_error::<FileDialogCapability>(),
            DialogError::Unsupported(unsupported) if no_window(unsupported)
        ));
        assert_eq!(
            Refusal::NoWindow.dialog_error::<MessageBoxCapability>(),
            DialogError::Unsupported(Unsupported::of::<MessageBoxCapability>(
                UnsupportedReason::NoWindow
            )),
            "the refusal names the capability that was asked"
        );
        assert_eq!(
            Refusal::Busy.dialog_error::<FileDialogCapability>(),
            DialogError::Busy
        );
        assert_eq!(
            Refusal::Raised.dialog_error::<MessageBoxCapability>(),
            DialogError::Failed(SystemFailure::MalformedAnswer),
            "an exception is a failure of the system, answered, not a crash"
        );

        let share = |reason| ShareError::Unsupported(Unsupported::of::<ShareCapability>(reason));
        assert_eq!(
            Refusal::NoWindow.share_error(),
            share(UnsupportedReason::NoWindow)
        );
        assert_eq!(
            Refusal::Busy.share_error(),
            share(UnsupportedReason::Unavailable),
            "a share has no busy answer: later may succeed"
        );
        assert_eq!(
            Refusal::Raised.share_error(),
            ShareError::Failed(SystemFailure::MalformedAnswer)
        );
    }
}
