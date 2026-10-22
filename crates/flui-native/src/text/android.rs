//! The Android host's text input: `FluiView`'s editor mirrors the focused
//! field for the input method (ADR-0247, ADR-0090), and the system font scale
//! sizes text natively.
//!
//! - `ime`: the decidable half, how the view's mirror and the field's store
//!   stay one document while two threads own the session;
//! - `input`: `AndroidTextInput`, the window's `TextStoreHost` over the view,
//!   and `ViewInputMethod`, which answers the input method's calls through
//!   `FluiView` (the host's table hands it the view through
//!   `owner::android::AndroidHost`);
//! - `sizing`: the system text size captured on the Java main thread, its
//!   acquisition checked on every host.
//!
//! Which key the native queue delivers belongs to the input method is the key
//! decoding's (`input::android::keys`).

#[cfg(any(target_os = "android", test))]
pub(crate) mod ime {
    //! The decidable half of the Android backend's input method (ADR-0247,
    //! ADR-0090): how `FluiView`'s mirror and the field's `TextStore` stay one
    //! document while two threads own the session.
    //!
    //! The input method reads and edits `FluiView`'s mirror on the Java main
    //! thread and never waits for FLUI's owner thread. The owner publishes a
    //! [`Snapshot`] of the focused store whenever the store changed outside the
    //! input method's edits, tagged with a generation; the mirror becomes that
    //! snapshot. After each input method edit (or batch of edits) the view
    //! reports its state, and [`Mirror::report`] turns the report into one
    //! [`ImeEdit`] against the snapshot it was made on. The owner replays the
    //! edits in order ([`replay_current`]); an edit made on an older snapshot
    //! than the one the owner published last is dropped, and the owner restarts
    //! the view on the store's text, because a newer snapshot that only moved the
    //! selection leaves the dropped edit's text in the input method. The owner
    //! is the authority; the mirror never is.
    //!
    //! Plain values over UTF-16, built for tests on every host, so Linux CI
    //! pins them; `input` moves them across JNI. What the view reports
    //! ([`EditorState`]) is the view's own vocabulary, `owner::android`'s.

    #![forbid(unsafe_code)]

    use flui_foundation::geometry::{Bounds, Point};
    use flui_platform::keyboard::NamedKey;
    use flui_platform::text_store::{
        Composition, Selection, TextInputPurpose, TextStoreEdit, TextStoreError, TextStoreRead,
        TextStoreStatus, Utf16Offset, Utf16Range,
    };

    use crate::owner::android::EditorState;
    use crate::text::ime::{diff, document_text};

    /// The focused store as the owner last published it to the view.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Snapshot {
        /// Which publication this is; every edit names the one it was made on.
        pub generation: u32,
        /// The text, or [`PROTECTED_UNIT`](crate::text::ime::PROTECTED_UNIT)s for a
        /// protected field.
        pub text: String,
        /// The store's selection.
        pub selection: Selection,
        /// The store's composition.
        pub composition: Option<Utf16Range>,
    }

    impl Snapshot {
        /// The store as `session` reads it, published as `generation`.
        #[must_use]
        pub fn read(session: &dyn TextStoreRead, generation: u32) -> Self {
            Self {
                generation,
                text: document_text(session),
                selection: session.selection(),
                composition: session.composition().map(|composition| composition.range),
            }
        }

        /// The offsets `FluiView` takes: selection start and end, composition
        /// start and end, `-1` for no composition.
        #[must_use]
        pub fn java_offsets(&self) -> [i32; 4] {
            let java = |offset: Utf16Offset| i32::try_from(offset.get()).unwrap_or(i32::MAX);
            let (composition_start, composition_end) = self
                .composition
                .map_or((-1, -1), |range| (java(range.start()), java(range.end())));
            [
                java(self.selection.anchor),
                java(self.selection.active),
                composition_start,
                composition_end,
            ]
        }
    }

    /// One input method edit, against the snapshot it was made on.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct ImeEdit {
        /// The snapshot generation the view held when it made the edit.
        pub base: u32,
        /// The span of that text the edit replaced, and what replaced it; `None`
        /// when only the selection or the composition moved.
        pub replaced: Option<(Utf16Range, String)>,
        /// The selection after the edit.
        pub selection: Selection,
        /// The composition after the edit.
        pub composition: Option<Utf16Range>,
    }

    impl ImeEdit {
        /// Whether the input method composes after this edit.
        #[must_use]
        pub const fn composes(&self) -> bool {
            self.composition.is_some()
        }

        /// Apply the edit to `session`: the replacement, then the selection and
        /// the composition the view ended with.
        ///
        /// # Errors
        ///
        /// The store's refusal of a step; the steps before it stay applied.
        pub fn replay(&self, session: &mut dyn TextStoreEdit) -> Result<(), TextStoreError> {
            if let Some((range, text)) = &self.replaced {
                session.replace(*range, text)?;
            }
            session.set_selection(self.selection)?;
            session.set_composition(self.composition.map(|range| Composition {
                range,
                hides_caret: false,
            }))
        }
    }

    /// What [`replay_current`] did with an edit.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Replayed {
        /// The store took it.
        Applied,
        /// It was made on an older snapshot than the owner's latest, which resets
        /// the view: dropped.
        Stale,
        /// The store refused a step: the view no longer mirrors the store, and
        /// the owner publishes a new snapshot.
        Refused,
    }

    impl Replayed {
        /// Whether the view must restart on a fresh snapshot of the store. A
        /// stale edit was dropped after the owner's newer snapshot reached the
        /// view as a selection-only update, which only tells the input method:
        /// the character it committed would otherwise stay in its cached text
        /// while the field never received it.
        #[must_use]
        pub const fn restarts_the_view(self) -> bool {
            !matches!(self, Self::Applied)
        }
    }

    /// Replay `edit` into `session` if it was made on the snapshot the owner
    /// published last (`published`).
    pub fn replay_current(
        edit: &ImeEdit,
        published: u32,
        session: &mut dyn TextStoreEdit,
    ) -> Replayed {
        if edit.base != published {
            return Replayed::Stale;
        }
        match edit.replay(session) {
            Ok(()) => Replayed::Applied,
            Err(_) => Replayed::Refused,
        }
    }

    /// The Java-thread half: the text of the snapshot the view holds now, which
    /// each report is diffed against.
    #[derive(Debug, Default)]
    pub struct Mirror {
        generation: u32,
        units: Vec<u16>,
    }

    impl Mirror {
        /// The view now holds `snapshot`.
        #[cfg(test)]
        pub fn install(&mut self, snapshot: &Snapshot) {
            self.hold(snapshot.generation, &snapshot.text);
        }

        /// The view now holds `text`, published as `generation`: a snapshot's
        /// text, which is all of it the reports are diffed against.
        pub fn hold(&mut self, generation: u32, text: &str) {
            self.generation = generation;
            self.units = text.encode_utf16().collect();
        }

        /// The edit that turned the mirror into `state`, which the view reported
        /// holding snapshot `generation`; `None` when the report is about an older
        /// snapshot than the one published since (the owner drops such edits
        /// anyway) or changes nothing.
        ///
        /// Offsets are clamped into the new text and moved off the middle of a
        /// surrogate pair, which a store refuses.
        pub fn report(&mut self, generation: u32, state: &EditorState) -> Option<ImeEdit> {
            if generation != self.generation {
                return None;
            }
            let units: Vec<u16> = state.text.encode_utf16().collect();
            let replaced = diff(&self.units, &units).map(|change| {
                let range = Utf16Range::new(
                    Utf16Offset::new(change.start),
                    Utf16Offset::new(change.old_end),
                )
                .expect("BUG: a diff is ordered");
                (
                    range,
                    String::from_utf16_lossy(&units[change.start..change.new_end]),
                )
            });
            let at = |offset: i32| Utf16Offset::new(scalar_boundary(&units, offset));
            let end = units.len();
            let selection = if state.selection_start < 0 || state.selection_end < 0 {
                Selection::collapsed(Utf16Offset::new(end))
            } else {
                Selection {
                    anchor: at(state.selection_start),
                    active: at(state.selection_end),
                }
            };
            let composition = (state.composition_start >= 0 && state.composition_end >= 0)
                .then(|| {
                    let (start, end) = (
                        at(state.composition_start.min(state.composition_end)),
                        at(state.composition_start.max(state.composition_end)),
                    );
                    Utf16Range::new(start, end).expect("BUG: ordered offsets make a range")
                })
                .filter(|range| !range.is_empty());
            self.units = units;
            Some(ImeEdit {
                base: generation,
                replaced,
                selection,
                composition,
            })
        }
    }

    /// `offset` clamped into `units` and moved back off the low half of a
    /// surrogate pair.
    fn scalar_boundary(units: &[u16], offset: i32) -> usize {
        let offset = usize::try_from(offset).unwrap_or(0).min(units.len());
        let splits_pair = offset > 0
            && offset < units.len()
            && matches!(units[offset - 1], 0xD800..=0xDBFF)
            && matches!(units[offset], 0xDC00..=0xDFFF);
        if splits_pair { offset - 1 } else { offset }
    }

    /// `EditorInfo.TYPE_CLASS_TEXT`.
    pub const TYPE_CLASS_TEXT: i32 = 0x1;
    /// `EditorInfo.TYPE_TEXT_VARIATION_PASSWORD`.
    pub const TYPE_TEXT_VARIATION_PASSWORD: i32 = 0x80;
    /// `EditorInfo.TYPE_TEXT_FLAG_MULTI_LINE`.
    pub const TYPE_TEXT_FLAG_MULTI_LINE: i32 = 0x2_0000;
    /// `EditorInfo.IME_ACTION_DONE`.
    pub const IME_ACTION_DONE: i32 = 6;
    /// `EditorInfo.IME_ACTION_NEXT`.
    pub const IME_ACTION_NEXT: i32 = 5;
    /// `EditorInfo.IME_ACTION_PREVIOUS`.
    pub const IME_ACTION_PREVIOUS: i32 = 7;
    /// `EditorInfo.IME_FLAG_NO_ENTER_ACTION`: Enter inserts a line break.
    pub const IME_FLAG_NO_ENTER_ACTION: i32 = 0x4000_0000;
    /// `EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING`.
    pub const IME_FLAG_NO_PERSONALIZED_LEARNING: i32 = 0x0100_0000;

    /// `EditorInfo.TYPE_CLASS_NUMBER`.
    pub const TYPE_CLASS_NUMBER: i32 = 0x2;
    /// `EditorInfo.TYPE_CLASS_PHONE`.
    pub const TYPE_CLASS_PHONE: i32 = 0x3;
    /// `EditorInfo.TYPE_TEXT_VARIATION_URI`.
    pub const TYPE_TEXT_VARIATION_URI: i32 = 0x10;
    /// `EditorInfo.TYPE_TEXT_VARIATION_EMAIL_ADDRESS`.
    pub const TYPE_TEXT_VARIATION_EMAIL_ADDRESS: i32 = 0x20;
    /// `EditorInfo.IME_ACTION_SEARCH`.
    pub const IME_ACTION_SEARCH: i32 = 3;

    /// The `EditorInfo` input type and options a field of `status` asks for: the
    /// keyboard its purpose names (email, web address, number, phone, search), a
    /// password variation that the keyboard does not learn from for a protected
    /// field (which wins over the purpose), a multi-line field whose Enter breaks
    /// lines, and Done otherwise.
    #[must_use]
    pub const fn editor_info(status: TextStoreStatus) -> (i32, i32) {
        let mut input_type = TYPE_CLASS_TEXT;
        let mut options = IME_ACTION_DONE;
        match status.purpose {
            TextInputPurpose::Email => input_type |= TYPE_TEXT_VARIATION_EMAIL_ADDRESS,
            TextInputPurpose::Url => input_type |= TYPE_TEXT_VARIATION_URI,
            TextInputPurpose::Number => input_type = TYPE_CLASS_NUMBER,
            TextInputPurpose::Phone => input_type = TYPE_CLASS_PHONE,
            TextInputPurpose::Search => options = IME_ACTION_SEARCH,
            // Free text, and any purpose added later that has no keyboard here.
            _ => {}
        }
        if status.protected {
            input_type = TYPE_CLASS_TEXT | TYPE_TEXT_VARIATION_PASSWORD;
            options |= IME_FLAG_NO_PERSONALIZED_LEARNING;
        }
        if !status.single_line && input_type & 0xf == TYPE_CLASS_TEXT {
            input_type |= TYPE_TEXT_FLAG_MULTI_LINE;
            options |= IME_FLAG_NO_ENTER_ACTION;
        }
        (input_type, options)
    }

    /// The key an editor action (`performEditorAction`) arrives as, and whether
    /// Shift is held: Next is Tab, Previous is Shift+Tab, and every other action
    /// (Done, Go, Search, Send) is Enter, which a field answers as it answers a
    /// keyboard's.
    #[must_use]
    pub const fn editor_action_key(action: i32) -> (NamedKey, bool) {
        match action {
            IME_ACTION_NEXT => (NamedKey::Tab, false),
            IME_ACTION_PREVIOUS => (NamedKey::Tab, true),
            _ => (NamedKey::Enter, false),
        }
    }

    /// Where the caret and the field are for `CursorAnchorInfo`, in the view's
    /// physical pixels: caret left, top and bottom, then the field's left, top,
    /// right and bottom (a negative width when the field has no layout). `None`
    /// when a coordinate would not be finite.
    #[must_use]
    pub fn cursor_anchor(
        caret: Bounds<f64>,
        field: Option<Bounds<f64>>,
        scale: f64,
    ) -> Option<[f32; 7]> {
        let pixel = |value: f64| {
            let scaled = value * scale;
            #[expect(
                clippy::cast_possible_truncation,
                reason = "CursorAnchorInfo takes floats; finite pixels fit"
            )]
            let narrowed = scaled as f32;
            narrowed.is_finite().then_some(narrowed)
        };
        let (field_left, field_top, field_right, field_bottom) = match field {
            Some(field) => (
                pixel(field.origin.x)?,
                pixel(field.origin.y)?,
                pixel(field.origin.x + field.size.width)?,
                pixel(field.origin.y + field.size.height)?,
            ),
            None => (0.0, 0.0, -1.0, 0.0),
        };
        Some([
            pixel(caret.origin.x)?,
            pixel(caret.origin.y)?,
            pixel(caret.origin.y + caret.size.height)?,
            field_left,
            field_top,
            field_right,
            field_bottom,
        ])
    }

    /// Whether a stylus press at `point` starts the input method's handwriting, on
    /// a device that has it (API 33): a pen tip down inside the focused field's
    /// bounds. Android's automatic initiation sees only the stylus events native
    /// code leaves unhandled, so FLUI, which hit-tests on its owner, asks for it
    /// explicitly.
    #[must_use]
    pub fn starts_handwriting(
        pen_tip_down: bool,
        field: Option<Bounds<f64>>,
        point: Point<f64>,
    ) -> bool {
        pen_tip_down
            && field.is_some_and(|field| {
                point.x >= field.origin.x
                    && point.y >= field.origin.y
                    && point.x <= field.origin.x + field.size.width
                    && point.y <= field.origin.y + field.size.height
            })
    }

    #[cfg(test)]
    mod tests;
}

#[cfg(target_os = "android")]
mod input {
    //! The Android window's text input: the focused field's `TextStore`
    //! (ADR-0090) served to the input method through `FluiView`'s
    //! `InputConnection` (ADR-0247).
    //!
    //! [`AndroidTextInput`] is the window's `TextStoreHost` (ADR-0135). The input
    //! method asks synchronously on the Java main thread and never waits for the
    //! owner: it reads and edits the view's mirror of the field, and the owner
    //! keeps that mirror honest. Focusing a field publishes a snapshot of its
    //! store, which becomes the mirror, and shows the soft keyboard. Each edit the
    //! view reports is replayed into the store under an asynchronous lock, in
    //! order, now if the store is free and else when its frame transaction lets
    //! it; an edit made on a snapshot older than the latest one the owner
    //! published is dropped (`text::android::ime`). When the field changes
    //! outside the input method's edits (the application's own edit, a moved
    //! caret) the store's observer publishes a new snapshot: a text change
    //! restarts the input method, a selection change only tells it.
    //!
    //! A request the presentation makes from inside a store call the host started
    //! waits and runs, in order, when that call returns.

    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::rc::{Rc, Weak};
    use std::sync::Arc;

    use flui_foundation::geometry::{Bounds, Point};
    use flui_platform::text_store::{
        CompositionEnd, LockGrant, LockOutcome, LockTiming, TextChange, TextStore, TextStoreEdit,
        TextStoreHost, TextStoreHostError, TextStoreObserver, TextStoreRead,
    };
    use jni::{JValue, jni_sig, jni_str};

    use super::ime::{
        ImeEdit, Mirror, Snapshot, cursor_anchor, editor_action_key, editor_info, replay_current,
        starts_handwriting,
    };
    use crate::owner::android::EditorState;
    use crate::owner::panic_boundary::contain_owner_callback;
    use crate::version::gate::Since;
    use crate::version::gate::android::{AndroidApi, StylusHandwriting};

    use crate::owner::android::ActivityWindow;
    use crate::owner::android::java_view::{InputMethod, InputMethodReply, JavaView};
    use crate::window::android::AndroidWindow;

    /// The key presses an input method's editor action arrives as: `key`
    /// pressed and released at `uptime_ms`, with Shift when `shift`.
    fn editor_action_keys(
        key: flui_platform::keyboard::NamedKey,
        shift: bool,
        uptime_ms: i64,
    ) -> [flui_platform::PlatformInput; 2] {
        use flui_platform::keyboard::{Code, Key, KeyEvent, KeyState, Modifiers};
        let time = flui_platform::EventTime::from_nanos(
            u64::try_from(uptime_ms)
                .unwrap_or(0)
                .saturating_mul(1_000_000),
        );
        let modifiers = if shift {
            Modifiers::SHIFT
        } else {
            Modifiers::NONE
        };
        [KeyState::Down, KeyState::Up].map(|state| {
            flui_platform::PlatformInput::Keyboard(
                KeyEvent::new(state, Key::Named(key), Code::Unidentified, time)
                    .with_modifiers(modifiers),
            )
        })
    }

    /// The text input of the window the owner runs an input method's work
    /// against: the Android window holds it.
    fn text_input_of(window: &dyn ActivityWindow) -> Option<Rc<AndroidTextInput>> {
        window
            .as_any()
            .downcast_ref::<AndroidWindow>()?
            .text_input()
    }

    /// `InputConnection.CURSOR_UPDATE_IMMEDIATE`: send the cursor once, now.
    const CURSOR_UPDATE_IMMEDIATE: i32 = 1;
    /// `InputConnection.CURSOR_UPDATE_MONITOR`: send it whenever it changes.
    const CURSOR_UPDATE_MONITOR: i32 = 2;

    /// What answers the input method's calls through `FluiView`: the text the
    /// view holds of the focused field, as the owner last published it or the
    /// input method last reported it, against which each report becomes one
    /// edit of the field; and the window's text input, which the work it hands
    /// the owner reaches.
    #[derive(Debug, Default)]
    pub(crate) struct ViewInputMethod {
        mirror: parking_lot::Mutex<Mirror>,
    }

    impl InputMethod for ViewInputMethod {
        fn hold(&self, generation: u32, text: &str) {
            self.mirror.lock().hold(generation, text);
        }

        fn edited(&self, generation: u32, state: EditorState) -> Option<InputMethodReply> {
            let edit = self.mirror.lock().report(generation, &state)?;
            Some(InputMethodReply {
                composing: Some(edit.composes()),
                work: Box::new(move |window: &dyn ActivityWindow| {
                    if let Some(input) = text_input_of(window) {
                        input.apply(edit);
                    }
                }),
            })
        }

        fn editor_action(&self, action: i32, uptime_ms: i64) -> InputMethodReply {
            InputMethodReply {
                composing: None,
                work: Box::new(move |window: &dyn ActivityWindow| {
                    if let Some(input) = text_input_of(window) {
                        input.editor_action();
                    }
                    // The action arrives as the key it stands for, which a field
                    // answers as it answers a keyboard's.
                    let (key, shift) = editor_action_key(action);
                    for key_input in editor_action_keys(key, shift, uptime_ms) {
                        window.callbacks().dispatch_input(key_input);
                    }
                }),
            }
        }

        fn cursor_updates(&self, mode: i32) -> InputMethodReply {
            InputMethodReply {
                composing: None,
                work: Box::new(move |window: &dyn ActivityWindow| {
                    if let Some(input) = text_input_of(window) {
                        input.cursor_updates(mode);
                    }
                }),
            }
        }

        fn pen_down(&self, window: &dyn ActivityWindow, at: Point<f64>) {
            if let Some(input) = text_input_of(window) {
                input.offer_pen(at);
            }
        }
    }

    /// A host request made while the host was already inside a call into a
    /// store: applied, in order, when that call returns.
    enum HostOp {
        Focus(Option<Rc<dyn TextStore>>),
        Complete(Rc<dyn TextStore>),
    }

    /// How a published snapshot reaches the view.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Publish {
        /// A field took focus: the view becomes its editor.
        Focus,
        /// The text changed: the input method restarts on the new text.
        Restart,
        /// Only the selection or the composition moved.
        Selection,
    }

    /// The window's text input (see the module doc).
    pub struct AndroidTextInput {
        view: Arc<JavaView>,
        /// Whether the input method can start stylus handwriting on the view.
        handwriting: Option<Since<StylusHandwriting>>,
        focused: RefCell<Option<Rc<dyn TextStore>>>,
        /// The generation of the snapshot published last; every replayed edit
        /// must have been made on it.
        published: Rc<Cell<u32>>,
        /// What `requestCursorUpdates` last asked for.
        cursor_mode: Cell<i32>,
        entry: Cell<u32>,
        pending: RefCell<VecDeque<HostOp>>,
        retired: Cell<bool>,
        this: Weak<Self>,
    }

    impl std::fmt::Debug for AndroidTextInput {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("AndroidTextInput").finish_non_exhaustive()
        }
    }

    /// Counts one call into a store, however it ends.
    struct Entered<'a>(&'a Cell<u32>);

    impl Drop for Entered<'_> {
        fn drop(&mut self) {
            self.0.set(self.0.get() - 1);
        }
    }

    /// Publish `snapshot` to the view: it becomes the mirror the next reports are
    /// diffed against, and the view takes it as `how` says.
    fn publish(view: &Arc<JavaView>, snapshot: Snapshot, how: Publish, status: (i32, i32, bool)) {
        view.input_method()
            .hold(snapshot.generation, &snapshot.text);
        view.set_composing(snapshot.composition.is_some());
        let [
            selection_start,
            selection_end,
            composition_start,
            composition_end,
        ] = snapshot.java_offsets();
        // The generation wraps; Java carries its bits as an opaque `int`.
        let generation = snapshot.generation.cast_signed();
        let text = snapshot.text;
        view.post(move |env, view| {
            let text = env.new_string(&text)?;
            match how {
                Publish::Focus => {
                    let (input_type, options, stylus) = status;
                    env.call_method(
                        view,
                        jni_str!("focusEditor"),
                        jni_sig!("(IIZLjava/lang/String;IIIII)V"),
                        &[
                            JValue::Int(input_type),
                            JValue::Int(options),
                            JValue::Bool(stylus),
                            JValue::Object(&text),
                            JValue::Int(selection_start),
                            JValue::Int(selection_end),
                            JValue::Int(composition_start),
                            JValue::Int(composition_end),
                            JValue::Int(generation),
                        ],
                    )?;
                }
                Publish::Restart | Publish::Selection => {
                    env.call_method(
                        view,
                        jni_str!("pushState"),
                        jni_sig!("(Ljava/lang/String;IIIIIZ)V"),
                        &[
                            JValue::Object(&text),
                            JValue::Int(selection_start),
                            JValue::Int(selection_end),
                            JValue::Int(composition_start),
                            JValue::Int(composition_end),
                            JValue::Int(generation),
                            JValue::Bool(how == Publish::Restart),
                        ],
                    )?;
                }
            }
            Ok(())
        });
    }

    impl AndroidTextInput {
        pub(crate) fn new(view: Arc<JavaView>, api: AndroidApi) -> Rc<Self> {
            Rc::new_cyclic(|this| Self {
                view,
                handwriting: api.since::<StylusHandwriting>(),
                focused: RefCell::new(None),
                published: Rc::new(Cell::new(0)),
                cursor_mode: Cell::new(0),
                entry: Cell::new(0),
                pending: RefCell::new(VecDeque::new()),
                retired: Cell::new(false),
                this: this.clone(),
            })
        }

        /// The window closed: detach the focused field, if any, for good.
        pub(crate) fn retire(&self) {
            self.retired.set(true);
            self.pending.borrow_mut().clear();
            self.detach();
        }

        fn focused(&self) -> Option<Rc<dyn TextStore>> {
            self.focused.borrow().clone()
        }

        fn enter(&self) -> Entered<'_> {
            self.entry.set(self.entry.get() + 1);
            Entered(&self.entry)
        }

        fn next_generation(published: &Cell<u32>) -> u32 {
            let next = published.get().wrapping_add(1);
            published.set(next);
            next
        }

        fn status(&self, store: &dyn TextStore) -> (i32, i32, bool) {
            let (input_type, options) = editor_info(store.status());
            let stylus = self.handwriting.is_some();
            (input_type, options, stylus)
        }

        /// Run the requests queued while the host was inside a store call.
        fn drain(&self) {
            if self.entry.get() > 0 {
                return;
            }
            loop {
                let next = self.pending.borrow_mut().pop_front();
                match next {
                    Some(HostOp::Focus(store)) => self.refocus(store),
                    Some(HostOp::Complete(store)) => {
                        let focused = self
                            .focused()
                            .is_some_and(|focused| Rc::ptr_eq(&focused, &store));
                        if !focused || self.end_composition(&store) == CompositionEnd::Abandoned {
                            let _ = flui_platform::text_store::commit_composition_in_place(&*store);
                        }
                    }
                    None => return,
                }
            }
        }

        /// One input method edit the view reported: replayed into the focused
        /// store in order, or dropped when it was made on an older snapshot.
        fn apply(&self, edit: ImeEdit) {
            let Some(store) = self.focused() else {
                return;
            };
            let published = Rc::clone(&self.published);
            let view = Arc::clone(&self.view);
            let status = self.status(&*store);
            let this = self.this.clone();
            let edited = Rc::clone(&store);
            let grant = LockGrant::read_write(move |session| {
                let outcome = replay_current(&edit, published.get(), session);
                // Focus may have moved to another field while the grant waited: its
                // snapshot is the one the view mirrors now, and a restart on this
                // store would overwrite it.
                let still_focused = this
                    .upgrade()
                    .and_then(|input| input.focused())
                    .is_some_and(|current| Rc::ptr_eq(&current, &edited));
                if outcome.restarts_the_view() && still_focused {
                    // The store refused a step, or the edit was dropped as stale:
                    // the input method's text no longer matches the store's.
                    let generation = Self::next_generation(&published);
                    publish(
                        &view,
                        Snapshot::read(&*session, generation),
                        Publish::Restart,
                        status,
                    );
                }
            });
            {
                let _entered = self.enter();
                if let Err(error) = store.request_lock(grant, LockTiming::Async) {
                    tracing::debug!(?error, "the focused store refused the input method's edit");
                    self.publish_now(Publish::Restart);
                }
            }
            self.drain();
        }

        /// Publish the focused store now, or when its frame transaction lets it.
        fn publish_now(&self, how: Publish) {
            let Some(store) = self.focused() else {
                return;
            };
            let published = Rc::clone(&self.published);
            let view = Arc::clone(&self.view);
            let status = self.status(&*store);
            let grant = move |session: &dyn TextStoreRead| {
                let generation = Self::next_generation(&published);
                publish(&view, Snapshot::read(session, generation), how, status);
            };
            let _entered = self.enter();
            let sync = {
                let grant = grant.clone();
                store.request_lock(LockGrant::read(grant), LockTiming::Sync)
            };
            if sync.is_err() {
                let _ = store.request_lock(LockGrant::read(grant), LockTiming::Async);
            }
        }

        fn detach(&self) {
            let previous = self.focused.borrow_mut().take();
            let Some(store) = previous else {
                return;
            };
            self.view.set_composing(false);
            self.cursor_mode.set(0);
            contain_owner_callback(|| store.set_observer(None));
            self.view.post(|env, view| {
                env.call_method(view, jni_str!("blurEditor"), jni_sig!("()V"), &[])
                    .map(drop)
            });
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
            let Some(store) = store else {
                return;
            };
            *self.focused.borrow_mut() = Some(Rc::clone(&store));
            let observer: Rc<dyn TextStoreObserver> = Rc::new(MirrorObserver {
                input: self.this.clone(),
            });
            contain_owner_callback(|| store.set_observer(Some(observer)));
            self.publish_now(Publish::Focus);
        }

        /// End the input method's composition in `store`: commit it in the store
        /// and restart the input method on the result, which drops its own.
        fn end_composition(&self, store: &Rc<dyn TextStore>) -> CompositionEnd {
            let published = Rc::clone(&self.published);
            let view = Arc::clone(&self.view);
            let status = self.status(&**store);
            let grant = LockGrant::read_write(move |session: &mut dyn TextStoreEdit| {
                if session.composition().is_some() {
                    let _ = session.set_composition(None);
                }
                let generation = Self::next_generation(&published);
                view.set_composing(false);
                publish(
                    &view,
                    Snapshot::read(&*session, generation),
                    Publish::Restart,
                    status,
                );
            });
            let outcome = {
                let _entered = self.enter();
                store.request_lock(grant, LockTiming::Async)
            };
            match outcome {
                Ok(LockOutcome::Granted) => CompositionEnd::Committed,
                Ok(LockOutcome::Deferred) => CompositionEnd::Deferred,
                Err(_) => CompositionEnd::Abandoned,
            }
        }

        /// The input method performs its editor action (`performEditorAction`):
        /// the action acts on the text the input method shows, so a composition
        /// still open in the focused store is committed first. An input method
        /// may send the action without finishing its composition (Samsung
        /// Keyboard's Done does); the field would otherwise take the action's
        /// Enter for the input method's own and ignore it. The commit is queued
        /// like an edit, so the key that follows lands after it.
        fn editor_action(&self) {
            let Some(store) = self.focused() else {
                return;
            };
            let published = Rc::clone(&self.published);
            let view = Arc::clone(&self.view);
            let status = self.status(&*store);
            let grant = LockGrant::read_write(move |session: &mut dyn TextStoreEdit| {
                if session.composition().is_none() {
                    return;
                }
                let _ = session.set_composition(None);
                view.set_composing(false);
                let generation = Self::next_generation(&published);
                publish(
                    &view,
                    Snapshot::read(&*session, generation),
                    Publish::Restart,
                    status,
                );
            });
            {
                let _entered = self.enter();
                if let Err(error) = store.request_lock(grant, LockTiming::Async) {
                    tracing::debug!(
                        ?error,
                        "the focused store refused the editor action's commit"
                    );
                }
            }
            self.drain();
        }

        /// `requestCursorUpdates(mode)`.
        fn cursor_updates(&self, mode: i32) {
            self.cursor_mode.set(mode & CURSOR_UPDATE_MONITOR);
            if mode & CURSOR_UPDATE_IMMEDIATE != 0 {
                self.push_cursor();
            }
        }

        /// Tell the input method where the caret and the field are.
        fn push_cursor(&self) {
            let Some(store) = self.focused() else {
                return;
            };
            let found: Rc<Cell<Option<[f32; 7]>>> = Rc::default();
            let out = Rc::clone(&found);
            let scale = self.view.scale();
            let grant = LockGrant::read(move |session| {
                let caret = session.rect_for_range(session.selection().range());
                if let Ok(caret) = caret {
                    out.set(cursor_anchor(
                        caret.bounds,
                        session.document_bounds().ok(),
                        scale,
                    ));
                }
            });
            {
                let _entered = self.enter();
                if store.request_lock(grant, LockTiming::Sync).is_err() {
                    return;
                }
            }
            let Some(anchor) = found.get() else {
                return;
            };
            self.view.post(move |env, view| {
                env.call_method(
                    view,
                    jni_str!("updateCursorAnchor"),
                    jni_sig!("(FFFFFFF)V"),
                    &anchor.map(JValue::Float),
                )
                .map(drop)
            });
        }

        /// A pen tip touched down at `point`: on a device that has the input
        /// method's handwriting, a stroke inside the focused field is handed to
        /// it.
        fn offer_pen(&self, point: Point<f64>) {
            let Some(handwriting) = self.handwriting else {
                return;
            };
            if starts_handwriting(true, self.field_bounds(), point) {
                self.start_handwriting(handwriting);
            }
        }

        /// The focused field's bounds, for a stylus press that may start
        /// handwriting; `None` when no field is focused or laid out.
        fn field_bounds(&self) -> Option<Bounds<f64>> {
            let store = self.focused()?;
            let found: Rc<Cell<Option<Bounds<f64>>>> = Rc::default();
            let out = Rc::clone(&found);
            let grant = LockGrant::read(move |session| out.set(session.document_bounds().ok()));
            let _entered = self.enter();
            let _granted = store.request_lock(grant, LockTiming::Sync).ok()?;
            found.get()
        }

        /// Hand the stylus stroke that just began to the input method.
        fn start_handwriting(&self, _handwriting: Since<StylusHandwriting>) {
            self.view.post(|env, view| {
                let started = env
                    .call_method(view, jni_str!("startHandwriting"), jni_sig!("()Z"), &[])?
                    .z()?;
                tracing::info!(target: "flui_native::android", started, "stylus handwriting requested");
                Ok(())
            });
        }
    }

    impl TextStoreHost for AndroidTextInput {
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
            // Retired with its window for good; a view not ready yet may be.
            if self.retired.get() {
                return Err(flui_platform::Unsupported::new(
                    flui_platform::UnsupportedReason::NoWindow,
                )
                .into());
            }
            if !self.view.is_ready() {
                return Err(flui_platform::Unsupported::new(
                    flui_platform::UnsupportedReason::Unavailable,
                )
                .into());
            }
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
            let end = self.end_composition(store);
            self.drain();
            Ok(end)
        }
    }

    impl crate::text::TextInput for Rc<AndroidTextInput> {
        fn window_text_input(&self) -> Option<crate::text::WindowTextInput> {
            Some(crate::text::WindowTextInput::Store(
                Rc::clone(self) as Rc<dyn TextStoreHost>
            ))
        }
    }

    /// What the store tells the view when the application changes the field.
    struct MirrorObserver {
        input: Weak<AndroidTextInput>,
    }

    impl TextStoreObserver for MirrorObserver {
        fn text_changed(&self, _change: TextChange) {
            if let Some(input) = self.input.upgrade() {
                input.publish_now(Publish::Restart);
            }
        }

        fn selection_changed(&self) {
            if let Some(input) = self.input.upgrade() {
                input.publish_now(Publish::Selection);
            }
        }

        fn layout_changed(&self) {
            if let Some(input) = self.input.upgrade()
                && input.cursor_mode.get() & CURSOR_UPDATE_MONITOR != 0
            {
                input.push_cursor();
            }
        }

        fn status_changed(&self) {
            if let Some(input) = self.input.upgrade() {
                input.publish_now(Publish::Focus);
            }
        }
    }
}

#[cfg(any(target_os = "android", test))]
pub(crate) mod sizing {
    //! Java-main acquisition; owner-lane queries use only a private metrics copy.

    use crate::{
        CapturedTextSizing, TextSizingCaptureError as Error, TextSizingCaptureState as Capture,
        owner::signal::OwnerSignal, text::sizing::NativeTextSize,
    };
    #[cfg(target_os = "android")]
    use android_activity::AndroidApp;
    #[cfg(target_os = "android")]
    use jni::{JValue, JavaVM, jni_sig, jni_str, objects::JObject, refs::Global};
    use parking_lot::Mutex;
    use std::sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    };
    use std::time::{Duration, Instant};

    /// A captured converter, which the Java main thread hands the owner.
    type Native = Arc<dyn NativeTextSize + Send + Sync>;

    #[cfg(target_os = "android")]
    struct Snapshot {
        vm: JavaVM,
        metrics: Global<JObject<'static>>,
        density: f64,
    }

    #[cfg(target_os = "android")]
    impl NativeTextSize for Snapshot {
        fn query(
            &self,
            request: flui_foundation::TextSizeRequest,
        ) -> Result<flui_foundation::TextSize, Error> {
            use flui_foundation::TextScaleProfile;
            match request.profile {
                TextScaleProfile::LargeTitle
                | TextScaleProfile::Title1
                | TextScaleProfile::Title2
                | TextScaleProfile::Title3
                | TextScaleProfile::Headline
                | TextScaleProfile::Body
                | TextScaleProfile::Callout
                | TextScaleProfile::Subheadline
                | TextScaleProfile::Footnote
                | TextScaleProfile::Caption1
                | TextScaleProfile::Caption2 => {}
                _ => return Err(Error::UnsupportedProfile),
            }
            #[expect(
                clippy::cast_possible_truncation,
                reason = "TextSize admits its positive finite f32 representation"
            )]
            let size = request.size.value() as f32;
            let physical = self
                .vm
                .attach_current_thread(|env| -> jni::errors::Result<f32> {
                    let result = env
                        .call_static_method(
                            jni_str!("android/util/TypedValue"),
                            jni_str!("applyDimension"),
                            jni_sig!("(IFLandroid/util/DisplayMetrics;)F"),
                            &[
                                JValue::Int(2),
                                JValue::Float(size),
                                JValue::Object(self.metrics.as_ref()),
                            ],
                        )
                        .and_then(jni::JValueOwned::f);
                    if result.is_err() {
                        env.exception_clear();
                    }
                    result
                })
                .map_err(native_error)?;
            flui_foundation::TextSize::new(f64::from(physical) / self.density).map_err(Into::into)
        }
    }

    #[cfg(target_os = "android")]
    fn native_error(error: jni::errors::Error) -> Error {
        Error::Native {
            message: error.to_string(),
        }
    }

    #[cfg(target_os = "android")]
    #[expect(
        unsafe_code,
        reason = "AndroidApp lends live VM/Activity references; no owned Activity reference is constructed"
    )]
    fn capture(app: &AndroidApp) -> Result<Snapshot, Error> {
        // SAFETY: AndroidApp retains the live Android VM for this capture callback.
        let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
        let (metrics, density) = vm
            .attach_current_thread(|env| -> jni::errors::Result<_> {
                let result = (|| {
                    let raw = app.activity_as_ptr() as jni::sys::jobject;
                    // SAFETY: borrow AndroidApp's global Activity; the temporary wrapper never deletes it.
                    let activity = unsafe { env.as_cast_raw::<Global<JObject<'_>>>(&raw)? };
                    let resources = env
                        .call_method(
                            activity.as_ref(),
                            jni_str!("getResources"),
                            jni_sig!("()Landroid/content/res/Resources;"),
                            &[],
                        )?
                        .l()?;
                    let live = env
                        .call_method(
                            &resources,
                            jni_str!("getDisplayMetrics"),
                            jni_sig!("()Landroid/util/DisplayMetrics;"),
                            &[],
                        )?
                        .l()?;
                    let copy = env.new_object(
                        jni_str!("android/util/DisplayMetrics"),
                        jni_sig!("()V"),
                        &[],
                    )?;
                    env.call_method(
                        &copy,
                        jni_str!("setTo"),
                        jni_sig!("(Landroid/util/DisplayMetrics;)V"),
                        &[JValue::Object(&live)],
                    )?;
                    let density = env
                        .get_field(&copy, jni_str!("density"), jni_sig!("F"))?
                        .f()?;
                    Ok((env.new_global_ref(&copy)?, f64::from(density)))
                })();
                if result.is_err() {
                    env.exception_clear();
                }
                result
            })
            .map_err(native_error)?;
        if !density.is_finite() || density <= 0.0 {
            return Err(Error::Native {
                message: "Android captured invalid text density".into(),
            });
        }
        Ok(Snapshot {
            vm,
            metrics,
            density,
        })
    }

    type NativeResult = Result<Native, Error>;

    struct Completion {
        ticket: Arc<()>,
        authority: Arc<AtomicBool>,
        result: NativeResult,
    }

    enum Flight {
        Awaiting { ticket: Arc<()>, deadline: Instant },
        Stalled { ticket: Arc<()> },
    }

    impl Flight {
        fn ticket(&self) -> &Arc<()> {
            match self {
                Self::Awaiting { ticket, .. } | Self::Stalled { ticket } => ticket,
            }
        }
    }

    struct State {
        authority: Arc<AtomicBool>,
        flight: Option<Flight>,
        completed: Option<Completion>,
        current: Option<Native>,
        retry: Option<Instant>,
    }

    pub(crate) struct Acquisition {
        state: Mutex<State>,
    }

    enum Admission {
        Waiting(Capture),
        Dispatch(Receipt),
    }

    /// Ownership of the posted callback also owns its completion obligation.
    /// The dispatcher may drop it before invoking it (for example, failed attach).
    struct Receipt {
        acquisition: Weak<Acquisition>,
        signal: Weak<OwnerSignal>,
        completion: Option<Completion>,
    }

    impl Receipt {
        fn complete(mut self, result: NativeResult) {
            let mut completion = self
                .completion
                .take()
                .expect("BUG: capture receipt settles once");
            completion.result = result;
            self.deliver(completion);
        }

        fn deliver(&self, completion: Completion) {
            let Some(acquisition) = self.acquisition.upgrade() else {
                return;
            };
            let mut outgoing = Some(completion);
            let committed = {
                let mut state = acquisition.state.lock();
                let ticket = &outgoing
                    .as_ref()
                    .expect("BUG: uncommitted completion exists")
                    .ticket;
                if state
                    .flight
                    .as_ref()
                    .is_some_and(|flight| Arc::ptr_eq(flight.ticket(), ticket))
                    && state.completed.is_none()
                {
                    state.completed = outgoing.take();
                    true
                } else {
                    false
                }
            };
            // Native ownership and outgoing wake never retire under the inbox guard.
            drop(outgoing);
            if committed && let Some(signal) = self.signal.upgrade() {
                let _ = signal.wake();
            }
        }
    }

    impl Drop for Receipt {
        fn drop(&mut self) {
            if let Some(completion) = self.completion.take() {
                crate::owner::panic_boundary::contain_owner_callback(|| self.deliver(completion));
            }
        }
    }

    impl Acquisition {
        pub(crate) fn new() -> Arc<Self> {
            Arc::new(Self {
                state: Mutex::new(State {
                    authority: Arc::new(AtomicBool::new(true)),
                    flight: None,
                    completed: None,
                    current: None,
                    retry: None,
                }),
            })
        }

        pub(crate) fn invalidate(&self) {
            let retired = {
                let mut state = self.state.lock();
                state.authority.store(false, Ordering::Release);
                state.authority = Arc::new(AtomicBool::new(true));
                state.retry = None;
                state.current.take()
            };
            drop(retired);
        }

        #[cfg(target_os = "android")]
        pub(crate) fn fence(&self) {
            self.state.lock().authority.store(false, Ordering::Release);
        }

        fn admit(
            self: &Arc<Self>,
            signal: &Weak<OwnerSignal>,
            owner: std::thread::ThreadId,
            now: Instant,
        ) -> Result<Admission, Error> {
            let completed = {
                let mut state = self.state.lock();
                let completed = state.completed.take();
                if completed.as_ref().is_some_and(|completion| {
                    state
                        .flight
                        .as_ref()
                        .is_some_and(|flight| Arc::ptr_eq(flight.ticket(), &completion.ticket))
                }) {
                    state.flight = None;
                }
                completed
            };
            if let Some(completion) = completed {
                let mut state = self.state.lock();
                if Arc::ptr_eq(&completion.authority, &state.authority)
                    && state.authority.load(Ordering::Acquire)
                {
                    match completion.result {
                        Ok(native) => state.current = Some(native),
                        Err(error) => {
                            state.retry = Some(now + Duration::from_millis(500));
                            return Err(error);
                        }
                    }
                } else {
                    drop(state);
                    drop(completion);
                }
            }
            let completion = {
                let mut state = self.state.lock();
                if !state.authority.load(Ordering::Acquire) {
                    return Err(Error::Unavailable);
                }
                if let Some(native) = &state.current {
                    return Ok(Admission::Waiting(Capture::Ready(CapturedTextSizing::new(
                        Arc::clone(native) as Arc<dyn NativeTextSize>,
                        Arc::clone(&state.authority),
                        owner,
                    ))));
                }
                if let Some(flight) = &state.flight {
                    match flight {
                        Flight::Awaiting { deadline, .. } => {
                            if let Some(check_after) = deadline
                                .checked_duration_since(now)
                                .filter(|delay| !delay.is_zero())
                            {
                                return Ok(Admission::Waiting(Capture::Pending { check_after }));
                            }
                            state.flight = Some(Flight::Stalled {
                                ticket: Arc::clone(flight.ticket()),
                            });
                        }
                        Flight::Stalled { .. } => {}
                    }
                    return Ok(Admission::Waiting(Capture::Stalled));
                }
                if let Some(remaining) = state
                    .retry
                    .and_then(|deadline| deadline.checked_duration_since(now))
                    .filter(|remaining| !remaining.is_zero())
                {
                    return Ok(Admission::Waiting(Capture::RetryAfter(remaining)));
                }
                state.retry = None;
                let ticket = Arc::new(());
                // This is an observation budget, not a cancellation acknowledgement
                // or a native execution SLA. Never extend it when polling.
                state.flight = Some(Flight::Awaiting {
                    ticket: Arc::clone(&ticket),
                    deadline: now + Duration::from_millis(500),
                });
                Completion {
                    ticket,
                    authority: Arc::clone(&state.authority),
                    result: Err(Error::Native {
                        message: "Android discarded the text sizing capture callback".into(),
                    }),
                }
            };
            Ok(Admission::Dispatch(Receipt {
                acquisition: Arc::downgrade(self),
                signal: signal.clone(),
                completion: Some(completion),
            }))
        }

        #[cfg(target_os = "android")]
        pub(crate) fn poll(
            self: &Arc<Self>,
            app: &AndroidApp,
            signal: &Weak<OwnerSignal>,
            owner: std::thread::ThreadId,
        ) -> Result<Capture, Error> {
            let admitted_at = Instant::now();
            let receipt = match self.admit(signal, owner, admitted_at)? {
                Admission::Waiting(capture) => return Ok(capture),
                Admission::Dispatch(receipt) => receipt,
            };
            let activity = app.clone();
            let posted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                app.run_on_java_main_thread(Box::new(move || {
                    let result =
                        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            capture(&activity)
                        })) {
                            Ok(result) => result.map(|snapshot| Arc::new(snapshot) as Native),
                            Err(payload) => {
                                std::mem::forget(payload);
                                Err(Error::Native {
                                    message: "Android text sizing capture panicked".into(),
                                })
                            }
                        };
                    crate::owner::panic_boundary::contain_owner_callback(|| {
                        receipt.complete(result);
                    });
                }));
            }));
            if let Err(payload) = posted {
                // Dropping an unposted callback settles its owned receipt. If posting
                // succeeded before panic, its queued callback still owns the ticket.
                std::mem::forget(payload);
            }
            // This is the remaining original flight deadline, not a reset from
            // posting completion. The owner will consume any immediate receipt
            // error/success on its next turn before testing that deadline.
            Ok(Capture::Pending {
                check_after: (admitted_at + Duration::from_millis(500))
                    .saturating_duration_since(Instant::now()),
            })
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn next(acquisition: &Arc<Acquisition>) -> Result<Admission, Error> {
            acquisition.admit(&Weak::new(), std::thread::current().id(), Instant::now())
        }

        fn receipt(acquisition: &Arc<Acquisition>) -> Receipt {
            match next(acquisition).expect("capture admission") {
                Admission::Dispatch(receipt) => receipt,
                Admission::Waiting(_) => panic!("expected new capture dispatch"),
            }
        }

        fn dropped_posted_callback_settles_and_recovers() {
            let acquisition = Acquisition::new();
            let pending = receipt(&acquisition);
            assert!(matches!(
                next(&acquisition),
                Ok(Admission::Waiting(Capture::Pending { .. }))
            ));
            // Exactly the ownership boundary android-activity drops on attach error.
            let callback: Box<dyn FnOnce() + Send> =
                Box::new(move || pending.complete(Err(Error::Unavailable)));
            drop(callback);
            assert!(
                matches!(next(&acquisition), Err(Error::Native { .. })),
                "discarded callback must settle accepted work"
            );
            assert!(
                matches!(next(&acquisition), Ok(Admission::Waiting(Capture::RetryAfter(delay))) if !delay.is_zero()),
                "retry without an accepted flight must expose its wake deadline"
            );
            acquisition.state.lock().retry = Some(Instant::now());
            drop(receipt(&acquisition));
        }

        fn stale_receipt_cannot_settle_a_new_flight() {
            let acquisition = Acquisition::new();
            let pending = receipt(&acquisition);
            let old = pending.completion.as_ref().expect("outstanding receipt");
            let duplicate = Receipt {
                acquisition: Arc::downgrade(&acquisition),
                signal: Weak::new(),
                completion: Some(Completion {
                    ticket: Arc::clone(&old.ticket),
                    authority: Arc::clone(&old.authority),
                    result: Err(Error::Unavailable),
                }),
            };
            acquisition.invalidate();
            drop(pending);
            let replacement = receipt(&acquisition);
            drop(duplicate);
            assert!(
                matches!(
                    next(&acquisition),
                    Ok(Admission::Waiting(Capture::Pending { .. }))
                ),
                "late receipt must not consume replacement debt"
            );
            drop(replacement);
            assert!(matches!(next(&acquisition), Err(Error::Native { .. })));
        }

        fn retry_deadline_boundary() {
            let now = Instant::now();
            for (name, deadline, waiting) in [
                ("before_deadline", now + Duration::from_millis(1), true),
                ("at_deadline", now, false),
                (
                    "after_deadline",
                    now.checked_sub(Duration::from_millis(1))
                        .expect("test instant permits one millisecond subtraction"),
                    false,
                ),
            ] {
                let acquisition = Acquisition::new();
                acquisition.state.lock().retry = Some(deadline);
                let admission = acquisition
                    .admit(&Weak::new(), std::thread::current().id(), now)
                    .expect("retry admission");
                match admission {
                    Admission::Waiting(Capture::RetryAfter(delay)) => {
                        assert!(waiting && !delay.is_zero(), "{name}");
                    }
                    Admission::Dispatch(receipt) => {
                        assert!(!waiting, "{name}");
                        drop(receipt);
                    }
                    Admission::Waiting(other) => {
                        panic!("unexpected readiness at {name}: {other:?}")
                    }
                }
            }
        }

        fn queued_capture_stalls_and_late_current_reply_recovers() {
            let acquisition = Acquisition::new();
            let now = Instant::now();
            let owner = std::thread::current().id();
            let Admission::Dispatch(pending) = acquisition
                .admit(&Weak::new(), owner, now)
                .expect("admission")
            else {
                panic!("first capture must dispatch");
            };
            assert!(
                matches!(acquisition.admit(&Weak::new(), owner, now + Duration::from_millis(250)), Ok(Admission::Waiting(Capture::Pending { check_after })) if check_after == Duration::from_millis(250))
            );
            for offset in [500, 1000, 5000] {
                assert!(
                    matches!(
                        acquisition.admit(&Weak::new(), owner, now + Duration::from_millis(offset)),
                        Ok(Admission::Waiting(Capture::Stalled))
                    ),
                    "an outstanding queued callback must not be reset or reposted"
                );
            }
            pending.complete(Ok(Arc::new(crate::text::sizing::QueryWith(
                |request: flui_foundation::TextSizeRequest| Ok(request.size),
            ))));
            assert!(
                matches!(
                    acquisition.admit(&Weak::new(), owner, now + Duration::from_secs(6)),
                    Ok(Admission::Waiting(Capture::Ready(_)))
                ),
                "late current reply must heal a stalled flight"
            );
        }

        fn completed_inbox_precedes_deadline_observation() {
            let acquisition = Acquisition::new();
            let now = Instant::now();
            let owner = std::thread::current().id();
            let Admission::Dispatch(pending) = acquisition
                .admit(&Weak::new(), owner, now)
                .expect("admission")
            else {
                panic!("first capture must dispatch");
            };
            pending.complete(Err(Error::Unavailable));
            assert!(
                matches!(
                    acquisition.admit(&Weak::new(), owner, now + Duration::from_secs(1)),
                    Err(Error::Unavailable)
                ),
                "settled native failure must remain authoritative over stalled observation"
            );
        }

        fn stalled_old_context_must_settle_before_replacement_dispatch() {
            let acquisition = Acquisition::new();
            let now = Instant::now();
            let owner = std::thread::current().id();
            let Admission::Dispatch(pending) = acquisition
                .admit(&Weak::new(), owner, now)
                .expect("admission")
            else {
                panic!("first capture must dispatch");
            };
            acquisition.invalidate();
            assert!(matches!(
                acquisition.admit(&Weak::new(), owner, now + Duration::from_secs(1)),
                Ok(Admission::Waiting(Capture::Stalled))
            ));
            pending.complete(Err(Error::Unavailable));
            let replacement = acquisition
                .admit(&Weak::new(), owner, now + Duration::from_secs(1))
                .expect("replacement admission");
            assert!(
                matches!(replacement, Admission::Dispatch(_)),
                "a stale settled receipt permits one latest-context dispatch"
            );
            assert!(matches!(
                acquisition.admit(&Weak::new(), owner, now + Duration::from_secs(1)),
                Ok(Admission::Waiting(Capture::Pending { .. }))
            ));
            drop(replacement);
        }

        #[test]
        fn android_capture_delivery_receipt() {
            crate::table_test::run_table(
                "android_capture_delivery_receipt",
                &[
                    (
                        "dropped_posted_callback_settles_and_recovers",
                        dropped_posted_callback_settles_and_recovers,
                    ),
                    (
                        "stale_receipt_cannot_settle_a_new_flight",
                        stale_receipt_cannot_settle_a_new_flight,
                    ),
                    ("retry_deadline_boundary", retry_deadline_boundary),
                    (
                        "queued_capture_stalls_and_late_current_reply_recovers",
                        queued_capture_stalls_and_late_current_reply_recovers,
                    ),
                    (
                        "completed_inbox_precedes_deadline_observation",
                        completed_inbox_precedes_deadline_observation,
                    ),
                    (
                        "stalled_old_context_must_settle_before_replacement_dispatch",
                        stalled_old_context_must_settle_before_replacement_dispatch,
                    ),
                ],
            );
        }
    }
}

#[cfg(target_os = "android")]
pub(crate) use input::{AndroidTextInput, ViewInputMethod};
