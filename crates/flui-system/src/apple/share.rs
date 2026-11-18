//! What AppKit's share picker and UIKit's share sheet are handed: the text
//! as a string, the link as a URL and each file as a file URL, in that
//! order, so the system offers the services that take all of them. Each
//! file is checked to exist off the main thread first (`queue::WorkQueue`).

use std::path::PathBuf;

use super::{ErrorCode, errno, failure};
use crate::share::{ShareError, ShareRequest};

/// One item a share is handed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ShareItem {
    /// Text, as a string.
    Text(String),
    /// A link, as a URL.
    Link(String),
    /// A file, as a file URL.
    File(PathBuf),
}

/// What a share is handed for `request`, in order: the text, the link,
/// then each file.
pub(crate) fn items(request: &ShareRequest) -> Vec<ShareItem> {
    let text = request.text.iter().cloned().map(ShareItem::Text);
    let link = request
        .uri
        .iter()
        .map(|uri| ShareItem::Link(uri.as_str().to_owned()));
    let files = request.files.iter().cloned().map(ShareItem::File);
    text.chain(link).chain(files).collect()
}

/// The error checking a shared file answered.
pub(crate) fn file_error(code: ErrorCode) -> ShareError {
    match code {
        ErrorCode::Posix(errno::ENOENT | errno::ENOTDIR) => ShareError::NotFound,
        code => ShareError::Failed(failure(code)),
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) use foundation::{check_files, objects};

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod foundation {
    use std::path::PathBuf;

    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_foundation::{NSArray, NSString, NSURL};

    use super::super::{ErrorCode, file_url};
    use super::{ShareItem, file_error};
    use crate::share::ShareError;

    /// Whether every file of `files` exists: what a work item checks before
    /// the share shows anything. Off the main thread only.
    pub(crate) fn check_files(files: &[PathBuf]) -> Result<(), ShareError> {
        for file in files {
            std::fs::metadata(file)
                .map_err(|error| file_error(ErrorCode::Posix(error.raw_os_error().unwrap_or(0))))?;
        }
        Ok(())
    }

    /// `items` as the objects a share takes; `None` when a link is no URL
    /// Foundation reads or a file's path is none, which a share answers as
    /// [`ShareError::NotFound`].
    pub(crate) fn objects(items: &[ShareItem]) -> Option<Retained<NSArray<AnyObject>>> {
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
                        Retained::into_super(Retained::into_super(file_url(file, false)?))
                    }
                })
            })
            .collect::<Option<Vec<Retained<AnyObject>>>>()?;
        Some(NSArray::from_retained_slice(&objects))
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
    fn a_share_is_handed_the_text_the_link_then_each_file() {
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
