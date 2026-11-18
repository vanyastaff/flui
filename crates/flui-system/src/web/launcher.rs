//! Launching from a page: a URI opens through the browser; a page has no
//! files or folders at paths to open, reveal or move to the trash.
//!
//! A page of `http` or `https` opens in a new browsing context: a blank one
//! is opened first, so a popup blocker's refusal is seen (`window.open`
//! answers `null`), its `opener` is cut, and the page is then followed from
//! a link of that blank document with `rel=noreferrer`, so the page cannot
//! reach the application and is not told where it came from. Any other
//! scheme the caller allowed is handed to the system's handler by a link
//! the application's page follows, which the HTML standard does without
//! unloading the page; `ftp`, `ws` and `wss` are nothing a browser opens for
//! the user. A browser opens a new context only while the page handles a
//! user's gesture, so a launch outside one, and a blocked one, answer
//! `Unavailable`: trying again from a gesture may succeed.

use std::path::PathBuf;
use std::rc::Rc;

use flui_platform::capability::Resolver;
use flui_platform::launch::{
    ExternalUri, LaunchError, LaunchResult, LauncherBackend, LauncherCapability,
};
use flui_platform::{Unsupported, UnsupportedReason};
use wasm_bindgen::{JsCast as _, JsValue};
use web_sys::{Document, HtmlAnchorElement, Node};

use super::page::{Page, exception};
use super::rules::{self, LaunchRoute};

/// A page's launcher.
pub(crate) struct WebLauncher {
    page: Rc<Page>,
}

impl WebLauncher {
    pub(crate) fn new(page: Rc<Page>) -> Self {
        Self { page }
    }

    /// Open `uri` in a new browsing context with no opener and no referrer.
    fn open_apart(&self, uri: &str) -> LaunchResult {
        let opened = self
            .page
            .view()
            .open_with_url_and_target("about:blank", "_blank")
            .map_err(|thrown| browser_failure(&thrown))?;
        let Some(context) = opened else {
            // The browser's popup blocker, or a gesture already spent.
            return Err(unsupported(UnsupportedReason::Unavailable));
        };
        let _ = context.set_opener(&JsValue::NULL);
        let Some(document) = context.document() else {
            context.close().ok();
            return Err(LaunchError::Failed(
                flui_platform::SystemFailure::MalformedAnswer,
            ));
        };
        follow(&document, document.body().map(Node::from), uri, true).inspect_err(|_| {
            context.close().ok();
        })
    }

    /// Hand `uri` to the system's handler by a link of the application's
    /// page.
    fn hand_off(&self, uri: &str) -> LaunchResult {
        follow(self.page.document(), self.page.host(), uri, false)
    }
}

/// Follow a link to `uri` from `document`, placed in `host` (some engines
/// follow only a link in the document); with no referrer when
/// `no_referrer`.
fn follow(document: &Document, host: Option<Node>, uri: &str, no_referrer: bool) -> LaunchResult {
    let link = document
        .create_element("a")
        .map_err(|thrown| browser_failure(&thrown))?
        .unchecked_into::<HtmlAnchorElement>();
    link.set_href(uri);
    if no_referrer {
        link.set_rel("noreferrer");
    }
    link.set_hidden(true);
    let attached = host.as_ref().map(|host| host.append_child(&link));
    link.click();
    if let (Some(host), Some(Ok(_))) = (host, attached) {
        let _ = host.remove_child(&link);
    }
    Ok(())
}

/// The refusal of a launch for `reason`.
fn unsupported(reason: UnsupportedReason) -> LaunchError {
    LaunchError::Unsupported(Unsupported::of::<LauncherCapability>(reason))
}

/// The failure a browser threw, a refusal of the gesture answered as the
/// provider's own check answers it.
fn browser_failure(thrown: &JsValue) -> LaunchError {
    match exception(thrown) {
        refusal if rules::is_activation_refusal(refusal) => {
            unsupported(UnsupportedReason::Unavailable)
        }
        other => LaunchError::Failed(other.into()),
    }
}

/// Answer `answer` that a page has no file at a path.
fn no_paths(answer: Resolver<LaunchResult>) {
    // A retired owner has nobody left to answer.
    let _ = answer.resolve(Err(unsupported(UnsupportedReason::NotOnThisPlatform)));
}

impl LauncherBackend for WebLauncher {
    fn open_uri(&self, uri: ExternalUri, answer: Resolver<LaunchResult>) {
        let route = rules::launch_route(uri.scheme());
        let result = if route == LaunchRoute::NoHandler {
            Err(LaunchError::NoHandler)
        } else if !self.page.is_activated() {
            Err(unsupported(UnsupportedReason::Unavailable))
        } else if route == LaunchRoute::NewContext {
            self.open_apart(uri.as_str())
        } else {
            self.hand_off(uri.as_str())
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
