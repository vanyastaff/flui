//! The browser capabilities the display source uses, detected once when the
//! platform is built.
//!
//! Detection asks for the function itself (see
//! [`crate::shared::web_feature`]); no browser version is read. The answers are
//! kept as proofs, so the code that calls `matchMedia`, the Permissions API or
//! `getScreenDetails` cannot be written without having the proof.

use wasm_bindgen::{JsCast, JsValue};

use crate::shared::web_feature::{
    FeatureProbe, GetScreenDetails, MatchMedia, PermissionsQuery, WebFeature,
};

/// Looks a function up by its dotted path from the global object.
struct GlobalProbe;

impl FeatureProbe for GlobalProbe {
    fn has_function(&self, path: &str) -> bool {
        let mut value: JsValue = js_sys::global().into();
        for segment in path.split('.') {
            match js_sys::Reflect::get(&value, &JsValue::from_str(segment)) {
                Ok(next) if !next.is_undefined() && !next.is_null() => value = next,
                _ => return false,
            }
        }
        value.is_instance_of::<js_sys::Function>()
    }
}

/// What this browser offers the display source.
#[derive(Debug, Clone, Copy)]
pub(super) struct BrowserFeatures {
    /// Media queries and their change events: the pixel-ratio watch.
    pub(super) match_media: Option<WebFeature<MatchMedia>>,
    /// Read a permission without prompting for it.
    pub(super) permissions: Option<WebFeature<PermissionsQuery>>,
    /// Every screen of the machine (Window Management).
    pub(super) screen_details: Option<WebFeature<GetScreenDetails>>,
}

impl BrowserFeatures {
    /// Probe the running browser.
    pub(super) fn detect() -> Self {
        Self {
            match_media: WebFeature::detect(&GlobalProbe),
            permissions: WebFeature::detect(&GlobalProbe),
            screen_details: WebFeature::detect(&GlobalProbe),
        }
    }
}
