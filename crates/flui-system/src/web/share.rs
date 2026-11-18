//! The share sheet of a page: the Web Share API (`navigator.share`).
//!
//! Browsers that have it (on phones, and some desktops) show it only in a
//! secure context and while the page handles a user's gesture. A page has
//! no files at paths to share, so a request with files is refused.

use std::rc::Rc;

use flui_platform::capability::{Resolver, WindowRef};
use flui_platform::{Unsupported, UnsupportedReason};
use js_sys::Object;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;

use super::page::{Page, call_promise, exception, method, set};
use super::rules;
use crate::share::{ShareBackend, ShareCapability, ShareError, ShareRequest, ShareResult};

/// A page's share sheet.
pub(crate) struct WebShare {
    page: Rc<Page>,
}

impl WebShare {
    pub(crate) fn new(page: Rc<Page>) -> Self {
        Self { page }
    }
}

/// The refusal of a share for `reason`.
fn unsupported(reason: UnsupportedReason) -> ShareError {
    ShareError::Unsupported(Unsupported::of::<ShareCapability>(reason))
}

impl ShareBackend for WebShare {
    fn share(&self, _: &WindowRef, request: ShareRequest, answer: Resolver<ShareResult>) {
        let navigator: JsValue = self.page.navigator().into();
        let refusal = if !request.files.is_empty() {
            Some(unsupported(UnsupportedReason::NotOnThisPlatform))
        } else if !self.page.is_activated() {
            Some(unsupported(UnsupportedReason::Unavailable))
        } else {
            None
        };
        let share = method(&navigator, "share");
        // The share's promise, or its answer when there is none to wait for.
        let started: Result<js_sys::Promise, ShareResult> = match (refusal, share) {
            (Some(refusal), _) => Err(Err(refusal)),
            // No Web Share API in this browser, or not in an insecure
            // context.
            (None, None) => Err(Err(unsupported(UnsupportedReason::NotOnThisDevice))),
            (None, Some(share)) => {
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
                call_promise(&share, &navigator, &data).map_err(rules::share_rejection)
            }
        };
        match started {
            Err(result) => {
                // A retired owner has nobody left to answer.
                let _ = answer.resolve(result);
            }
            Ok(promise) => wasm_bindgen_futures::spawn_local(async move {
                let result = match JsFuture::from(promise).await {
                    Ok(_) => Ok(()),
                    Err(rejection) => rules::share_rejection(exception(&rejection)),
                };
                let _ = answer.resolve(result);
            }),
        }
    }
}
