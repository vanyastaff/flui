//! What an agent asks an element to do.

use crate::tree::ElementId;
use crate::wire::ActionName;

/// One action on one element: a tool the node advertised in its `actions`,
/// addressed by its handle.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct ActionRequest {
    /// The element to act on.
    pub element: ElementId,
    /// What to do, one of the element's advertised actions.
    pub action: ActionName,
    /// The new value, for `set_value`.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub value: Option<String>,
}

impl ActionRequest {
    /// `action` on `element`, with no value.
    #[must_use]
    pub fn new(element: ElementId, action: ActionName) -> Self {
        Self {
            element,
            action,
            value: None,
        }
    }

    /// `set_value` on `element`, to `value`.
    #[must_use]
    pub fn set_value(element: ElementId, value: impl Into<String>) -> Self {
        Self {
            element,
            action: ActionName::SetValue,
            value: Some(value.into()),
        }
    }
}
