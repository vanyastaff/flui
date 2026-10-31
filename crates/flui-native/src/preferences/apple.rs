//! The Apple hosts' system preferences: what AppKit reports on macOS
//! (`macos`) and UIKit on iOS (`ios`), each observed through its notification
//! centres and sampled on the main thread. The two share the model's
//! preferred-language rule and Foundation's regional formats (`regional`).

#[cfg(target_os = "ios")]
pub(crate) mod ios;
#[cfg(target_os = "macos")]
pub(crate) mod macos;
pub(crate) mod regional;
