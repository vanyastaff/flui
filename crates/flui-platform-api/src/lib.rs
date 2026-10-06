//! Platform contracts for FLUI.
//!
//! This crate holds what the framework, plugins and tests program against
//! when they talk to the platform, and nothing that implements it:
//!
//! - the capability traits [`PlatformTextInput`], [`PlatformHaptics`],
//!   [`PlatformDisplay`], [`Clipboard`] and the data-transfer transport
//!   ([`data_transfer::DataTransferSource`], ADR-0038);
//! - the byte-storage capability [`Storage`] and its vocabulary
//!   ([`StorageName`], [`StoredVersion`], [`WriteMode`], [`StorageError`]);
//! - the text store an input method pulls from ([`TextStore`] and the rest of
//!   [`text_store`], ADR-0090);
//! - the input vocabulary ([`PlatformInput`], [`DispatchEventResult`],
//!   [`DragDropEvent`] and the conversion helpers);
//! - the per-window contract [`PlatformWindow`] and the window vocabulary
//!   ([`WindowId`], [`WindowOptions`], [`WindowMode`], [`WindowEvent`],
//!   [`WindowExecutionState`], [`CursorIcon`] and the errors window
//!   operations return).
//!
//! No OS, winit, AccessKit or tokio type may appear here (ADR-0082 §1). That
//! is the point of the crate: naming a contract must not link a backend, so a
//! framework crate or plugin that names or implements a capability depends on
//! this crate, stays free of every OS stack and builds without
//! `flui-platform`. The one AccessKit-speaking window capability, the
//! accessibility bridge, is reached through `flui_platform::HostWindow`, a
//! subtrait of [`PlatformWindow`] that only the composition root sees.
//!
//! The host-facing `Platform` trait, the owner-thread capability, the
//! host-side window subtrait and every OS backend live in `flui-platform`,
//! which re-exports everything defined here at its old paths. Only
//! composition roots depend on `flui-platform` (ADR-0082 §2).
//!
//! The pointer and keyboard types re-exported from `ui-events` (and, through
//! it, `keyboard-types`) are ADR-0089 debt: this crate's own types replace
//! them before its first release.
//!
//! # Implementing a capability without a backend
//!
//! ```
//! use std::sync::{Arc, Mutex};
//!
//! use flui_platform_api::{PlatformHaptics, PlatformTextInput};
//! use flui_platform_api::HapticFeedback;
//! use flui_foundation::geometry::{Bounds, Point, Size};
//!
//! #[derive(Default)]
//! struct Recorder {
//!     ime_allowed: Mutex<Vec<bool>>,
//!     feedback: Mutex<Vec<HapticFeedback>>,
//! }
//!
//! impl PlatformTextInput for Recorder {
//!     fn set_ime_allowed(&self, allowed: bool) {
//!         self.ime_allowed.lock().expect("unpoisoned").push(allowed);
//!     }
//!
//!     fn set_ime_cursor_area(&self, _area: Bounds) {}
//! }
//!
//! impl PlatformHaptics for Recorder {
//!     fn perform(&self, feedback: HapticFeedback) {
//!         self.feedback.lock().expect("unpoisoned").push(feedback);
//!     }
//!
//!     fn as_any(&self) -> &dyn std::any::Any {
//!         self
//!     }
//! }
//!
//! let recorder = Arc::new(Recorder::default());
//! let text_input: Arc<dyn PlatformTextInput> = recorder.clone();
//! let haptics: Arc<dyn PlatformHaptics> = recorder.clone();
//!
//! text_input.set_ime_allowed(true);
//! text_input.set_ime_cursor_area(Bounds::new(
//!     Point::new(0.0, 0.0),
//!     Size::new(10.0, 20.0),
//! ));
//! haptics.perform(HapticFeedback::LightImpact);
//!
//! assert_eq!(*recorder.ime_allowed.lock().expect("unpoisoned"), [true]);
//! assert_eq!(
//!     *recorder.feedback.lock().expect("unpoisoned"),
//!     [HapticFeedback::LightImpact]
//! );
//! ```

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod brightness;
mod clipboard;
pub mod data_transfer;
mod display;
mod haptic_feedback;
mod haptics;
mod ime;
mod input;
mod locale;
mod platform_window;
mod storage;
mod target_platform;
mod text_input;
pub mod text_store;
mod window;

pub use brightness::Brightness;
pub use clipboard::{Clipboard, ClipboardItem, InMemoryClipboard};
/// The pointer-cursor shape [`PlatformWindow::set_cursor`] takes: the
/// `cursor-icon` crate's, which ADR-0089 allows in stable signatures.
pub use cursor_icon::CursorIcon;
pub use data_transfer::{DataTransferOffer, DataTransferSource, NullDataTransferSource};
pub use display::{DisplayId, PlatformDisplay};
pub use haptic_feedback::HapticFeedback;
pub use haptics::PlatformHaptics;
pub use ime::ImeEvent;
pub use input::{
    DispatchEventResult, DragDropEvent, Key, KeyboardEvent, Modifiers, PlatformInput,
    PointerButton, PointerButtons, PointerEvent, PointerId, PointerType, PointerUpdate,
    ScrollDelta, delta_offset_from_coords, device_to_logical, logical_to_device,
    offset_from_coords,
};
pub use locale::Locale;
pub use platform_window::PlatformWindow;
pub use storage::{
    Storage, StorageError, StorageFuture, StorageName, Stored, StoredVersion, WriteMode,
};
pub use target_platform::TargetPlatform;
pub use text_input::PlatformTextInput;
pub use text_store::{TextStore, TextStoreEdit, TextStoreObserver, TextStoreRead};
pub use window::{
    CursorError, WindowAppearance, WindowBackgroundAppearance, WindowBounds, WindowEvent,
    WindowExecutionState, WindowId, WindowMode, WindowOptions, WindowReveal, WindowShowError,
};
