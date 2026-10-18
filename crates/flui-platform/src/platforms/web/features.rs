//! The browser capabilities the display source uses, detected once when the
//! platform is built.
//!
//! Detection asks for the function itself (see
//! [`crate::shared::web_feature`]); no browser version is read. The answers are
//! kept as proofs, and the calls into the browser that need a capability are
//! methods on its proof: `matchMedia`, the Permissions API, `getScreenDetails`
//! and `screen.onchange` cannot be reached without the check having happened.

use wasm_bindgen::{JsCast, JsValue};

use crate::shared::web_feature::{
    FeatureKind, FeatureProbe, GetScreenDetails, MatchMedia, PermissionsQuery, ScreenChange,
    WebFeature,
};

/// Looks a feature up by its dotted path from the global object.
struct GlobalProbe;

impl FeatureProbe for GlobalProbe {
    fn has(&self, path: &str, kind: FeatureKind) -> bool {
        let mut segments = path.split('.').peekable();
        let mut value: JsValue = js_sys::global().into();
        while let Some(segment) = segments.next() {
            let name = JsValue::from_str(segment);
            if segments.peek().is_none() && kind == FeatureKind::Property {
                // A slot that holds `null` is still there.
                return js_sys::Reflect::has(&value, &name).unwrap_or(false);
            }
            match js_sys::Reflect::get(&value, &name) {
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
    /// The screen object's own `change` event.
    pub(super) screen_change: Option<WebFeature<ScreenChange>>,
}

impl BrowserFeatures {
    /// Probe the running browser.
    pub(super) fn detect() -> Self {
        Self {
            match_media: WebFeature::detect(&GlobalProbe),
            permissions: WebFeature::detect(&GlobalProbe),
            screen_details: WebFeature::detect(&GlobalProbe),
            screen_change: WebFeature::detect(&GlobalProbe),
        }
    }
}

#[expect(
    clippy::unused_self,
    reason = "the proof is the capability: the call cannot be written without it"
)]
impl WebFeature<MatchMedia> {
    /// `window.matchMedia(query)`.
    pub(super) fn query(
        &self,
        window: &web_sys::Window,
        query: &str,
    ) -> Result<Option<web_sys::MediaQueryList>, JsValue> {
        window.match_media(query)
    }
}

#[expect(
    clippy::unused_self,
    reason = "the proof is the capability: the call cannot be written without it"
)]
impl WebFeature<PermissionsQuery> {
    /// `navigator.permissions.query({ name })`: the promise of a
    /// `PermissionStatus`. Reading a permission never prompts. `None` when the
    /// call is refused (an unknown permission name throws in some browsers).
    pub(super) fn query(&self, window: &web_sys::Window, name: &str) -> Option<js_sys::Promise> {
        let navigator = window.navigator();
        let permissions = super::display::get(navigator.as_ref(), "permissions")?;
        let query: js_sys::Function = super::display::get(&permissions, "query")?
            .dyn_into()
            .ok()?;
        let descriptor = js_sys::Object::new();
        js_sys::Reflect::set(&descriptor, &"name".into(), &name.into()).ok()?;
        query.call1(&permissions, &descriptor).ok()?.dyn_into().ok()
    }
}

#[expect(
    clippy::unused_self,
    reason = "the proof is the capability: the call cannot be written without it"
)]
impl WebFeature<GetScreenDetails> {
    /// `window.getScreenDetails()`: the promise of a `ScreenDetails`. It
    /// prompts when the permission is neither granted nor denied, so callers
    /// ask only after reading the permission as granted.
    pub(super) fn request(&self, window: &web_sys::Window) -> Option<js_sys::Promise> {
        let function: js_sys::Function = super::display::get(window.as_ref(), "getScreenDetails")?
            .dyn_into()
            .ok()?;
        function.call0(window.as_ref()).ok()?.dyn_into().ok()
    }
}

#[expect(
    clippy::unused_self,
    reason = "the proof is the capability: the call cannot be written without it"
)]
impl WebFeature<ScreenChange> {
    /// The `window.screen` object, which raises `change`.
    pub(super) fn target(&self, window: &web_sys::Window) -> Option<web_sys::EventTarget> {
        let screen = window.screen().ok()?;
        screen.dyn_into().ok()
    }
}
