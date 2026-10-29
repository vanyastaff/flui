//! Text input and the system text size: the feature's model beside one module
//! per platform (ADR-0151 §10).
//!
//! The model is what more than one platform uses: `ime`, what the Android
//! and web hosts mirror of a focused field (its text with a protected field
//! masked, and the one edit between two versions of it), `sizing`, the native
//! text size an owner captures for a presentation, and [`TextInput`], the
//! text position of a host's table. Beside it:
//!
//! - `windows`: the Text Services Framework (`tsf`), which reads the focused
//!   field's store, and the screen geometry its input methods are placed by;
//! - `apple`: AppKit's `NSTextInputClient` (`macos`) and UIKit's Dynamic Type
//!   sizing (`ios`);
//! - `android`: `FluiView`'s editor, which mirrors the focused field, and
//!   the system font scale;
//! - `web`: the browser's `EditContext` or a hidden textarea, which mirrors
//!   the focused field;
//! - `winit`: winit's IME enablement and cursor area;
//! - `headless`: the recording double of IME requests.
//!
//! The model's `ime` is built for the two hosts that mirror a field and for
//! tests. The key a backend's input method owns is the input feature's
//! (`input::android::keys`, `input::web::keys`), which the backend conformance
//! table in `tests/` decodes with. Each platform module is built for its own
//! target; Android's `ime` and `sizing`, the web's `ime` and the whole of
//! `windows` for tests too, which check their pure halves on every host.

use std::rc::Rc;
use std::sync::Arc;

use flui_platform::PlatformTextInput;
use flui_platform::text_store::TextStoreHost;

use crate::host::{Absent, position};

pub(crate) mod android;
#[cfg(any(target_os = "macos", target_os = "ios", test))]
pub(crate) mod apple;
pub(crate) mod headless;
#[cfg(any(target_os = "android", target_arch = "wasm32", test))]
pub(crate) mod ime;
pub(crate) mod sizing;
pub(crate) mod web;
#[cfg(any(windows, test))]
pub(crate) mod windows;
#[cfg(all(
    feature = "winit-backend",
    any(target_os = "windows", target_os = "macos", target_os = "linux")
))]
pub(crate) mod winit;

/// What one window holds of the text feature, made by its table's text
/// position (ADR-0151 §3, [`host::Text`](crate::host::Text)): the window
/// hands out what it answers through the contract's accessors
/// (`PlatformWindow::text_input`, `HostWindow::text_store_host`,
/// `HostWindow::capture_text_sizing`) and tells it when the window goes.
///
/// A platform's text input reaches the focused field in one of two ways,
/// [`WindowTextInput`]'s variants; [`Absent`] hands out none. Where the text
/// feature keeps a window's state itself, keyed by the window's identity, this
/// is the window's handle to it.
pub trait TextInput: Send + Sync + 'static {
    /// What the window hands out for text input; `None` where the platform
    /// has none. A store host is owner-thread state: read on that thread.
    fn window_text_input(&self) -> Option<WindowTextInput>;

    /// Acquire the window's native text-size converter, on its owner thread
    /// (`HostWindow::capture_text_sizing`).
    ///
    /// # Errors
    /// What [`crate::TextSizingCaptureError`] says: the wrong thread, a
    /// withdrawn presentation, a failed native capture.
    fn capture_sizing(
        &self,
    ) -> Result<crate::TextSizingCaptureState, crate::TextSizingCaptureError>;

    /// What was captured of the window's text size no longer holds (its
    /// configuration changed, its surface came or went): the next capture
    /// reads it again.
    fn invalidate_sizing(&self);

    /// No capture is handed out until the window can present again (the
    /// platform is quitting). Any thread.
    fn fence_sizing(&self);

    /// The window is going away, on its owner thread: what the text input
    /// put on it (an editor, a field the input method edits) is detached. A
    /// later call is a no-op.
    fn detach(&self);
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

    fn capture_sizing(
        &self,
    ) -> Result<crate::TextSizingCaptureState, crate::TextSizingCaptureError> {
        Ok(crate::TextSizingCaptureState::Unsupported)
    }

    fn invalidate_sizing(&self) {}

    fn fence_sizing(&self) {}

    fn detach(&self) {}
}
