//! The share sheet of a page: the Web Share API (`navigator.share`).
//!
//! Browsers that have it (on phones, and some desktops) show it only in a
//! secure context and while the page handles a user's gesture; a browser
//! without it has no share sheet, and the handle is refused when it is
//! acquired. A page has no files at paths to share, and FLUI shares only
//! `http` and `https` links from it, the links a share target can open.

use std::rc::Rc;

use flui_platform::capability::{Resolver, WindowRef};
use flui_platform::{Unsupported, UnsupportedReason};
use js_sys::{Function, Object};
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;

use super::page::{Page, call_promise, exception, method, set};
use super::rules;
use crate::share::{ShareBackend, ShareCapability, ShareError, ShareRequest, ShareResult};

/// A page's share sheet, over the browser's `navigator.share`.
pub(crate) struct WebShare {
    page: Rc<Page>,
    navigator: JsValue,
    share: Function,
}

impl WebShare {
    /// The share sheet of `page`'s browser.
    ///
    /// # Errors
    ///
    /// `NotOnThisDevice` for a browser without the Web Share API (Firefox on
    /// a desktop, any browser in an insecure context), as the badge is
    /// refused without the Badging API: a later call could not change it.
    pub(crate) fn try_new(page: Rc<Page>) -> Result<Self, Unsupported> {
        let navigator: JsValue = page.navigator().into();
        let share = method(&navigator, "share").ok_or(Unsupported::of::<ShareCapability>(
            UnsupportedReason::NotOnThisDevice,
        ))?;
        Ok(Self {
            page,
            navigator,
            share,
        })
    }
}

/// The refusal of a share for `reason`.
fn unsupported(reason: UnsupportedReason) -> ShareError {
    ShareError::Unsupported(Unsupported::of::<ShareCapability>(reason))
}

impl ShareBackend for WebShare {
    fn share(&self, _: &WindowRef, request: ShareRequest, answer: Resolver<ShareResult>) {
        let refusal = if !request.files.is_empty() {
            Some(unsupported(UnsupportedReason::NotOnThisPlatform))
        } else if request
            .uri
            .as_ref()
            .is_some_and(|uri| !rules::is_shareable_link(uri.scheme()))
        {
            // FLUI shares `http` and `https` links from a page.
            Some(unsupported(UnsupportedReason::NotOnThisPlatform))
        } else if !self.page.is_activated() {
            Some(unsupported(UnsupportedReason::Unavailable))
        } else {
            None
        };
        if let Some(refusal) = refusal {
            // A retired owner has nobody left to answer.
            let _ = answer.resolve(Err(refusal));
            return;
        }
        // Whether the browser itself said the page handles a gesture, which
        // makes its refusal one a later gesture would not change.
        let saw_activation = self.page.reported_activation() == Some(true);
        let data = Object::new();
        if let Some(title) = &request.title {
            set(&data, "title", &JsValue::from_str(title));
        }
        if let Some(text) = &request.text {
            set(&data, "text", &JsValue::from_str(text));
        }
        if let Some(uri) = &request.uri {
            set(&data, "url", &JsValue::from_str(uri.as_str()));
        }
        match call_promise(&self.share, &self.navigator, &data) {
            Err(thrown) => {
                // A retired owner has nobody left to answer.
                let _ = answer.resolve(rules::share_rejection(thrown, saw_activation));
            }
            Ok(promise) => wasm_bindgen_futures::spawn_local(async move {
                let result = match JsFuture::from(promise).await {
                    Ok(_) => Ok(()),
                    Err(rejection) => rules::share_rejection(exception(&rejection), saw_activation),
                };
                let _ = answer.resolve(result);
            }),
        }
    }
}
