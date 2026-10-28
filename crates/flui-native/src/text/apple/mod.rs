//! The Apple hosts' text: AppKit's text input (`macos`), the
//! `NSTextInputClient` half of the IME contract the window's view implements,
//! and UIKit's Dynamic Type sizing (`ios`).

#[cfg(any(target_os = "ios", test))]
pub(crate) mod composition;
#[cfg(target_os = "ios")]
pub(crate) mod ios;
#[cfg(target_os = "macos")]
pub(crate) mod macos;
