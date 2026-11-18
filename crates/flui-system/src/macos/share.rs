//! Sharing through `NSSharingServicePicker`, the share menu AppKit shows as
//! a popover on the acquiring widget's window.
//!
//! The picker is handed what `apple::share` makes of the request (the text
//! as a string, the link as a URL and each file as a file URL), and offers
//! the services that take all of it. Each file is checked to exist off the
//! main thread first (`apple::queue::WorkQueue`), and the picker is shown
//! at the owner turn that answer settles at, unless the window closed
//! meanwhile. The picker names no title, so the request's title is not
//! shown. The share answers once the picker shows the content; which
//! service the user picks, or whether they close it, is not reported, as
//! the contract allows.

#[cfg(target_os = "macos")]
pub(crate) use appkit::MacShare;

#[cfg(target_os = "macos")]
#[expect(unsafe_code, reason = "the share picker's initializer")]
mod appkit {
    use std::cell::RefCell;
    use std::rc::Rc;

    use flui_platform::capability::{CompletionSource, Resolver, WindowRef};
    use flui_platform::{Unsupported, UnsupportedReason};
    use objc2::rc::Retained;
    use objc2::{AnyThread as _, MainThreadMarker};
    use objc2_app_kit::NSSharingServicePicker;
    use objc2_foundation::{NSPoint, NSRect, NSRectEdge, NSSize};

    use super::super::appkit::ns_window;
    use crate::apple::queue::WorkQueue;
    use crate::apple::share::{ShareItem, check_files, items, objects};
    use crate::share::{ShareBackend, ShareCapability, ShareError, ShareRequest, ShareResult};

    /// [`ShareBackend`] over the share picker.
    pub(crate) struct MacShare {
        mtm: MainThreadMarker,
        queue: Rc<WorkQueue>,
        completions: CompletionSource,
        /// The picker last shown, kept while its popover may be up.
        shown: Rc<RefCell<Option<Retained<NSSharingServicePicker>>>>,
    }

    impl MacShare {
        pub(crate) fn new(
            mtm: MainThreadMarker,
            queue: Rc<WorkQueue>,
            completions: CompletionSource,
        ) -> Self {
            Self {
                mtm,
                queue,
                completions,
                shown: Rc::default(),
            }
        }
    }

    fn refused(reason: UnsupportedReason) -> ShareError {
        ShareError::Unsupported(Unsupported::of::<ShareCapability>(reason))
    }

    /// Show the picker with `items` on `window`'s content view.
    fn show(
        mtm: MainThreadMarker,
        window: &WindowRef,
        items: &[ShareItem],
        shown: &RefCell<Option<Retained<NSSharingServicePicker>>>,
    ) -> ShareResult {
        let parent = ns_window(window, mtm).ok_or_else(|| refused(UnsupportedReason::NoWindow))?;
        let view = parent
            .contentView()
            .ok_or_else(|| refused(UnsupportedReason::NoWindow))?;
        let objects = objects(items).ok_or(ShareError::NotFound)?;
        // SAFETY: the items are strings and URLs, which the picker takes.
        let picker = unsafe {
            NSSharingServicePicker::initWithItems(NSSharingServicePicker::alloc(), &objects)
        };
        let bounds = view.bounds();
        let anchor = NSRect::new(
            NSPoint::new(
                bounds.origin.x + bounds.size.width / 2.0,
                bounds.origin.y + bounds.size.height / 2.0,
            ),
            NSSize::new(1.0, 1.0),
        );
        picker.showRelativeToRect_ofView_preferredEdge(anchor, &view, NSRectEdge::MinY);
        let replaced = shown.replace(Some(picker));
        drop(replaced);
        Ok(())
    }

    impl ShareBackend for MacShare {
        fn share(&self, window: &WindowRef, request: ShareRequest, answer: Resolver<ShareResult>) {
            let items = items(&request);
            let files = request.files;
            let (checked, check) = self.completions.pending::<ShareResult>();
            let busy = Err(refused(UnsupportedReason::Unavailable));
            self.queue.run(check, busy, move || check_files(&files));
            let mtm = self.mtm;
            let window = window.clone();
            let shown = Rc::clone(&self.shown);
            let mut answer = Some(answer);
            checked.on_complete(move |outcome| {
                let Some(answer) = answer.take() else {
                    return;
                };
                let result = match outcome {
                    Ok(Ok(())) if window.lifetime().is_closed() => Err(ShareError::WindowClosed),
                    Ok(Ok(())) => show(mtm, &window, &items, &shown),
                    Ok(Err(refusal)) => Err(refusal),
                    // The check's work item panicked: its caller's completion
                    // ends as the check's did.
                    Err(_) => return,
                };
                // The owner retiring first leaves nothing waiting.
                let _ = answer.resolve(result);
            });
        }
    }
}
