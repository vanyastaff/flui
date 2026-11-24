//! The macOS accessibility API (AX), shared by the lanes that read through it: the macOS lane,
//! which reads an app's own window, and the iOS Simulator lane, which reads the Simulator's
//! window, where the Simulator bridges the iOS app's elements into the Mac's. Each lane keeps its
//! own projection of an element onto the wire's nodes; what is here is what both must agree on.
//!
//! - [`label`]: the one rule for the name an element reads with, so the two lanes cannot read a
//!   static text's label from different attributes. It builds on every host, so the iOS lane's
//!   scripted reads apply it as the Mac's do.
//! - The client (`client`, on macOS): the C calls, their errors and the bounded walk's provider.

#[cfg(target_os = "macos")]
mod client;

#[cfg(target_os = "macos")]
pub(crate) use self::client::{
    MESSAGING_TIMEOUT_SECONDS, Visit, Walker, actions, application, attribute, boolean, child_at,
    elements, frame, name, number_text, perform, read_error, settable, string, timeout, trusted,
};

use flui_protocol::wire::Role;

/// The name an element of the wire role `role` reads with: `named`, the attribute its lane reads
/// a label from, or else, for an element that folds to static text ([`Role::Label`]), its
/// `value`, where AppKit's static text carries its text (`NSAccessibilityStaticTextRole`, whose
/// `AXValue` is the text and whose `AXTitle` is empty).
#[must_use]
pub(crate) fn label(role: Role, named: Option<String>, value: Option<String>) -> Option<String> {
    named.or_else(|| (role == Role::Label).then_some(value).flatten())
}
