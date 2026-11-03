//! The input method paths' pure halves as the backend conformance table in
//! `tests/` drives them (ADR-0090): each takes the events its platform's
//! input method sends, in that platform's terms, and makes the edits the
//! backend makes of them in the focused field's store. The platform's side
//! of the conversation (what the client sends back) is a silent stand-in.
//!
//! - [`TextInputV3`]: `zwp_text_input_v3`'s events on Wayland, which the X11
//!   host's IBus, Fcitx5 and XIM contexts are translated into as well;
//! - [`AppKitClient`]: `NSTextInputClient`'s verbs on macOS, and UIKit's on iOS;
//! - [`AndroidMirror`]: what `FluiView` reports of its editor after each
//!   `InputConnection` edit, replayed into the store as the owner replays it.
//!
//! Public and hidden from the documentation; nothing outside the crate's own
//! tests names them.

use std::rc::Rc;

use flui_platform::text_store::{TextStore, TextStoreHost};

/// A field focused on the Linux hosts' text input client, with a seat's
/// `zwp_text_input_v3` in its window.
#[cfg(any(wayland_host, x11_host))]
pub struct TextInputV3 {
    client: Rc<crate::text::linux::text_input::TextInputClient>,
    wire: Rc<v3::Wire>,
}

#[cfg(any(wayland_host, x11_host))]
impl TextInputV3 {
    /// A client serving `store`, its seat's text input entered and enabled.
    #[must_use]
    pub fn focus(store: Rc<dyn TextStore>) -> Self {
        let client = crate::text::linux::text_input::TextInputClient::new();
        let wire = Rc::new(v3::Wire::default());
        client.focus_store(Some(store));
        client.entered(Rc::clone(&wire) as Rc<dyn crate::text::linux::text_input::TextInputWire>);
        Self { client, wire }
    }

    /// `preedit_string`, its cursor `begin..end` in bytes of `text`, negative
    /// for none.
    pub fn preedit_string(&self, text: Option<&str>, begin: i32, end: i32) {
        self.client
            .preedit(&*self.wire, text.map(str::to_owned), begin, end);
    }

    /// `commit_string`.
    pub fn commit_string(&self, text: Option<&str>) {
        self.client
            .commit_string(&*self.wire, text.map(str::to_owned));
    }

    /// `delete_surrounding_text`, in bytes before and after the selection.
    pub fn delete_surrounding_text(&self, before: u32, after: u32) {
        self.client.delete_surrounding(&*self.wire, before, after);
    }

    /// `done`, its serial counting every `commit` the client sent.
    pub fn done(&self) {
        use crate::text::linux::text_input::TextInputWire as _;
        self.client.done(&*self.wire, self.wire.commits());
    }
}

#[cfg(any(wayland_host, x11_host))]
mod v3 {
    use std::cell::Cell;

    use flui_foundation::geometry::Bounds;

    use crate::text::linux::text_input::{ChangeCause, ContentType, TextInputWire};

    /// A seat's text input that takes the client's requests and counts its
    /// commits, as the compositor's serial does.
    #[derive(Default)]
    pub(super) struct Wire {
        commits: Cell<u32>,
    }

    impl TextInputWire for Wire {
        fn enable(&self) {}

        fn disable(&self) {}

        fn set_surrounding_text(&self, _text: &str, _cursor: usize, _anchor: usize) {}

        fn set_text_change_cause(&self, _cause: ChangeCause) {}

        fn set_content_type(&self, _content: ContentType) {}

        fn set_cursor_rectangle(&self, _rect: Bounds<f64>) {}

        fn commit(&self) {
            self.commits.set(self.commits.get().wrapping_add(1));
        }

        fn commits(&self) -> u32 {
            self.commits.get()
        }
    }
}

/// A field focused on the macOS content view's text input client, bound to
/// an input context.
pub struct AppKitClient {
    client: Rc<crate::text::apple::client::TextClient>,
}

impl AppKitClient {
    /// A client serving `store`, bound to an input context.
    #[must_use]
    pub fn focus(store: Rc<dyn TextStore>) -> Self {
        let client = crate::text::apple::client::TextClient::new();
        client.bind(Rc::new(appkit::Context) as Rc<dyn crate::text::apple::client::InputContext>);
        client.focus_store(Some(store));
        Self { client }
    }

    /// `setMarkedText:selectedRange:replacementRange:`, the ranges in UTF-16
    /// code units: `selected` within `text`, `replacement` within the field.
    pub fn set_marked_text(
        &self,
        text: &str,
        selected: std::ops::Range<usize>,
        replacement: Option<std::ops::Range<usize>>,
    ) {
        self.client.set_marked_text(
            text.to_owned(),
            appkit::range(selected),
            replacement.map(appkit::range),
        );
    }

    /// `insertText:replacementRange:`.
    pub fn insert_text(&self, text: &str, replacement: Option<std::ops::Range<usize>>) {
        self.client
            .insert_text(text.to_owned(), replacement.map(appkit::range));
    }

    /// `unmarkText`.
    pub fn unmark_text(&self) {
        self.client.unmark_text();
    }
}

mod appkit {
    use flui_platform::text_store::TextStoreStatus;
    use flui_platform::text_store::utf16::{Utf16Offset, Utf16Range};

    use crate::text::apple::client::{DocumentChange, InputContext, MarkedText};

    /// An input context that hears the client and answers nothing.
    pub(super) struct Context;

    impl InputContext for Context {
        fn discard_marked_text(&self) {}

        fn invalidate_character_coordinates(&self) {}

        fn field_changed(&self, _status: Option<TextStoreStatus>) {}

        fn document_changed(&self, _change: DocumentChange, _marked_text: MarkedText) {}

        fn document_will_change(&self, _change: DocumentChange) {}
    }

    pub(super) fn range(range: std::ops::Range<usize>) -> Utf16Range {
        Utf16Range::try_new(Utf16Offset::new(range.start), Utf16Offset::new(range.end))
            .expect("BUG: the table names ordered ranges")
    }
}

#[cfg(any(wayland_host, x11_host))]
impl std::fmt::Debug for TextInputV3 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextInputV3").finish_non_exhaustive()
    }
}

impl std::fmt::Debug for AppKitClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppKitClient").finish_non_exhaustive()
    }
}

/// A field focused on the Android backend's mirror: the owner published the
/// store's snapshot to `FluiView`, and each report the view makes after an
/// input method edit is replayed into the store.
pub struct AndroidMirror {
    store: Rc<dyn TextStore>,
    mirror: std::cell::RefCell<crate::text::android::ime::Mirror>,
    snapshot: crate::text::android::ime::Snapshot,
}

impl AndroidMirror {
    /// The generation of the one snapshot the owner publishes.
    const PUBLISHED: u32 = 1;

    /// The store's snapshot published to the view, which now holds it.
    ///
    /// # Panics
    ///
    /// When the store refuses a synchronous read, which a free store grants.
    #[must_use]
    pub fn focus(store: Rc<dyn TextStore>) -> Self {
        use flui_platform::text_store::{LockGrant, LockTiming};

        let read = Rc::new(std::cell::RefCell::new(None));
        let out = Rc::clone(&read);
        let _granted = store
            .request_lock(
                LockGrant::read(move |session| {
                    *out.borrow_mut() = Some(crate::text::android::ime::Snapshot::read(
                        session,
                        Self::PUBLISHED,
                    ));
                }),
                LockTiming::Sync,
            )
            .expect("BUG: the table focuses a free store, which grants a sync read");
        let snapshot = read
            .take()
            .expect("BUG: a granted sync read has run when the grant returns");
        let mut mirror = crate::text::android::ime::Mirror::default();
        mirror.hold(Self::PUBLISHED, &snapshot.text);
        Self {
            store,
            mirror: std::cell::RefCell::new(mirror),
            snapshot,
        }
    }

    /// The text of the snapshot the view holds.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.snapshot.text
    }

    /// `nativeImeState`: the view's editor holds `text`, with the selection
    /// `anchor..active` and the composing span `composing`, in UTF-16 code
    /// units. The edit it makes of the mirror is replayed into the store
    /// under an asynchronous read-write lock, as the owner replays it.
    pub fn report(&self, text: &str, selection: (usize, usize), composing: Option<(usize, usize)>) {
        use flui_platform::text_store::{LockGrant, LockTiming};

        let java = |offset: usize| i32::try_from(offset).unwrap_or(i32::MAX);
        let (composition_start, composition_end) =
            composing.map_or((-1, -1), |(start, end)| (java(start), java(end)));
        let report = crate::text::android::ime::EditorReport {
            text: text.to_owned(),
            selection_start: java(selection.0),
            selection_end: java(selection.1),
            composition_start,
            composition_end,
        };
        let Some(edit) = self.mirror.borrow_mut().report(Self::PUBLISHED, &report) else {
            return;
        };
        let grant = LockGrant::read_write(move |session| {
            let _replayed =
                crate::text::android::ime::replay_current(&edit, Self::PUBLISHED, session);
        });
        if let Err(error) = self.store.request_lock(grant, LockTiming::Async) {
            tracing::debug!(?error, "the focused store refused the input method's edit");
        }
    }
}

impl std::fmt::Debug for AndroidMirror {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AndroidMirror").finish_non_exhaustive()
    }
}
