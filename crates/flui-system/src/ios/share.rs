//! Sharing through `UIActivityViewController`, the share sheet, presented
//! from the requesting window's top controller (`presenter`).
//!
//! The sheet is handed what `apple::share` makes of the request (the text
//! as a string, the link as a URL and each file as a file URL), each file
//! checked off the main thread first, and is presented at the owner turn
//! that answer settles at, unless the window closed meanwhile. On an iPad
//! the sheet is a popover, anchored in the middle of the presenter's view.
//! The share answers once UIKit took the sheet; which service the user
//! picks, or whether they close it (a swipe, a tap outside the popover), is
//! not reported, as the contract allows. A sheet still up when its window
//! closes is dismissed. The sheet shows no title, so the request's title is
//! not shown.

#[cfg(target_os = "ios")]
pub(crate) use uikit::IOSShare;

#[cfg(target_os = "ios")]
#[expect(
    unsafe_code,
    reason = "the share sheet's initializer and completion handler"
)]
mod uikit {
    use std::cell::Cell;
    use std::rc::Rc;

    use block2::RcBlock;
    use flui_platform::capability::{CompletionSource, Resolver, WindowRef};
    use flui_platform::{Unsupported, UnsupportedReason};
    use objc2::rc::Weak;
    use objc2::runtime::Bool;
    use objc2::{MainThreadMarker, MainThreadOnly as _};
    use objc2_foundation::{NSArray, NSError};
    use objc2_ui_kit::{UIActivityType, UIActivityViewController};

    use super::super::presenter::{Refusal, dismiss, present, presenter};
    use crate::apple::queue::WorkQueue;
    use crate::apple::share::{ShareItem, check_files, items, objects};
    use crate::owner::panic_boundary::{AfterPanic, os_entry};
    use crate::share::{ShareBackend, ShareCapability, ShareError, ShareRequest, ShareResult};

    /// [`ShareBackend`] over the share sheet.
    pub(crate) struct IOSShare {
        mtm: MainThreadMarker,
        queue: Rc<WorkQueue>,
        completions: CompletionSource,
    }

    impl IOSShare {
        pub(crate) fn new(
            mtm: MainThreadMarker,
            queue: Rc<WorkQueue>,
            completions: CompletionSource,
        ) -> Self {
            Self {
                mtm,
                queue,
                completions,
            }
        }
    }

    fn refused(reason: UnsupportedReason) -> ShareError {
        ShareError::Unsupported(Unsupported::of::<ShareCapability>(reason))
    }

    /// Present the sheet with `items` over `window`, dismissed when the
    /// window closes while it is up.
    fn show(mtm: MainThreadMarker, window: &WindowRef, items: &[ShareItem]) -> ShareResult {
        let from = presenter(window, mtm).map_err(|refusal| match refusal {
            Refusal::NoWindow => refused(UnsupportedReason::NoWindow),
            Refusal::Busy => refused(UnsupportedReason::Unavailable),
        })?;
        let objects = objects(items).ok_or(ShareError::NotFound)?;
        // SAFETY: the items are strings and URLs, which the sheet takes, and
        // the controller is made on the main thread.
        let sheet = unsafe {
            UIActivityViewController::initWithActivityItems_applicationActivities(
                UIActivityViewController::alloc(mtm),
                &objects,
                None,
            )
        };
        let closing = Weak::from_retained(&sheet);
        let watch = window.lifetime().on_close(move || {
            if let Some(sheet) = closing.load() {
                dismiss(&sheet);
            }
        });
        // Held until the sheet ends, by the handler the sheet keeps: a
        // sheet UIKit releases unended releases the watch with it.
        let watched = Rc::new(Cell::new(Some(watch)));
        let completion = RcBlock::new(
            move |_service: *mut UIActivityType,
                  _completed: Bool,
                  _returned: *mut NSArray,
                  _error: *mut NSError| {
                os_entry(
                    "UIActivityViewController completion handler",
                    (),
                    AfterPanic::Answer(|()| ()),
                    || drop(watched.take()),
                );
            },
        );
        // SAFETY: the block is valid for the call, and the sheet copies it.
        unsafe { sheet.setCompletionWithItemsHandler(RcBlock::as_ptr(&completion)) };
        if present(&from, &sheet) {
            Ok(())
        } else {
            // UIKit presented nothing: the top controller was no longer one
            // to present from.
            Err(refused(UnsupportedReason::Unavailable))
        }
    }

    impl ShareBackend for IOSShare {
        fn share(&self, window: &WindowRef, request: ShareRequest, answer: Resolver<ShareResult>) {
            let items = items(&request);
            let files = request.files;
            let (checked, check) = self.completions.pending::<ShareResult>();
            let busy = Err(refused(UnsupportedReason::Unavailable));
            self.queue.run(check, busy, move || check_files(&files));
            let mtm = self.mtm;
            let window = window.clone();
            let mut answer = Some(answer);
            checked.on_complete(move |outcome| {
                let Some(answer) = answer.take() else {
                    return;
                };
                let result = match outcome {
                    Ok(Ok(())) if window.lifetime().is_closed() => Err(ShareError::WindowClosed),
                    Ok(Ok(())) => show(mtm, &window, &items),
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
