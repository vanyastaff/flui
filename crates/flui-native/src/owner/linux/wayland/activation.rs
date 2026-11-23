//! What the loop and its windows say of an activation (`xdg-activation`):
//! the token a request is answered with, and the token a launcher
//! handed the process. The user input a token names is
//! [`UserInput`](super::user_input::UserInput).

#![forbid(unsafe_code)]

use std::ffi::OsString;

/// An activation token: what the compositor answered a token request
/// with, or what a launcher handed the process. Opaque; only the
/// compositor reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActivationToken(String);

impl ActivationToken {
    /// The token the compositor sent.
    pub(crate) const fn new(token: String) -> Self {
        Self(token)
    }

    /// The token, as `xdg_activation_v1.activate` takes it.
    pub(crate) fn into_string(self) -> String {
        self.0
    }
}

/// The variable a launcher hands its process's activation token in.
pub(crate) const LAUNCH_TOKEN_VARIABLE: &str = "XDG_ACTIVATION_TOKEN";

/// The token a launcher handed the process, from the value of
/// [`LAUNCH_TOKEN_VARIABLE`]: none where it is unset, empty or not text.
pub(crate) fn launch_token(value: Option<OsString>) -> Option<ActivationToken> {
    value
        .and_then(|value| value.into_string().ok())
        .filter(|token| !token.is_empty())
        .map(ActivationToken)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A launcher's token is the variable's text; unset, empty or not
    /// text is none.
    #[test]
    fn a_launch_token_is_the_variables_text() {
        assert_eq!(
            launch_token(Some("launcher-1".into())),
            Some(ActivationToken::new("launcher-1".to_owned()))
        );
        assert_eq!(launch_token(Some(OsString::new())), None);
        assert_eq!(launch_token(None), None);
        assert_eq!(
            LAUNCH_TOKEN_VARIABLE, "XDG_ACTIVATION_TOKEN",
            "the variable launchers set"
        );
        assert_eq!(
            ActivationToken::new("handed".to_owned()).into_string(),
            "handed",
            "the compositor gets back what it sent"
        );
    }
}
