//! The fixed error codes of ADR-0080 "Errors" and ADR-0238 §3, whether a failed call can
//! succeed later, and the error object that carries them.

use std::fmt;

use crate::act::{ActionabilityCheck, Effect};
use crate::version::ProtocolVersion;

vocabulary! {
    /// Why a call failed: one fixed code an agent branches on, the message
    /// beside it being for people. ADR-0080 fixed the list and its spelling,
    /// and ADR-0238 added four; a new code is additive.
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
        /// A strict locator matched more than one element; `candidates` lists the first ten
        /// (ADR-0238 §3).
        Ambiguous => "ambiguous",
        /// An actionability check never held within the timeout; `failed` names it and
        /// `covered_by` what the hit-test reached instead.
        NotActionable => "not_actionable",
        /// A handle from an earlier generation that is still unknown after re-reading; unlike
        /// `gone`, the agent did not see its node removed.
        Stale => "stale",
        /// Client and app share no protocol version; `app` and `client` say which each speaks.
        Version => "version",
        /// The platform failed in a way no other code names. A code a newer
        /// schema added reads as this one.
        #[cfg_attr(feature = "serde", serde(other))]
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

vocabulary! {
    /// What kind of handle an error names.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(
        feature = "serde",
        derive(serde::Serialize, serde::Deserialize),
        serde(rename_all = "snake_case")
    )]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum HandleKind {
        /// An element, `e12` or `w3/e12`.
        Element => "element",
        /// A window, `w3`.
        Window => "window",
        /// The app's process.
        Process => "process",
        /// An event subscription.
        Subscription => "subscription",
    }
}

/// ADR-0080's error object, `{code, message, retry, effect?, ...fields}`, with the fields
/// ADR-0238's codes carry. A field a code does not use is left out.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct ErrorObject {
    /// What went wrong, for the caller to branch on.
    pub code: ErrorCode,
    /// What went wrong, for people.
    pub message: String,
    /// Whether the same call can succeed later.
    pub retry: Retry,
    /// How much of an action reached the app, when any of it did.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub effect: Option<Effect>,
    /// The kind of handle that is gone, stale or unknown.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub kind: Option<HandleKind>,
    /// That handle, as the caller sent it.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub handle: Option<String>,
    /// The process's exit code, for a `gone` process that reported one.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub exit: Option<i32>,
    /// For `ambiguous`, the first ten matches as outline lines; for `not_found`, the nearest
    /// elements by role, name and identifier.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub candidates: Vec<String>,
    /// For `not_actionable`, the check that never held.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub failed: Option<ActionabilityCheck>,
    /// For `not_actionable`, what the hit-test reached instead, as its outline line.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub covered_by: Option<String>,
    /// For `not_supported`, the capability the target lacks.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub capability: Option<String>,
    /// For `version`, the schema the app speaks.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub app: Option<ProtocolVersion>,
    /// For `version`, the schema the client asked for.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub client: Option<ProtocolVersion>,
}

impl ErrorObject {
    /// `code` with `message`, and `retry` as the code implies.
    #[must_use]
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            retry: code.default_retry(),
            effect: None,
            kind: None,
            handle: None,
            exit: None,
            candidates: Vec::new(),
            failed: None,
            covered_by: None,
            capability: None,
            app: None,
            client: None,
        }
    }

    /// `ambiguous`, listing `candidates`.
    #[must_use]
    pub fn ambiguous(message: impl Into<String>, candidates: Vec<String>) -> Self {
        Self {
            candidates,
            ..Self::new(ErrorCode::Ambiguous, message)
        }
    }

    /// `not_actionable`: `failed` never held, and the point reached `covered_by`.
    #[must_use]
    pub fn not_actionable(
        message: impl Into<String>,
        failed: ActionabilityCheck,
        covered_by: Option<String>,
    ) -> Self {
        Self {
            failed: Some(failed),
            covered_by,
            ..Self::new(ErrorCode::NotActionable, message)
        }
    }

    /// `version`: the app speaks `app`, the client asked for `client`.
    #[must_use]
    pub fn version(app: ProtocolVersion, client: ProtocolVersion) -> Self {
        Self {
            app: Some(app),
            client: Some(client),
            ..Self::new(
                ErrorCode::Version,
                format!("the app speaks protocol {app}, the client {client}"),
            )
        }
    }

    /// The same error, with `retry`.
    #[must_use]
    pub fn with_retry(mut self, retry: Retry) -> Self {
        self.retry = retry;
        self
    }

    /// The same error, with `effect`.
    #[must_use]
    pub fn with_effect(mut self, effect: Effect) -> Self {
        self.effect = Some(effect);
        self
    }

    /// The same error, naming `handle` of `kind`.
    #[must_use]
    pub fn with_handle(mut self, kind: HandleKind, handle: impl Into<String>) -> Self {
        self.kind = Some(kind);
        self.handle = Some(handle.into());
        self
    }
}

impl ErrorCode {
    /// Whether a call failing with this code can succeed later, when the failure says nothing
    /// more: `busy` soon, `not_found` once something appears, the rest not unchanged.
    #[must_use]
    pub const fn default_retry(self) -> Retry {
        match self {
            Self::Busy | Self::InputHeld => Retry::Soon,
            Self::NotFound | Self::NotActionable => Retry::WhenAppears,
            _ => Retry::Never,
        }
    }
}

impl fmt::Display for ErrorObject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ErrorObject {}

#[cfg(test)]
mod tests {
    use super::*;

    /// ADR-0080's "Errors" list in its order, then ADR-0238's four, the catch-all last.
    /// A code renamed, dropped or added without an ADR changing fails here.
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
                "ambiguous",
                "not_actionable",
                "stale",
                "version",
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
        assert_eq!(
            serde_json::from_value::<ErrorCode>(serde_json::json!("rate_limited")).ok(),
            Some(ErrorCode::Platform),
            "a code a newer schema added reads as the catch-all"
        );
        for retry in [Retry::Never, Retry::Soon, Retry::WhenAppears] {
            assert_eq!(
                serde_json::to_value(retry).ok(),
                Some(serde_json::Value::String(retry.to_string()))
            );
        }
    }
}
