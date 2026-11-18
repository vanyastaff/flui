//! An input method's sessions on a text store: the locks it held and each call within them.

/// How many calls one [`StoreLock`] row holds; a session that makes more is recorded
/// [`incomplete`](StoreLock::incomplete).
pub const MAX_STORE_STEPS: usize = 256;

/// How many bytes of text one [`StoreLock`] row holds, its opened document and every call's
/// text together; a session that reads or writes more is recorded
/// [`incomplete`](StoreLock::incomplete).
pub const MAX_STORE_TEXT_BYTES: usize = 64 * 1024;

vocabulary! {
    /// The kind of lock an input method was granted on a text store.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum StoreLockKind {
        /// Reads only.
        Read => "read",
        /// Reads and edits.
        ReadWrite => "read_write",
    }
}

vocabulary! {
    /// Why a text store refused a call, by kind.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum StoreError {
        /// A synchronous lock was unavailable.
        SyncLockUnavailable => "sync_lock_unavailable",
        /// The deferred-lock queue was full.
        DeferredQueueFull => "deferred_queue_full",
        /// An offset or range named no position in the document.
        Offset => "offset",
        /// The text must not be read out.
        Protected => "protected",
        /// The field had no layout for its text.
        NoLayout => "no_layout",
        /// The point missed the text.
        PointOutside => "point_outside",
        /// The field was gone.
        Detached => "detached",
    }
}

vocabulary! {
    /// What a text field is for.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum InputPurpose {
        /// Free text.
        #[default]
        Text => "text",
        /// An email address.
        Email => "email",
        /// A web address.
        Url => "url",
        /// A telephone number.
        Phone => "phone",
        /// A number; [`StoreStatus::signed`] and [`StoreStatus::decimal`] say
        /// what it holds beside the digits.
        Number => "number",
        /// A search query.
        Search => "search",
    }
}

vocabulary! {
    /// What a text field's on-screen Enter key says.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum EnterAction {
        /// The keyboard's own label.
        Default => "default",
        /// Done.
        Done => "done",
        /// Go.
        Go => "go",
        /// Next.
        Next => "next",
        /// Previous.
        Previous => "previous",
        /// Search.
        Search => "search",
        /// Send.
        SendMessage => "send_message",
    }
}

vocabulary! {
    /// Which letters a text field's on-screen keyboard shifts on its own.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum Capitalization {
        /// None.
        #[default]
        None => "none",
        /// The first letter of each sentence.
        Sentences => "sentences",
        /// The first letter of each word.
        Words => "words",
        /// Every letter.
        Characters => "characters",
    }
}

vocabulary! {
    /// Whether a text field's input method suggests and corrects words.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum Correction {
        /// Neither.
        Off => "off",
        /// Suggestions and the spell check, no word replaced on its own.
        Suggestions => "suggestions",
        /// Suggestions, and misspelled words replaced as the user types.
        Autocorrect => "autocorrect",
    }
}

vocabulary! {
    /// What the system may fill a text field with, beyond its purpose.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum Autofill {
        /// An account's user name.
        Username => "username",
        /// An account's existing password.
        CurrentPassword => "current_password",
        /// A password being chosen.
        NewPassword => "new_password",
        /// A one-time code.
        OneTimeCode => "one_time_code",
    }
}

/// A text store's standing properties, as the window was told to focus it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct StoreStatus {
    /// Its text must not be read out (a password field).
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "std::ops::Not::not")
    )]
    pub protected: bool,
    /// It holds one line.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "std::ops::Not::not")
    )]
    pub single_line: bool,
    /// What it is for.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "crate::skip::is_default")
    )]
    pub purpose: InputPurpose,
    /// A number purpose's minus sign: the number may be negative. Set only with
    /// [`InputPurpose::Number`].
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "std::ops::Not::not")
    )]
    pub signed: bool,
    /// A number purpose's decimal separator: the number may have a fractional
    /// part. Set only with [`InputPurpose::Number`].
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "std::ops::Not::not")
    )]
    pub decimal: bool,
    /// What its Enter key says; absent, the default its purpose picks (Search
    /// for a search, the keyboard's own otherwise).
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub enter_action: Option<EnterAction>,
    /// Which letters its keyboard shifts on its own.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "crate::skip::is_default")
    )]
    pub capitalization: Capitalization,
    /// Whether its input method suggests and corrects; absent, the default its
    /// purpose picks (prose corrected, an address or a number not).
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub correction: Option<Correction>,
    /// What the system may fill it with.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub autofill: Option<Autofill>,
}

/// The text store a window's input method was told to serve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct StoreFocus {
    /// The store, numbered within the recording each time the window is told to serve a store
    /// it was not serving; never zero, never reused.
    pub store: u64,
    /// Its standing properties.
    pub status: StoreStatus,
}

/// A composition in a text store: its span in UTF-16 code units, and whether the input method
/// hides the caret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct StoreComposition {
    /// Its start and end.
    pub range: [u64; 2],
    /// The input method draws no caret.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "std::ops::Not::not")
    )]
    pub hides_caret: bool,
}

/// A text store's document: its text, its selection as anchor and active end in UTF-16 code
/// units, and its composition.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct StoreDocument {
    /// The text.
    pub text: String,
    /// The selection's anchor and active end.
    pub selection: [u64; 2],
    /// The composition, when one is in progress.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub composition: Option<StoreComposition>,
}

/// One call an input method made inside a lock on a text store, with the store's answer. An
/// offset is in UTF-16 code units; a call the store refused names its error instead of an
/// answer.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(tag = "call", rename_all = "snake_case"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum StoreStep {
    /// It read the document's length.
    DocumentLen {
        /// The length.
        len: u64,
    },
    /// It read the text of `range`: the context of a reconversion or a surrounding-text query.
    Text {
        /// The range read.
        range: [u64; 2],
        /// The text answered.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "Option::is_none")
        )]
        text: Option<String>,
        /// Why the store refused.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "Option::is_none")
        )]
        error: Option<StoreError>,
    },
    /// It read the selection.
    Selection {
        /// The anchor and active end answered.
        selection: [u64; 2],
    },
    /// It read the composition.
    Composition {
        /// The composition answered; absent when none was in progress.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "Option::is_none")
        )]
        composition: Option<StoreComposition>,
    },
    /// It asked which offset lies at a point, in window-root logical pixels. The answer
    /// depends on the device's layout, so a replay keeps it and does not ask again.
    IndexAtPoint {
        /// The point.
        point: [f64; 2],
        /// A point inside a character's box answers its nearer edge.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "std::ops::Not::not")
        )]
        round_to_nearest_edge: bool,
        /// A point outside every box answers the nearest offset.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "std::ops::Not::not")
        )]
        nearest_when_outside: bool,
        /// The offset answered.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "Option::is_none")
        )]
        index: Option<u64>,
        /// Why the store refused.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "Option::is_none")
        )]
        error: Option<StoreError>,
    },
    /// It replaced `range` with `text`.
    Replace {
        /// The range replaced.
        range: [u64; 2],
        /// The text put there.
        text: String,
        /// The edit's start, the replaced span's end and the inserted text's end.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "Option::is_none")
        )]
        change: Option<[u64; 3]>,
        /// Why the store refused.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "Option::is_none")
        )]
        error: Option<StoreError>,
    },
    /// It replaced the selection with `text`.
    InsertAtSelection {
        /// The text put there.
        text: String,
        /// The edit's start, the replaced span's end and the inserted text's end.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "Option::is_none")
        )]
        change: Option<[u64; 3]>,
        /// Why the store refused.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "Option::is_none")
        )]
        error: Option<StoreError>,
    },
    /// It set the selection.
    SetSelection {
        /// The anchor and active end.
        selection: [u64; 2],
        /// Why the store refused.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "Option::is_none")
        )]
        error: Option<StoreError>,
    },
    /// It started, moved or ended the composition.
    SetComposition {
        /// The composition; absent to end it.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "Option::is_none")
        )]
        composition: Option<StoreComposition>,
        /// Why the store refused.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "Option::is_none")
        )]
        error: Option<StoreError>,
    },
}

/// One lock an input method was granted on a text store, and its calls inside it in order. A
/// geometry read (a range's rect, the document's bounds) places the input method's window and
/// changes nothing in the store, so it is not recorded.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct StoreLock {
    /// The store, as its [`StoreFocus`] numbered it.
    pub store: u64,
    /// The lock's kind.
    pub kind: StoreLockKind,
    /// The document as the lock found it, when the recording did not hold it yet: at the
    /// first lock after the store was focused, and at the first after the application changed
    /// it.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub opened: Option<StoreDocument>,
    /// The calls, at most [`MAX_STORE_STEPS`].
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub steps: Vec<StoreStep>,
    /// Part of the session is missing: it made more calls than [`MAX_STORE_STEPS`] or carried
    /// more text than [`MAX_STORE_TEXT_BYTES`], the store would not read its document out (a
    /// protected field), or the input method's code panicked inside the lock. A replay refuses
    /// the row.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "std::ops::Not::not")
    )]
    pub incomplete: bool,
    /// The store was protected (a password field): one such row stands for every lock until a
    /// lock finds it unprotected, and holds nothing derived from the field's content (no text,
    /// range, offset, length or count of keys), so it has no `opened` and no `steps`; a replay
    /// refuses it.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "std::ops::Not::not")
    )]
    pub protected: bool,
}
