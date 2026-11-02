//! Drag and drop: the OS transport behind the data-transfer contract
//! (ADR-0038, ADR-0211, ADR-0151 §10).
//!
//! The model is `offers`: the offer table every source mints its ids in, the
//! producer half of a delivery, the URI list the hosts that read paths or
//! URIs share, and what a drag that names its representations by media type
//! offers and the payload their bytes carry, which the Android and Web files
//! share. Beside it, one file per platform holds the host's one
//! `DataTransferSource` (the drag position of its table) and what a window
//! needs to take and start drags:
//!
//! - `windows`: OLE;
//! - `android`: `FluiView`'s drag events and system drags;
//! - `web`: HTML drag and drop;
//! - `winit`: the files winit reports dragged and dropped;
//! - `x11`: XDND drops on the native X11 host's windows, and the sessions they
//!   start;
//! - `headless`: a double that plays the operating system's side.
//!
//! What the first three decide with no OS call (what crosses OLE, what a
//! clip offers and holds, how `DataTransfer` types map) is a module of the
//! file under `forbid(unsafe_code)`, and the file is compiled under
//! `cfg(any(<os>, test))` with its OS side under `cfg(<os>)`, so every host
//! checks it.
//!
//! macOS, iOS and the native Wayland host have no drag and drop yet: their
//! table names `Absent<Drag>`.

#[cfg(any(target_os = "android", test))]
pub(crate) mod android;
pub(crate) mod headless;
pub(crate) mod offers;
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) mod web;
#[cfg(any(windows, test))]
pub(crate) mod windows;
#[cfg(all(
    feature = "winit-backend",
    any(target_os = "windows", target_os = "macos", target_os = "linux")
))]
pub(crate) mod winit;
#[cfg(x11_host)]
pub(crate) mod x11;
