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

use flui_platform::ClipboardRead;
use flui_platform::capability::{
    Capability, Completion, CompletionSource, ProviderContext, Resolver,
};

use super::shared_read::SharedRead;

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
    /// Read the clipboard's text and answer `answer`: `Ok(None)` when it
    /// holds no text, a [`ClipboardError`](flui_platform::ClipboardError)
    /// when the host could not read it. The handle asks one read at a time
    /// and shares its answer with every reader that asked meanwhile.
    fn read_text(&self, answer: Resolver<ClipboardRead>);

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
    /// The read in flight of every handle of this UI runtime.
    reads: Rc<ClipboardReads>,
}

impl ClipboardHandle {
    /// A handle over `backend` for the acquisition `cx`: its reads settle
    /// through the UI runtime's completions, and share the read in flight
    /// with every other handle of that runtime.
    #[must_use]
    pub fn new(backend: impl ClipboardBackend, cx: &ProviderContext<'_>) -> Self {
        Self {
            backend: Rc::new(backend),
            completions: cx.completions().clone(),
            reads: cx.runtime_local::<ClipboardReads>(),
        }
    }

    /// Replace the clipboard's text.
    pub fn write_text(&self, text: impl Into<String>) {
        self.backend.write_text(text.into());
    }

    /// Read the clipboard's text: `Ok(None)` when it holds none, an error
    /// when the host could not read it.
    ///
    /// Reads asked while one is in flight join it and get the same answer,
    /// one shared copy of the text (ADR-0152 §5).
    ///
    /// The completion settles at an owner turn after the operating system
    /// answers, never inside this call.
    pub fn read_text(&self) -> Completion<ClipboardRead> {
        self.reads
            .0
            .read(&self.completions, |answer| self.backend.read_text(answer))
    }
}

impl fmt::Debug for ClipboardHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClipboardHandle").finish_non_exhaustive()
    }
}

/// The slot of the clipboard reads in flight a UI runtime keeps
/// (ADR-0152 §5).
#[derive(Default)]
struct ClipboardReads(SharedRead);
