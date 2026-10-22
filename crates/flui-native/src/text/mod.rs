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

/// The text position of a host's table (ADR-0151 §3): the handle a window
/// holds its text input by, which it hands out through the contract's
/// accessors (`PlatformWindow::text_input`, `HostWindow::text_store_host`).
///
/// A platform's text input reaches the focused field in one of two ways,
/// [`WindowTextInput`]'s variants; [`Absent`] hands out none.
pub trait TextInput: 'static {
    /// What the window hands out for text input; `None` where the platform
    /// has none.
    fn window_text_input(&self) -> Option<WindowTextInput>;
}

/// How a window's text input reaches the focused field.
pub enum WindowTextInput {
    /// The field tells the input method whether it may compose and where its
    /// caret is: AppKit, winit and the headless host.
    Ime(Arc<dyn PlatformTextInput>),
    /// The platform reads the field's store (TSF) or mirrors it into an
    /// editor of its own (Android's `FluiView`, the browser's `EditContext` or
    /// textarea), and the window serves the field through this host.
    Store(Rc<dyn TextStoreHost>),
}

impl WindowTextInput {
    /// The IME requests' receiver, for `PlatformWindow::text_input`; `None`
    /// for a store host.
    #[must_use]
    pub fn ime(self) -> Option<Arc<dyn PlatformTextInput>> {
        match self {
            Self::Ime(input) => Some(input),
            Self::Store(_) => None,
        }
    }

    /// The store host, for `HostWindow::text_store_host`; `None` for an IME
    /// requests' receiver.
    #[must_use]
    pub fn store_host(self) -> Option<Rc<dyn TextStoreHost>> {
        match self {
            Self::Ime(_) => None,
            Self::Store(host) => Some(host),
        }
    }
}

impl std::fmt::Debug for WindowTextInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Ime(_) => "WindowTextInput::Ime",
            Self::Store(_) => "WindowTextInput::Store",
        })
    }
}

impl TextInput for Absent<position::Text> {
    fn window_text_input(&self) -> Option<WindowTextInput> {
        None
    }
}
