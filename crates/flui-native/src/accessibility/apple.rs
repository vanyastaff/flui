//! The Apple accessibility adapters: NSAccessibility on macOS. UIKit windows
//! hand out none yet.

mod macos;

pub(crate) use macos::MacosAccessibility;
