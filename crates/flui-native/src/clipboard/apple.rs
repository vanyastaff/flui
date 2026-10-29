//! The Apple clipboards: AppKit's `NSPasteboard`, reached through the owner
//! lane, and UIKit's `UIPasteboard`, on the main thread. They share no code.

#[cfg(target_os = "ios")]
mod ios;
#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "ios")]
pub(crate) use ios::IOSClipboard;
#[cfg(target_os = "macos")]
pub(crate) use macos::MacOSClipboard;
