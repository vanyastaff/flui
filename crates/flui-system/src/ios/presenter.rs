//! Where a picker, an alert or a share sheet is presented from, and how it
//! is taken down: the requesting window's root view controller, or the
//! controller it presents on top, as deep as the chain goes.
//!
//! UIKit presents one controller over another only from a controller that
//! presents nothing: asked of one that already presents, or of one whose
//! presented controller is still leaving, it logs a warning, presents
//! nothing and calls nobody. So the presenter is the last controller of the
//! chain, and every presentation is checked right after it was asked for: a
//! controller that is not the presenter's presented one was not shown, and
//! the request answers at once instead of waiting for an end that never
//! comes.
//!
//! A controller of the chain may be on its way out when a request arrives:
//! a dialog asked for in the answer of another finds that other's
//! controller still animating away. The request then waits for that
//! transition to end and looks at the chain again ([`when_settled`]), at
//! most [`MAX_DISMISSALS_AWAITED`] times; a chain that never settles, or a
//! dismissal whose end nobody can hear, answers that the window is busy.
//!
//! A controller is dismissed through the controller that presents it:
//! asked of a controller that itself presents another (a share sheet over a
//! picker), UIKit dismisses only what is above it and leaves the controller
//! up with nobody to answer ([`dismissed_through`]).
//!
//! An iPad shows a share sheet and an action sheet in a popover, which
//! raises an exception when it has no anchor. A request names no control to
//! point at, so whatever is presented with a popover gets it anchored in
//! the middle of the presenter's view, with no arrow; a phone presents the
//! same controller as a sheet and reads none of it. The anchor is set once
//! UIKit took the controller, which it does a turn before it shows it:
//! asking a controller that is never presented for its presentation makes
//! one that keeps the controller alive for good.
//!
//! UIKit reports a controller it cannot make or present by raising an
//! Objective-C exception, which would end the process from inside a
//! widget's call. Each call that makes, presents or dismisses a controller
//! runs where the exception is caught (`guarded`), which logs its name and
//! reason and answers a typed refusal ([`Refusal::Raised`]) instead.

use std::cell::Cell;
use std::rc::Rc;

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
    /// The window's chain of controllers did not settle, or UIKit took
    /// nothing when asked; asking again later may succeed.
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

/// What a window's chain of presented controllers ends in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Top<C> {
    /// The last controller of the chain, which presents nothing: the one to
    /// present from.
    Presenter(C),
    /// A controller of the chain that is being dismissed: UIKit presents
    /// over neither it nor the controller that presented it until it has
    /// left.
    Leaving(C),
}

/// What the chain that starts at `root` and follows `presented` ends in:
/// its last controller, or the first one that is `leaving`.
pub(super) fn topmost<C>(
    root: C,
    presented: impl Fn(&C) -> Option<C>,
    leaving: impl Fn(&C) -> bool,
) -> Top<C> {
    let mut presenter = root;
    while let Some(next) = presented(&presenter) {
        if leaving(&next) {
            return Top::Leaving(next);
        }
        presenter = next;
    }
    Top::Presenter(presenter)
}

/// The chain of controllers a window presents, as a presentation asks it.
pub(super) trait Chain<C> {
    /// What the chain ends in now; [`Refusal::NoWindow`] for a window with
    /// no root controller.
    fn top(&self) -> Result<Top<C>, Refusal>;

    /// Run `then` once `leaving` has left the chain. `false` when its
    /// leaving has no end to hear; `then` is then dropped unrun.
    fn after(&self, leaving: &C, then: Box<dyn FnOnce()>) -> bool;
}

/// How many dismissals in a row a presentation waits out before it answers
/// that the window is busy.
pub(super) const MAX_DISMISSALS_AWAITED: u32 = 4;

/// Hand `then` the controller of `chain` to present from: at once when the
/// chain ends in one, and otherwise after the controller that is leaving
/// has left, looking again each time, [`MAX_DISMISSALS_AWAITED`] times at
/// most. `then` hears [`Refusal::Busy`] for a chain that did not settle or
/// a leaving nobody can hear the end of, and is called once, unless a
/// transition it waits on never ends: it is then dropped with it.
pub(super) fn when_settled<C: 'static>(
    chain: Rc<dyn Chain<C>>,
    then: impl FnOnce(Result<C, Refusal>) + 'static,
) {
    settle(chain, MAX_DISMISSALS_AWAITED, Box::new(then));
}

/// [`when_settled`], with `waits_left` dismissals still to wait out.
fn settle<C: 'static>(
    chain: Rc<dyn Chain<C>>,
    waits_left: u32,
    then: Box<dyn FnOnce(Result<C, Refusal>)>,
) {
    let leaving = match chain.top() {
        Err(refusal) => return then(Err(refusal)),
        Ok(Top::Presenter(presenter)) => return then(Ok(presenter)),
        Ok(Top::Leaving(leaving)) => leaving,
    };
    let Some(waits_left) = waits_left.checked_sub(1) else {
        return then(Err(Refusal::Busy));
    };
    // Shared with the wait, which may drop it unrun.
    let then = Rc::new(Cell::new(Some(then)));
    let resumed = Rc::clone(&then);
    let again = Rc::clone(&chain);
    let waiting = chain.after(
        &leaving,
        Box::new(move || {
            if let Some(then) = resumed.take() {
                settle(again, waits_left, then);
            }
        }),
    );
    if !waiting && let Some(then) = then.take() {
        then(Err(Refusal::Busy));
    }
}

/// The controller to ask to dismiss `controller`: the one `presenting` it,
/// which takes `controller` down with whatever it presents. `None` for a
/// controller nobody presents, or one already `leaving`: it has dismissed
/// itself, and asking again would close whatever its presenter shows next.
pub(super) fn dismissed_through<C: ?Sized, P>(
    controller: &C,
    presenting: impl FnOnce(&C) -> Option<P>,
    leaving: impl FnOnce(&C) -> bool,
) -> Option<P> {
    if leaving(controller) {
        return None;
    }
    presenting(controller)
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
pub(super) use uikit::{dismiss, guarded, present, when_presentable};

#[cfg(target_os = "ios")]
#[expect(unsafe_code, reason = "a raw window handle's view")]
mod uikit {
    use std::cell::Cell;
    use std::ptr::NonNull;
    use std::rc::Rc;

    use block2::RcBlock;
    use flui_platform::capability::WindowRef;
    use objc2::MainThreadMarker;
    use objc2::rc::Retained;
    use objc2::runtime::ProtocolObject;
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};
    use objc2_ui_kit::{
        UIPopoverArrowDirection, UIView, UIViewController, UIViewControllerTransitionCoordinator,
        UIViewControllerTransitionCoordinatorContext,
    };
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    use super::{Chain, Refusal, Top, dismissed_through, middle, topmost, when_settled};
    use crate::owner::panic_boundary::{AfterPanic, os_entry};

    /// Run UIKit calls that may raise an Objective-C exception while
    /// `doing` something: [`Refusal::Raised`] when one did, its name and
    /// reason logged. A Rust panic in `calls` passes through unchanged.
    pub(in crate::ios) fn guarded<R>(
        doing: &'static str,
        calls: impl FnOnce() -> R,
    ) -> Result<R, Refusal> {
        objc2::exception::catch(std::panic::AssertUnwindSafe(calls)).map_err(|exception| {
            // An `NSException` is written as its name and reason; the
            // contract carries no code for one.
            tracing::error!(doing, ?exception, "UIKit raised an Objective-C exception");
            Refusal::Raised
        })
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

    /// The controllers `window` presents.
    struct WindowChain {
        window: WindowRef,
        mtm: MainThreadMarker,
    }

    impl Chain<Retained<UIViewController>> for WindowChain {
        fn top(&self) -> Result<Top<Retained<UIViewController>>, Refusal> {
            let root = root_view_controller(&self.window, self.mtm).ok_or(Refusal::NoWindow)?;
            Ok(topmost(
                root,
                |controller| controller.presentedViewController(),
                |controller| controller.isBeingDismissed(),
            ))
        }

        fn after(&self, leaving: &Retained<UIViewController>, then: Box<dyn FnOnce()>) -> bool {
            // A dismissal that is not animated, or is over, has no
            // transition to hear the end of.
            let Some(transition) = leaving.transitionCoordinator() else {
                return false;
            };
            // Taken by the one completion UIKit calls, so it runs, and is
            // dropped, inside the boundary.
            let then = Cell::new(Some(then));
            let completion = RcBlock::new(
                move |_context: NonNull<
                    ProtocolObject<dyn UIViewControllerTransitionCoordinatorContext>,
                >| {
                    os_entry(
                        "transition coordinator completion",
                        (),
                        AfterPanic::Answer(|()| ()),
                        || {
                            if let Some(then) = then.take() {
                                then();
                            }
                        },
                    );
                },
            );
            transition.animateAlongsideTransition_completion(None, Some(&completion))
        }
    }

    /// Hand `then` the controller to present from on `window`, once its
    /// chain of controllers has settled (see the module): at once, or after
    /// a dismissal that is under way.
    pub(in crate::ios) fn when_presentable(
        window: &WindowRef,
        mtm: MainThreadMarker,
        then: impl FnOnce(Result<Retained<UIViewController>, Refusal>) + 'static,
    ) {
        let chain = WindowChain {
            window: window.clone(),
            mtm,
        };
        when_settled(Rc::new(chain), then);
    }

    /// Anchor the popover UIKit presents `controller` in, when it presents
    /// it in one, in the middle of `presenter`'s view with no arrow (see
    /// the module).
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

    /// Present `controller` from `presenter`, animated, and anchor its
    /// popover once UIKit took it. [`Refusal::Busy`] when UIKit took
    /// nothing (see the module), [`Refusal::Raised`] when it raised; after
    /// either nothing of the presentation keeps `controller`.
    pub(in crate::ios) fn present(
        presenter: &UIViewController,
        controller: &UIViewController,
    ) -> Result<(), Refusal> {
        let taken = guarded("presenting a controller", || {
            presenter.presentViewController_animated_completion(controller, true, None);
            let taken = presenter
                .presentedViewController()
                .is_some_and(|presented| {
                    std::ptr::eq(Retained::as_ptr(&presented), std::ptr::from_ref(controller))
                });
            if taken {
                anchor_popover(controller, presenter);
            }
            taken
        })?;
        if taken { Ok(()) } else { Err(Refusal::Busy) }
    }

    /// Dismiss `controller` and whatever it presents, through the
    /// controller that presents it (see the module). A dismissal UIKit
    /// raises on dismisses nothing.
    pub(in crate::ios) fn dismiss(controller: &UIViewController) {
        // Nobody is left to tell of a refusal: the dialog has ended for its
        // caller.
        let _ = guarded("dismissing a controller", || {
            let presenter = dismissed_through(
                controller,
                UIViewController::presentingViewController,
                UIViewController::isBeingDismissed,
            );
            if let Some(presenter) = presenter {
                presenter.dismissViewControllerAnimated_completion(true, None);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use flui_platform::{SystemFailure, Unsupported, UnsupportedReason};

    use super::{
        Chain, MAX_DISMISSALS_AWAITED, Refusal, Top, dismissed_through, middle, topmost,
        when_settled,
    };
    use crate::dialog::{DialogError, FileDialogCapability, MessageBoxCapability};
    use crate::share::{ShareCapability, ShareError};

    /// A window's controllers by index, each presenting the next, as UIKit
    /// keeps them: `leaving` marks the ones being dismissed, and a
    /// dismissal asked of one takes down everything above it.
    #[derive(Default)]
    struct Stack {
        /// Whether each controller, the root first, is being dismissed. An
        /// empty stack is a window with no root controller.
        leaving: RefCell<Vec<bool>>,
        /// Whether a leaving controller's transition can be heard.
        hears: Cell<bool>,
        /// What waits for a transition to end.
        waiting: RefCell<Vec<Box<dyn FnOnce()>>>,
    }

    impl Stack {
        fn of(leaving: &[bool]) -> Rc<Self> {
            Rc::new(Self {
                leaving: RefCell::new(leaving.to_vec()),
                hears: Cell::new(true),
                waiting: RefCell::default(),
            })
        }

        /// UIKit's rule: the controller asked dismisses what it presents,
        /// and everything above that.
        fn dismiss_asked_of(&self, controller: usize) {
            self.leaving.borrow_mut().truncate(controller + 1);
        }

        /// The transition of the lowest leaving controller ends: it and
        /// everything above it are gone, and whoever waited hears.
        fn finish_transition(&self) {
            let first = self.leaving.borrow().iter().position(|leaving| *leaving);
            if let Some(first) = first {
                self.leaving.borrow_mut().truncate(first);
            }
            let waiting = std::mem::take(&mut *self.waiting.borrow_mut());
            for then in waiting {
                then();
            }
        }
    }

    impl Chain<usize> for Stack {
        fn top(&self) -> Result<Top<usize>, Refusal> {
            let leaving = self.leaving.borrow();
            if leaving.is_empty() {
                return Err(Refusal::NoWindow);
            }
            Ok(topmost(
                0,
                |&controller| (controller + 1 < leaving.len()).then_some(controller + 1),
                |&controller| leaving[controller],
            ))
        }

        fn after(&self, _leaving: &usize, then: Box<dyn FnOnce()>) -> bool {
            if self.hears.get() {
                self.waiting.borrow_mut().push(then);
            }
            self.hears.get()
        }
    }

    type Heard = Rc<RefCell<Vec<Result<usize, Refusal>>>>;

    /// Ask `stack` for its presenter; what the request heard, each time.
    fn ask(stack: &Rc<Stack>) -> Heard {
        let heard = Heard::default();
        let into = Rc::clone(&heard);
        let chain: Rc<dyn Chain<usize>> = Rc::clone(stack) as Rc<dyn Chain<usize>>;
        when_settled(chain, move |presenter| into.borrow_mut().push(presenter));
        heard
    }

    #[test]
    fn a_presentation_starts_from_the_last_controller_of_the_chain() {
        assert_eq!(*ask(&Stack::of(&[false])).borrow(), [Ok(0)], "the root");
        assert_eq!(
            *ask(&Stack::of(&[false, false, false])).borrow(),
            [Ok(2)],
            "the last of the chain presents"
        );
        assert_eq!(
            *ask(&Stack::of(&[])).borrow(),
            [Err(Refusal::NoWindow)],
            "a window with no root controller"
        );
        assert_eq!(
            topmost(0, |_| None::<usize>, |_| true),
            Top::Presenter(0),
            "the root is never being dismissed; only a presented one is asked"
        );
    }

    #[test]
    fn a_request_made_while_a_controller_leaves_waits_for_it_and_presents_then() {
        // A dialog asked for in another's answer: the other's controller is
        // still animating away.
        let stack = Stack::of(&[false, true]);
        let heard = ask(&stack);
        assert!(heard.borrow().is_empty(), "nothing is presented over it");
        stack.finish_transition();
        assert_eq!(*heard.borrow(), [Ok(0)], "presented once it has left");
        stack.finish_transition();
        assert_eq!(heard.borrow().len(), 1, "and heard once");

        // A controller leaving under one it presents takes that one along.
        let stack = Stack::of(&[false, true, false]);
        let heard = ask(&stack);
        stack.finish_transition();
        assert_eq!(*heard.borrow(), [Ok(0)]);

        // Another dismissal began before the first ended.
        let stack = Stack::of(&[false, false, true]);
        let heard = ask(&stack);
        stack.leaving.borrow_mut()[1] = true;
        stack.finish_transition();
        assert_eq!(*heard.borrow(), [Ok(0)]);
    }

    #[test]
    fn a_chain_that_does_not_settle_answers_busy_and_never_waits_for_good() {
        // A dismissal with no transition to hear the end of.
        let stack = Stack::of(&[false, true]);
        stack.hears.set(false);
        assert_eq!(*ask(&stack).borrow(), [Err(Refusal::Busy)]);
        assert!(stack.waiting.borrow().is_empty());

        // Every transition's end finds another controller leaving.
        let stack = Stack::of(&[false, true]);
        let heard = ask(&stack);
        for _ in 0..MAX_DISMISSALS_AWAITED {
            assert!(heard.borrow().is_empty());
            let waiting = std::mem::take(&mut *stack.waiting.borrow_mut());
            assert_eq!(waiting.len(), 1, "one wait at a time");
            for then in waiting {
                then();
            }
        }
        assert_eq!(
            *heard.borrow(),
            [Err(Refusal::Busy)],
            "the waits are bounded"
        );
        assert!(stack.waiting.borrow().is_empty());

        // A transition that never ends drops the request with its wait:
        // whatever the request held (its caller's resolver) goes with it.
        let stack = Stack::of(&[false, true]);
        let held = Rc::new(());
        let holds = Rc::clone(&held);
        let chain: Rc<dyn Chain<usize>> = Rc::clone(&stack) as Rc<dyn Chain<usize>>;
        when_settled(chain, move |_| drop(holds));
        assert_eq!(Rc::strong_count(&held), 2, "held while it waits");
        stack.waiting.borrow_mut().clear();
        assert_eq!(Rc::strong_count(&held), 1, "dropped with the wait");
    }

    #[test]
    fn a_dismissal_goes_through_the_presenter_so_the_controller_itself_leaves() {
        let presenting = |controller: &usize| controller.checked_sub(1);
        // A share sheet (2) over a picker (1) over the root: the picker's
        // dialog ends.
        let stack = Stack::of(&[false, false, false]);
        let through = dismissed_through(&1, presenting, |_| false);
        assert_eq!(through, Some(0), "asked of the controller that presents it");
        stack.dismiss_asked_of(through.expect("a presenter"));
        assert_eq!(
            stack.leaving.borrow().len(),
            1,
            "the picker goes, and the sheet over it: asked of the picker itself, UIKit would \
             dismiss the sheet alone"
        );
        assert_eq!(
            dismissed_through(&0, presenting, |_| false),
            None,
            "a controller nobody presents"
        );
        assert_eq!(
            dismissed_through(&1, presenting, |_| true),
            None,
            "one already leaving has dismissed itself"
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
