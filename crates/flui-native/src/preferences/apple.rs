//! The Apple hosts' system preferences: what AppKit reports on macOS
//! (`macos`) and UIKit on iOS (`ios`), each observed through its notification
//! centres and sampled on the main thread. The two share no code beyond the
//! model's preferred-language rule.

#[cfg(target_os = "ios")]
pub(crate) mod ios;
#[cfg(target_os = "macos")]
pub(crate) mod macos;
