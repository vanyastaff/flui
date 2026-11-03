//! Taking the page's launch object ([`PAGE_LAUNCH_GLOBAL`], ADR-0288 §7) as the web platform
//! starts: the global is removed whatever it holds, then what it held is read within
//! [`page_launch`](crate::owner::page_launch)'s bounds.

use flui_platform::PAGE_LAUNCH_GLOBAL;
use js_sys::{Array, JsString, Object, Reflect};
use wasm_bindgen::{JsCast, JsValue};

use crate::error::PlatformError;
use crate::owner::page_launch::{
    MAX_PAGE_LAUNCH_KEY_UNITS, MAX_PAGE_LAUNCH_VALUE_LEN, PageLaunch, PageLaunchError,
    PageLaunchProperty,
};

/// Take the page's launch values off the page. Empty when the page was given none.
///
/// # Errors
///
/// [`PlatformError::Init`] when the global is not a plain object of launch keys' strings
/// within the bounds, or when it cannot be removed from the page; the error is traced too.
pub(crate) fn take() -> Result<PageLaunch, PlatformError> {
    let global = js_sys::global();
    let name = JsValue::from_str(PAGE_LAUNCH_GLOBAL);
    let installed = Reflect::get(&global, &name).unwrap_or(JsValue::UNDEFINED);
    if installed.is_undefined() {
        return Ok(PageLaunch::default());
    }
    // Removed before anything is read: neither a malformed object nor a failed read leaves
    // the token for a script the page loads later.
    let taken = remove(&global, &name)
        .map_err(|()| "it could not be removed from the page".to_owned())
        .and_then(|()| read(&installed).map_err(|error| error.to_string()));
    taken.map_err(|message| {
        let message = format!("the page's launch values (`{PAGE_LAUNCH_GLOBAL}`): {message}");
        tracing::error!(%message, "the web platform does not start");
        PlatformError::Init { message }
    })
}

/// Remove `name` from `global`: deleted, or, for a global a page script declared with `var`,
/// which cannot be deleted, overwritten with `undefined`.
fn remove(global: &Object, name: &JsValue) -> Result<(), ()> {
    if Reflect::delete_property(global, name).unwrap_or(false) {
        return Ok(());
    }
    let cleared = Reflect::set(global, name, &JsValue::UNDEFINED).unwrap_or(false);
    let gone = Reflect::get(global, name).is_ok_and(|value| value.is_undefined());
    if cleared && gone { Ok(()) } else { Err(()) }
}

/// The launch `installed` holds, its property count, key lengths and value lengths each
/// checked before anything is copied.
fn read(installed: &JsValue) -> Result<PageLaunch, PageLaunchError> {
    if !installed.is_object() || installed.is_function() || Array::is_array(installed) {
        return Err(PageLaunchError::NotAnObject);
    }
    let object: &Object = installed.unchecked_ref();
    let keys = Object::keys(object);
    PageLaunch::check_count(usize::try_from(keys.length()).unwrap_or(usize::MAX))?;
    let properties = keys.iter().map(|key| {
        let name = key
            .dyn_ref::<JsString>()
            .filter(|key| units(key) <= MAX_PAGE_LAUNCH_KEY_UNITS)
            .and_then(|_| key.as_string());
        let value = Reflect::get(object, &key).unwrap_or(JsValue::UNDEFINED);
        let property = match value.dyn_ref::<JsString>() {
            Some(text) if units(text) > MAX_PAGE_LAUNCH_VALUE_LEN => PageLaunchProperty::TooLong,
            Some(_) => value
                .as_string()
                .map_or(PageLaunchProperty::NotText, PageLaunchProperty::Text),
            None => PageLaunchProperty::NotText,
        };
        (name, property)
    });
    PageLaunch::try_from_properties(properties)
}

/// A page string's length, in the UTF-16 units the page counts.
fn units(text: &JsString) -> usize {
    usize::try_from(text.length()).unwrap_or(usize::MAX)
}
