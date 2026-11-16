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

// Built on every host with `testing` too, for the conformance table's Android
// row (`text::conformance`), which reaches only the mirror and its replay.
#[cfg(any(target_os = "android", test, feature = "testing"))]
#[cfg_attr(
    not(any(target_os = "android", test)),
    allow(
        dead_code,
        reason = "off Android only the conformance table's mirror and replay are reached"
    )
)]
pub(crate) mod ime {
    //! The decidable half of the Android backend's input method (ADR-0247,
    //! ADR-0090): how `FluiView`'s mirror and the field's `TextStore` stay one
    //! document while two threads own the session.
    //!
    //! The input method reads and edits `FluiView`'s mirror on the Java main
    //! thread and never waits for FLUI's owner thread. Two ledgers keep the
    //! threads in step (ADR-0346):
    //!
    //! - the owner's [`Ledger`] mints a [`Publication`] for each [`Snapshot`]
    //!   of the focused store it publishes (a field took focus, the
    //!   application changed the text, the selection or the composition), and
    //!   records which of the view's reports it had already replayed when it
    //!   read the store;
    //! - the Java thread's [`Mirror`] holds the text the view holds, installs
    //!   each snapshot when the view takes it ([`Mirror::install`]), and turns
    //!   each report of the view into one [`ImeEdit`] against the publication
    //!   it was made on, numbered by a [`ReportId`].
    //!
    //! A snapshot that missed reports the view had already made raced the
    //! input method. If it only moved the selection, the view keeps the input
    //! method's text and the owner still replays the edits ([`Install::Keep`]):
    //! nothing typed is lost. If it changed the text, the application's text
    //! wins: the view takes it and the input method starts over, and the owner
    //! drops the edits made before it ([`Replayed::Stale`]). Otherwise the
    //! input method starts over only when the snapshot ends its composition
    //! or changes the composed text; a text or selection change outside the
    //! composition is only told (`updateSelection`), so a formatter that
    //! rewrites the text on every key leaves the keyboard's composition and
    //! suggestions alone.
    //!
    //! Plain values over UTF-16, built for tests on every host, so Linux CI
    //! pins them; `input` moves them across JNI. What the view reports
    //! ([`EditorReport`]) is the view's own vocabulary, which `owner::android`
    //! reads from JNI.

    #![forbid(unsafe_code)]

    use std::num::NonZeroU64;

    use flui_foundation::IdMint;
    use flui_foundation::geometry::{Bounds, Point};
    use flui_platform::keyboard::NamedKey;
    use flui_platform::pointer::PointerId;
    use flui_platform::text_store::{
        Composition, Selection, TextInputPurpose, TextStoreEdit, TextStoreError, TextStoreRead,
        TextStoreStatus, Utf16Offset, Utf16Range,
    };

    use crate::text::ime::{diff, document_text};

    /// What `FluiView` reports of its editor after an input method edit, as
    /// `owner::android`'s `EditorState` carries it across JNI, field for field:
    /// the text it holds and the raw offsets of `Selection` and the composing
    /// span (`-1` for none), as Java counts them, in UTF-16 code units.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct EditorReport {
        /// The view's text.
        pub text: String,
        /// `Selection.getSelectionStart`: the selection's anchor.
        pub selection_start: i32,
        /// `Selection.getSelectionEnd`: the selection's moving end.
        pub selection_end: i32,
        /// `BaseInputConnection.getComposingSpanStart`.
        pub composition_start: i32,
        /// `BaseInputConnection.getComposingSpanEnd`.
        pub composition_end: i32,
    }

    /// One publication of the focused store to the view, minted by the
    /// owner's [`Ledger`] in the order it publishes. A later publication
    /// compares greater, and none is ever minted twice.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub struct Publication(NonZeroU64);

    /// One report of the view's editor, minted by the [`Mirror`] on the Java
    /// main thread in the order the view reports. A later report compares
    /// greater, and none is ever minted twice.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub struct ReportId(NonZeroU64);

    /// Why the owner publishes the focused store, which decides how the view
    /// takes the snapshot ([`Mirror::install`]).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Change {
        /// A field took focus, or its status changed: the view becomes its
        /// editor and the input method starts on it.
        Focus,
        /// The view no longer mirrors the store (the store refused a step):
        /// the input method starts over on the store's text.
        Resync,
        /// The owner changed the text: the application's edit, or a
        /// composition the owner committed.
        Text,
        /// Only the selection or the composition moved.
        Selection,
    }

    impl Change {
        /// Whether the snapshot replaces the text the input method edits, so
        /// an edit made before it was made on text the store no longer holds.
        const fn replaces_text(self) -> bool {
            !matches!(self, Self::Selection)
        }
    }

    /// The focused store as the owner published it to the view.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Snapshot {
        /// Which publication this is; every edit names the one it was made on.
        pub publication: Publication,
        /// Why it was published.
        pub change: Change,
        /// The last of the view's reports the owner had replayed or dropped
        /// when it read the store; `None` before the first.
        pub seen: Option<ReportId>,
        /// The text, or [`PROTECTED_UNIT`](crate::text::ime::PROTECTED_UNIT)s for a
        /// protected field.
        pub text: String,
        /// The store's selection.
        pub selection: Selection,
        /// The store's composition.
        pub composition: Option<Utf16Range>,
    }

    impl Snapshot {
        /// The text of the composition, in UTF-16 code units; empty with none.
        fn composed_units(&self) -> impl Iterator<Item = u16> + '_ {
            let (start, len) = self
                .composition
                .map_or((0, 0), |range| (range.start().get(), range.len()));
            self.text.encode_utf16().skip(start).take(len)
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
        /// The publication the view held when it made the edit.
        pub base: Publication,
        /// Which report of the view this is.
        pub report: ReportId,
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

    /// What [`Ledger::replay`] did with an edit.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Replayed {
        /// The store took it.
        Applied,
        /// It was made before a publication that replaced the text, on text
        /// the store no longer holds: dropped. The view took that publication
        /// after the edit and restarted the input method on it
        /// ([`Mirror::install`]), so nothing is left to correct.
        Stale,
        /// The store refused a step: the view no longer mirrors the store, and
        /// the owner publishes it again ([`Change::Resync`]).
        Refused,
    }

    impl Replayed {
        /// Whether the owner must restart the view on a fresh snapshot of the
        /// store: only after a refused step, which no publication in flight
        /// corrects.
        #[must_use]
        pub const fn restarts_the_view(self) -> bool {
            matches!(self, Self::Refused)
        }
    }

    /// The owner's half: the publications it made and the view's reports it
    /// replayed. One per [`Mirror`], so per view, whichever window's field
    /// the view edits: a publication of one window's field never compares
    /// equal to another's, so an edit made for a field of a window that has
    /// gone is dropped by the next window's focus. Owner thread only; every
    /// method takes `&self` and holds no borrow while the store runs, since
    /// the store is application code.
    #[derive(Debug, Default)]
    pub struct Ledger {
        publications: IdMint<NonZeroU64>,
        /// The last publication that replaced the view's text.
        text_base: std::cell::Cell<Option<Publication>>,
        /// The last report replayed or dropped.
        seen: std::cell::Cell<Option<ReportId>>,
    }

    impl Ledger {
        /// The store as `session` reads it, published for `change`.
        #[must_use]
        pub fn publish(&self, session: &dyn TextStoreRead, change: Change) -> Snapshot {
            let publication = Publication(
                self.publications
                    .next()
                    .expect("BUG: a window publishes fewer than 2^64 snapshots"),
            );
            if change.replaces_text() {
                self.text_base.set(Some(publication));
            }
            Snapshot {
                publication,
                change,
                seen: self.seen.get(),
                text: document_text(session),
                selection: session.selection(),
                composition: session.composition().map(|composition| composition.range),
            }
        }

        /// Replay `edit` into `session`, unless a publication that replaced the
        /// text came after the one it was made on. Edits arrive in the order
        /// the view reported them.
        pub fn replay(&self, edit: &ImeEdit, session: &mut dyn TextStoreEdit) -> Replayed {
            self.seen.set(self.seen.get().max(Some(edit.report)));
            if Some(edit.base) < self.text_base.get() {
                return Replayed::Stale;
            }
            match edit.replay(session) {
                Ok(()) => Replayed::Applied,
                Err(_) => Replayed::Refused,
            }
        }
    }

    /// How the view takes a snapshot ([`Mirror::install`]).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Install {
        /// The view's editor becomes the snapshot; the input method starts
        /// over when `restart`, and is told the new selection otherwise.
        Replace {
            /// Whether the input method starts over on the snapshot.
            restart: bool,
        },
        /// The snapshot only moved the selection and missed edits the view
        /// had already reported: the view keeps the input method's text and
        /// selection, which the owner replays on top of the store.
        Keep,
    }

    /// The Java-thread half: what the view holds of the focused field, which
    /// each report is diffed against.
    #[derive(Debug, Default)]
    pub struct Mirror {
        /// The publication the view holds; `None` before the first.
        publication: Option<Publication>,
        /// The view's text, in UTF-16 code units.
        units: Vec<u16>,
        /// The view's composing span.
        composition: Option<Utf16Range>,
        reports: IdMint<NonZeroU64>,
        /// The last report made on the text the view holds since it last
        /// took a snapshot's text: a snapshot that has not seen it raced it.
        /// Reports made before that are on text the view no longer holds.
        unconfirmed: Option<ReportId>,
    }

    impl Mirror {
        /// The view takes `snapshot`; how it takes it (see the module doc).
        pub fn install(&mut self, snapshot: &Snapshot) -> Install {
            let raced = self.unconfirmed > snapshot.seen;
            let install = match snapshot.change {
                Change::Focus | Change::Resync => Install::Replace { restart: true },
                Change::Selection if raced => Install::Keep,
                Change::Text if raced => Install::Replace { restart: true },
                Change::Text | Change::Selection => Install::Replace {
                    restart: self.composition_touched(snapshot),
                },
            };
            self.publication = Some(snapshot.publication);
            if matches!(install, Install::Replace { .. }) {
                self.units = snapshot.text.encode_utf16().collect();
                self.composition = snapshot.composition;
                self.unconfirmed = None;
            }
            install
        }

        /// Whether `snapshot` touches the composition the input method holds:
        /// ends it or changes its text. A composition the snapshot only moves
        /// (text inserted before it) is told with its new bounds, as a text
        /// change outside it is; with no composition the input method has
        /// nothing a new text could break.
        fn composition_touched(&self, snapshot: &Snapshot) -> bool {
            let Some(held) = self.composition else {
                return false;
            };
            if snapshot.composition.is_none() {
                return true;
            }
            let held_units = self
                .units
                .get(held.start().get()..held.end().get())
                .unwrap_or_default();
            !held_units.iter().copied().eq(snapshot.composed_units())
        }

        /// The edit that turned the mirror into `state`; `None` before the view
        /// holds a publication, when there is nothing to diff against.
        ///
        /// Offsets are clamped into the new text and moved off the middle of a
        /// surrogate pair, which a store refuses.
        pub fn report(&mut self, state: &EditorReport) -> Option<ImeEdit> {
            let base = self.publication?;
            let units: Vec<u16> = state.text.encode_utf16().collect();
            let replaced = diff(&self.units, &units).map(|change| {
                let range = Utf16Range::try_new(
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
                    Utf16Range::try_new(start, end).expect("BUG: ordered offsets make a range")
                })
                .filter(|range| !range.is_empty());
            let report = ReportId(
                self.reports
                    .next()
                    .expect("BUG: a view reports fewer than 2^64 edits"),
            );
            self.unconfirmed = Some(report);
            self.units = units;
            self.composition = composition;
            Some(ImeEdit {
                base,
                report,
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

    /// `CursorAnchorInfo.FLAG_HAS_VISIBLE_REGION`.
    pub const FLAG_HAS_VISIBLE_REGION: i32 = 0x01;
    /// `CursorAnchorInfo.FLAG_HAS_INVISIBLE_REGION`.
    pub const FLAG_HAS_INVISIBLE_REGION: i32 = 0x02;

    /// The most composing code units whose boxes go to the input method in
    /// one update: a composition is a word or a phrase, and a longer one has
    /// its first boxes sent rather than an update the size of the text.
    pub const MAX_CHARACTER_BOUNDS: usize = 256;

    /// The boxes of the composing text's characters for
    /// `CursorAnchorInfo.Builder.addCharacterBounds`, in the view's physical
    /// pixels.
    #[derive(Debug, Clone, PartialEq)]
    pub struct CharacterBounds {
        /// The composing text the boxes were measured on: the view attaches
        /// them only while it composes this same text at [`Self::start`], so
        /// an input method edit that arrived after the measure never gets
        /// another text's boxes.
        pub text: String,
        /// The UTF-16 index of the first box, where the composition starts.
        pub start: i32,
        /// Left, top, right and bottom of each code unit's box, in order.
        pub rects: Vec<f32>,
        /// `FLAG_HAS_VISIBLE_REGION`, with `FLAG_HAS_INVISIBLE_REGION` for a box
        /// part of which lies outside the field's visible area, for each code
        /// unit. The store says only that part is outside, so a clipped box
        /// reads as partly visible.
        pub flags: Vec<i32>,
    }

    /// The boxes of the composition `session` holds, one per UTF-16 code unit
    /// as Android indexes them: both halves of a surrogate pair take the
    /// pair's box. `None` with no composition, or when the field has no layout
    /// for it, a box would not be finite, or the store is protected (its text
    /// cannot be read, so the boxes could not be matched to it).
    #[must_use]
    pub fn character_bounds(
        session: &(impl TextStoreRead + ?Sized),
        scale: f64,
    ) -> Option<CharacterBounds> {
        let composition = session.composition()?.range;
        let start = composition.start().get();
        let end = composition
            .end()
            .get()
            .min(start.saturating_add(MAX_CHARACTER_BOUNDS));
        let pixel = |value: f64| {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "CursorAnchorInfo takes floats; finite pixels fit"
            )]
            let narrowed = (value * scale) as f32;
            narrowed.is_finite().then_some(narrowed)
        };
        let mut bounds = CharacterBounds {
            text: session.text(composition).ok()?,
            start: i32::try_from(start).ok()?,
            rects: Vec::with_capacity((end - start) * 4),
            flags: Vec::with_capacity(end - start),
        };
        let mut unit = start;
        while unit < end {
            let box_of = |units: usize| {
                let range =
                    Utf16Range::try_new(Utf16Offset::new(unit), Utf16Offset::new(unit + units))?;
                session.rect_for_range(range).ok()
            };
            // One unit is a whole character unless it opens a surrogate pair,
            // whose first half alone is no boundary.
            let (units, rect) = match box_of(1) {
                Some(rect) => (1, rect),
                None => (2, box_of(2)?),
            };
            let area = rect.bounds;
            let edges = [
                pixel(area.origin.x)?,
                pixel(area.origin.y)?,
                pixel(area.origin.x + area.size.width)?,
                pixel(area.origin.y + area.size.height)?,
            ];
            let flag = if rect.clipped {
                FLAG_HAS_VISIBLE_REGION | FLAG_HAS_INVISIBLE_REGION
            } else {
                FLAG_HAS_VISIBLE_REGION
            };
            for _ in 0..units.min(end - unit) {
                bounds.rects.extend_from_slice(&edges);
                bounds.flags.push(flag);
            }
            unit += units;
        }
        Some(bounds)
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
        pen_tip_down && field_contains(field, point)
    }

    /// Whether `point` lies inside the focused field's bounds, edges included;
    /// `false` with no field laid out.
    #[must_use]
    pub fn field_contains(field: Option<Bounds<f64>>, point: Point<f64>) -> bool {
        field.is_some_and(|field| {
            point.x >= field.origin.x
                && point.y >= field.origin.y
                && point.x <= field.origin.x + field.size.width
                && point.y <= field.origin.y + field.size.height
        })
    }

    /// Whether a tap shows the soft keyboard over the focused field, as a tap
    /// on Android's own `EditText` does: a contact that went down and came up
    /// inside the field, so a keyboard the user put away with Back returns on
    /// the next tap. A contact that went down elsewhere, or was cancelled,
    /// shows nothing.
    #[derive(Debug, Default)]
    pub struct KeyboardTap {
        /// The contact that went down inside the field and is still down.
        pressed: Option<PointerId>,
    }

    impl KeyboardTap {
        /// `pointer` went down, inside the focused field or not.
        pub fn press(&mut self, pointer: PointerId, inside: bool) {
            if inside {
                self.pressed = Some(pointer);
            } else if self.pressed == Some(pointer) {
                self.pressed = None;
            }
        }

        /// Whether `pointer` went down inside the field and is still down.
        #[must_use]
        pub fn holds(&self, pointer: PointerId) -> bool {
            self.pressed == Some(pointer)
        }

        /// `pointer` came up, inside the focused field or not: whether the
        /// keyboard shows.
        pub fn release(&mut self, pointer: PointerId, inside: bool) -> bool {
            if self.pressed != Some(pointer) {
                return false;
            }
            self.pressed = None;
            inside
        }

        /// `pointer`'s contact ended without coming up.
        pub fn cancel(&mut self, pointer: PointerId) {
            if self.pressed == Some(pointer) {
                self.pressed = None;
            }
        }
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
    //! store, which becomes the mirror, and shows the soft keyboard; so does a
    //! tap on the focused field ([`KeyboardTap`]). Each edit the view reports is
    //! replayed into the store under an asynchronous lock, in order, now if the
    //! store is free and else when its frame transaction lets it. When the field
    //! changes outside the input method's edits (the application's own edit, a
    //! moved caret) the store's observer publishes a new snapshot, which the
    //! view takes on the Java main thread by `text::android::ime`'s rule: the
    //! input method restarts only when its composition was touched or the
    //! snapshot raced its edits.
    //!
    //! A request the presentation makes from inside a store call the host started
    //! waits and runs, in order, when that call returns.

    use std::cell::{Cell, RefCell};
    use std::rc::{Rc, Weak};
    use std::sync::Arc;

    use flui_foundation::geometry::Bounds;
    use flui_platform::pointer::{PenTool, PointerEvent, PointerKind};
    use flui_platform::text_store::{
        CompositionEnd, LockGrant, LockOutcome, LockTiming, TextChange, TextStore, TextStoreEdit,
        TextStoreHost, TextStoreHostError, TextStoreObserver, TextStoreRead,
    };
    use jni::{JValue, jni_sig, jni_str};

    use super::ime::{
        Change, CharacterBounds, EditorReport, ImeEdit, Install, KeyboardTap, Ledger, Mirror,
        Snapshot, character_bounds, cursor_anchor, editor_action_key, editor_info, field_contains,
        starts_handwriting,
    };
    use crate::owner::android::EditorState;
    use crate::owner::panic_boundary::contain_owner_callback;
    use crate::owner::signal::OwnerSignal;
    use crate::owner::slot::OwnerSlot;
    use crate::text::host_queue::{HostQueue, MirrorHostOp};
    use crate::text::{TextInput, WindowTextInput};
    use crate::version::gate::Since;
    use crate::version::gate::android::{AndroidApi, StylusHandwriting};

    use super::sizing::Acquisition;
    use crate::owner::android::java_view::{InputMethod, InputMethodReply, JavaView};
    use crate::owner::android::{ActivityRef, ActivityWindow, ActivityWindowId};

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

    /// `InputConnection.CURSOR_UPDATE_IMMEDIATE`: send the cursor once, now.
    const CURSOR_UPDATE_IMMEDIATE: i32 = 1;
    /// `InputConnection.CURSOR_UPDATE_MONITOR`: send it whenever it changes.
    const CURSOR_UPDATE_MONITOR: i32 = 2;

    /// The field of one window: what the input method edits while the window
    /// is open, kept on the owner thread.
    struct Field {
        window: ActivityWindowId,
        input: OwnerSlot<Rc<AndroidTextInput>>,
    }

    /// The fields of the windows the loop opened, by window identity.
    #[derive(Default)]
    struct Fields(parking_lot::Mutex<Vec<Field>>);

    impl Fields {
        /// The field of `window`, held for no lock: running it runs the
        /// field's store, which is user code. `None` off the owner thread and
        /// for a window this does not hold.
        fn of(&self, window: ActivityWindowId) -> Option<Rc<AndroidTextInput>> {
            let fields = self.0.lock();
            let field = fields.iter().find(|field| field.window == window)?;
            let input = field.input.take()?;
            let shared = Rc::clone(&input);
            let _ = field.input.restore(input);
            Some(shared)
        }
    }

    /// The text position of the Android table: what answers the input
    /// method's calls through `FluiView` — the text the view holds of the
    /// focused field, as the view last took the owner's snapshot or the input
    /// method last edited it, against which each report becomes one edit of
    /// the field, touched only on the Java main thread —
    /// and the field of each window, keyed by the window's identity, which
    /// the work it hands the owner reaches. Public where it is defined, as the
    /// host's table names it, and nameable only inside the crate.
    pub struct ViewInputMethod {
        mirror: parking_lot::Mutex<Mirror>,
        /// The owner's half of the mirror, which every window's field
        /// publishes through and replays against ([`Ledger`]); made with the
        /// first window, on the owner thread that made this.
        ledger: OwnerSlot<Rc<Ledger>>,
        fields: Arc<Fields>,
    }

    /// Made by the loop, on the owner thread, with the view.
    impl Default for ViewInputMethod {
        fn default() -> Self {
            Self {
                mirror: parking_lot::Mutex::default(),
                ledger: OwnerSlot::new(),
                fields: Arc::default(),
            }
        }
    }

    impl ViewInputMethod {
        /// The view's ledger, made on the first call; `None` off the owner
        /// thread.
        fn ledger(&self) -> Option<Rc<Ledger>> {
            if let Some(ledger) = self.ledger.cloned() {
                return Some(ledger);
            }
            let ledger = Rc::new(Ledger::default());
            self.ledger.replace(Rc::clone(&ledger)).ok()?;
            Some(ledger)
        }
    }

    impl std::fmt::Debug for ViewInputMethod {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("ViewInputMethod").finish_non_exhaustive()
        }
    }

    impl InputMethod for ViewInputMethod {
        fn install(&self, snapshot: &Snapshot) -> Install {
            self.mirror.lock().install(snapshot)
        }

        fn edited(&self, state: EditorState) -> Option<InputMethodReply> {
            let EditorState {
                text,
                selection_start,
                selection_end,
                composition_start,
                composition_end,
            } = state;
            let report = EditorReport {
                text,
                selection_start,
                selection_end,
                composition_start,
                composition_end,
            };
            let edit = self.mirror.lock().report(&report)?;
            let fields = Arc::clone(&self.fields);
            Some(InputMethodReply {
                composing: Some(edit.composes()),
                work: Box::new(move |window: &dyn ActivityWindow| {
                    if let Some(input) = fields.of(window.activity_id()) {
                        input.apply(edit);
                    }
                }),
            })
        }

        fn editor_action(&self, action: i32, uptime_ms: i64) -> InputMethodReply {
            let fields = Arc::clone(&self.fields);
            InputMethodReply {
                composing: None,
                work: Box::new(move |window: &dyn ActivityWindow| {
                    if let Some(input) = fields.of(window.activity_id()) {
                        input.editor_action();
                    }
                    // The action arrives as the key it stands for, which a field
                    // answers as it answers a keyboard's.
                    let (key, shift) = editor_action_key(action);
                    for key_input in editor_action_keys(key, shift, uptime_ms) {
                        window.delivery().dispatch_input(key_input);
                    }
                }),
            }
        }

        fn cursor_updates(&self, mode: i32) -> InputMethodReply {
            let fields = Arc::clone(&self.fields);
            InputMethodReply {
                composing: None,
                work: Box::new(move |window: &dyn ActivityWindow| {
                    if let Some(input) = fields.of(window.activity_id()) {
                        input.cursor_updates(mode);
                    }
                }),
            }
        }

        fn pointer(&self, window: &dyn ActivityWindow, event: &PointerEvent) {
            if let Some(input) = self.fields.of(window.activity_id()) {
                input.pointer(event);
            }
        }

        fn window_opened(&self, window: ActivityWindowId, view: &Arc<JavaView>, api: AndroidApi) {
            let input = OwnerSlot::new();
            let made = self.ledger().is_some_and(|ledger| {
                input
                    .replace(AndroidTextInput::new(Arc::clone(view), api, ledger))
                    .is_ok()
            });
            if !made {
                tracing::warn!(
                    target: "flui_native::android",
                    "a window's field was made off its owner thread"
                );
                return;
            }
            let mut fields = self.fields.0.lock();
            debug_assert!(
                fields.iter().all(|field| field.window != window),
                "BUG: a window identity is opened once"
            );
            fields.push(Field { window, input });
        }

        fn window_closed(&self, window: ActivityWindowId) {
            let field = {
                let mut fields = self.fields.0.lock();
                fields
                    .iter()
                    .position(|field| field.window == window)
                    .map(|index| fields.swap_remove(index))
            };
            // Detached and dropped with no lock held: the field's store is
            // user code.
            if let Some(field) = field
                && let Some(input) = field.input.take()
            {
                input.retire();
            }
        }

        fn store_host(&self, window: ActivityWindowId) -> Option<Rc<dyn TextStoreHost>> {
            self.fields
                .of(window)
                .map(|input| input as Rc<dyn TextStoreHost>)
        }
    }

    /// What one window holds of the text feature: its identity, by which the
    /// input method keeps its field, and the capture of its system text size.
    struct ActivityText {
        window: ActivityWindowId,
        view: Arc<JavaView>,
        sizing: Arc<Acquisition>,
        app: android_activity::AndroidApp,
        signal: std::sync::Weak<OwnerSignal>,
        owner: std::thread::ThreadId,
    }

    /// Each window of the Activity gets its field, which the input method
    /// edits through `FluiView`, and the capture of its text size.
    impl crate::host::Text<ActivityRef> for ViewInputMethod {
        fn for_window(window: &ActivityRef) -> Arc<dyn TextInput> {
            window
                .view
                .input_method()
                .window_opened(window.id, &window.view, window.api);
            Arc::new(ActivityText {
                window: window.id,
                view: Arc::clone(&window.view),
                sizing: Acquisition::new(),
                app: window.app.clone(),
                signal: window.signal.clone(),
                owner: window.owner,
            })
        }
    }

    impl TextInput for ActivityText {
        /// The window's field, which serves the focused field to the input
        /// method; on the owner thread only.
        fn window_text_input(&self) -> Option<WindowTextInput> {
            self.view
                .input_method()
                .store_host(self.window)
                .map(WindowTextInput::Store)
        }

        fn capture_sizing(
            &self,
        ) -> Result<crate::TextSizingCaptureState, crate::TextSizingCaptureError> {
            self.sizing.poll(&self.app, &self.signal, self.owner)
        }

        fn invalidate_sizing(&self) {
            self.sizing.invalidate();
            if let Some(signal) = self.signal.upgrade() {
                let _ = signal.wake();
            }
        }

        fn fence_sizing(&self) {
            self.sizing.fence();
        }

        /// The window is going away: its field is detached for good.
        fn detach(&self) {
            self.view.input_method().window_closed(self.window);
        }
    }

    /// The window's text input (see the module doc).
    pub struct AndroidTextInput {
        view: Arc<JavaView>,
        /// Whether the input method can start stylus handwriting on the view.
        handwriting: Option<Since<StylusHandwriting>>,
        focused: RefCell<Option<Rc<dyn TextStore>>>,
        /// The view's publications and the reports replayed, which decide
        /// whether an edit still applies; shared with every window's field
        /// over the view.
        ledger: Rc<Ledger>,
        /// The contact that may be a tap on the focused field.
        tap: RefCell<KeyboardTap>,
        /// What `requestCursorUpdates` last asked for.
        cursor_mode: Cell<i32>,
        queue: HostQueue<MirrorHostOp>,
        retired: Cell<bool>,
        this: Weak<Self>,
    }

    impl std::fmt::Debug for AndroidTextInput {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("AndroidTextInput").finish_non_exhaustive()
        }
    }

    /// Publish `snapshot` to the view. On the Java main thread, in the call
    /// that hands it to the view and so ordered with the view's reports, the
    /// view's mirror installs it ([`Mirror::install`]) and the view takes it
    /// as the install says: as a new editor for a focus, by replacing its
    /// text with or without a restart, or not at all when it keeps the input
    /// method's newer edits. A view that is not installed takes neither.
    fn publish(view: &Arc<JavaView>, snapshot: Snapshot, status: (i32, i32, bool)) {
        let bridge = Arc::clone(view);
        view.post(move |env, view| {
            // Made before the mirror changes, so a refused allocation leaves
            // the mirror and the view's editor holding the same text.
            let text = env.new_string(&snapshot.text)?;
            let Install::Replace { restart } = bridge.input_method().install(&snapshot) else {
                return Ok(());
            };
            bridge.set_composing(snapshot.composition.is_some());
            let [
                selection_start,
                selection_end,
                composition_start,
                composition_end,
            ] = snapshot.java_offsets();
            if snapshot.change == Change::Focus {
                let (input_type, options, stylus) = status;
                env.call_method(
                    view,
                    jni_str!("focusEditor"),
                    jni_sig!("(IIZLjava/lang/String;IIII)V"),
                    &[
                        JValue::Int(input_type),
                        JValue::Int(options),
                        JValue::Bool(stylus),
                        JValue::Object(&text),
                        JValue::Int(selection_start),
                        JValue::Int(selection_end),
                        JValue::Int(composition_start),
                        JValue::Int(composition_end),
                    ],
                )?;
            } else {
                env.call_method(
                    view,
                    jni_str!("pushState"),
                    jni_sig!("(Ljava/lang/String;IIIIZ)V"),
                    &[
                        JValue::Object(&text),
                        JValue::Int(selection_start),
                        JValue::Int(selection_end),
                        JValue::Int(composition_start),
                        JValue::Int(composition_end),
                        JValue::Bool(restart),
                    ],
                )?;
            }
            Ok(())
        });
    }

    impl AndroidTextInput {
        pub(crate) fn new(view: Arc<JavaView>, api: AndroidApi, ledger: Rc<Ledger>) -> Rc<Self> {
            Rc::new_cyclic(|this| Self {
                view,
                handwriting: api.since::<StylusHandwriting>(),
                focused: RefCell::new(None),
                ledger,
                tap: RefCell::new(KeyboardTap::default()),
                cursor_mode: Cell::new(0),
                queue: HostQueue::new(),
                retired: Cell::new(false),
                this: this.clone(),
            })
        }

        /// The window closed: detach the focused field, if any, for good.
        pub(crate) fn retire(&self) {
            self.retired.set(true);
            self.queue.clear();
            self.detach();
        }

        fn focused(&self) -> Option<Rc<dyn TextStore>> {
            self.focused.borrow().clone()
        }

        fn status(&self, store: &dyn TextStore) -> (i32, i32, bool) {
            let (input_type, options) = editor_info(store.status());
            let stylus = self.handwriting.is_some();
            (input_type, options, stylus)
        }

        /// Run the requests queued while the host was inside a store call.
        fn drain(&self) {
            while let Some(op) = self.queue.next() {
                self.run(op);
            }
        }

        fn run(&self, op: MirrorHostOp) {
            match op {
                MirrorHostOp::Focus(store) => self.refocus(store),
                MirrorHostOp::Complete(store) => {
                    let focused = self
                        .focused()
                        .is_some_and(|focused| Rc::ptr_eq(&focused, &store));
                    if !focused || self.end_composition(&store) == CompositionEnd::Abandoned {
                        crate::text::refusal::commit_in_place(&*store);
                    }
                }
            }
        }

        /// One input method edit the view reported: replayed into the focused
        /// store in order, or dropped when a publication that replaced the
        /// text came after the one it was made on ([`Ledger::replay`]).
        fn apply(&self, edit: ImeEdit) {
            let Some(store) = self.focused() else {
                return;
            };
            let ledger = Rc::clone(&self.ledger);
            let view = Arc::clone(&self.view);
            let status = self.status(&*store);
            let this = self.this.clone();
            let edited = Rc::clone(&store);
            let grant = LockGrant::read_write(move |session| {
                let outcome = ledger.replay(&edit, session);
                // Focus may have moved to another field while the grant waited: its
                // snapshot is the one the view mirrors now, and a restart on this
                // store would overwrite it.
                let still_focused = this
                    .upgrade()
                    .and_then(|input| input.focused())
                    .is_some_and(|current| Rc::ptr_eq(&current, &edited));
                if outcome.restarts_the_view() && still_focused {
                    // The store refused a step: the input method's text no longer
                    // matches the store's.
                    publish(&view, ledger.publish(&*session, Change::Resync), status);
                }
            });
            {
                let _entered = self.queue.enter();
                if let Err(error) = store.request_lock(grant, LockTiming::Async) {
                    tracing::debug!(?error, "the focused store refused the input method's edit");
                    self.publish_now(Change::Resync);
                }
            }
            self.drain();
        }

        /// Publish the focused store now, or when its frame transaction lets it.
        fn publish_now(&self, change: Change) {
            let Some(store) = self.focused() else {
                return;
            };
            let ledger = Rc::clone(&self.ledger);
            let view = Arc::clone(&self.view);
            let status = self.status(&*store);
            let grant = move |session: &dyn TextStoreRead| {
                publish(&view, ledger.publish(session, change), status);
            };
            let _entered = self.queue.enter();
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
            self.publish_now(Change::Focus);
        }

        /// End the input method's composition in `store`: commit it in the store
        /// and publish the result, which ends the input method's own (the view
        /// restarts it when it was composing).
        fn end_composition(&self, store: &Rc<dyn TextStore>) -> CompositionEnd {
            let ledger = Rc::clone(&self.ledger);
            let view = Arc::clone(&self.view);
            let status = self.status(&**store);
            let grant = LockGrant::read_write(move |session: &mut dyn TextStoreEdit| {
                if session.composition().is_some() {
                    let _ = session.set_composition(None);
                }
                view.set_composing(false);
                publish(&view, ledger.publish(&*session, Change::Text), status);
            });
            let outcome = {
                let _entered = self.queue.enter();
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
            let ledger = Rc::clone(&self.ledger);
            let view = Arc::clone(&self.view);
            let status = self.status(&*store);
            let grant = LockGrant::read_write(move |session: &mut dyn TextStoreEdit| {
                if session.composition().is_none() {
                    return;
                }
                let _ = session.set_composition(None);
                view.set_composing(false);
                publish(&view, ledger.publish(&*session, Change::Text), status);
            });
            {
                let _entered = self.queue.enter();
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

        /// Tell the input method where the caret, the field and the composing
        /// text's characters are.
        fn push_cursor(&self) {
            let Some(store) = self.focused() else {
                return;
            };
            type Found = Option<([f32; 7], Option<CharacterBounds>)>;
            let found: Rc<RefCell<Found>> = Rc::default();
            let out = Rc::clone(&found);
            let scale = self.view.scale().get();
            let grant = LockGrant::read(move |session| {
                let caret = session.rect_for_range(session.selection().range());
                if let Ok(caret) = caret {
                    let anchor = cursor_anchor(caret.bounds, session.document_bounds().ok(), scale);
                    *out.borrow_mut() =
                        anchor.map(|anchor| (anchor, character_bounds(session, scale)));
                }
            });
            {
                let _entered = self.queue.enter();
                if store.request_lock(grant, LockTiming::Sync).is_err() {
                    return;
                }
            }
            let Some((anchor, characters)) = found.take() else {
                return;
            };
            self.view.post(move |env, view| {
                let (text, start, rects, flags) = characters
                    .map_or((String::new(), -1, Vec::new(), Vec::new()), |bounds| {
                        (bounds.text, bounds.start, bounds.rects, bounds.flags)
                    });
                let text = env.new_string(text)?;
                let rect_array = env.new_float_array(rects.len())?;
                rect_array.set_region(env, 0, &rects)?;
                let flag_array = env.new_int_array(flags.len())?;
                flag_array.set_region(env, 0, &flags)?;
                let mut arguments = anchor.map(JValue::Float).to_vec();
                arguments.extend([
                    JValue::Object(&text),
                    JValue::Int(start),
                    JValue::Object(&rect_array),
                    JValue::Object(&flag_array),
                ]);
                env.call_method(
                    view,
                    jni_str!("updateCursorAnchor"),
                    jni_sig!("(FFFFFFFLjava/lang/String;I[F[I)V"),
                    &arguments,
                )
                .map(drop)
            });
        }

        /// A contact of the window's pointer input, before the window takes
        /// it: a pen tip that touches down inside the focused field is handed
        /// to the input method's handwriting where the device has it, and a
        /// tap on the focused field shows the soft keyboard ([`KeyboardTap`]).
        /// With no field focused, nothing is read.
        fn pointer(&self, event: &PointerEvent) {
            if self.focused.borrow().is_none() {
                return;
            }
            match event {
                PointerEvent::Down(press) => {
                    let at = press.sample.position.get();
                    let pen_tip = press.pointer.kind == (PointerKind::Pen { tool: PenTool::Tip });
                    let field = self.field_bounds();
                    if let Some(handwriting) = self.handwriting
                        && starts_handwriting(pen_tip, field, at)
                    {
                        self.start_handwriting(handwriting);
                    }
                    let inside = field_contains(field, at);
                    self.tap.borrow_mut().press(press.pointer.id, inside);
                }
                PointerEvent::Up(release) => {
                    let pointer = release.pointer.id;
                    if !self.tap.borrow().holds(pointer) {
                        return;
                    }
                    // Read with no borrow held: the read runs the store.
                    let inside = field_contains(self.field_bounds(), release.sample.position.get());
                    if self.tap.borrow_mut().release(pointer, inside) {
                        self.show_keyboard();
                    }
                }
                PointerEvent::Cancel(cancel) => self.tap.borrow_mut().cancel(cancel.pointer.id),
                _ => {}
            }
        }

        /// Show the soft keyboard over the focused field again, as a tap on
        /// it asks; the view does nothing when it edits no field.
        fn show_keyboard(&self) {
            self.view.post(|env, view| {
                env.call_method(view, jni_str!("showKeyboard"), jni_sig!("()V"), &[])
                    .map(drop)
            });
        }

        /// The focused field's bounds, for a press that may start handwriting
        /// or show the keyboard; `None` when no field is focused or laid out.
        fn field_bounds(&self) -> Option<Bounds<f64>> {
            let store = self.focused()?;
            let found: Rc<Cell<Option<Bounds<f64>>>> = Rc::default();
            let out = Rc::clone(&found);
            let grant = LockGrant::read(move |session| out.set(session.document_bounds().ok()));
            let _entered = self.queue.enter();
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
            if let Some(op) = self.queue.defer(MirrorHostOp::Focus(store)) {
                self.run(op);
                self.drain();
            }
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
            let end = self.end_composition(store);
            self.drain();
            Ok(end)
        }
    }

    /// What the store tells the view when the application changes the field.
    struct MirrorObserver {
        input: Weak<AndroidTextInput>,
    }

    impl TextStoreObserver for MirrorObserver {
        fn text_changed(&self, _change: TextChange) {
            if let Some(input) = self.input.upgrade() {
                input.publish_now(Change::Text);
            }
        }

        // Composition-only publications also send this notice. The snapshot
        // carries both the selection and composing bounds to updateSelection.
        fn selection_changed(&self) {
            if let Some(input) = self.input.upgrade() {
                input.publish_now(Change::Selection);
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
                input.publish_now(Change::Focus);
            }
        }
    }
}

#[cfg(any(target_os = "android", test))]
pub(crate) mod sizing {
    //! Java-main acquisition; owner-lane queries use only a private metrics copy.

    #[cfg(target_os = "android")]
    use crate::owner::panic_boundary::{contain_os_entry, contain_owner_callback};
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
            flui_foundation::TextSize::try_new(f64::from(physical) / self.density)
                .map_err(Into::into)
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
            // Dropping an unposted callback settles its owned receipt. If posting
            // succeeded before a panic, its queued callback still owns the ticket.
            contain_os_entry("android text sizing post", || {
                crate::owner::android::main_thread::run(
                    app,
                    "android text sizing capture",
                    move || {
                        let result =
                            contain_os_entry("android text sizing capture", || capture(&activity))
                                .unwrap_or_else(|| {
                                    Err(Error::Native {
                                        message: "Android text sizing capture panicked".into(),
                                    })
                                })
                                .map(|snapshot| Arc::new(snapshot) as Native);
                        contain_owner_callback(|| {
                            receipt.complete(result);
                        });
                    },
                );
            });
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
pub(crate) use input::ViewInputMethod;
