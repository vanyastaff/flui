//! The launcher over `UIApplication`'s `openURL:options:completionHandler:`.
//!
//! A URI goes to the system as written, which hands it to the application
//! that claims its scheme (Safari, Mail, Phone, an application's own
//! scheme); the seam admitted only the schemes its caller named. The
//! completion says whether an application took it: one that did not is
//! `NoHandler`. Asking first with `canOpenURL:` would need every scheme
//! listed in the application's `LSApplicationQueriesSchemes`, and adds
//! nothing the completion does not say.
//!
//! iOS has no file manager an application drives, no trash, and no default
//! application for a document: handing a file to another application is a
//! share. So the path verbs answer `NotOnThisPlatform`, without touching
//! the file system.

use flui_platform::launch::{LaunchError, LauncherCapability};
use flui_platform::{Unsupported, UnsupportedReason};

/// What `openURL:`'s completion means: whether an application took the URL.
pub(super) fn opened(success: bool) -> Result<(), LaunchError> {
    if success {
        Ok(())
    } else {
        Err(LaunchError::NoHandler)
    }
}

/// What a path verb answers on iOS.
pub(super) fn no_path_verbs() -> LaunchError {
    LaunchError::Unsupported(Unsupported::of::<LauncherCapability>(
        UnsupportedReason::NotOnThisPlatform,
    ))
}

#[cfg(target_os = "ios")]
pub(crate) use uikit::IOSLauncher;

#[cfg(target_os = "ios")]
#[expect(unsafe_code, reason = "UIApplication's openURL:")]
mod uikit {
    use std::cell::Cell;
    use std::path::PathBuf;

    use block2::RcBlock;
    use flui_platform::capability::Resolver;
    use flui_platform::launch::{ExternalUri, LaunchError, LaunchResult, LauncherBackend};
    use objc2::MainThreadMarker;
    use objc2::runtime::Bool;
    use objc2_foundation::{NSDictionary, NSString, NSURL};
    use objc2_ui_kit::UIApplication;

    use super::{no_path_verbs, opened};
    use flui_foundation::panic::{AfterPanic, os_entry};

    /// [`LauncherBackend`] over `UIApplication`.
    pub(crate) struct IOSLauncher {
        mtm: MainThreadMarker,
    }

    impl IOSLauncher {
        pub(crate) fn new(mtm: MainThreadMarker) -> Self {
            Self { mtm }
        }
    }

    /// Answer `answer` with what a path verb answers on iOS.
    fn refuse_path(answer: Resolver<LaunchResult>) {
        // The owner retiring first leaves nothing waiting.
        let _ = answer.resolve(Err(no_path_verbs()));
    }

    impl LauncherBackend for IOSLauncher {
        fn open_uri(&self, uri: ExternalUri, answer: Resolver<LaunchResult>) {
            let Some(url) = NSURL::URLWithString(&NSString::from_str(uri.as_str())) else {
                // The owner retiring first leaves nothing waiting.
                let _ = answer.resolve(Err(LaunchError::NoHandler));
                return;
            };
            // Taken by the one completion UIKit calls.
            let answer = Cell::new(Some(answer));
            let completion = RcBlock::new(move |success: Bool| {
                os_entry(
                    "openURL:options:completionHandler:",
                    success,
                    AfterPanic::Answer(|_| ()),
                    || {
                        if let Some(answer) = answer.take() {
                            // The owner retiring first leaves nothing waiting.
                            let _ = answer.resolve(opened(success.as_bool()));
                        }
                    },
                );
            });
            // SAFETY: an empty dictionary holds no value of a wrong type for
            // its options; the URL and the block outlive the call, and UIKit
            // copies the block. Called on the main thread.
            unsafe {
                UIApplication::sharedApplication(self.mtm).openURL_options_completionHandler(
                    &url,
                    &NSDictionary::new(),
                    Some(&completion),
                );
            }
        }

        fn open_path(&self, _path: PathBuf, answer: Resolver<LaunchResult>) {
            refuse_path(answer);
        }

        fn reveal_path(&self, _path: PathBuf, answer: Resolver<LaunchResult>) {
            refuse_path(answer);
        }

        fn trash_path(&self, _path: PathBuf, answer: Resolver<LaunchResult>) {
            refuse_path(answer);
        }
    }
}

#[cfg(test)]
mod tests {
    use flui_platform::launch::{LaunchError, LauncherCapability};
    use flui_platform::{Unsupported, UnsupportedReason};

    use super::{no_path_verbs, opened};

    #[test]
    fn a_link_opens_where_an_application_takes_it_and_a_path_has_no_verb() {
        assert_eq!(opened(true), Ok(()));
        assert_eq!(opened(false), Err(LaunchError::NoHandler));
        assert_eq!(
            no_path_verbs(),
            LaunchError::Unsupported(Unsupported::of::<LauncherCapability>(
                UnsupportedReason::NotOnThisPlatform
            ))
        );
    }
}
