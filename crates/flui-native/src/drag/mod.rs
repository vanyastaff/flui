//! Drag and drop: the OS transport behind the data-transfer contract
//! (ADR-0038, ADR-0211, ADR-0151 §10).
//!
//! The model is `offers`: what a drag that names its representations by media
//! type offers, and the payload their bytes carry, which the Android and Web
//! files share. Beside it, one file per platform holds the host's one
//! `DataTransferSource` (the drag position of its table) and what a window
//! needs to take and start drags:
//!
//! - `windows`: OLE, its decidable half checked on every host;
//! - `android`: `FluiView`'s drag events and system drags, likewise;
//! - `web`: HTML drag and drop, likewise;
//! - `winit`: the files winit reports dragged and dropped;
//! - `headless`: a double that plays the operating system's side.
//!
//! macOS and iOS have no drag and drop yet: their table names `Absent<Drag>`.

#[cfg(any(target_os = "android", test))]
pub(crate) mod android;
pub(crate) mod headless;
#[cfg(any(target_os = "android", target_arch = "wasm32", test))]
mod offers;
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) mod web;
#[cfg(any(windows, test))]
pub(crate) mod windows;
#[cfg(all(
    feature = "winit-backend",
    any(target_os = "windows", target_os = "macos", target_os = "linux")
))]
pub(crate) mod winit;
