//! Sharing through `NSSharingServicePicker`, the share menu AppKit shows as
//! a popover on the acquiring widget's window.
//!
//! The text is shared as a string, the link as a URL and each file as a
//! file URL, in that order; the picker offers the services that take all of
//! them. Each file is checked to exist off the main thread first
//! ([`WorkQueue`]), and the picker is shown at the owner turn that answer
//! settles at, unless the window closed meanwhile. The picker names no
//! title, so the request's title is not shown. The share answers once the
//! picker shows the content; which service the user picks, or whether they
//! close it, is not reported, as the contract allows.

use std::path::PathBuf;

use crate::share::{ShareError, ShareRequest};

use super::{ErrorCode, errno, failure};

/// One item the picker is handed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ShareItem {
    /// Text, as a string.
    Text(String),
    /// A link, as a URL.
    Link(String),
    /// A file, as a file URL.
    File(PathBuf),
}

/// What the picker is handed for `request`, in order: the text, the link,
/// then each file.
pub(super) fn items(request: &ShareRequest) -> Vec<ShareItem> {
    let text = request.text.iter().cloned().map(ShareItem::Text);
    let link = request
        .uri
        .iter()
        .map(|uri| ShareItem::Link(uri.as_str().to_owned()));
    let files = request.files.iter().cloned().map(ShareItem::File);
    text.chain(link).chain(files).collect()
}

/// The error checking a shared file answered.
pub(super) fn file_error(code: ErrorCode) -> ShareError {
    match code {
        ErrorCode::Posix(errno::ENOENT | errno::ENOTDIR) => ShareError::NotFound,
        code => ShareError::Failed(failure(code)),
    }
}

#[cfg(target_os = "macos")]
pub(crate) use appkit::MacShare;

#[cfg(target_os = "macos")]
#[expect(unsafe_code, reason = "the share picker's initializer")]
mod appkit {
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::rc::Rc;

    use flui_platform::capability::{CompletionSource, Resolver, WindowRef};
    use flui_platform::{Unsupported, UnsupportedReason};
    use objc2::{AnyThread as _, MainThreadMarker};
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_app_kit::NSSharingServicePicker;
    use objc2_foundation::{NSArray, NSPoint, NSRect, NSRectEdge, NSSize, NSString, NSURL};

    use super::super::ErrorCode;
    use super::super::appkit::{file_url, ns_window};
    use super::super::queue::WorkQueue;
    use super::{ShareItem, file_error, items};
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

    /// Whether every file of `files` exists.
    fn check_files(files: &[PathBuf]) -> Result<(), ShareError> {
        for file in files {
            std::fs::metadata(file)
                .map_err(|error| file_error(ErrorCode::Posix(error.raw_os_error().unwrap_or(0))))?;
        }
        Ok(())
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
        let objects = items
            .iter()
            .map(|item| -> Option<Retained<AnyObject>> {
                Some(match item {
                    ShareItem::Text(text) => {
                        Retained::into_super(Retained::into_super(NSString::from_str(text)))
                    }
                    ShareItem::Link(link) => Retained::into_super(Retained::into_super(
                        NSURL::URLWithString(&NSString::from_str(link))?,
                    )),
                    ShareItem::File(file) => {
                        Retained::into_super(Retained::into_super(file_url(file)?))
                    }
                })
            })
            .collect::<Option<Vec<Retained<AnyObject>>>>()
            .ok_or(ShareError::NotFound)?;
        let objects = NSArray::from_retained_slice(&objects);
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use flui_platform::SystemFailure;
    use flui_platform::launch::{ExternalUri, Scheme};

    use super::super::{ErrorCode, errno};
    use super::{ShareItem, file_error, items};
    use crate::share::{ShareError, ShareRequest};

    #[test]
    fn the_picker_is_handed_the_text_the_link_then_each_file() {
        let uri = ExternalUri::try_new("https://example.com/a", &[Scheme::HTTPS]).expect("a URI");
        let request = ShareRequest::new()
            .title("Not shown")
            .file("/Users/a/one.pdf")
            .text("Look")
            .uri(uri)
            .file("/Users/a/two.png");
        assert_eq!(
            items(&request),
            [
                ShareItem::Text("Look".to_owned()),
                ShareItem::Link("https://example.com/a".to_owned()),
                ShareItem::File(PathBuf::from("/Users/a/one.pdf")),
                ShareItem::File(PathBuf::from("/Users/a/two.png")),
            ]
        );
        assert_eq!(
            file_error(ErrorCode::Posix(errno::ENOENT)),
            ShareError::NotFound
        );
        assert_eq!(
            file_error(ErrorCode::Posix(errno::EACCES)),
            ShareError::Failed(SystemFailure::Os(errno::EACCES))
        );
    }
}
