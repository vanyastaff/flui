//! The widget-facing plain-text clipboard, the seam's first capability
//! (ADR-0152).
//!
//! A widget acquires a [`ClipboardHandle`] through
//! `cx.capability::<ClipboardCapability>()` in a lifecycle hook. The marker
//! and the handle live here because framework widgets name them; the
//! operating system's clipboard reaches the handle through a
//! [`ClipboardBackend`] that the application's provider builds over the
//! host's owner-local clipboard.

use std::{fmt, rc::Rc};

use flui_platform::capability::{Capability, Completion, CompletionSource, Resolver};

/// The clipboard capability: `cx.capability::<ClipboardCapability>()` hands
/// out a [`ClipboardHandle`].
#[derive(Debug)]
#[allow(
    clippy::exhaustive_enums,
    reason = "a capability marker has no values; it is only ever named"
)]
pub enum ClipboardCapability {}

impl Capability for ClipboardCapability {
    type Handle = ClipboardHandle;
    const NAME: &'static str = "clipboard";
}

/// The operating system's plain-text clipboard as a handle reaches it.
///
/// Owner-local. A backend answers a read through its [`Resolver`] when the
/// operating system answers: at once where the clipboard reads
/// synchronously, later where it does not (the web, Wayland). It never calls
/// back into the handle.
pub trait ClipboardBackend: 'static {
    /// Read the clipboard's text and answer `answer` with it, or with `None`
    /// when it holds no text.
    fn read_text(&self, answer: Resolver<Option<String>>);

    /// Replace the clipboard's text.
    fn write_text(&self, text: String);
}

/// Widget-facing plain-text clipboard for one presentation.
///
/// Owner-thread (`!Send`), cheap to clone. A read answers with a
/// [`Completion`], which settles at an owner turn after the operating system
/// answers, so a caller reads the same way where the clipboard reads
/// synchronously and where it does not.
#[derive(Clone)]
pub struct ClipboardHandle {
    backend: Rc<dyn ClipboardBackend>,
    completions: CompletionSource,
}

impl ClipboardHandle {
    /// A handle over `backend`, whose reads settle through `completions`.
    #[must_use]
    pub fn new(backend: impl ClipboardBackend, completions: CompletionSource) -> Self {
        Self {
            backend: Rc::new(backend),
            completions,
        }
    }

    /// Replace the clipboard's text.
    pub fn write_text(&self, text: impl Into<String>) {
        self.backend.write_text(text.into());
    }

    /// Read the clipboard's text: `None` when it holds none.
    ///
    /// The completion settles at an owner turn after the operating system
    /// answers, never inside this call.
    pub fn read_text(&self) -> Completion<Option<String>> {
        let (completion, answer) = self.completions.pending();
        self.backend.read_text(answer);
        completion
    }
}

impl fmt::Debug for ClipboardHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClipboardHandle").finish_non_exhaustive()
    }
}
