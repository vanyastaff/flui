//! Launching through `ACTION_VIEW`.
//!
//! A URI is viewed as written, under `CATEGORY_BROWSABLE`: only an activity
//! that declares itself safe to reach from a link takes it, as a browser
//! starts one, and the intent carries no grant. The seam admits no `file`
//! URI ([`flui_platform::launch::REFUSED_SCHEMES`]), which Android refuses
//! to hand to another application anyway.
//!
//! A document never leaves as a path. It is opened first, on a worker,
//! without blocking and without taking a terminal, and what the descriptor
//! holds is judged ([`judge`]): only a regular file is a document, by its
//! final name (`/proc/self/fd`), which must neither run when opened
//! ([`runs_when_opened`]) nor install a package. The open file is then
//! offered through the crate's content provider (`FluiDocuments`) under a
//! URI that names a random token, and viewed with
//! `FLAG_GRANT_READ_URI_PERMISSION`, so the one activity Android resolves
//! reads that file and nothing else (ADR-0360 §2). The activity starts on
//! Java's main thread, and the launch answers what the start did.
//!
//! Android shows no file in a file manager and has no trash for a path, so
//! reveal and trash answer `NotOnThisPlatform`, as opening a folder does.

use std::path::Path;

use flui_platform::launch::{LaunchError, LaunchResult, LauncherCapability, runs_when_opened};
use flui_platform::{SystemFailure, Unsupported, UnsupportedReason};

use super::opened::Held;
use super::{StartAnswer, TOO_MANY_OFFERED, UNNAMED_FAILURE};
use crate::off_owner::NotRun;

/// `ENOENT`, `EACCES`, `ENOTDIR` and `EPERM`, on Linux and so on Android.
const ENOENT: i32 = 2;
const EACCES: i32 = 13;
const ENOTDIR: i32 = 20;
const EPERM: i32 = 1;

/// The refusal of what Android has no form of.
pub(crate) fn not_on_android() -> LaunchError {
    LaunchError::Unsupported(Unsupported::of::<LauncherCapability>(
        UnsupportedReason::NotOnThisPlatform,
    ))
}

/// Whether viewing a file named `name` installs a package: Android's own
/// kind of file that acts when opened, beside the names every host refuses.
pub(crate) fn installs_when_viewed(name: &Path) -> bool {
    name.extension().is_some_and(|extension| {
        ["apk", "apks", "apex", "xapk"]
            .iter()
            .any(|package| extension.eq_ignore_ascii_case(package))
    })
}

/// Whether the file a launch opened, which holds `held` and whose final
/// name is `final_name`, is a document Android may be handed.
///
/// # Errors
///
/// [`LaunchError::NotADocument`] for anything but a regular file, and for
/// one whose final name runs or installs when opened; `NotOnThisPlatform`
/// for a folder, which no Android activity opens at a path.
pub(crate) fn judge(held: Held, final_name: &Path) -> Result<(), LaunchError> {
    match held {
        Held::Folder => Err(not_on_android()),
        Held::Other | Held::Unlinked => Err(LaunchError::NotADocument),
        Held::File if runs_when_opened(final_name) || installs_when_viewed(final_name) => {
            Err(LaunchError::NotADocument)
        }
        Held::File => Ok(()),
    }
}

/// The refusal of a path the system could not open, by its `errno`.
pub(crate) fn open_error(errno: Option<i32>) -> LaunchError {
    match errno {
        Some(ENOENT | ENOTDIR) => LaunchError::NotFound,
        Some(EACCES | EPERM) => LaunchError::Denied,
        Some(code) => LaunchError::Failed(SystemFailure::Os(code)),
        None => LaunchError::Failed(SystemFailure::MalformedAnswer),
    }
}

/// What a launch answers for what its start did.
pub(crate) fn start_result(started: StartAnswer) -> LaunchResult {
    match started {
        StartAnswer::Started => Ok(()),
        StartAnswer::NoHandler => Err(LaunchError::NoHandler),
        StartAnswer::Denied => Err(LaunchError::Denied),
        StartAnswer::TooManyOffered => Err(LaunchError::Failed(TOO_MANY_OFFERED)),
        StartAnswer::Failed => Err(LaunchError::Failed(UNNAMED_FAILURE)),
    }
}

/// The launch's answer when the request never reached Android: no worker
/// took it, or the Activity or its Java classes could not be called.
pub(crate) fn unavailable() -> LaunchError {
    LaunchError::Unsupported(Unsupported::of::<LauncherCapability>(
        UnsupportedReason::Unavailable,
    ))
}

/// What a worker's refusal answers a launch.
pub(crate) fn not_run<T>(reason: NotRun) -> Result<T, LaunchError> {
    Err(match reason {
        NotRun::Busy => unavailable(),
        NotRun::NoThread(code) => open_error(code),
    })
}

/// The document at `path`, opened and judged ([`super::opened`]): the open
/// file and the name it is handed over under, its final one.
///
/// # Errors
///
/// What [`open_error`] and [`judge`] refuse.
#[cfg(any(target_os = "android", all(test, target_os = "linux")))]
pub(crate) fn open_document(path: &Path) -> Result<(std::fs::File, String), LaunchError> {
    let opened = super::opened::open(path).map_err(|error| open_error(error.raw_os_error()))?;
    judge(opened.held, &opened.final_name)?;
    let name = opened.file_name().ok_or(LaunchError::NotADocument)?;
    Ok((opened.file, name))
}

#[cfg(target_os = "android")]
pub(crate) use provider::AndroidLauncher;

#[cfg(target_os = "android")]
mod provider {
    use std::path::PathBuf;
    use std::rc::Rc;
    use std::sync::Arc;

    use flui_platform::capability::{Activity, CompletionSource, Resolver};
    use flui_platform::launch::{ExternalUri, LaunchResult, LauncherBackend};

    use super::super::java::{Java, Started, view};
    use super::{not_on_android, not_run, open_document, start_result, unavailable};
    use crate::off_owner::OffOwner;

    /// Answer `answer` with what `started` did: a start the host did not
    /// run on Java's main thread never reached Android.
    fn answer_start(started: Started, answer: Resolver<LaunchResult>) {
        let mut answer = Some(answer);
        started.on_complete(move |outcome| {
            let Some(answer) = answer.take() else {
                return;
            };
            match outcome {
                // The owner retiring first leaves nothing waiting.
                Ok(Ok(started)) => drop(answer.resolve(start_result(started))),
                Ok(Err(_refused)) => drop(answer.resolve(Err(unavailable()))),
                // The owner retired: the caller's completion ends as its
                // own did.
                Err(_) => {}
            }
        });
    }

    /// [`LauncherBackend`] over `ACTION_VIEW`.
    pub(crate) struct AndroidLauncher {
        activity: Activity,
        java: Arc<Java>,
        workers: Rc<OffOwner>,
        completions: CompletionSource,
    }

    impl AndroidLauncher {
        pub(crate) fn new(
            activity: Activity,
            java: Arc<Java>,
            workers: Rc<OffOwner>,
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

    impl LauncherBackend for AndroidLauncher {
        fn open_uri(&self, uri: ExternalUri, answer: Resolver<LaunchResult>) {
            answer_start(
                view(&self.activity, &self.completions, uri.as_str().to_owned()),
                answer,
            );
        }

        fn open_path(&self, path: PathBuf, answer: Resolver<LaunchResult>) {
            let (opened, opening) = self.completions.pending();
            self.workers
                .run("flui-system-launch", opening, not_run, move || {
                    open_document(&path)
                });
            let activity = self.activity.clone();
            let java = Arc::clone(&self.java);
            let completions = self.completions.clone();
            let mut answer = Some(answer);
            opened.on_complete(move |outcome| {
                let Some(answer) = answer.take() else {
                    return;
                };
                match outcome {
                    Ok(Ok((file, name))) => answer_start(
                        java.view_document(&activity, &completions, file, name),
                        answer,
                    ),
                    // The owner retiring first leaves nothing waiting.
                    Ok(Err(refusal)) => drop(answer.resolve(Err(refusal))),
                    // The worker panicked: the caller's completion ends as
                    // its own did.
                    Err(_) => {}
                }
            });
        }

        fn reveal_path(&self, _path: PathBuf, answer: Resolver<LaunchResult>) {
            // The owner retiring first leaves nothing waiting.
            let _ = answer.resolve(Err(not_on_android()));
        }

        fn trash_path(&self, _path: PathBuf, answer: Resolver<LaunchResult>) {
            // The owner retiring first leaves nothing waiting.
            let _ = answer.resolve(Err(not_on_android()));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use flui_platform::launch::LaunchError;
    use flui_platform::{SystemFailure, UnsupportedReason};

    use super::super::StartAnswer;
    use super::{Held, installs_when_viewed, judge, not_run, open_error, start_result};
    use crate::off_owner::NotRun;

    fn refused(error: &LaunchError) -> Option<UnsupportedReason> {
        error.unsupported().map(|unsupported| unsupported.reason)
    }

    /// Only a regular file whose final name neither runs nor installs is
    /// handed to Android; a folder has no activity that opens it at a path.
    #[test]
    fn only_a_regular_file_that_neither_runs_nor_installs_is_a_document() {
        for (held, name, expected) in [
            (Held::File, "/data/user/0/app/files/report.pdf", Ok(())),
            (Held::File, "/data/user/0/app/files/Report.PDF", Ok(())),
            (Held::File, "/data/user/0/app/files/no-extension", Ok(())),
            (
                Held::File,
                "/data/user/0/app/files/setup.apk",
                Err(LaunchError::NotADocument),
            ),
            (
                Held::File,
                "/data/user/0/app/files/Bundle.XAPK",
                Err(LaunchError::NotADocument),
            ),
            (
                Held::File,
                "/data/user/0/app/files/run.py",
                Err(LaunchError::NotADocument),
            ),
            (
                Held::Other,
                "/data/user/0/app/files/pipe.pdf",
                Err(LaunchError::NotADocument),
            ),
            (
                Held::Unlinked,
                "/data/user/0/app/files/run.py (deleted)",
                Err(LaunchError::NotADocument),
            ),
        ] {
            assert_eq!(judge(held, Path::new(name)), expected, "{held:?} {name}");
        }
        let folder = judge(Held::Folder, Path::new("/data/user/0/app/files"))
            .expect_err("no activity opens a folder at a path");
        assert_eq!(refused(&folder), Some(UnsupportedReason::NotOnThisPlatform));
        assert!(installs_when_viewed(Path::new("a.apks")));
        assert!(!installs_when_viewed(Path::new("apk")));
    }

    #[test]
    fn a_path_that_cannot_be_opened_and_a_start_answer_what_android_said() {
        assert_eq!(open_error(Some(2)), LaunchError::NotFound);
        assert_eq!(open_error(Some(20)), LaunchError::NotFound);
        assert_eq!(open_error(Some(13)), LaunchError::Denied);
        assert_eq!(
            open_error(Some(5)),
            LaunchError::Failed(SystemFailure::Os(5))
        );
        assert_eq!(start_result(StartAnswer::Started), Ok(()));
        assert_eq!(
            start_result(StartAnswer::NoHandler),
            Err(LaunchError::NoHandler)
        );
        assert_eq!(start_result(StartAnswer::Denied), Err(LaunchError::Denied));
        assert_eq!(
            start_result(StartAnswer::Failed),
            Err(LaunchError::Failed(SystemFailure::MalformedAnswer))
        );
        assert_eq!(
            start_result(StartAnswer::TooManyOffered),
            Err(LaunchError::Failed(SystemFailure::Os(24))),
            "a full provider is its own failure, not a retry-later refusal"
        );
        let busy = not_run::<()>(NotRun::Busy).expect_err("no worker took it");
        assert_eq!(refused(&busy), Some(UnsupportedReason::Unavailable));
        assert_eq!(
            not_run::<()>(NotRun::NoThread(Some(11))),
            Err(LaunchError::Failed(SystemFailure::Os(11)))
        );
    }

    /// The file judged is the one the open reached: a link with a harmless
    /// name that leads to a script is refused by the script's name, a link
    /// to a document is handed over under the document's name, and nothing
    /// but a regular file is a document.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_launch_judges_the_file_it_opened_by_its_final_name() {
        use super::open_document;

        let folder = tempfile::tempdir().expect("a folder of the test's own");
        let document = folder.path().join("report.pdf");
        let script = folder.path().join("run.py");
        std::fs::write(&document, b"%PDF").expect("written");
        std::fs::write(&script, b"#!/bin/sh\n").expect("written");
        let to_script = folder.path().join("notes.pdf");
        let to_document = folder.path().join("link.txt");
        std::os::unix::fs::symlink(&script, &to_script).expect("linked");
        std::os::unix::fs::symlink(&document, &to_document).expect("linked");

        let (_, name) = open_document(&document).expect("a document");
        assert_eq!(name, "report.pdf");
        let (_, name) = open_document(&to_document).expect("a link to a document");
        assert_eq!(name, "report.pdf", "handed over under its final name");
        assert_eq!(
            open_document(&to_script).map(|(_, name)| name),
            Err(LaunchError::NotADocument)
        );
        assert_eq!(
            open_document(&folder.path().join("gone.pdf")).map(|(_, name)| name),
            Err(LaunchError::NotFound)
        );
        let refused_folder = open_document(folder.path())
            .map(|(_, name)| name)
            .expect_err("a folder is no document");
        assert_eq!(
            refused(&refused_folder),
            Some(UnsupportedReason::NotOnThisPlatform)
        );
    }
}
