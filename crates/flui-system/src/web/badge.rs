//! The launcher entry of a page: the Badging API's count on an installed web
//! application's icon (`navigator.setAppBadge`, `clearAppBadge`).
//!
//! A page has no progress to show on its icon, so the entry honours the
//! count alone. The browser applies a badge only to an installed application
//! and does not say whether one shows, so the count is not confirmed, as the
//! Linux launcher entry's signal is not.

use std::rc::Rc;

use flui_platform::{Unsupported, UnsupportedReason};
use js_sys::Function;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;

use super::page::{Page, call_promise, method};
use crate::taskbar::{EntryState, Honoured, TaskbarBackend, TaskbarCapability};

/// The largest count a JavaScript number holds exactly
/// (`Number.MAX_SAFE_INTEGER`); a larger one is shown as this.
const MAX_BADGE_COUNT: u64 = (1 << 53) - 1;

/// A page's badge, over the browser's two Badging API calls.
pub(crate) struct WebBadge {
    navigator: JsValue,
    set: Function,
    clear: Function,
}

impl WebBadge {
    /// The badge of `page`'s browser.
    ///
    /// # Errors
    ///
    /// `NotOnThisDevice` for a browser without the Badging API (Firefox).
    pub(crate) fn try_new(page: &Rc<Page>) -> Result<Self, Unsupported> {
        let navigator: JsValue = page.navigator().into();
        match (
            method(&navigator, "setAppBadge"),
            method(&navigator, "clearAppBadge"),
        ) {
            (Some(set), Some(clear)) => Ok(Self {
                navigator,
                set,
                clear,
            }),
            _ => Err(Unsupported::of::<TaskbarCapability>(
                UnsupportedReason::NotOnThisDevice,
            )),
        }
    }
}

impl TaskbarBackend for WebBadge {
    fn show(&self, state: EntryState) {
        let call = match state.count {
            Some(count) => {
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "the count is at most MAX_BADGE_COUNT, which a double holds exactly"
                )]
                let count = count.min(MAX_BADGE_COUNT) as f64;
                call_promise(&self.set, &self.navigator, &JsValue::from_f64(count))
            }
            None => call_promise(&self.clear, &self.navigator, &JsValue::UNDEFINED),
        };
        if let Ok(promise) = call {
            // The browser's answer is not one the entry reports (`confirmed`
            // is false); awaiting it keeps a refusal from surfacing as an
            // unhandled rejection.
            wasm_bindgen_futures::spawn_local(async move {
                let _ = JsFuture::from(promise).await;
            });
        }
    }

    fn honours(&self) -> Honoured {
        Honoured::new(false, false, true, false)
    }
}
