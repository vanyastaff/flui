//! Sharing through `ACTION_SEND` and Android's chooser.
//!
//! The text and the link travel together as `EXTRA_TEXT`, the link on a
//! line of its own, which is how Android applications share a page; the
//! title is the chooser's and the content's subject. Each file is opened
//! first, on a worker, without blocking, and only a regular file is shared,
//! under the name it ends at (`open_shared`): the open file is offered
//! through the crate's content provider (`FluiDocuments`) under a
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
use super::{StartAnswer, TOO_MANY_OFFERED, UNNAMED_FAILURE};
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

/// Each of `files` opened and judged ([`super::opened`]): only a regular
/// file is shared, under its final name, so a link named `notes.pdf` that
/// leads to `secret.db` goes out as `secret.db`, typed by that name, and a
/// named pipe is refused without holding the worker. Unlike a launch, a share
/// hands over contents, not something to run, so a file of any name is
/// shared.
///
/// # Errors
///
/// [`file_error`] for a path the system could not open, [`not_a_file`] for
/// anything but a regular file.
#[cfg(any(target_os = "android", all(test, target_os = "linux")))]
pub(crate) fn open_shared(
    files: &[std::path::PathBuf],
) -> Result<Vec<(std::fs::File, String)>, ShareError> {
    files
        .iter()
        .map(|path| {
            let opened =
                super::opened::open(path).map_err(|error| file_error(error.raw_os_error()))?;
            if opened.held != super::opened::Held::File {
                return Err(not_a_file());
            }
            let name = opened.file_name().ok_or_else(not_a_file)?;
            Ok((opened.file, name))
        })
        .collect()
}

/// What a share answers for what starting the chooser did.
pub(crate) fn start_result(started: StartAnswer) -> ShareResult {
    match started {
        StartAnswer::Started => Ok(()),
        // The chooser is the system's own: a device without one shares
        // nothing.
        StartAnswer::NoHandler => Err(refused(UnsupportedReason::NotOnThisDevice)),
        StartAnswer::TooManyOffered => Err(ShareError::Failed(TOO_MANY_OFFERED)),
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

    use super::super::java::Java;
    use super::super::workers::Workers;
    use super::{extra_text, not_run, open_shared, start_result, unavailable};
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

    /// Show the chooser with `request`'s text and `files` and answer
    /// `answer` with what its start did; a start the host did not run on
    /// Java's main thread never reached Android.
    fn show(
        java: &Arc<Java>,
        activity: &Activity,
        completions: &CompletionSource,
        request: &ShareRequest,
        files: Vec<(File, String)>,
        answer: Resolver<ShareResult>,
    ) {
        let started = java.share(
            activity,
            completions,
            request.title.clone(),
            extra_text(request),
            files,
        );
        let mut answer = Some(answer);
        started.on_complete(move |outcome| {
            let Some(answer) = answer.take() else {
                return;
            };
            match outcome {
                // The owner retiring first leaves nothing waiting.
                Ok(Ok(started)) => drop(answer.resolve(start_result(started))),
                Ok(Err(_refused)) => drop(answer.resolve(Err(unavailable()))),
                // The owner retired: the caller's completion ends as its own
                // did.
                Err(_) => {}
            }
        });
    }

    impl ShareBackend for AndroidShare {
        fn share(&self, window: &WindowRef, request: ShareRequest, answer: Resolver<ShareResult>) {
            if request.files.is_empty() {
                show(
                    &self.java,
                    &self.activity,
                    &self.completions,
                    &request,
                    Vec::new(),
                    answer,
                );
                return;
            }
            let files: Vec<PathBuf> = request.files.clone();
            let (opened, opening) = self.completions.pending();
            self.workers
                .run("flui-system-share", opening, not_run, move || {
                    open_shared(&files)
                });
            let activity = self.activity.clone();
            let java = Arc::clone(&self.java);
            let completions = self.completions.clone();
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
                    Ok(Ok(files)) => show(&java, &activity, &completions, &request, files, answer),
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

    /// When the provider serves as many documents as it keeps, a share
    /// answers that typed failure, not a retry-later refusal, and only a
    /// start that happened keeps its offers: every refused or failed start
    /// withdraws them, so it holds none of the provider's places.
    #[test]
    fn a_full_provider_is_its_own_failure_and_only_a_started_share_keeps_its_offers() {
        assert_eq!(
            start_result(StartAnswer::TooManyOffered),
            Err(ShareError::Failed(SystemFailure::Os(24)))
        );
        for (answer, keeps) in [
            (StartAnswer::Started, true),
            (StartAnswer::NoHandler, false),
            (StartAnswer::Denied, false),
            (StartAnswer::TooManyOffered, false),
            (StartAnswer::Failed, false),
        ] {
            assert_eq!(answer.keeps_offers(), keeps, "{answer:?}");
        }
    }

    /// A shared file is the one its open reached, named by where it ends: a
    /// link named like a document goes out under its target's name, a named
    /// pipe is refused at once instead of holding the worker until a writer
    /// comes, and a folder is no file.
    #[cfg(target_os = "linux")]
    #[test]
    #[expect(unsafe_code, reason = "`mkfifo` makes the named pipe the test shares")]
    fn a_share_opens_each_file_and_names_it_by_where_it_ends() {
        use std::os::unix::ffi::OsStrExt as _;

        use super::open_shared;

        let folder = tempfile::tempdir().expect("a folder of the test's own");
        let secret = folder.path().join("secret.db");
        std::fs::write(&secret, b"rows").expect("written");
        let disguised = folder.path().join("notes.pdf");
        std::os::unix::fs::symlink(&secret, &disguised).expect("linked");
        let pipe = folder.path().join("pipe.pdf");
        let pipe_name =
            std::ffi::CString::new(pipe.as_os_str().as_bytes()).expect("no NUL in a temp path");
        // SAFETY: `pipe_name` is a NUL-terminated path that outlives the call.
        let made = unsafe { libc::mkfifo(pipe_name.as_ptr(), 0o600) };
        assert_eq!(made, 0, "mkfifo");

        let shared = open_shared(&[disguised]).expect("a regular file behind a link");
        assert_eq!(
            shared
                .iter()
                .map(|(_, name)| name.as_str())
                .collect::<Vec<&str>>(),
            ["secret.db"],
            "shared under the name of what was opened"
        );
        let refused = open_shared(&[pipe]).map(|_| ()).map_err(reason);
        assert_eq!(refused, Err(Some(UnsupportedReason::NotOnThisPlatform)));
        let refused = open_shared(&[folder.path().to_path_buf()])
            .map(|_| ())
            .map_err(reason);
        assert_eq!(refused, Err(Some(UnsupportedReason::NotOnThisPlatform)));
        assert_eq!(
            open_shared(&[folder.path().join("gone.pdf")]).map(|_| ()),
            Err(ShareError::NotFound)
        );
    }
}
