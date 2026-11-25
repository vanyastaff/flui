//! The macOS accessibility API (AX), shared by the lanes that read through it: the macOS lane,
//! which reads an app's own window, and the iOS Simulator lane, which reads the Simulator's
//! window, where the Simulator bridges the iOS app's elements into the Mac's. Each lane keeps its
//! own projection of an element onto the wire's nodes; what is here is what both must agree on.
//!
//! - [`label`]: the one rule for the name an element reads with, so the two lanes cannot read a
//!   static text's label from different attributes. It builds on every host, so the iOS lane's
//!   scripted reads apply it as the Mac's do.
//! - [`pressed`]: the one answer to an `AXPress`, its effect and its lost permission, which both
//!   lanes' presses give.
//! - The client (`client`, on macOS): the C calls, their errors and the bounded walk's provider.

#[cfg(target_os = "macos")]
mod client;

#[cfg(target_os = "macos")]
pub(crate) use self::client::{
    MESSAGING_TIMEOUT_SECONDS, Visit, Walker, actions, application, attribute, boolean, child_at,
    elements, frame, name, number_text, perform, read_error, set_messaging_timeout, settable,
    string, string_within, timeout, trusted,
};

use std::time::Duration;

use flui_protocol::act::{Effect, EffectKind};
use flui_protocol::error::{ErrorCode, HandleKind, Retry};
use flui_protocol::wire::Role;

use crate::error::{Error, Result};
use crate::lane::walk::ProviderError;

/// The deadline a call ran under, for people: `deadline`, the one the client gave the element the
/// call was asked of, or, as `None`, the timeout that element kept when it refused the client's.
pub(crate) fn deadline_clause(deadline: Option<Duration>) -> String {
    match deadline {
        Some(deadline) => format!("within {} ms", deadline.as_millis()),
        None => {
            "within the timeout the element kept when it refused the client's deadline".to_owned()
        }
    }
}

/// What pressing the element `label` through `AXPress` answered: `offered`, whether it offers
/// the action, and then `perform`, the press, made only when it does. `trusted` is asked after a
/// failure, and `untrusted` is the lane's refusal for a process that is no trusted accessibility
/// client.
///
/// Before the press is sent, a failure while the process is untrusted is `untrusted`, whatever
/// the call said. Once it is sent, the failure decides: a press not answered, or whose element
/// went away during it, may have run and says so (`may_have_run`) whatever the trust, the
/// permission taken back named in its message, so it is never sent again as if it had not; only
/// a press that failed otherwise is `untrusted` when trust is gone. A call that did not answer
/// names `deadline`, the one the client gave this element, or, as `None`, says it refused that
/// deadline and kept its own.
///
/// # Errors
///
/// `untrusted`'s error; [`Error::ActionUnsupported`] when it offers no press; [`Error::Timeout`]
/// when a call was not answered and [`Error::Gone`] when the element went away, each with
/// `may_have_run` once the press was sent; [`Error::Platform`] when a call failed otherwise.
pub(crate) fn pressed(
    offered: std::result::Result<bool, ProviderError>,
    perform: impl FnOnce() -> std::result::Result<(), ProviderError>,
    label: &str,
    deadline: Option<Duration>,
    trusted: impl Fn() -> bool,
    untrusted: impl Fn() -> Error,
) -> Result<()> {
    let gone = |when: &str, effect: Option<Effect>| Error::Gone {
        kind: Some(HandleKind::Element),
        handle: None,
        exit: None,
        effect,
        message: format!("the element {label:?} went away {when}"),
        retry: ErrorCode::Gone.default_retry(),
    };
    let offers = match offered {
        Ok(offers) => offers,
        Err(_) if !trusted() => return Err(untrusted()),
        Err(ProviderError::Timeout) => {
            return Err(Error::Timeout {
                awaited: crate::error::Awaited::Host,
                effect: None,
                message: format!(
                    "the accessibility API did not answer whether {label:?} offers a press {}",
                    deadline_clause(deadline)
                ),
                retry: ErrorCode::Timeout.default_retry(),
            });
        }
        Err(ProviderError::Gone) => return Err(gone("before it was asked for its press", None)),
        Err(ProviderError::Failed(why)) => {
            return Err(Error::Platform {
                effect: None,
                message: format!("whether {label:?} offers a press could not be read: {why}"),
                retry: Retry::Never,
            });
        }
    };
    if !offers {
        return Err(Error::ActionUnsupported {
            message: format!("the element {label:?} offers no AXPress"),
            retry: ErrorCode::ActionUnsupported.default_retry(),
        });
    }
    let Err(failure) = perform() else {
        return Ok(());
    };
    let permission = if trusted() {
        ""
    } else {
        "; the process has lost the Accessibility permission since, so grant it again under \
         System Settings > Privacy & Security > Accessibility before the next call"
    };
    let may_have_run = || Some(Effect::new(EffectKind::MayHaveRun));
    match failure {
        ProviderError::Timeout => Err(Error::Timeout {
            awaited: crate::error::Awaited::Host,
            effect: may_have_run(),
            message: format!(
                "the accessibility API did not answer the press of {label:?} {}: read the tree \
                 for its effect{permission}",
                deadline_clause(deadline)
            ),
            retry: ErrorCode::Timeout.default_retry(),
        }),
        ProviderError::Gone => Err(gone(
            &format!("while it was pressed, which may have run first{permission}"),
            may_have_run(),
        )),
        ProviderError::Failed(_) if !permission.is_empty() => Err(untrusted()),
        ProviderError::Failed(why) => Err(Error::Platform {
            effect: None,
            message: format!("pressing {label:?} failed: {why}"),
            retry: Retry::Never,
        }),
    }
}

/// The name an element of the wire role `role` reads with: `named`, the attribute its lane reads
/// a label from, or else, for an element that folds to static text ([`Role::Label`]), its
/// `value`, where AppKit's static text carries its text (`NSAccessibilityStaticTextRole`, whose
/// `AXValue` is the text and whose `AXTitle` is empty).
#[must_use]
pub(crate) fn label(role: Role, named: Option<String>, value: Option<String>) -> Option<String> {
    named.or_else(|| (role == Role::Label).then_some(value).flatten())
}
