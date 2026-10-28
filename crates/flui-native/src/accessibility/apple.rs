//! The Apple accessibility adapters: NSAccessibility on macOS, UIAccessibility
//! on iOS, and the adapter cell the iOS bridge shares with its view.

#[cfg(any(target_os = "ios", test))]
mod adapter_cell;
#[cfg(target_os = "ios")]
mod ios;
#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "ios")]
pub(crate) use ios::IosAccessibility;
#[cfg(target_os = "macos")]
pub(crate) use macos::MacosAccessibility;
