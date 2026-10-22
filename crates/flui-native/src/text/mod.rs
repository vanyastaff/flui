//! Text input and the system text size: the feature's model beside one module
//! per platform (ADR-0151 §10).
//!
//! The model is what more than one platform uses: [`ime`], what the Android
//! and web hosts mirror of a focused field (its text with a protected field
//! masked, and the one edit between two versions of it), `sizing`, the native
//! text size an owner captures for a presentation, and [`TextInput`], the
//! text position of a host's table. Beside it:
//!
//! - `windows`: the Text Services Framework (`tsf`), which reads the focused
//!   field's store, and the screen geometry its input methods are placed by;
//! - `apple`: AppKit's `NSTextInputClient` (`macos`) and UIKit's Dynamic Type
//!   sizing (`ios`);
//! - [`android`]: `FluiView`'s editor, which mirrors the focused field, and
//!   the system font scale;
//! - [`web`]: the browser's `EditContext` or a hidden textarea, which mirrors
//!   the focused field;
//! - `winit`: winit's IME enablement and cursor area;
//! - `headless`: the recording double of IME requests.
//!
//! The pure halves of the Android and web modules (their `ime`) are built on
//! every target and public, hidden from the documentation: the backend
//! conformance table in `tests/` decodes a key the input method owns with
//! them. Every other platform module is built for its own target, and
//! `windows` for tests too, which check its geometry on every host.

use std::rc::Rc;
use std::sync::Arc;

use flui_platform::PlatformTextInput;
use flui_platform::text_store::TextStoreHost;

use crate::host::{Absent, position};

#[doc(hidden)]
pub mod android;
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) mod apple;
pub(crate) mod headless;
#[doc(hidden)]
pub mod ime;
pub(crate) mod sizing;
#[doc(hidden)]
pub mod web;
#[cfg(any(windows, test))]
pub(crate) mod windows;
#[cfg(all(
    feature = "winit-backend",
    any(target_os = "windows", target_os = "macos", target_os = "linux")
))]
pub(crate) mod winit;

/// The text position of a host's table (ADR-0151 §3): what a window hands out
/// for text input.
///
/// An input method reaches the focused field in one of two ways, and a
/// platform's text input takes one of them:
///
/// - **IME requests**: the field tells the input method whether it may
///   compose and where its caret is ([`PlatformTextInput`]); AppKit, winit and
///   the headless host.
/// - **The field's store**: the platform reads it (TSF) or mirrors it into an
///   editor of its own (Android's `FluiView`, the browser's `EditContext` or
///   textarea), and the window serves the field through a [`TextStoreHost`].
///
/// [`Absent`] takes neither.
pub trait TextInput: 'static {
    /// This text input as the receiver of a field's IME requests; `None` when
    /// it serves the field's store instead.
    fn ime(self: Arc<Self>) -> Option<Arc<dyn PlatformTextInput>>;

    /// This text input as the host a field's store is served through; `None`
    /// when it takes IME requests instead.
    fn store_host(self: Rc<Self>) -> Option<Rc<dyn TextStoreHost>>;
}

impl TextInput for Absent<position::Text> {
    fn ime(self: Arc<Self>) -> Option<Arc<dyn PlatformTextInput>> {
        None
    }

    fn store_host(self: Rc<Self>) -> Option<Rc<dyn TextStoreHost>> {
        None
    }
}
