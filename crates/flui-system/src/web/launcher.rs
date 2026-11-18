//! Launching from a page: a URI opens through `window.open`; a page has no
//! files or folders at paths to open, reveal or move to the trash.
//!
//! A page of `http` or `https` opens in a new browsing context with
//! `noopener` and `noreferrer`, so it cannot reach the application and is
//! not told where it came from; any other scheme the caller allowed is
//! handed to the system's handler from the application's own context,
//! which a browser does without unloading the page. A browser opens a new
//! context only while the page handles a user's gesture, so a launch
//! outside one is refused rather than left to the popup blocker. With
//! `noopener` the browser reports nothing back (`window.open` answers
//! `null`), so a launch answers `Ok` once the browser has the URI: a blocker
//! the browser still applies is not seen.

use std::path::PathBuf;
use std::rc::Rc;

use flui_platform::capability::Resolver;
use flui_platform::launch::{
    ExternalUri, LaunchError, LaunchResult, LauncherBackend, LauncherCapability,
};
use flui_platform::{Unsupported, UnsupportedReason};

use super::page::{Page, exception};
use super::rules::{self, LinkTarget};

/// A page's launcher.
pub(crate) struct WebLauncher {
    page: Rc<Page>,
}

impl WebLauncher {
    pub(crate) fn new(page: Rc<Page>) -> Self {
        Self { page }
    }
}

/// The refusal of a launch for `reason`.
fn unsupported(reason: UnsupportedReason) -> LaunchError {
    LaunchError::Unsupported(Unsupported::of::<LauncherCapability>(reason))
}

/// Answer `answer` that a page has no file at a path.
fn no_paths(answer: Resolver<LaunchResult>) {
    // A retired owner has nobody left to answer.
    let _ = answer.resolve(Err(unsupported(UnsupportedReason::NotOnThisPlatform)));
}

impl LauncherBackend for WebLauncher {
    fn open_uri(&self, uri: ExternalUri, answer: Resolver<LaunchResult>) {
        let result = if self.page.is_activated() {
            let view = self.page.view();
            let opened = match rules::link_target(uri.scheme()) {
                LinkTarget::NewContext => view.open_with_url_and_target_and_features(
                    uri.as_str(),
                    "_blank",
                    "noopener,noreferrer",
                ),
                LinkTarget::HandOff => view.open_with_url_and_target(uri.as_str(), "_self"),
            };
            opened
                .map(|_| ())
                .map_err(|thrown| LaunchError::Failed(exception(&thrown).into()))
        } else {
            Err(unsupported(UnsupportedReason::Unavailable))
        };
        // A retired owner has nobody left to answer.
        let _ = answer.resolve(result);
    }

    fn open_path(&self, _: PathBuf, answer: Resolver<LaunchResult>) {
        no_paths(answer);
    }

    fn reveal_path(&self, _: PathBuf, answer: Resolver<LaunchResult>) {
        no_paths(answer);
    }

    fn trash_path(&self, _: PathBuf, answer: Resolver<LaunchResult>) {
        no_paths(answer);
    }
}
