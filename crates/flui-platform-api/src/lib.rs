//! Platform contracts for FLUI.
//!
//! This crate holds what the framework, plugins and tests program against
//! when they talk to the platform, and nothing that implements it:
//!
//! - the capability traits [`PlatformTextInput`], [`PlatformHaptics`],
//!   [`PlatformDisplay`], [`Clipboard`] and the data-transfer transport
//!   ([`data_transfer::DataTransferSource`], ADR-0038);
//! - the input vocabulary ([`PlatformInput`], [`DispatchEventResult`],
//!   [`DragDropEvent`] and the conversion helpers);
//! - the window vocabulary ([`WindowId`], [`WindowOptions`], [`WindowMode`],
//!   [`WindowEvent`], [`WindowExecutionState`] and the errors window
//!   operations return).
//!
//! No OS, winit, AccessKit or tokio type may appear here (ADR-0082 §1). That
//! is the point of the crate: naming a contract must not link a backend, so a
//! framework crate or plugin that names or implements a capability depends on
//! this crate, stays free of every OS stack and builds without
//! `flui-platform`.
//!
//! The per-window contract `PlatformWindow`, the host-facing `Platform`
//! trait, the owner-thread capability and every OS backend live in
//! `flui-platform`, which re-exports everything defined here at its old
//! paths. Only composition roots depend on `flui-platform` (ADR-0082 §2).
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
//! use flui_types::HapticFeedback;
//! use flui_types::geometry::{Bounds, Pixels, Point, Size, px};
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
//!     fn set_ime_cursor_area(&self, _area: Bounds<Pixels>) {}
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
//!     Point::new(px(0.0), px(0.0)),
//!     Size::new(px(10.0), px(20.0)),
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

mod clipboard;
pub mod data_transfer;
mod display;
mod haptics;
mod input;
mod text_input;
mod window;

pub use clipboard::{Clipboard, ClipboardItem};
pub use data_transfer::{DataTransferOffer, DataTransferSource, NullDataTransferSource};
pub use display::{DisplayId, PlatformDisplay};
pub use haptics::PlatformHaptics;
pub use input::{
    DispatchEventResult, DragDropEvent, Key, KeyboardEvent, Modifiers, PlatformInput,
    PointerButton, PointerButtons, PointerEvent, PointerId, PointerType, PointerUpdate,
    ScrollDelta, delta_offset_from_coords, device_to_logical, logical_to_device,
    offset_from_coords,
};
pub use text_input::PlatformTextInput;
pub use window::{
    CursorError, WindowAppearance, WindowBackgroundAppearance, WindowBounds, WindowEvent,
    WindowExecutionState, WindowId, WindowMode, WindowOptions, WindowReveal, WindowShowError,
};
