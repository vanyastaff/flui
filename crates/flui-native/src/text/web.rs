//! The web host's text input (ADR-0090, ADR-0135): the focused field's store
//! mirrored into an `EditContext` on the canvas, or a hidden textarea beside
//! it where the browser has none, and served to the browser's input method.
//!
//! - `ime`: the decidable half, which virtual keyboard a field asks for and
//!   what the browser fills it with;
//!   pure, built for the web and for tests on every host;
//! - `input`: `WebTextInput`, the window's `TextStoreHost` over the DOM.

#[cfg(any(target_arch = "wasm32", test))]
mod ime {
    //! The decidable half of the web backend's text input (ADR-0090): which
    //! virtual keyboard a field asks for, what the browser fills it with and
    //! how its Enter key is labelled. What a hidden `<textarea>`'s change
    //! was is the mirror's one edit ([`crate::text::ime::diff`]); which key
    //! belongs to the input method is `input::web::keys`'s. Cfg-free plain
    //! functions, so Linux CI pins them; `text::web::input` reads the DOM.

    #![forbid(unsafe_code)]

    use flui_platform::text_store::{TextInputPurpose, TextStoreStatus};

    /// The `inputmode` (and `EditContext.inputMode`) a field of `purpose` asks the
    /// browser's virtual keyboard for.
    #[must_use]
    pub(crate) const fn input_mode(purpose: TextInputPurpose) -> &'static str {
        match purpose {
            TextInputPurpose::Email => "email",
            TextInputPurpose::Url => "url",
            TextInputPurpose::Phone => "tel",
            TextInputPurpose::Number => "numeric",
            TextInputPurpose::Search => "search",
            // Free text, and any purpose added later that has no keyboard here.
            _ => "text",
        }
    }

    /// The `autocomplete` an editor of a field of `status` gives the browser.
    /// The hidden editor beside the canvas offers nothing to fill, and a
    /// protected one tells a password manager it is no login field. The
    /// semantics host's field (`native`), which a screen reader user reads and
    /// edits as the page's own, is filled as a native field of its purpose
    /// would be: with an address, a phone number or a password.
    #[must_use]
    pub(crate) const fn autocomplete(status: &TextStoreStatus, native: bool) -> &'static str {
        match (native, status.protected, status.purpose) {
            (false, true, _) => "new-password",
            (true, true, _) => "current-password",
            (true, false, TextInputPurpose::Email) => "email",
            (true, false, TextInputPurpose::Url) => "url",
            (true, false, TextInputPurpose::Phone) => "tel",
            // Free text, a number or a query has nothing to fill it with.
            _ => "off",
        }
    }

    /// The `enterkeyhint` a field of `status` labels the virtual keyboard's
    /// Enter key with: a search for a query, a new line for a field of many
    /// lines; `None` leaves the browser's own.
    #[must_use]
    pub(crate) const fn enter_key_hint(status: &TextStoreStatus) -> Option<&'static str> {
        match status.purpose {
            TextInputPurpose::Search => Some("search"),
            _ if !status.single_line => Some("enter"),
            _ => None,
        }
    }

    const LINE_FEED: u16 = 0x0A;
    const CARRIAGE_RETURN: u16 = 0x0D;

    /// What a field's element shows of a store's text, and where each of
    /// its offsets is in the store. The browser sanitizes the value a script
    /// gives a field (HTML's value sanitization): a single-line input drops
    /// line feeds and carriage returns, and a textarea shows one line feed
    /// for a CRLF and one for a lone carriage return. The store keeps its
    /// text: an edit of the shown value is the edit of the store's code
    /// units it covers, a CRLF's one shown line feed covering both.
    #[derive(Debug, Clone, Default, PartialEq, Eq)]
    pub(crate) struct FieldValue {
        shown: Vec<u16>,
        /// The store range each shown code unit stands for.
        spans: Vec<(usize, usize)>,
        /// The store's text.
        store: Vec<u16>,
        single_line: bool,
    }

    impl FieldValue {
        /// What a `single_line` input, or else a textarea, shows of `store`.
        #[must_use]
        pub(crate) fn of(store: &[u16], single_line: bool) -> Self {
            let mut shown = Vec::with_capacity(store.len());
            let mut spans = Vec::with_capacity(store.len());
            let mut units = store.iter().copied().enumerate().peekable();
            while let Some((index, unit)) = units.next() {
                let (unit, end) = match unit {
                    LINE_FEED | CARRIAGE_RETURN if single_line => continue,
                    CARRIAGE_RETURN if units.next_if(|&(_, next)| next == LINE_FEED).is_some() => {
                        (LINE_FEED, index + 2)
                    }
                    CARRIAGE_RETURN => (LINE_FEED, index + 1),
                    unit => (unit, index + 1),
                };
                shown.push(unit);
                spans.push((index, end));
            }
            Self {
                shown,
                spans,
                store: store.to_vec(),
                single_line,
            }
        }

        /// The element's value, in UTF-16 code units.
        #[must_use]
        pub(crate) fn shown(&self) -> &[u16] {
            &self.shown
        }

        /// The store range the shown range between `start` and `end`
        /// covers, ordered and clamped to the shown text: it starts past
        /// the units the field drops before `start` and ends before those
        /// it drops after `end`, so an edit next to a dropped unit keeps
        /// it. A collapsed range is the caret past the dropped units.
        #[must_use]
        pub(crate) fn store_range(&self, start: usize, end: usize) -> (usize, usize) {
            let shown = self.shown.len();
            let (start, end) = (start.min(end).min(shown), start.max(end).min(shown));
            let from = self
                .spans
                .get(start)
                .map_or(self.store.len(), |&(from, _)| from);
            let to = end
                .checked_sub(1)
                .and_then(|before| self.spans.get(before))
                .map_or(0, |&(_, to)| to);
            (from, to.max(from))
        }

        /// The shown text's edit of `start..old_end` into `inserted`, as an
        /// edit of the store: the store range it replaces
        /// ([`Self::store_range`]), after which `self` shows the edited
        /// store as the browser would hold it, which can differ from what
        /// the browser holds after the edit (a line feed typed after a lone
        /// carriage return makes a CRLF, shown as one).
        pub(crate) fn replace(
            &mut self,
            start: usize,
            old_end: usize,
            inserted: &[u16],
        ) -> (usize, usize) {
            let (from, to) = self.store_range(start, old_end);
            self.store.splice(from..to, inserted.iter().copied());
            *self = Self::of(&self.store, self.single_line);
            (from, to)
        }

        /// The shown offset of the store's `offset`: the units the field
        /// drops before it are not counted, and an offset inside a CRLF is
        /// before its line feed.
        #[must_use]
        pub(crate) fn shown_offset(&self, offset: usize) -> usize {
            self.spans.partition_point(|&(_, end)| end <= offset)
        }
    }

    #[cfg(test)]
    mod tests;
}

#[cfg(target_arch = "wasm32")]
mod input {
    //! The web window's text input: the field's `TextStore` (ADR-0090) served
    //! to the browser's input method through an `EditContext` on the canvas
    //! where the browser has one (Chromium), and through a hidden `<textarea>`
    //! beside the canvas where it does not. While the semantics host mirrors
    //! the tree's focus as a text field (`FocusedField`), the store is served
    //! through that `<input>` or `<textarea>` instead, so a screen reader reads
    //! and edits the field the input method edits: the host places it and
    //! routes its focus and keys, and the input writes its value from the
    //! store, which takes every edit first. The field gone, or named no more,
    //! the store moves back to the canvas.
    //!
    //! [`WebTextInput`] is the window's `TextStoreHost` (ADR-0135): the
    //! presentation names the focused field's store, and the host mirrors that
    //! store into its surface. The browser edits the surface's copy, and every
    //! edit it reports (`textupdate`, or a textarea's `input`) is applied to the
    //! store under an asynchronous lock, the composition marked as composing
    //! until `compositionend`; what the application changes reaches the surface
    //! through the store's observer. Offsets are UTF-16 code units on both sides.
    //!
    //! A key the input method processes stays the input method's (decided by
    //! `input::web::keys::key_composition`): the window delivers it
    //! marked composing while the surface composes, so no shortcut or drag
    //! cancellation takes Escape from the input method. Keys the application
    //! handles have their default prevented, so the browser does not also type
    //! them into the surface.
    //!
    //! The textarea is part of the canvas's focus area: it is named on the
    //! canvas's claim, focus moving between the two is not a window focus
    //! change, its keys reach the window as the canvas's do, and focusing the
    //! canvas forwards focus to it while a field is focused.

    use std::cell::{Cell, RefCell};

    use std::rc::{Rc, Weak};

    use flui_platform::keyboard::KeyState;
    use flui_platform::text_store::Composition;
    use flui_platform::text_store::{
        CompositionEnd, LockGrant, LockTiming, Selection, TextChange, TextStore, TextStoreEdit,
        TextStoreHost, TextStoreHostError, TextStoreObserver, TextStoreStatus, Utf16Offset,
        Utf16Range, commit_composition_in_place,
    };
    use wasm_bindgen::{JsCast, prelude::*};

    use super::ime::{FieldValue, autocomplete, enter_key_hint, input_mode};
    use crate::owner::panic_boundary::contain_owner_callback;
    use crate::owner::slot::OwnerSlot;
    use crate::owner::web::listener::{CanvasListener, viewport_listeners};
    use crate::owner::web::{Accepting, CanvasEditor, CanvasKeys, CanvasRef, FocusedField};
    use crate::text::host_queue::{HostQueue, MirrorHostOp};
    use crate::text::ime::{diff, document_text, scalar_spans};

    #[wasm_bindgen(inline_js = "
    export function hasEditContext() { return typeof globalThis.EditContext === 'function'; }
    export function createEditContext(text, start, end) {
        return new EditContext({ text, selectionStart: start, selectionEnd: end });
    }
    export function attachEditContext(element, context) { element.editContext = context; }
    ")]
    extern "C" {
        #[wasm_bindgen(js_name = hasEditContext)]
        fn has_edit_context() -> bool;
        #[wasm_bindgen(catch, js_name = createEditContext)]
        fn create_edit_context(text: &str, start: u32, end: u32) -> Result<EditContext, JsValue>;
        #[wasm_bindgen(catch, js_name = attachEditContext)]
        fn attach_edit_context(
            element: &web_sys::HtmlElement,
            context: &JsValue,
        ) -> Result<(), JsValue>;

        /// The EditContext API, which web-sys does not bind.
        #[wasm_bindgen(extends = web_sys::EventTarget)]
        type EditContext;
        #[wasm_bindgen(method, catch, js_name = updateText)]
        fn update_text(this: &EditContext, start: u32, end: u32, text: &str)
        -> Result<(), JsValue>;
        #[wasm_bindgen(method, catch, js_name = updateSelection)]
        fn update_selection(this: &EditContext, start: u32, end: u32) -> Result<(), JsValue>;
        #[wasm_bindgen(method, catch, js_name = updateControlBounds)]
        fn update_control_bounds(
            this: &EditContext,
            bounds: &web_sys::DomRect,
        ) -> Result<(), JsValue>;
        #[wasm_bindgen(method, catch, js_name = updateSelectionBounds)]
        fn update_selection_bounds(
            this: &EditContext,
            bounds: &web_sys::DomRect,
        ) -> Result<(), JsValue>;
        #[wasm_bindgen(method, catch, js_name = updateCharacterBounds)]
        fn update_character_bounds(
            this: &EditContext,
            range_start: u32,
            bounds: &js_sys::Array,
        ) -> Result<(), JsValue>;

        /// `TextUpdateEvent`.
        type TextUpdate;
        #[wasm_bindgen(method, getter, js_name = updateRangeStart)]
        fn update_range_start(this: &TextUpdate) -> u32;
        #[wasm_bindgen(method, getter, js_name = updateRangeEnd)]
        fn update_range_end(this: &TextUpdate) -> u32;
        #[wasm_bindgen(method, getter, js_name = text)]
        fn update_text_value(this: &TextUpdate) -> String;
        #[wasm_bindgen(method, getter, js_name = selectionStart)]
        fn update_selection_start(this: &TextUpdate) -> u32;
        #[wasm_bindgen(method, getter, js_name = selectionEnd)]
        fn update_selection_end(this: &TextUpdate) -> u32;

        /// `CharacterBoundsUpdateEvent`.
        type CharacterBoundsUpdate;
        #[wasm_bindgen(method, getter, js_name = rangeStart)]
        fn bounds_range_start(this: &CharacterBoundsUpdate) -> u32;
        #[wasm_bindgen(method, getter, js_name = rangeEnd)]
        fn bounds_range_end(this: &CharacterBoundsUpdate) -> u32;
    }

    /// What the browser's input method edits.
    enum Surface {
        /// An `EditContext` on the canvas.
        Context(EditContext),
        /// An element the browser edits, with what it last showed of the
        /// store's text: the hidden textarea the input keeps beside the
        /// canvas (`hidden`), or the semantics host's field, which the host
        /// keeps.
        Editor {
            element: EditorElement,
            hidden: bool,
            mirror: RefCell<FieldValue>,
        },
    }

    /// An editor element: a `<textarea>`, or a single-line `<input>`.
    #[derive(Clone)]
    enum EditorElement {
        Area(web_sys::HtmlTextAreaElement),
        Line(web_sys::HtmlInputElement),
    }

    impl EditorElement {
        /// `element` as an editor; `None` when it is neither.
        fn of(element: &web_sys::HtmlElement) -> Option<Self> {
            if let Some(area) = element.dyn_ref::<web_sys::HtmlTextAreaElement>() {
                Some(Self::Area(area.clone()))
            } else {
                element
                    .dyn_ref::<web_sys::HtmlInputElement>()
                    .map(|line| Self::Line(line.clone()))
            }
        }

        fn html(&self) -> &web_sys::HtmlElement {
            match self {
                Self::Area(area) => area,
                Self::Line(line) => line,
            }
        }

        fn value(&self) -> String {
            match self {
                Self::Area(area) => area.value(),
                Self::Line(line) => line.value(),
            }
        }

        /// Show the store's `text`, as the browser's value sanitization lets
        /// the element hold it, and return what it shows.
        fn show(&self, text: &str) -> FieldValue {
            let units: Vec<u16> = text.encode_utf16().collect();
            let shown = FieldValue::of(&units, matches!(self, Self::Line(_)));
            self.set_shown(&shown);
            shown
        }

        /// Set the element's value to what `shown` shows.
        fn set_shown(&self, shown: &FieldValue) {
            let value = String::from_utf16_lossy(shown.shown());
            match self {
                Self::Area(area) => area.set_value(&value),
                Self::Line(line) => line.set_value(&value),
            }
        }

        /// The element's selection in the store's text it shows as `shown`,
        /// its direction kept; the start of the text when it reports none.
        fn selection(&self, shown: &FieldValue) -> Selection {
            let (start, end, direction) = match self {
                Self::Area(area) => (
                    area.selection_start(),
                    area.selection_end(),
                    area.selection_direction(),
                ),
                Self::Line(line) => (
                    line.selection_start(),
                    line.selection_end(),
                    line.selection_direction(),
                ),
            };
            let offset = |offset: Result<Option<u32>, JsValue>| {
                usize::try_from(offset.ok().flatten().unwrap_or(0)).unwrap_or(usize::MAX)
            };
            let (start, end) = shown.store_range(offset(start), offset(end));
            let (start, end) = (Utf16Offset::new(start), Utf16Offset::new(end));
            if direction.ok().flatten().as_deref() == Some("backward") {
                Selection {
                    anchor: end,
                    active: start,
                }
            } else {
                Selection {
                    anchor: start,
                    active: end,
                }
            }
        }

        /// Select the store's `selection` in the element showing `shown`,
        /// its direction kept.
        fn select(&self, shown: &FieldValue, selection: Selection) {
            let selected = selection.range();
            let start = shown_u32(shown, selected.start());
            let end = shown_u32(shown, selected.end());
            let direction = if selection.active < selection.anchor {
                "backward"
            } else {
                "forward"
            };
            let _ = match self {
                Self::Area(area) => area.set_selection_range_with_direction(start, end, direction),
                Self::Line(line) => line.set_selection_range_with_direction(start, end, direction),
            };
        }

        fn is(&self, element: &web_sys::HtmlElement) -> bool {
            self.html().is_same_node(Some(element.as_ref()))
        }
    }

    /// The focused field: its store, the surface mirroring it, and the
    /// surface's listeners.
    struct Attached {
        store: Rc<dyn TextStore>,
        surface: Rc<Surface>,
        _listeners: Vec<CanvasListener>,
    }

    /// The window's text input (see the module doc). Public where it is
    /// defined, as the host's table names it as its text position, and
    /// nameable only inside the crate.
    pub struct WebTextInput {
        canvas: web_sys::HtmlCanvasElement,
        /// The canvas's focus area, which the editor joins and the window
        /// follows.
        editor: Rc<dyn CanvasEditor>,
        accepting: Accepting,
        keys: Rc<dyn CanvasKeys>,
        /// The semantics host's field, which the store is served through
        /// while the host names it.
        field: Rc<FocusedField>,
        /// Whether this browser has `EditContext`, decided when the window
        /// opens.
        edit_context: bool,
        attached: RefCell<Option<Attached>>,
        /// The requests made while the host is inside a call into a store.
        queue: HostQueue<MirrorHostOp>,
        retired: Cell<bool>,
        /// The stores whose re-attach, republishing them to the surface, is
        /// queued.
        resync_queued: RefCell<Vec<Rc<dyn TextStore>>>,
        this: Weak<Self>,
    }

    fn offset(value: u32) -> Utf16Offset {
        Utf16Offset::new(value as usize)
    }

    fn range(start: u32, end: u32) -> Utf16Range {
        let (start, end) = (start.min(end), start.max(end));
        Utf16Range::try_new(offset(start), offset(end)).expect("BUG: an ordered range")
    }

    /// `range` clamped into a document of `len` code units.
    fn clamped(range: Utf16Range, len: Utf16Offset) -> Utf16Range {
        let start = range.start().min(len);
        let end = range.end().min(len);
        Utf16Range::try_new(start, end).expect("BUG: clamping keeps the order")
    }

    fn len16(text: &str) -> usize {
        text.encode_utf16().count()
    }

    /// The offset in what `shown` shows of the store's `offset`.
    fn shown_u32(shown: &FieldValue, offset: Utf16Offset) -> u32 {
        u32::try_from(shown.shown_offset(offset.get())).unwrap_or(u32::MAX)
    }

    fn as_u32(offset: Utf16Offset) -> u32 {
        u32::try_from(offset.get()).unwrap_or(u32::MAX)
    }

    /// The store's text and selection, read under a synchronous lock; `None`
    /// when the store cannot grant one now. A protected store reads as one
    /// bullet per code unit, so the surface's offsets stay the field's.
    fn snapshot(store: &dyn TextStore) -> Option<(String, Selection)> {
        let read: Rc<RefCell<Option<(String, Selection)>>> = Rc::default();
        let out = Rc::clone(&read);
        let grant = LockGrant::read(move |session| {
            *out.borrow_mut() = Some((document_text(session), session.selection()));
        });
        // A read under a synchronous lock runs at once or is refused.
        let _ran = store.request_lock(grant, LockTiming::Sync).ok()?;
        read.take()
    }

    impl WebTextInput {
        fn new(window: &CanvasRef) -> Rc<Self> {
            let input = Rc::new_cyclic(|this| Self {
                canvas: window.canvas.clone(),
                editor: Rc::clone(&window.editor),
                accepting: window.accepting.clone(),
                keys: Rc::clone(&window.keys),
                field: Rc::clone(&window.field),
                edit_context: has_edit_context(),
                attached: RefCell::new(None),
                queue: HostQueue::new(),
                retired: Cell::new(false),
                resync_queued: RefCell::default(),
                this: this.clone(),
            });
            let this = Rc::downgrade(&input);
            input.field.on_change(Some(Rc::new(move || {
                if let Some(this) = this.upgrade() {
                    this.field_changed();
                }
            })));
            input
        }

        /// The window closed: detach the focused field, if any, for good.
        fn retire(&self) {
            self.retired.set(true);
            self.field.on_change(None);
            self.queue.clear();
            self.detach();
        }

        /// The store the host serves now.
        fn focused(&self) -> Option<Rc<dyn TextStore>> {
            self.attached
                .borrow()
                .as_ref()
                .map(|attached| Rc::clone(&attached.store))
        }

        /// The semantics host named another field, or none: the focused store
        /// moves to the field it names, or back to the canvas, once the
        /// current task ends, as the host may be changing its elements now.
        fn field_changed(&self) {
            let found = self
                .attached
                .borrow()
                .as_ref()
                .map(|attached| (Rc::clone(&attached.store), Rc::clone(&attached.surface)));
            let Some((store, surface)) = found else {
                return;
            };
            let served = match &*surface {
                Surface::Editor {
                    element,
                    hidden: false,
                    ..
                } => Some(element),
                Surface::Context(_) | Surface::Editor { .. } => None,
            };
            let named = self.field.named();
            let moved = match (served, named.as_ref().and_then(EditorElement::of)) {
                (Some(served), Some(named)) => !served.is(named.html()),
                (None, None) => false,
                _ => true,
            };
            if moved {
                self.schedule_resync(store);
            }
        }

        /// Apply the requests queued while the host was inside a store call.
        fn drain(&self) {
            while let Some(op) = self.queue.next() {
                self.run(op);
            }
        }

        fn run(&self, op: MirrorHostOp) {
            match op {
                MirrorHostOp::Focus(store) => self.refocus(store),
                MirrorHostOp::Complete(store) => {
                    // Whoever was told `Deferred` owes nothing more: what the
                    // browser does not commit is committed in place here.
                    let focused = self
                        .focused()
                        .is_some_and(|focused| Rc::ptr_eq(&focused, &store));
                    let end = if focused {
                        self.end_composition()
                    } else {
                        CompositionEnd::Abandoned
                    };
                    if end != CompositionEnd::Committed {
                        let _ = commit_composition_in_place(&*store);
                    }
                }
            }
        }

        /// Edit the focused store as the browser reported, under an asynchronous
        /// lock: now if the store is free, else when its frame transaction lets
        /// it.
        ///
        /// The browser has already applied what it reports, so an edit the store
        /// does not take (`body` returns `false`, or the lock is refused) leaves
        /// the surface ahead of the store: the surface is republished from the
        /// store.
        fn edit(&self, body: impl FnOnce(&mut dyn TextStoreEdit) -> bool + 'static) {
            let Some(store) = self.focused() else {
                return;
            };
            let this = self.this.clone();
            let target = Rc::clone(&store);
            let grant = LockGrant::read_write(move |session| {
                if !body(session)
                    && let Some(this) = this.upgrade()
                {
                    this.schedule_resync(target);
                }
            });
            {
                let _entered = self.queue.enter();
                if let Err(error) = store.request_lock(grant, LockTiming::Async) {
                    tracing::debug!(?error, "the focused store refused the browser's edit");
                    self.schedule_resync(Rc::clone(&store));
                }
            }
            self.drain();
        }

        /// Republish `store` to its surface once the current task ends: the edit
        /// may be running inside the store's lock, which a re-attach must not
        /// enter. Focus may have moved on by then, and the store that took the
        /// focus is left alone.
        fn schedule_resync(&self, store: Rc<dyn TextStore>) {
            {
                let mut queued = self.resync_queued.borrow_mut();
                if queued.iter().any(|queued| Rc::ptr_eq(queued, &store)) {
                    return;
                }
                queued.push(Rc::clone(&store));
            }
            let this = self.this.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let Some(this) = this.upgrade() else {
                    return;
                };
                this.resync_queued
                    .borrow_mut()
                    .retain(|queued| !Rc::ptr_eq(queued, &store));
                let still_focused = this
                    .focused()
                    .is_some_and(|focused| Rc::ptr_eq(&focused, &store));
                if this.retired.get() || !still_focused {
                    return;
                }
                // Detaching forgets the browser's composition, so the store's is
                // committed in place, as when the host ends one.
                let composing = this.keys.composing();
                // The browser has no call that replaces a surface's whole text
                // and selection with exact offsets, so the surface is made anew.
                this.detach();
                if composing {
                    let _ = commit_composition_in_place(&*store);
                }
                this.attach(store);
            });
        }

        fn detach(&self) {
            let attached = self.attached.borrow_mut().take();
            let Some(Attached {
                store,
                surface,
                _listeners: listeners,
            }) = attached
            else {
                return;
            };
            self.keys.set_composing(false);
            // Listeners go first, so nothing the surface does while it is torn
            // down reaches the store.
            drop(listeners);
            match &*surface {
                Surface::Context(_) => {
                    let _ = self.canvas.remove_attribute("inputmode");
                    if let Err(error) =
                        attach_edit_context(self.canvas.unchecked_ref(), &JsValue::NULL)
                    {
                        tracing::debug!(?error, "the EditContext could not be detached");
                    }
                }
                Surface::Editor {
                    element,
                    hidden: true,
                    ..
                } => {
                    let element = element.html();
                    let had_focus = self
                        .canvas
                        .owner_document()
                        .and_then(|document| document.active_element())
                        .is_some_and(|active| active.is_same_node(Some(element.as_ref())));
                    if had_focus {
                        let _ = self.canvas.focus();
                    }
                    self.editor.set(None);
                    element.remove();
                }
                // The field is the host's, its focus too: only the hints go.
                Surface::Editor { element, .. } => {
                    self.field.serve(None);
                    for name in EDITOR_HINTS {
                        let _ = element.html().remove_attribute(name);
                    }
                }
            }
            contain_owner_callback(|| store.set_observer(None));
        }

        fn refocus(&self, store: Option<Rc<dyn TextStore>>) {
            let same = match (&store, &self.focused()) {
                (Some(next), Some(current)) => Rc::ptr_eq(next, current),
                (None, None) => true,
                _ => false,
            };
            if same || self.retired.get() {
                return;
            }
            self.detach();
            if let Some(store) = store {
                self.attach(store);
            }
        }

        /// Whether the page's focus is on `element`.
        fn page_focus_is(&self, element: &web_sys::Node) -> bool {
            self.canvas
                .owner_document()
                .and_then(|document| document.active_element())
                .is_some_and(|active| active.is_same_node(Some(element)))
        }

        /// Give `store` a fresh surface, holding its text and selection: the
        /// semantics host's field while it names one, else an `EditContext` on
        /// the canvas, else a hidden textarea beside it.
        fn attach(&self, store: Rc<dyn TextStore>) {
            let (text, selection) = snapshot(&*store)
                .unwrap_or_else(|| (String::new(), Selection::collapsed(Utf16Offset::ZERO)));
            let field = self.field.named().as_ref().and_then(EditorElement::of);
            let attached = match field {
                Some(field) => self.attach_editor(&store, &text, selection, field, false),
                None if self.edit_context => self.attach_context(&store, &text, selection),
                None => self.hidden_editor().and_then(|element| {
                    let element = EditorElement::Area(element);
                    self.attach_editor(&store, &text, selection, element, true)
                }),
            };
            match attached {
                Ok(attached) => {
                    let observer: Rc<dyn TextStoreObserver> = Rc::new(SurfaceObserver {
                        input: self.this.clone(),
                        surface: Rc::downgrade(&attached.surface),
                    });
                    *self.attached.borrow_mut() = Some(attached);
                    contain_owner_callback(|| store.set_observer(Some(observer)));
                    self.push_bounds();
                }
                Err(error) => {
                    tracing::warn!(?error, "the browser's text input could not attach");
                }
            }
        }

        fn attach_context(
            &self,
            store: &Rc<dyn TextStore>,
            text: &str,
            selection: Selection,
        ) -> Result<Attached, JsValue> {
            let selected = selection.range();
            let context =
                create_edit_context(text, as_u32(selected.start()), as_u32(selected.end()))?;
            // The browser reads the keyboard hints from the element the context
            // is associated with, not from the context.
            self.canvas
                .set_attribute("inputmode", input_mode(store.status().purpose))?;
            let target: &web_sys::EventTarget = context.as_ref();
            let mut listeners = Vec::new();
            let listen = |kind: &'static str, body: Box<dyn Fn(web_sys::Event)>| {
                CanvasListener::install(target, kind, &self.accepting, body)
                    .map_err(|error| JsValue::from_str(&error.to_string()))
            };
            {
                let this = self.this.clone();
                listeners.push(listen(
                    "textupdate",
                    Box::new(move |event| {
                        let Some(this) = this.upgrade() else {
                            return;
                        };
                        let update: &TextUpdate = event.unchecked_ref();
                        let replaced =
                            range(update.update_range_start(), update.update_range_end());
                        let text = update.update_text_value();
                        let selection = (
                            update.update_selection_start(),
                            update.update_selection_end(),
                        );
                        let composing = this.keys.composing();
                        this.edit(move |session| {
                            let replaced = clamped(replaced, session.document_len());
                            let inserted = len16(&text);
                            if session.replace(replaced, &text).is_err() {
                                return false;
                            }
                            let composition = composing.then(|| Composition {
                                range: Utf16Range::try_new(
                                    replaced.start(),
                                    Utf16Offset::new(replaced.start().get() + inserted),
                                )
                                .expect("BUG: an inserted range is ordered"),
                                hides_caret: false,
                            });
                            let _ = session.set_composition(composition);
                            let selected =
                                clamped(range(selection.0, selection.1), session.document_len());
                            let _ = session.set_selection(Selection {
                                anchor: selected.start(),
                                active: selected.end(),
                            });
                            true
                        });
                    }),
                )?);
            }
            for (kind, composing) in [("compositionstart", true), ("compositionend", false)] {
                let this = self.this.clone();
                listeners.push(listen(
                    kind,
                    Box::new(move |_| {
                        let Some(this) = this.upgrade() else {
                            return;
                        };
                        this.keys.set_composing(composing);
                        if !composing {
                            this.edit(|session| {
                                let _ = session.set_composition(None);
                                true
                            });
                        }
                    }),
                )?);
            }
            {
                let this = self.this.clone();
                listeners.push(listen(
                    "characterboundsupdate",
                    Box::new(move |event| {
                        let Some(this) = this.upgrade() else {
                            return;
                        };
                        let update: &CharacterBoundsUpdate = event.unchecked_ref();
                        this.push_character_bounds(
                            update.bounds_range_start(),
                            update.bounds_range_end(),
                        );
                    }),
                )?);
            }
            listeners.extend(self.viewport_listeners()?);
            attach_edit_context(self.canvas.unchecked_ref(), context.as_ref())?;
            Ok(Attached {
                store: Rc::clone(store),
                surface: Rc::new(Surface::Context(context)),
                _listeners: listeners,
            })
        }
        /// The attached field's purpose or protection changed (a rebuild of the
        /// focused field): the browser's hints follow it without a blur and
        /// refocus.
        fn refresh_input_hints(&self, surface: &Surface) {
            // The store is user code and may call back into the host: ask it with
            // no borrow of the attachment held.
            let store = match self.attached.try_borrow() {
                Ok(attached) => attached.as_ref().map(|attached| Rc::clone(&attached.store)),
                Err(_) => None,
            };
            let Some(store) = store else {
                return;
            };
            let status = store.status();
            let result = match surface {
                Surface::Context(_) => self
                    .canvas
                    .set_attribute("inputmode", input_mode(status.purpose)),
                Surface::Editor {
                    element, hidden, ..
                } => set_editor_hints(element.html(), &status, !*hidden),
            };
            if let Err(error) = result {
                tracing::debug!(?error, "the keyboard hints could not be updated");
            }
        }

        /// A hidden textarea for the canvas's field, not yet in the page.
        fn hidden_editor(&self) -> Result<web_sys::HtmlTextAreaElement, JsValue> {
            let document = self
                .canvas
                .owner_document()
                .ok_or_else(|| JsValue::from_str("the canvas has no document"))?;
            let element: web_sys::HtmlTextAreaElement =
                document.create_element("textarea")?.unchecked_into();
            for (name, value) in [
                ("autocapitalize", "off"),
                ("autocorrect", "off"),
                ("spellcheck", "false"),
                ("tabindex", "-1"),
            ] {
                element.set_attribute(name, value)?;
            }
            let style = element.style();
            for (property, value) in [
                ("position", "fixed"),
                ("opacity", "0"),
                ("pointer-events", "none"),
                ("width", "1px"),
                ("height", "1em"),
                ("padding", "0"),
                ("border", "0"),
                ("margin", "0"),
                ("resize", "none"),
                ("overflow", "hidden"),
                ("white-space", "pre"),
            ] {
                style.set_property(property, value)?;
            }
            Ok(element)
        }

        /// Serve `store` through `element`, holding `text` and `selection`:
        /// the `hidden` textarea, which the input puts beside the canvas and
        /// whose focus and keys it routes, or the semantics host's field, which
        /// the host places and whose focus and keys the host routes.
        fn attach_editor(
            &self,
            store: &Rc<dyn TextStore>,
            text: &str,
            selection: Selection,
            element: EditorElement,
            hidden: bool,
        ) -> Result<Attached, JsValue> {
            set_editor_hints(element.html(), &store.status(), !hidden)?;
            let shown = element.show(text);
            element.select(&shown, selection);
            let target: &web_sys::EventTarget = element.html().as_ref();
            let mut listeners = Vec::new();
            let listen = |target: &web_sys::EventTarget,
                          kind: &'static str,
                          body: Box<dyn Fn(web_sys::Event)>| {
                CanvasListener::install(target, kind, &self.accepting, body)
                    .map_err(|error| JsValue::from_str(&error.to_string()))
            };
            if hidden {
                for (kind, state) in [("keydown", KeyState::Down), ("keyup", KeyState::Up)] {
                    let keys = Rc::clone(&self.keys);
                    listeners.push(listen(
                        target,
                        kind,
                        Box::new(move |event| keys.deliver(&event, state)),
                    )?);
                }
            }
            for kind in ["compositionstart", "compositionend"] {
                let this = self.this.clone();
                listeners.push(listen(
                    target,
                    kind,
                    Box::new(move |event| {
                        let Some(this) = this.upgrade() else {
                            return;
                        };
                        let composing = event.type_() == "compositionstart";
                        this.keys.set_composing(composing);
                        if !composing {
                            this.read_editor(false);
                        }
                    }),
                )?);
            }
            for kind in ["input", "select", "selectionchange"] {
                let this = self.this.clone();
                listeners.push(listen(
                    target,
                    kind,
                    Box::new(move |_| {
                        if let Some(this) = this.upgrade() {
                            this.read_editor(this.keys.composing());
                        }
                    }),
                )?);
            }
            if hidden {
                for kind in ["focus", "blur"] {
                    let focus = Rc::clone(&self.editor);
                    listeners.push(listen(
                        target,
                        kind,
                        Box::new(move |event| {
                            if !focus.is_handoff(&event) {
                                focus.sync();
                            }
                        }),
                    )?);
                }
            }
            // The canvas forwards its focus to the field's editor: the hidden
            // one, or the host's field while the host still names it (the
            // tree's focus that left it puts the page's focus on the canvas
            // before the store moves off it).
            {
                let editor = element.html().clone();
                let field = (!hidden).then(|| Rc::clone(&self.field));
                listeners.push(listen(
                    self.canvas.as_ref(),
                    "focus",
                    Box::new(move |_| {
                        let named = field.as_ref().is_none_or(|field| {
                            field
                                .named()
                                .is_some_and(|named| named.is_same_node(Some(editor.as_ref())))
                        });
                        if named {
                            let _ = editor.focus();
                        }
                    }),
                )?);
            }
            if hidden {
                listeners.extend(self.viewport_listeners()?);
                let parent = self
                    .canvas
                    .parent_node()
                    .ok_or_else(|| JsValue::from_str("the canvas is not in a document"))?;
                parent.insert_before(element.html(), self.canvas.next_sibling().as_ref())?;
                self.editor.set(Some(element.html()));
            } else {
                self.field.serve(Some(element.html()));
            }
            // The editor takes the canvas's focus (the hidden editor's, or
            // the canvas's while the store moved off it), never the focus of
            // another element of the semantics host (a slider the tree's
            // focus moved to keeps its keys).
            if self.page_focus_is(self.canvas.as_ref()) {
                let _ = element.html().focus();
            }
            Ok(Attached {
                store: Rc::clone(store),
                surface: Rc::new(Surface::Editor {
                    element,
                    hidden,
                    mirror: RefCell::new(shown),
                }),
                _listeners: listeners,
            })
        }

        /// Apply what changed in the editor since the host last read it: the
        /// one edit between the two values, the composition when `composing`,
        /// and the selection.
        fn read_editor(&self, composing: bool) {
            let surface = self
                .attached
                .borrow()
                .as_ref()
                .map(|attached| Rc::clone(&attached.surface));
            let Some(surface) = surface else {
                return;
            };
            let Surface::Editor {
                element, mirror, ..
            } = &*surface
            else {
                return;
            };
            let now: Vec<u16> = element.value().encode_utf16().collect();
            let change = diff(mirror.borrow().shown(), &now);
            let change = change.map(|change| {
                let inserted = &now[change.start..change.new_end];
                let (from, to) =
                    mirror
                        .borrow_mut()
                        .replace(change.start, change.old_end, inserted);
                (from, to, String::from_utf16_lossy(inserted), inserted.len())
            });
            // An edit whose store the browser would show otherwise (a line
            // feed typed after a lone carriage return) is shown again, the
            // caret after what it inserted.
            let reshown = change
                .as_ref()
                .filter(|_| mirror.borrow().shown() != now.as_slice())
                .map(|&(from, _, _, units)| {
                    let caret = Selection::collapsed(Utf16Offset::new(from + units));
                    element.set_shown(&mirror.borrow());
                    element.select(&mirror.borrow(), caret);
                    caret
                });
            let selection = reshown.unwrap_or_else(|| element.selection(&mirror.borrow()));
            self.edit(move |session| {
                if let Some((from, to, inserted, units)) = &change {
                    let replaced = clamped(
                        Utf16Range::try_new(Utf16Offset::new(*from), Utf16Offset::new(*to))
                            .expect("BUG: a store range is ordered"),
                        session.document_len(),
                    );
                    if session.replace(replaced, inserted).is_err() {
                        return false;
                    }
                    let composition = composing.then(|| Composition {
                        range: Utf16Range::try_new(
                            replaced.start(),
                            Utf16Offset::new(replaced.start().get() + units),
                        )
                        .expect("BUG: an inserted range is ordered"),
                        hides_caret: false,
                    });
                    let _ = session.set_composition(composition);
                } else if !composing {
                    let _ = session.set_composition(None);
                }
                let len = session.document_len();
                let _ = session.set_selection(Selection {
                    anchor: selection.anchor.min(len),
                    active: selection.active.min(len),
                });
                true
            });
        }

        /// The store's text changed outside the browser's edits: give the
        /// surface the new text.
        fn push_text(&self, surface: &Surface, change: TextChange) {
            let Some(store) = self.focused() else {
                return;
            };
            let Some((text, selection)) = snapshot(&*store) else {
                return;
            };
            let selected = selection.range();
            match surface {
                Surface::Context(context) => {
                    let units: Vec<u16> = text.encode_utf16().collect();
                    let start = change.start.get().min(units.len());
                    let end = change.new_end.get().clamp(start, units.len());
                    let inserted = String::from_utf16_lossy(&units[start..end]);
                    let _ = context.update_text(
                        as_u32(change.start),
                        as_u32(change.old_end),
                        &inserted,
                    );
                    let _ =
                        context.update_selection(as_u32(selected.start()), as_u32(selected.end()));
                }
                Surface::Editor {
                    element, mirror, ..
                } => {
                    let shown = element.show(&text);
                    element.select(&shown, selection);
                    *mirror.borrow_mut() = shown;
                }
            }
        }

        fn push_selection(&self, surface: &Surface) {
            let Some(store) = self.focused() else {
                return;
            };
            let Some((_, selection)) = snapshot(&*store) else {
                return;
            };
            let selected = selection.range();
            match surface {
                Surface::Context(context) => {
                    let _ =
                        context.update_selection(as_u32(selected.start()), as_u32(selected.end()));
                }
                Surface::Editor {
                    element, mirror, ..
                } => element.select(&mirror.borrow(), selection),
            }
            self.push_bounds();
        }

        /// The canvas's position in the viewport, which the store's logical
        /// window coordinates are offset by.
        fn origin(&self) -> (f64, f64) {
            let rect = self.canvas.get_bounding_client_rect();
            (rect.left(), rect.top())
        }

        /// Listeners that tell the surface where the field moved to when the
        /// canvas moves in the viewport without the store's layout changing.
        fn viewport_listeners(&self) -> Result<Vec<CanvasListener>, JsValue> {
            let this = self.this.clone();
            viewport_listeners(&self.accepting, move || {
                if let Some(this) = this.upgrade() {
                    this.push_bounds();
                }
            })
            .map_err(|error| JsValue::from_str(&error.to_string()))
        }

        /// Tell the surface where the field and its selection are, so the input
        /// method's window sits beside the caret.
        fn push_bounds(&self) {
            let found = self
                .attached
                .borrow()
                .as_ref()
                .map(|attached| (Rc::clone(&attached.store), Rc::clone(&attached.surface)));
            let Some((store, surface)) = found else {
                return;
            };
            let bounds: Rc<RefCell<Option<_>>> = Rc::default();
            let out = Rc::clone(&bounds);
            let grant = LockGrant::read(move |session| {
                let selection = session.selection().range();
                *out.borrow_mut() = Some((
                    session.document_bounds().ok(),
                    session
                        .rect_for_range(selection)
                        .ok()
                        .map(|rect| rect.bounds),
                ));
            });
            if store.request_lock(grant, LockTiming::Sync).is_err() {
                return;
            }
            let Some((document, caret)) = bounds.take() else {
                return;
            };
            let (left, top) = self.origin();
            let dom = |bounds: flui_foundation::geometry::Bounds<f64>| {
                web_sys::DomRect::new_with_x_and_y_and_width_and_height(
                    bounds.origin.x + left,
                    bounds.origin.y + top,
                    bounds.size.width,
                    bounds.size.height,
                )
                .ok()
            };
            match &*surface {
                Surface::Context(context) => {
                    if let Some(rect) = document.and_then(dom) {
                        let _ = context.update_control_bounds(&rect);
                    }
                    if let Some(rect) = caret.and_then(dom) {
                        let _ = context.update_selection_bounds(&rect);
                    }
                }
                Surface::Editor {
                    element,
                    hidden: true,
                    ..
                } => {
                    if let Some(caret) = caret {
                        let style = element.html().style();
                        let _ = style.set_property("left", &format!("{}px", caret.origin.x + left));
                        let _ = style.set_property("top", &format!("{}px", caret.origin.y + top));
                        let _ = style
                            .set_property("height", &format!("{}px", caret.size.height.max(1.0)));
                    }
                }
                // The semantics host places its field.
                Surface::Editor { .. } => {}
            }
        }

        /// The bounds of each character of `start..end`, which the input method
        /// asked for.
        fn push_character_bounds(&self, start: u32, end: u32) {
            let found = self
                .attached
                .borrow()
                .as_ref()
                .map(|attached| (Rc::clone(&attached.store), Rc::clone(&attached.surface)));
            let Some((store, surface)) = found else {
                return;
            };
            let Surface::Context(context) = &*surface else {
                return;
            };
            let rects: Rc<RefCell<Vec<flui_foundation::geometry::Bounds<f64>>>> = Rc::default();
            let out = Rc::clone(&rects);
            let grant = LockGrant::read(move |session| {
                // One rect per code unit: both halves of a surrogate pair get the
                // rect of the scalar they make. A protected document reads as one
                // bullet per unit, which splits a real pair, so a unit the store
                // refuses is retried together with the next one, or with the one before it
                // when the request starts at the second half of a pair.
                let text = document_text(session);
                let (start, end) = (start as usize, end as usize);
                let spans = scalar_spans(&text, start, end);
                let mut spans = spans.into_iter().peekable();
                while let Some(span) = spans.next() {
                    let found = if let Ok(rect) = session.rect_for_range(span) {
                        Some((span, rect))
                    } else if let Some(next) = spans.peek().copied()
                        && let Some(pair) = Utf16Range::try_new(span.start(), next.end())
                        && let Ok(rect) = session.rect_for_range(pair)
                    {
                        spans.next();
                        Some((pair, rect))
                    } else if let Some(before) = span.start().get().checked_sub(1)
                        && let Some(pair) =
                            Utf16Range::try_new(Utf16Offset::new(before), span.end())
                        && let Ok(rect) = session.rect_for_range(pair)
                    {
                        Some((pair, rect))
                    } else {
                        None
                    };
                    let Some((span, rect)) = found else {
                        break;
                    };
                    let from = span.start().get().max(start);
                    let to = span.end().get().min(end);
                    out.borrow_mut()
                        .extend(std::iter::repeat_n(rect.bounds, to - from));
                }
            });
            if store.request_lock(grant, LockTiming::Sync).is_err() {
                return;
            }
            let (left, top) = self.origin();
            let bounds: js_sys::Array = rects
                .take()
                .into_iter()
                .filter_map(|bounds| {
                    web_sys::DomRect::new_with_x_and_y_and_width_and_height(
                        bounds.origin.x + left,
                        bounds.origin.y + top,
                        bounds.size.width,
                        bounds.size.height,
                    )
                    .ok()
                })
                .collect();
            let _ = context.update_character_bounds(start, &bounds);
        }

        /// End the browser's composition in the focused store: the browser has
        /// no call that commits it, so its own is discarded by re-attaching the
        /// surface and the caller commits the store's in place.
        fn end_composition(&self) -> CompositionEnd {
            if !self.keys.composing() {
                return CompositionEnd::Committed;
            }
            self.keys.set_composing(false);
            let surface = self
                .attached
                .borrow()
                .as_ref()
                .map(|attached| Rc::clone(&attached.surface));
            match surface.as_deref() {
                Some(Surface::Context(context)) => {
                    let element: &web_sys::HtmlElement = self.canvas.unchecked_ref();
                    let _ = attach_edit_context(element, &JsValue::NULL);
                    let _ = attach_edit_context(element, context.as_ref());
                }
                Some(Surface::Editor { element, .. }) => {
                    let _ = element.html().blur();
                    let _ = element.html().focus();
                }
                None => {}
            }
            CompositionEnd::Abandoned
        }
    }

    impl std::fmt::Debug for WebTextInput {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("WebTextInput").finish_non_exhaustive()
        }
    }

    /// Each canvas window gets an input over its canvas: an `EditContext` or
    /// a hidden textarea, which joins the canvas's focus area.
    impl crate::host::Text<CanvasRef> for WebTextInput {
        fn for_window(window: &CanvasRef) -> std::sync::Arc<dyn crate::text::TextInput> {
            let input = OwnerSlot::new();
            if input.replace(WebTextInput::new(window)).is_err() {
                tracing::warn!("a canvas's text input was made off the page's thread");
            }
            std::sync::Arc::new(CanvasText(input))
        }
    }

    /// What one canvas window holds of the text feature: its input, on the
    /// page's thread.
    struct CanvasText(OwnerSlot<Rc<WebTextInput>>);

    impl CanvasText {
        /// The input, held for no borrow: its calls run the field's store.
        fn input(&self) -> Option<Rc<WebTextInput>> {
            let input = self.0.take()?;
            let shared = Rc::clone(&input);
            let _ = self.0.restore(input);
            Some(shared)
        }
    }

    impl crate::text::TextInput for CanvasText {
        /// The canvas's `EditContext` or hidden textarea, serving the focused
        /// field's store; `None` once the window closed.
        fn window_text_input(&self) -> Option<crate::text::WindowTextInput> {
            self.input()
                .map(|input| crate::text::WindowTextInput::Store(input as Rc<dyn TextStoreHost>))
        }

        /// The browser's text size is linear: nothing is captured.
        fn capture_sizing(
            &self,
        ) -> Result<crate::TextSizingCaptureState, crate::TextSizingCaptureError> {
            Ok(crate::TextSizingCaptureState::Unsupported)
        }

        fn invalidate_sizing(&self) {}

        fn fence_sizing(&self) {}

        /// The window closed: the focused field, if any, is detached for good.
        fn detach(&self) {
            if let Some(input) = self.0.take() {
                input.retire();
            }
        }
    }

    impl TextStoreHost for WebTextInput {
        fn focus_store(&self, store: Option<Rc<dyn TextStore>>) {
            if let Some(op) = self.queue.defer(MirrorHostOp::Focus(store)) {
                self.run(op);
                self.drain();
            }
        }

        fn complete_composition(
            &self,
            store: &Rc<dyn TextStore>,
        ) -> Result<CompositionEnd, TextStoreHostError> {
            if self.retired.get() {
                return Err(flui_platform::Unsupported::new(
                    flui_platform::UnsupportedReason::NoWindow,
                )
                .into());
            }
            // The focus a queued request will give is the one asked about.
            let focused = self.queue.focus_after_queue(|| self.focused());
            if !focused.is_some_and(|focused| Rc::ptr_eq(&focused, store)) {
                return Err(TextStoreHostError::NotFocused);
            }
            if self
                .queue
                .defer(MirrorHostOp::Complete(Rc::clone(store)))
                .is_none()
            {
                return Ok(CompositionEnd::Deferred);
            }
            let end = self.end_composition();
            self.drain();
            Ok(end)
        }
    }

    /// What the store tells the browser's surface when the application changes
    /// the field.
    struct SurfaceObserver {
        input: Weak<WebTextInput>,
        surface: Weak<Surface>,
    }

    impl SurfaceObserver {
        fn with(&self, f: impl FnOnce(&WebTextInput, &Surface)) {
            if let (Some(input), Some(surface)) = (self.input.upgrade(), self.surface.upgrade()) {
                f(&input, &surface);
            }
        }
    }

    impl TextStoreObserver for SurfaceObserver {
        fn text_changed(&self, change: TextChange) {
            self.with(|input, surface| input.push_text(surface, change));
        }

        fn selection_changed(&self) {
            self.with(WebTextInput::push_selection);
        }

        fn layout_changed(&self) {
            self.with(|input, _| input.push_bounds());
        }

        fn status_changed(&self) {
            self.with(WebTextInput::refresh_input_hints);
        }
    }

    /// The attributes an editor's hints are written to, which the semantics
    /// host's field loses once nothing is served through it.
    const EDITOR_HINTS: [&str; 3] = ["inputmode", "autocomplete", "enterkeyhint"];

    /// The hints an editor gives the browser for the field's status
    /// ([`EDITOR_HINTS`]): the keyboard its purpose asks for, what the
    /// browser may fill it with (`native` for the semantics host's field,
    /// see [`autocomplete`]) and the label of the keyboard's Enter key.
    fn set_editor_hints(
        element: &web_sys::HtmlElement,
        status: &TextStoreStatus,
        native: bool,
    ) -> Result<(), JsValue> {
        element.set_attribute("inputmode", input_mode(status.purpose))?;
        element.set_attribute("autocomplete", autocomplete(status, native))?;
        match enter_key_hint(status) {
            Some(hint) => element.set_attribute("enterkeyhint", hint),
            None => element.remove_attribute("enterkeyhint"),
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) use input::WebTextInput;
