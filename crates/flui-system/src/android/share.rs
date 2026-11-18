//! Sharing through `ACTION_SEND` and Android's chooser.
//!
//! The text and the link travel together as `EXTRA_TEXT`, the link on a
//! line of its own, which is how Android applications share a page; the
//! title is the chooser's and the content's subject. Each file is opened
//! first, on a worker, and only a regular file is shared: the open file is
//! offered through the crate's content provider (`FluiDocuments`) under a
//! URI that names a random token, and the intent's clip carries the read
//! grant, which the chooser hands to the target the user picks and to no
//! other application. Several files are `ACTION_SEND_MULTIPLE`, typed by
//! what their types have in common.
//!
//! The chooser starts on Java's main thread, and the share answers once it
//! is up; which target the user picks, or whether they leave, is not
//! reported, as the contract allows.

use flui_platform::launch::ExternalUri;
use flui_platform::{SystemFailure, Unsupported, UnsupportedReason};

use super::workers::NotRun;
use super::{StartAnswer, UNNAMED_FAILURE};
use crate::share::{ShareCapability, ShareError, ShareRequest, ShareResult};

/// `ENOENT` and `ENOTDIR`, on Linux and so on Android.
const ENOENT: i32 = 2;
const ENOTDIR: i32 = 20;

fn refused(reason: UnsupportedReason) -> ShareError {
    ShareError::Unsupported(Unsupported::of::<ShareCapability>(reason))
}

/// What `EXTRA_TEXT` carries for `request`: its text, then its link on a
/// line of its own; `None` when it has neither.
pub(crate) fn extra_text(request: &ShareRequest) -> Option<String> {
    let link = request.uri.as_ref().map(ExternalUri::as_str);
    match (request.text.as_deref(), link) {
        (Some(text), Some(link)) => Some(format!("{text}\n{link}")),
        (Some(only), None) | (None, Some(only)) => Some(only.to_owned()),
        (None, None) => None,
    }
}

/// The MIME type a share of files of `types` states: the one type they all
/// are, else the family they share (`image/*`), else every type; `None` for
/// no file.
pub(crate) fn common_type<'a>(types: impl IntoIterator<Item = &'a str>) -> Option<String> {
    types.into_iter().fold(None, |common, mime_type| {
        let Some(common) = common else {
            return Some(mime_type.to_owned());
        };
        if common == mime_type {
            return Some(common);
        }
        let family = |of: &str| of.split_once('/').map(|(family, _)| family.to_owned());
        Some(match (family(&common), family(mime_type)) {
            (Some(one), Some(other)) if one == other && one != "*" => format!("{one}/*"),
            _ => "*/*".to_owned(),
        })
    })
}

/// The refusal of a shared file the system could not open, by its `errno`.
pub(crate) fn file_error(errno: Option<i32>) -> ShareError {
    match errno {
        Some(ENOENT | ENOTDIR) => ShareError::NotFound,
        Some(code) => ShareError::Failed(SystemFailure::Os(code)),
        None => ShareError::Failed(SystemFailure::MalformedAnswer),
    }
}

/// The refusal of a shared path that holds no regular file: Android shares
/// a file's contents, and a folder or a device has none to hand over.
pub(crate) fn not_a_file() -> ShareError {
    refused(UnsupportedReason::NotOnThisPlatform)
}

/// What a share answers for what starting the chooser did.
pub(crate) fn start_result(started: StartAnswer) -> ShareResult {
    match started {
        StartAnswer::Started => Ok(()),
        // The chooser is the system's own: a device without one shares
        // nothing.
        StartAnswer::NoHandler => Err(refused(UnsupportedReason::NotOnThisDevice)),
        StartAnswer::Denied | StartAnswer::Failed => Err(ShareError::Failed(UNNAMED_FAILURE)),
    }
}

/// The share's answer when the request never reached Android.
pub(crate) fn unavailable() -> ShareError {
    refused(UnsupportedReason::Unavailable)
}

/// What a worker's refusal answers a share.
pub(crate) fn not_run<T>(reason: NotRun) -> Result<T, ShareError> {
    Err(match reason {
        NotRun::Busy => unavailable(),
        NotRun::NoThread(code) => file_error(code),
    })
}

#[cfg(target_os = "android")]
pub(crate) use provider::AndroidShare;

#[cfg(target_os = "android")]
mod provider {
    use std::fs::File;
    use std::path::PathBuf;
    use std::rc::Rc;
    use std::sync::Arc;

    use flui_platform::capability::{Activity, CompletionSource, Resolver, WindowRef};

    use super::super::java::{Java, Waiting};
    use super::super::workers::Workers;
    use super::{extra_text, file_error, not_a_file, not_run, start_result, unavailable};
    use crate::share::{ShareBackend, ShareError, ShareRequest, ShareResult};

    /// [`ShareBackend`] over `ACTION_SEND`.
    pub(crate) struct AndroidShare {
        activity: Activity,
        java: Arc<Java>,
        workers: Rc<Workers>,
        completions: CompletionSource,
    }

    impl AndroidShare {
        pub(crate) fn new(
            activity: Activity,
            java: Arc<Java>,
            workers: Rc<Workers>,
            completions: CompletionSource,
        ) -> Self {
            Self {
                activity,
                java,
                workers,
                completions,
            }
        }
    }

    /// Each of `files` opened, with the name it is shared under.
    fn open_files(files: &[PathBuf]) -> Result<Vec<(File, String)>, ShareError> {
        files
            .iter()
            .map(|path| {
                let io_error = |error: std::io::Error| file_error(error.raw_os_error());
                let file = File::open(path).map_err(io_error)?;
                if !file.metadata().map_err(io_error)?.is_file() {
                    return Err(not_a_file());
                }
                let name = path.file_name().ok_or_else(not_a_file)?;
                Ok((file, name.to_string_lossy().into_owned()))
            })
            .collect()
    }

    /// Show the chooser with `request`'s text and `files` and answer
    /// `answer` with what its start did.
    fn show(
        java: &Arc<Java>,
        activity: &Activity,
        request: &ShareRequest,
        files: Vec<(File, String)>,
        answer: Resolver<ShareResult>,
    ) {
        let waiting = Waiting::new(answer);
        let heard = waiting.clone();
        let posted = java.share(
            activity,
            request.title.clone(),
            extra_text(request),
            files,
            move |started| heard.answer(start_result(started)),
        );
        if !posted {
            waiting.answer(Err(unavailable()));
        }
    }

    impl ShareBackend for AndroidShare {
        fn share(&self, window: &WindowRef, request: ShareRequest, answer: Resolver<ShareResult>) {
            if request.files.is_empty() {
                show(&self.java, &self.activity, &request, Vec::new(), answer);
                return;
            }
            let files = request.files.clone();
            let (opened, opening) = self.completions.pending();
            self.workers
                .run("flui-system-share", opening, not_run, move || {
                    open_files(&files)
                });
            let activity = self.activity.clone();
            let java = Arc::clone(&self.java);
            let window = window.clone();
            let mut answer = Some(answer);
            opened.on_complete(move |outcome| {
                let Some(answer) = answer.take() else {
                    return;
                };
                match outcome {
                    // The owner retiring first leaves nothing waiting.
                    Ok(Ok(_)) if window.lifetime().is_closed() => {
                        let _ = answer.resolve(Err(ShareError::WindowClosed));
                    }
                    Ok(Ok(files)) => show(&java, &activity, &request, files, answer),
                    Ok(Err(refusal)) => drop(answer.resolve(Err(refusal))),
                    // The worker panicked: the caller's completion ends as
                    // its own did.
                    Err(_) => {}
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use flui_platform::launch::{ExternalUri, Scheme};
    use flui_platform::{SystemFailure, UnsupportedReason};

    use super::super::StartAnswer;
    use super::super::workers::NotRun;
    use super::{common_type, extra_text, file_error, not_a_file, not_run, start_result};
    use crate::share::{ShareError, ShareRequest};

    /// Several files are typed by what their types share, so the chooser
    /// offers the targets that take all of them.
    #[test]
    fn several_files_are_typed_by_what_their_types_have_in_common() {
        for (types, expected) in [
            (vec![], None),
            (vec!["image/png"], Some("image/png")),
            (vec!["image/png", "image/png"], Some("image/png")),
            (vec!["image/png", "image/jpeg"], Some("image/*")),
            (
                vec!["image/png", "image/jpeg", "image/png"],
                Some("image/*"),
            ),
            (vec!["image/png", "application/pdf"], Some("*/*")),
            (vec!["image/png", "image/jpeg", "text/plain"], Some("*/*")),
            (vec!["image/png", "text/plain", "text/html"], Some("*/*")),
        ] {
            assert_eq!(
                common_type(types.iter().copied()).as_deref(),
                expected,
                "{types:?}"
            );
        }
    }

    fn reason(error: ShareError) -> Option<UnsupportedReason> {
        match error {
            ShareError::Unsupported(unsupported) => Some(unsupported.reason),
            _ => None,
        }
    }

    #[test]
    fn the_text_and_the_link_travel_together_the_link_on_its_own_line() {
        let uri = ExternalUri::try_new("https://example.com/a", &[Scheme::HTTPS]).expect("a URI");
        let both = ShareRequest::new().text("Look").uri(uri.clone());
        assert_eq!(
            extra_text(&both).as_deref(),
            Some("Look\nhttps://example.com/a")
        );
        assert_eq!(
            extra_text(&ShareRequest::new().text("Look")).as_deref(),
            Some("Look")
        );
        assert_eq!(
            extra_text(&ShareRequest::new().uri(uri)).as_deref(),
            Some("https://example.com/a")
        );
        assert_eq!(extra_text(&ShareRequest::new().file("/a/b.pdf")), None);
    }

    #[test]
    fn a_share_answers_what_the_file_and_the_chooser_said() {
        assert_eq!(file_error(Some(2)), ShareError::NotFound);
        assert_eq!(file_error(Some(20)), ShareError::NotFound);
        assert_eq!(
            file_error(Some(13)),
            ShareError::Failed(SystemFailure::Os(13))
        );
        assert_eq!(
            reason(not_a_file()),
            Some(UnsupportedReason::NotOnThisPlatform)
        );
        assert_eq!(start_result(StartAnswer::Started), Ok(()));
        assert_eq!(
            start_result(StartAnswer::NoHandler).map_err(reason),
            Err(Some(UnsupportedReason::NotOnThisDevice))
        );
        assert_eq!(
            start_result(StartAnswer::Denied),
            Err(ShareError::Failed(SystemFailure::MalformedAnswer))
        );
        assert_eq!(
            not_run::<()>(NotRun::Busy).map_err(reason),
            Err(Some(UnsupportedReason::Unavailable))
        );
    }
}
