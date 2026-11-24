//! The macOS accessibility API's calls (`AXUIElement`), on macOS: see the [parent](super).
//!
//! The client waits [`MESSAGING_TIMEOUT_SECONDS`] for an app unless a caller asks for less, and
//! a call that does not answer reads as [`ProviderError::Timeout`], never as an element of
//! defaults. Text is cut while it is read, to [`OUTLINE_TEXT_LIMIT`] characters, where the
//! projection cuts the app's. Elements live for one call on the lane's thread, so a lane stays
//! `Send`: nothing here keeps one past the call that read it.
#![expect(
    unsafe_code,
    reason = "the accessibility API's C calls; each takes a live element and locals it writes"
)]

use std::ptr::NonNull;
use std::time::Duration;

use objc2_application_services::{AXError, AXIsProcessTrusted, AXUIElement, AXValue, AXValueType};
use objc2_core_foundation::{
    CFArray, CFBoolean, CFIndex, CFNumber, CFRange, CFRetained, CFString, CFType, CGPoint, CGSize,
    ConcreteType,
};

use flui_protocol::tree::{ElementId, Node, OUTLINE_TEXT_LIMIT};

use crate::error::Error;
use crate::lane::walk::{Properties, Provider, ProviderError};

/// How long the client waits for an app to answer one call, in seconds.
pub(crate) const MESSAGING_TIMEOUT_SECONDS: f32 = 5.0;

/// [`MESSAGING_TIMEOUT_SECONDS`] as a duration, which a timeout names.
pub(crate) fn timeout() -> Duration {
    Duration::from_secs_f32(MESSAGING_TIMEOUT_SECONDS)
}

/// Whether this process is a trusted accessibility client, asked without a prompt.
pub(crate) fn trusted() -> bool {
    // SAFETY: no arguments; the call reads this process's trust.
    unsafe { AXIsProcessTrusted() }
}

/// What an accessibility error means for a read.
pub(crate) fn classify(error: AXError) -> ProviderError {
    match error {
        AXError::CannotComplete => ProviderError::Timeout,
        AXError::InvalidUIElement => ProviderError::Gone,
        AXError::APIDisabled => ProviderError::Failed(
            "the accessibility API is off for this process: it is not trusted".to_owned(),
        ),
        other => ProviderError::Failed(format!("the accessibility API answered {}", other.0)),
    }
}

/// An attribute's or an action's name, as the accessibility API spells it.
pub(crate) fn name(attribute: &'static str) -> CFRetained<CFString> {
    CFString::from_static_str(attribute)
}

/// `attribute` of `element`; `Ok(None)` when the element has no such attribute or no value.
pub(crate) fn attribute(
    element: &AXUIElement,
    attribute: &'static str,
) -> Result<Option<CFRetained<CFType>>, ProviderError> {
    let mut value: *const CFType = std::ptr::null();
    // SAFETY: a live element, an attribute name, and a local the call writes a +1 value into.
    let error =
        unsafe { element.copy_attribute_value(&name(attribute), NonNull::from(&mut value)) };
    match error {
        AXError::Success => Ok(NonNull::new(value.cast_mut()).map(|value| {
            // SAFETY: a `Copy` call's value, owned by the caller with its +1 count.
            unsafe { CFRetained::from_raw(value) }
        })),
        AXError::NoValue | AXError::AttributeUnsupported => Ok(None),
        error => Err(classify(error)),
    }
}

/// `value` as text, when it is a string; `None` for an empty one.
pub(crate) fn string(value: &CFType) -> Option<String> {
    value.downcast_ref::<CFString>().and_then(text)
}

/// The first [`OUTLINE_TEXT_LIMIT`] characters of `value`, copied only that far; `None` for an
/// empty one.
fn text(value: &CFString) -> Option<String> {
    // Two UTF-16 units hold any character, so this many units hold the characters kept.
    let limit = CFIndex::try_from(OUTLINE_TEXT_LIMIT * 2).unwrap_or(CFIndex::MAX);
    let length = value.length();
    let text: String = if length > limit {
        // SAFETY: the range starts at the string's first unit and ends inside it, since the
        // string is longer than `limit` units.
        let cut = unsafe { CFString::with_substring(None, Some(value), CFRange::new(0, limit)) };
        match cut {
            Some(cut) => cut.to_string(),
            None => String::new(),
        }
    } else {
        value.to_string()
    };
    let text: String = text.chars().take(OUTLINE_TEXT_LIMIT).collect();
    (!text.is_empty()).then_some(text)
}

/// `value` as a flag: a boolean, or a number that is not zero.
pub(crate) fn boolean(value: &CFType) -> Option<bool> {
    if let Some(flag) = value.downcast_ref::<CFBoolean>() {
        return Some(flag.as_bool());
    }
    value
        .downcast_ref::<CFNumber>()
        .and_then(CFNumber::as_i64)
        .map(|n| n != 0)
}

/// `value` as the text of a number: a whole number as its digits, any other as `f64` prints it.
pub(crate) fn number_text(value: &CFType) -> Option<String> {
    let number = value.downcast_ref::<CFNumber>()?;
    if number.is_float_type() {
        number.as_f64().map(|n| n.to_string())
    } else {
        number.as_i64().map(|n| n.to_string())
    }
}

/// `value` as a point, `(x, y)`, when it holds one.
pub(crate) fn point(value: &CFType) -> Option<(f64, f64)> {
    let value = value.downcast_ref::<AXValue>()?;
    let mut point = CGPoint::new(0.0, 0.0);
    // SAFETY: a live value and a local point the call writes when the value holds one.
    let read = unsafe { value.value(AXValueType::CGPoint, NonNull::from(&mut point).cast()) };
    read.then_some((point.x, point.y))
}

/// `value` as a size, `(width, height)`, when it holds one.
pub(crate) fn size(value: &CFType) -> Option<(f64, f64)> {
    let value = value.downcast_ref::<AXValue>()?;
    let mut size = CGSize::new(0.0, 0.0);
    // SAFETY: a live value and a local size the call writes when the value holds one.
    let read = unsafe { value.value(AXValueType::CGSize, NonNull::from(&mut size).cast()) };
    read.then_some((size.width, size.height))
}

/// The frame of `element` in global points, `(x, y, width, height)`: its `AXPosition` and
/// `AXSize`, `None` when either is missing.
pub(crate) fn frame(element: &AXUIElement) -> Result<Option<(f64, f64, f64, f64)>, ProviderError> {
    let position = attribute(element, "AXPosition")?.as_deref().and_then(point);
    let extent = attribute(element, "AXSize")?.as_deref().and_then(size);
    Ok(position
        .zip(extent)
        .map(|((x, y), (width, height))| (x, y, width, height)))
}

/// The items of `array` that are `T`s, each retained; the others are left out.
pub(crate) fn items<T: ConcreteType>(array: CFRetained<CFArray>) -> Vec<CFRetained<T>> {
    // SAFETY: an array the accessibility API returns holds Core Foundation objects, and each is
    // checked against `T`'s type before it is read as one.
    let array = unsafe { CFRetained::cast_unchecked::<CFArray<CFType>>(array) };
    array
        .iter()
        .filter_map(|item| item.downcast::<T>().ok())
        .collect::<Vec<CFRetained<T>>>()
}

/// The elements `attribute` of `element` lists; none when it lists nothing.
pub(crate) fn elements(
    element: &AXUIElement,
    attribute_name: &'static str,
) -> Result<Vec<CFRetained<AXUIElement>>, ProviderError> {
    let listed =
        attribute(element, attribute_name)?.and_then(|value| value.downcast::<CFArray>().ok());
    Ok(match listed {
        Some(array) => items::<AXUIElement>(array),
        None => Vec::new(),
    })
}

/// The child of `parent` at `index`, read alone (`AXUIElementCopyAttributeValues` for one value),
/// so a walk copies no more children than its budget visits; `None` past the last child, which
/// the API answers as an index outside the array.
pub(crate) fn child_at(
    parent: &AXUIElement,
    index: CFIndex,
) -> Result<Option<CFRetained<AXUIElement>>, ProviderError> {
    let mut values: *const CFArray = std::ptr::null();
    // SAFETY: a live element, an attribute name, a range of one value, and a local the call writes
    // a +1 array into.
    let error = unsafe {
        parent.copy_attribute_values(&name("AXChildren"), index, 1, NonNull::from(&mut values))
    };
    match error {
        AXError::Success => {}
        AXError::NoValue | AXError::AttributeUnsupported | AXError::IllegalArgument => {
            return Ok(None);
        }
        error => return Err(classify(error)),
    }
    let Some(values) = NonNull::new(values.cast_mut()) else {
        return Ok(None);
    };
    // SAFETY: a `Copy` call's array, owned by the caller with its +1 count.
    let values: CFRetained<CFArray> = unsafe { CFRetained::from_raw(values) };
    Ok(items::<AXUIElement>(values).into_iter().next())
}

/// Whether `attribute` of `element` can be set.
pub(crate) fn settable(
    element: &AXUIElement,
    attribute: &'static str,
) -> Result<bool, ProviderError> {
    let mut settable: u8 = 0;
    // SAFETY: a live element, an attribute name, and a local the call writes.
    let error =
        unsafe { element.is_attribute_settable(&name(attribute), NonNull::from(&mut settable)) };
    match error {
        AXError::Success => Ok(settable != 0),
        AXError::NoValue | AXError::AttributeUnsupported => Ok(false),
        error => Err(classify(error)),
    }
}

/// The app `pid`'s element, its replies awaited `seconds`.
pub(crate) fn application(pid: i32, seconds: f32) -> CFRetained<AXUIElement> {
    // SAFETY: any process id names an element; one with no app behind it answers every call
    // with an error.
    let app = unsafe { AXUIElement::new_application(pid) };
    // SAFETY: a live element and a positive timeout. A refused timeout leaves the default one.
    let _ = unsafe { app.set_messaging_timeout(seconds) };
    app
}

/// The actions `element` offers, by name.
pub(crate) fn actions(element: &AXUIElement) -> Result<Vec<String>, ProviderError> {
    let mut names: *const CFArray = std::ptr::null();
    // SAFETY: a live element and a local the call writes a +1 array into.
    let error = unsafe { element.copy_action_names(NonNull::from(&mut names)) };
    match error {
        AXError::Success => {}
        AXError::NoValue | AXError::ActionUnsupported => return Ok(Vec::new()),
        error => return Err(classify(error)),
    }
    let Some(names) = NonNull::new(names.cast_mut()) else {
        return Ok(Vec::new());
    };
    // SAFETY: a `Copy` call's array, owned by the caller with its +1 count.
    let names: CFRetained<CFArray> = unsafe { CFRetained::from_raw(names) };
    let mut found = Vec::new();
    for name in items::<CFString>(names) {
        found.push(name.to_string());
    }
    Ok(found)
}

/// Perform the action `action` on `element`.
pub(crate) fn perform(element: &AXUIElement, action: &'static str) -> Result<(), ProviderError> {
    // SAFETY: a live element and an action name.
    let error = unsafe { element.perform_action(&name(action)) };
    match error {
        AXError::Success => Ok(()),
        error => Err(classify(error)),
    }
}

/// What a failed read of an app's accessibility tree answers.
pub(crate) fn read_error(error: ProviderError) -> Error {
    match error {
        ProviderError::Timeout => Error::Timeout {
            awaited: crate::error::Awaited::Host,
            effect: None,
            message: format!(
                "the app's accessibility API did not answer within {} ms: the app may be hung",
                timeout().as_millis()
            ),
            retry: flui_protocol::error::ErrorCode::Timeout.default_retry(),
        },
        ProviderError::Gone => Error::gone(
            flui_protocol::error::HandleKind::Window,
            None,
            None,
            "the app's window went away while it was read",
        ),
        ProviderError::Failed(why) => Error::Platform {
            effect: None,
            message: format!("the OS accessibility tree could not be read: {why}"),
            retry: flui_protocol::error::Retry::Never,
        },
    }
}

/// An element as a walk visits it: the element, its parent and its index among the parent's
/// children (none for the walk's root).
pub(crate) struct Visit {
    pub(crate) element: CFRetained<AXUIElement>,
    parent: Option<CFRetained<AXUIElement>>,
    index: CFIndex,
}

impl Visit {
    /// `element` as the root of a walk.
    pub(crate) fn root(element: CFRetained<AXUIElement>) -> Self {
        Self {
            element,
            parent: None,
            index: 0,
        }
    }
}

/// The accessibility API as `lane::walk` reads it, each element projected by `describe`, a lane's
/// own projection.
pub(crate) struct Walker<D> {
    describe: D,
}

impl<D> Walker<D>
where
    D: FnMut(&AXUIElement, ElementId, &mut Properties) -> Node,
{
    /// A walker that projects each element through `describe`.
    pub(crate) fn new(describe: D) -> Self {
        Self { describe }
    }
}

impl<D> Provider for Walker<D>
where
    D: FnMut(&AXUIElement, ElementId, &mut Properties) -> Node,
{
    type Element = Visit;

    fn first_child(&mut self, visit: &Visit) -> Result<Option<Visit>, ProviderError> {
        Ok(child_at(&visit.element, 0)?.map(|element| Visit {
            element,
            parent: Some(visit.element.clone()),
            index: 0,
        }))
    }

    fn next_sibling(&mut self, visit: &Visit) -> Result<Option<Visit>, ProviderError> {
        let Some(parent) = &visit.parent else {
            return Ok(None);
        };
        let Some(index) = visit.index.checked_add(1) else {
            return Ok(None);
        };
        Ok(child_at(parent, index)?.map(|element| Visit {
            element,
            parent: Some(parent.clone()),
            index,
        }))
    }

    fn node(&mut self, visit: &Visit, id: ElementId) -> Result<Node, ProviderError> {
        let mut reads = Properties::new();
        let node = (self.describe)(&visit.element, id, &mut reads);
        reads.finish(node)
    }
}
