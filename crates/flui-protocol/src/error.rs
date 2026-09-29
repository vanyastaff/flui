//! The fixed error codes of ADR-0080 "Errors", and whether a failed call can
//! succeed later.

use std::fmt;

vocabulary! {
    /// Why a call failed: one fixed code an agent branches on, the message
    /// beside it being for people. ADR-0080 fixed the list and its spelling;
    /// a new code is additive.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(
        feature = "serde",
        derive(serde::Serialize, serde::Deserialize),
        serde(rename_all = "snake_case")
    )]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum ErrorCode {
        /// A parameter is malformed or contradicts another.
        InvalidArgument => "invalid_argument",
        /// The backend cannot do this at all.
        NotSupported => "not_supported",
        /// The target is busy; try again soon.
        Busy => "busy",
        /// Nothing matched.
        NotFound => "not_found",
        /// The handle was never issued to this session.
        UnknownHandle => "unknown_handle",
        /// The handle's element, window, screenshot or process is gone; the
        /// handle is never re-bound.
        Gone => "gone",
        /// The element refuses interaction.
        Disabled => "disabled",
        /// The element does not offer this action.
        ActionUnsupported => "action_unsupported",
        /// The target window is not in the foreground.
        NotForeground => "not_foreground",
        /// Keyboard focus is outside the target.
        FocusElsewhere => "focus_elsewhere",
        /// The location is outside the target.
        OutsideTarget => "outside_target",
        /// The wait ran out.
        Timeout => "timeout",
        /// Input is held by a previous call.
        InputHeld => "input_held",
        /// The server or the application is shutting down.
        ShuttingDown => "shutting_down",
        /// The platform failed in a way no other code names.
        Platform => "platform",
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Whether the same call can succeed later; polling tools poll on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum Retry {
    /// Not without changing the call.
    Never,
    /// Soon, unchanged: the target was busy.
    Soon,
    /// Once what it names appears.
    WhenAppears,
}

impl fmt::Display for Retry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Never => "never",
            Self::Soon => "soon",
            Self::WhenAppears => "when_appears",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ADR-0080's "Errors" list, in its order. A code renamed, dropped or
    /// added without the ADR changing fails here.
    #[test]
    fn the_error_codes_are_adr_0080s() {
        let names: Vec<&str> = ErrorCode::ALL.iter().map(|c| c.name()).collect();
        assert_eq!(
            names,
            [
                "invalid_argument",
                "not_supported",
                "busy",
                "not_found",
                "unknown_handle",
                "gone",
                "disabled",
                "action_unsupported",
                "not_foreground",
                "focus_elsewhere",
                "outside_target",
                "timeout",
                "input_held",
                "shutting_down",
                "platform",
            ],
        );
    }

    #[cfg(feature = "serde")]
    #[test]
    fn codes_and_retry_serialize_as_their_names() {
        for &code in ErrorCode::ALL {
            assert_eq!(
                serde_json::to_value(code).ok(),
                Some(serde_json::Value::String(code.to_string()))
            );
        }
        for retry in [Retry::Never, Retry::Soon, Retry::WhenAppears] {
            assert_eq!(
                serde_json::to_value(retry).ok(),
                Some(serde_json::Value::String(retry.to_string()))
            );
        }
    }
}
