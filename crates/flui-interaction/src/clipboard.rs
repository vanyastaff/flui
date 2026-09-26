//! The widget-facing plain-text clipboard.
//!
//! A presentation resolves its backend through `Platform::clipboard`
//! (ADR-0038 §9) and hands widgets a [`ClipboardHandle`] over it. The handle
//! is the value the ADR-0084 capability registry will hand out for
//! `Clipboard`, so a widget that acquires it through
//! `LifecycleContext::clipboard_handle` today changes only that one line when
//! the registry lands.

use std::{fmt, marker::PhantomData, rc::Rc, sync::Arc};

use flui_platform_api::Clipboard;

/// Widget-facing plain-text clipboard for one presentation.
///
/// Owner-thread (`!Send`), cheap to clone. The read side is callback-shaped:
/// it completes synchronously over the synchronous [`Clipboard`] substrate
/// today, and callers keep working unchanged when the ADR-0038 §6 transport
/// makes it asynchronous.
#[derive(Clone)]
pub struct ClipboardHandle {
    clipboard: Arc<dyn Clipboard>,
    _owner: PhantomData<Rc<()>>,
}

impl ClipboardHandle {
    /// A handle over `clipboard`.
    #[must_use]
    pub fn new(clipboard: Arc<dyn Clipboard>) -> Self {
        Self {
            clipboard,
            _owner: PhantomData,
        }
    }

    /// Replace the clipboard's text.
    pub fn write_text(&self, text: impl Into<String>) {
        self.clipboard.write_text(text.into());
    }

    /// Read the clipboard's text and hand it to `on_done` on the owner thread.
    ///
    /// `on_done` may run before this returns: release every borrow it needs
    /// before calling.
    pub fn read_text(&self, on_done: impl FnOnce(Option<String>) + 'static) {
        on_done(self.clipboard.read_text());
    }
}

impl fmt::Debug for ClipboardHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClipboardHandle").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use flui_platform_api::InMemoryClipboard;

    use super::*;

    #[test]
    fn a_handle_writes_through_and_reads_back_from_its_clipboard() {
        let backend = Arc::new(InMemoryClipboard::new());
        let handle = ClipboardHandle::new(backend.clone());

        handle.write_text("copied");
        assert_eq!(backend.read_text().as_deref(), Some("copied"));

        backend.write_text("external".to_owned());
        let seen = Rc::new(RefCell::new(None));
        let sink = seen.clone();
        handle.read_text(move |text| *sink.borrow_mut() = text);
        assert_eq!(seen.borrow().as_deref(), Some("external"));
    }
}
