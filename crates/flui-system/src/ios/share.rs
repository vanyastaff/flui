//! Sharing through `UIActivityViewController`, the share sheet, presented
//! from the requesting window's top controller (`presenter`).
//!
//! The sheet is handed what `apple::share` makes of the request (the text
//! as a string, the link as a URL and each file as a file URL), each file
//! checked off the main thread first, and is presented at the owner turn
//! that answer settles at, or once a controller of the window that is
//! still leaving has left, unless the window closed meanwhile. On an iPad
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
    use objc2::rc::{Retained, Weak};
    use objc2::runtime::{AnyObject, Bool};
    use objc2::{MainThreadMarker, MainThreadOnly as _};
    use objc2_foundation::{NSArray, NSError};
    use objc2_ui_kit::{UIActivityType, UIActivityViewController, UIViewController};

    use super::super::presenter::{Refusal, dismiss, guarded, present, when_presentable};
    use crate::apple::queue::WorkQueue;
    use crate::apple::share::{objects, share_checked};
    use crate::owner::panic_boundary::{AfterPanic, os_entry};
    use crate::share::{ShareBackend, ShareError, ShareRequest, ShareResult};

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

    /// Present the sheet with `objects` from `from`, on `window`, and
    /// dismiss it when the window closes while it is up.
    fn present_sheet(
        mtm: MainThreadMarker,
        window: &WindowRef,
        objects: &NSArray<AnyObject>,
        from: &UIViewController,
    ) -> Result<(), Refusal> {
        let sheet = guarded("making a share sheet", || {
            // SAFETY: the items are strings and URLs, which the sheet takes,
            // and the controller is made on the main thread.
            unsafe {
                UIActivityViewController::initWithActivityItems_applicationActivities(
                    UIActivityViewController::alloc(mtm),
                    objects,
                    None,
                )
            }
        })?;
        // A sheet UIKit did not take holds nothing of the request: the
        // watch is made only once it is up.
        present(from, &sheet)?;
        let closing = Weak::from_retained(&sheet);
        let watch = window.lifetime().on_close(move || {
            if let Some(sheet) = closing.load() {
                dismiss(&sheet);
            }
        });
        // Held by the handler the sheet keeps, and taken when the sheet
        // ends, so it is dropped inside the boundary; a sheet UIKit
        // releases unended releases it with the handler.
        let watch = Cell::new(Some(watch));
        let completion = RcBlock::new(
            move |_service: *mut UIActivityType,
                  _completed: Bool,
                  _returned: *mut NSArray,
                  _error: *mut NSError| {
                os_entry(
                    "UIActivityViewController completion handler",
                    (),
                    AfterPanic::Answer(|()| ()),
                    || drop(watch.take()),
                );
            },
        );
        // SAFETY: the block is valid for the call, and the sheet copies it.
        unsafe { sheet.setCompletionWithItemsHandler(RcBlock::as_ptr(&completion)) };
        Ok(())
    }

    /// Show the sheet with `objects` on `window` once its controllers have
    /// settled, and answer `answer` when UIKit took it, or why it did not.
    fn show(
        mtm: MainThreadMarker,
        window: &WindowRef,
        objects: Retained<NSArray<AnyObject>>,
        answer: Resolver<ShareResult>,
    ) {
        let shown_on = window.clone();
        when_presentable(window, mtm, move |from| {
            let result = if shown_on.lifetime().is_closed() {
                // It closed while a controller was leaving.
                Err(ShareError::WindowClosed)
            } else {
                from.and_then(|from| present_sheet(mtm, &shown_on, &objects, &from))
                    .map_err(Refusal::share_error)
            };
            // The owner retiring first leaves nothing waiting.
            let _ = answer.resolve(result);
        });
    }

    impl ShareBackend for IOSShare {
        fn share(&self, window: &WindowRef, request: ShareRequest, answer: Resolver<ShareResult>) {
            let mtm = self.mtm;
            share_checked(
                &self.queue,
                &self.completions,
                window,
                request,
                answer,
                move |window, items, answer| match objects(items) {
                    Some(objects) => show(mtm, window, objects, answer),
                    None => {
                        // The owner retiring first leaves nothing waiting.
                        let _ = answer.resolve(Err(ShareError::NotFound));
                    }
                },
            );
        }
    }
}
