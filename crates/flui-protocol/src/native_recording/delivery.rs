//! What the loop delivered: system notices, a session's end and an input method's events.

vocabulary! {
    /// How much memory the system wants back.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum MemoryPressureLevel {
        /// Free what is easy to restore.
        Moderate => "moderate",
        /// Free everything the current frame does not need.
        Critical => "critical",
    }
}

vocabulary! {
    /// A phase of the user's session ending.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum SessionEndPhase {
        /// The session is about to end; the application is asked whether to wait.
        Query => "query",
        /// The session end was cancelled.
        Cancelled => "cancelled",
        /// The session is ending now.
        Ending => "ending",
    }
}

/// A one-off system event the host reported to the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum Notice {
    /// The machine is about to sleep.
    Sleep,
    /// The machine woke from sleep.
    Wake,
    /// The user's session was locked.
    SessionLocked,
    /// The user's session was unlocked.
    SessionUnlocked,
    /// The system wants memory back.
    MemoryPressure(MemoryPressureLevel),
}

/// An input method's event, as the window delivered it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[cfg_attr(
    feature = "schemars",
    derive(schemars::JsonSchema),
    schemars(rename = "NativeImeEvent")
)]
#[non_exhaustive]
pub enum ImeEvent {
    /// Composition became available for the focused input.
    Enabled,
    /// The composition text changed.
    Preedit {
        /// The composition text.
        text: String,
        /// The cursor or selection, as byte offsets into `text`; none hides the caret.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "Option::is_none")
        )]
        cursor: Option<[usize; 2]>,
    },
    /// Composition finished with this text.
    Commit(String),
    /// Composition ended.
    Disabled,
}

/// What a window's input method did through an event. An input method that reads and edits the
/// focused text store instead is recorded as [`Row::StoreFocus`](super::Row::StoreFocus) and [`Row::StoreLock`](super::Row::StoreLock) rows.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum Ime {
    /// An event the window delivered.
    Event(ImeEvent),
}
