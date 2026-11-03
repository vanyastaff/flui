//! The Apple hosts' text: the window's view serving the focused field's
//! store to the input method (`client`), through AppKit's
//! `NSTextInputClient` verbs (`macos`) or UIKit's invisible `UITextInput`
//! responder (`ios`, with Dynamic Type sizing), the keyboard traits that
//! responder reports (`traits`), and macOS's secure event input
//! (`secure_input`).
//!
//! Both input methods speak UTF-16, as `NSString` counts, and so does the
//! store.

// Built on every host with `testing` too, for the conformance table's
// AppKit row (`text::conformance`), which reaches only its input-method verbs.
#[cfg(any(target_os = "macos", target_os = "ios", test, feature = "testing"))]
#[cfg_attr(
    not(any(target_os = "macos", target_os = "ios", test)),
    allow(
        dead_code,
        reason = "off Apple hosts only the conformance table's verbs are reached"
    )
)]
pub(crate) mod client;
#[cfg(target_os = "ios")]
pub(crate) mod ios;
#[cfg(target_os = "macos")]
pub(crate) mod macos;
#[cfg(any(target_os = "macos", test))]
pub(crate) mod secure_input;
#[cfg(any(target_os = "ios", test))]
pub(crate) mod traits;
