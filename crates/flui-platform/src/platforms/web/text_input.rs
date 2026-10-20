//! The web window's text input: the field's `TextStore` (ADR-0090) served
//! to the browser's input method through an `EditContext` on the canvas
//! where the browser has one (Chromium), and through a hidden `<textarea>`
//! beside the canvas where it does not.
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
//! `shared::web_text::key_composition`): the window delivers it
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
use std::collections::VecDeque;
use std::rc::{Rc, Weak};

use flui_platform_api::keyboard::KeyState;
use flui_platform_api::text_store::Composition;
use flui_platform_api::text_store::{
    CompositionEnd, LockGrant, LockTiming, Selection, TextChange, TextStore, TextStoreEdit,
    TextStoreHost, TextStoreHostError, TextStoreObserver, Utf16Offset, Utf16Range,
    commit_composition_in_place,
};
use wasm_bindgen::{JsCast, prelude::*};

use crate::shared::ime_text::{document_text, scalar_spans};
use crate::shared::panic_boundary::contain_owner_callback;
use crate::shared::web_text::{diff, input_mode};

use super::events::{KeyRoute, is_focus_handoff};
use super::registration::{EditorSlot, Listener};
use super::window::FocusSync;

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
    fn update_text(this: &EditContext, start: u32, end: u32, text: &str) -> Result<(), JsValue>;
    #[wasm_bindgen(method, catch, js_name = updateSelection)]
    fn update_selection(this: &EditContext, start: u32, end: u32) -> Result<(), JsValue>;
    #[wasm_bindgen(method, catch, js_name = updateControlBounds)]
    fn update_control_bounds(this: &EditContext, bounds: &web_sys::DomRect) -> Result<(), JsValue>;
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
    /// A hidden textarea beside the canvas, with the text it last held.
    TextArea {
        element: web_sys::HtmlTextAreaElement,
        mirror: RefCell<Vec<u16>>,
    },
}

/// The focused field: its store, the surface mirroring it, and the
/// surface's listeners.
struct Attached {
    store: Rc<dyn TextStore>,
    surface: Rc<Surface>,
    _listeners: Vec<Listener>,
}

/// A host request made while the host was already inside a call into a
/// store: applied, in order, when that call returns.
enum HostOp {
    Focus(Option<Rc<dyn TextStore>>),
    Complete(Rc<dyn TextStore>),
}

/// The window's text input (see the module doc).
pub(super) struct WebTextInput {
    canvas: web_sys::HtmlCanvasElement,
    editor: EditorSlot,
    accepting: Rc<Cell<bool>>,
    keys: Rc<KeyRoute>,
    focus: Rc<FocusSync>,
    /// Whether this browser has `EditContext`, decided when the window
    /// opens.
    edit_context: bool,
    attached: RefCell<Option<Attached>>,
    /// How deep the host is in calls into a store; requests made meanwhile
    /// wait in `pending`.
    entry: Cell<u32>,
    pending: RefCell<VecDeque<HostOp>>,
    retired: Cell<bool>,
    /// The stores whose re-attach, republishing them to the surface, is
    /// queued.
    resync_queued: RefCell<Vec<Rc<dyn TextStore>>>,
    this: Weak<Self>,
}

/// Counts one call into a store, however it ends.
struct Entered<'a>(&'a Cell<u32>);

impl Drop for Entered<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() - 1);
    }
}

fn offset(value: u32) -> Utf16Offset {
    Utf16Offset::new(value as usize)
}

fn range(start: u32, end: u32) -> Utf16Range {
    let (start, end) = (start.min(end), start.max(end));
    Utf16Range::new(offset(start), offset(end)).expect("BUG: an ordered range")
}

/// `range` clamped into a document of `len` code units.
fn clamped(range: Utf16Range, len: Utf16Offset) -> Utf16Range {
    let start = range.start().min(len);
    let end = range.end().min(len);
    Utf16Range::new(start, end).expect("BUG: clamping keeps the order")
}

fn len16(text: &str) -> usize {
    text.encode_utf16().count()
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
    pub(super) fn new(
        canvas: &web_sys::HtmlCanvasElement,
        editor: EditorSlot,
        accepting: Rc<Cell<bool>>,
        keys: Rc<KeyRoute>,
        focus: Rc<FocusSync>,
    ) -> Rc<Self> {
        Rc::new_cyclic(|this| Self {
            canvas: canvas.clone(),
            editor,
            accepting,
            keys,
            focus,
            edit_context: has_edit_context(),
            attached: RefCell::new(None),
            entry: Cell::new(0),
            pending: RefCell::new(VecDeque::new()),
            retired: Cell::new(false),
            resync_queued: RefCell::default(),
            this: this.clone(),
        })
    }

    /// The window closed: detach the focused field, if any, for good.
    pub(super) fn retire(&self) {
        self.retired.set(true);
        self.pending.borrow_mut().clear();
        self.detach();
    }

    /// The store the host serves now.
    fn focused(&self) -> Option<Rc<dyn TextStore>> {
        self.attached
            .borrow()
            .as_ref()
            .map(|attached| Rc::clone(&attached.store))
    }

    /// Enter a call into a store; requests made until the guard drops queue.
    fn enter(&self) -> Entered<'_> {
        self.entry.set(self.entry.get() + 1);
        Entered(&self.entry)
    }

    /// Apply the requests queued while the host was inside a store call.
    fn drain(&self) {
        if self.entry.get() > 0 {
            return;
        }
        loop {
            let next = self.pending.borrow_mut().pop_front();
            match next {
                Some(HostOp::Focus(store)) => self.refocus(store),
                Some(HostOp::Complete(store)) => {
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
                None => return,
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
            let _entered = self.enter();
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
            let composing = this.keys.composing.get();
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
        self.keys.composing.set(false);
        // Listeners go first, so nothing the surface does while it is torn
        // down reaches the store.
        drop(listeners);
        match &*surface {
            Surface::Context(_) => {
                if let Err(error) = attach_edit_context(self.canvas.unchecked_ref(), &JsValue::NULL)
                {
                    tracing::debug!(?error, "the EditContext could not be detached");
                }
            }
            Surface::TextArea { element, .. } => {
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

    /// Give `store` a fresh surface, holding its text and selection.
    fn attach(&self, store: Rc<dyn TextStore>) {
        let (text, selection) = snapshot(&*store)
            .unwrap_or_else(|| (String::new(), Selection::collapsed(Utf16Offset::ZERO)));
        let attached = if self.edit_context {
            self.attach_context(&store, &text, selection)
        } else {
            self.attach_textarea(&store, &text, selection)
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
        let context = create_edit_context(text, as_u32(selected.start()), as_u32(selected.end()))?;
        js_sys::Reflect::set(
            context.as_ref(),
            &JsValue::from_str("inputMode"),
            &JsValue::from_str(input_mode(store.status().purpose)),
        )?;
        let target: &web_sys::EventTarget = context.as_ref();
        let mut listeners = Vec::new();
        let listen = |kind: &'static str, body: Box<dyn Fn(web_sys::Event)>| {
            Listener::install(target, kind, &self.accepting, body)
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
                    let replaced = range(update.update_range_start(), update.update_range_end());
                    let text = update.update_text_value();
                    let selection = (
                        update.update_selection_start(),
                        update.update_selection_end(),
                    );
                    let composing = this.keys.composing.get();
                    this.edit(move |session| {
                        let replaced = clamped(replaced, session.document_len());
                        let inserted = len16(&text);
                        if session.replace(replaced, &text).is_err() {
                            return false;
                        }
                        let composition = composing.then(|| Composition {
                            range: Utf16Range::new(
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
                    this.keys.composing.set(composing);
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

    fn attach_textarea(
        &self,
        store: &Rc<dyn TextStore>,
        text: &str,
        selection: Selection,
    ) -> Result<Attached, JsValue> {
        let document = self
            .canvas
            .owner_document()
            .ok_or_else(|| JsValue::from_str("the canvas has no document"))?;
        let element: web_sys::HtmlTextAreaElement =
            document.create_element("textarea")?.unchecked_into();
        for (name, value) in [
            ("autocomplete", "off"),
            ("autocapitalize", "off"),
            ("autocorrect", "off"),
            ("spellcheck", "false"),
            ("tabindex", "-1"),
        ] {
            element.set_attribute(name, value)?;
        }
        let status = store.status();
        element.set_attribute("inputmode", input_mode(status.purpose))?;
        if status.protected {
            // A password manager must not treat the editor as a login field.
            element.set_attribute("autocomplete", "new-password")?;
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
        element.set_value(text);
        let selected = selection.range();
        let _ = element.set_selection_range(as_u32(selected.start()), as_u32(selected.end()));
        let target: &web_sys::EventTarget = element.as_ref();
        let mut listeners = Vec::new();
        let listen = |target: &web_sys::EventTarget,
                      kind: &'static str,
                      body: Box<dyn Fn(web_sys::Event)>| {
            Listener::install(target, kind, &self.accepting, body)
                .map_err(|error| JsValue::from_str(&error.to_string()))
        };
        for (kind, state) in [("keydown", KeyState::Down), ("keyup", KeyState::Up)] {
            let keys = Rc::clone(&self.keys);
            listeners.push(listen(
                target,
                kind,
                Box::new(move |event| keys.deliver(&event, state)),
            )?);
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
                    this.keys.composing.set(composing);
                    if !composing {
                        this.read_textarea(false);
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
                        this.read_textarea(this.keys.composing.get());
                    }
                }),
            )?);
        }
        for kind in ["focus", "blur"] {
            let (focus, canvas) = (Rc::clone(&self.focus), self.canvas.clone());
            listeners.push(listen(
                target,
                kind,
                Box::new(move |event| {
                    if !is_focus_handoff(&canvas, &event) {
                        focus.sync();
                    }
                }),
            )?);
        }
        // The canvas forwards its focus to the field's editor.
        {
            let editor = element.clone();
            listeners.push(listen(
                self.canvas.as_ref(),
                "focus",
                Box::new(move |_| {
                    let _ = editor.focus();
                }),
            )?);
        }
        listeners.extend(self.viewport_listeners()?);
        let parent = self
            .canvas
            .parent_node()
            .ok_or_else(|| JsValue::from_str("the canvas is not in a document"))?;
        parent.insert_before(&element, self.canvas.next_sibling().as_ref())?;
        self.editor.set(Some(element.unchecked_ref()));
        let canvas_focused = super::registration::effective_focus(&self.canvas);
        if canvas_focused {
            let _ = element.focus();
        }
        Ok(Attached {
            store: Rc::clone(store),
            surface: Rc::new(Surface::TextArea {
                element,
                mirror: RefCell::new(text.encode_utf16().collect()),
            }),
            _listeners: listeners,
        })
    }

    /// Apply what changed in the textarea since the host last read it: the
    /// one edit between the two values, the composition when `composing`,
    /// and the selection.
    fn read_textarea(&self, composing: bool) {
        let surface = self
            .attached
            .borrow()
            .as_ref()
            .map(|attached| Rc::clone(&attached.surface));
        let Some(surface) = surface else {
            return;
        };
        let Surface::TextArea { element, mirror } = &*surface else {
            return;
        };
        let now: Vec<u16> = element.value().encode_utf16().collect();
        let change = diff(&mirror.borrow(), &now);
        mirror.borrow_mut().clone_from(&now);
        let selection = (
            element.selection_start().ok().flatten().unwrap_or(0),
            element.selection_end().ok().flatten().unwrap_or(0),
        );
        self.edit(move |session| {
            if let Some(change) = change {
                let replaced = clamped(
                    Utf16Range::new(
                        Utf16Offset::new(change.start),
                        Utf16Offset::new(change.old_end),
                    )
                    .expect("BUG: a diff is ordered"),
                    session.document_len(),
                );
                let inserted = String::from_utf16_lossy(&now[change.start..change.new_end]);
                if session.replace(replaced, &inserted).is_err() {
                    return false;
                }
                let composition = composing.then(|| Composition {
                    range: Utf16Range::new(
                        replaced.start(),
                        Utf16Offset::new(replaced.start().get() + (change.new_end - change.start)),
                    )
                    .expect("BUG: an inserted range is ordered"),
                    hides_caret: false,
                });
                let _ = session.set_composition(composition);
            } else if !composing {
                let _ = session.set_composition(None);
            }
            let selected = clamped(range(selection.0, selection.1), session.document_len());
            let _ = session.set_selection(Selection {
                anchor: selected.start(),
                active: selected.end(),
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
                let _ =
                    context.update_text(as_u32(change.start), as_u32(change.old_end), &inserted);
                let _ = context.update_selection(as_u32(selected.start()), as_u32(selected.end()));
            }
            Surface::TextArea { element, mirror } => {
                element.set_value(&text);
                *mirror.borrow_mut() = text.encode_utf16().collect();
                let _ =
                    element.set_selection_range(as_u32(selected.start()), as_u32(selected.end()));
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
                let _ = context.update_selection(as_u32(selected.start()), as_u32(selected.end()));
            }
            Surface::TextArea { element, .. } => {
                let _ =
                    element.set_selection_range(as_u32(selected.start()), as_u32(selected.end()));
            }
        }
        self.push_bounds();
    }

    /// The canvas's position in the viewport, which the store's logical
    /// window coordinates are offset by.
    fn origin(&self) -> (f64, f64) {
        let rect = self.canvas.get_bounding_client_rect();
        (rect.left(), rect.top())
    }

    /// Listeners that tell the surface where the field moved to when the page
    /// or one of the canvas's ancestors scrolls, or the viewport resizes or
    /// pans under a pinch zoom, which move the canvas without the store's
    /// layout changing. `scroll` does not bubble, so it is caught on the
    /// window in the capture phase.
    fn viewport_listeners(&self) -> Result<Vec<Listener>, JsValue> {
        let window = web_sys::window().ok_or_else(|| JsValue::from_str("there is no window"))?;
        let mut targets: Vec<(web_sys::EventTarget, bool)> = vec![(window.clone().into(), true)];
        // Not every browser has a visual viewport.
        if let Ok(viewport) = js_sys::Reflect::get(&window, &JsValue::from_str("visualViewport"))
            && let Ok(viewport) = viewport.dyn_into::<web_sys::EventTarget>()
        {
            targets.push((viewport, false));
        }
        let mut listeners = Vec::new();
        for (target, capture) in &targets {
            for kind in ["scroll", "resize"] {
                let this = self.this.clone();
                listeners.push(
                    Listener::install_in_phase(
                        target,
                        kind,
                        &self.accepting,
                        *capture,
                        move |_| {
                            if let Some(this) = this.upgrade() {
                                this.push_bounds();
                            }
                        },
                    )
                    .map_err(|error| JsValue::from_str(&error.to_string()))?,
                );
            }
        }
        Ok(listeners)
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
            Surface::TextArea { element, .. } => {
                if let Some(caret) = caret {
                    let style = element.style();
                    let _ = style.set_property("left", &format!("{}px", caret.origin.x + left));
                    let _ = style.set_property("top", &format!("{}px", caret.origin.y + top));
                    let _ =
                        style.set_property("height", &format!("{}px", caret.size.height.max(1.0)));
                }
            }
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
                    && let Some(pair) = Utf16Range::new(span.start(), next.end())
                    && let Ok(rect) = session.rect_for_range(pair)
                {
                    spans.next();
                    Some((pair, rect))
                } else if let Some(before) = span.start().get().checked_sub(1)
                    && let Some(pair) = Utf16Range::new(Utf16Offset::new(before), span.end())
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
        if !self.keys.composing.get() {
            return CompositionEnd::Committed;
        }
        self.keys.composing.set(false);
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
            Some(Surface::TextArea { element, .. }) => {
                let _ = element.blur();
                let _ = element.focus();
            }
            None => {}
        }
        CompositionEnd::Abandoned
    }
}

impl TextStoreHost for WebTextInput {
    fn focus_store(&self, store: Option<Rc<dyn TextStore>>) {
        if self.entry.get() > 0 {
            self.pending.borrow_mut().push_back(HostOp::Focus(store));
            return;
        }
        self.refocus(store);
        self.drain();
    }

    fn complete_composition(
        &self,
        store: &Rc<dyn TextStore>,
    ) -> Result<CompositionEnd, TextStoreHostError> {
        if self.retired.get() {
            return Err(TextStoreHostError::Unavailable);
        }
        // The focus a queued request will give is the one asked about.
        let queued = self.pending.borrow().iter().rev().find_map(|op| match op {
            HostOp::Focus(store) => Some(store.clone()),
            HostOp::Complete(_) => None,
        });
        let focused = queued.unwrap_or_else(|| self.focused());
        if !focused.is_some_and(|focused| Rc::ptr_eq(&focused, store)) {
            return Err(TextStoreHostError::NotFocused);
        }
        if self.entry.get() > 0 {
            self.pending
                .borrow_mut()
                .push_back(HostOp::Complete(Rc::clone(store)));
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

    fn status_changed(&self) {}
}
