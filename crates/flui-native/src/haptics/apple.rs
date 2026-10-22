//! The Apple actuators: AppKit's haptic feedback manager on macOS, UIKit's
//! feedback generators and Core Haptics on iOS. They share no code.

#[cfg(target_os = "ios")]
mod ios;
#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "ios")]
pub(crate) use ios::IOSHaptics;
#[cfg(target_os = "macos")]
pub(crate) use macos::MacHaptics;
