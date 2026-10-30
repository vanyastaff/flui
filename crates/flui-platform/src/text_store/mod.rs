//! The text store an input method pulls from (ADR-0090).
//!
//! A text field that accepts IME implements [`TextStore`]: the platform
//! asks it for a lock, and reads or edits the document through the session
//! the lock's grant receives. The shape is TSF's `ITextStoreACP`, because
//! Windows is the platform whose input methods ask the most of a document
//! (edits under asynchronous locks, reconversion, geometry queries); AppKit's
//! `NSTextInputClient` and Android's `InputConnection` map onto the same
//! verbs (ADR-0090 "Platform mapping").
//!
//! - [`utf16`]: every offset on this surface counts UTF-16 code units
//!   ([`Utf16Offset`]); the conversion to a field's own representation lives
//!   here, once.
//! - [`TextStoreRead`] and [`TextStoreEdit`]: what a session can do. A read
//!   lock hands out only the first, so an edit under it does not compile.
//! - [`LockGrant`], [`LockOutcome`] and [`TextStoreError`]: the lock and its
//!   answers. [`CommitGate`] is the presentation's frame transaction as a
//!   store reads it.
//! - [`TextStore`] and [`TextStoreObserver`]: the field side and the
//!   platform side of the connection.
//! - [`TextStoreHost`]: what a pull-model window offers the presentation:
//!   which field it serves, and ending that field's composition (ADR-0135);
//!   [`commit_composition_in_place`] when the platform cannot end it.
//! - [`project_ime_event`]: a push-model [`ImeEvent`](crate::ImeEvent)
//!   (winit) applied as store edits, so there is one editing path.
//!
//! Stores are owner-thread objects (`Rc<dyn TextStore>`, not `Send`). The
//! machinery a widget's store is built from (the lock arbiter, the
//! composition ledger, the edit generation) is `flui_interaction::text_input`'s,
//! and the reference store is `flui_interaction::testing::InMemoryTextStore`
//! (ADR-0151 §6, ADR-0264).

mod host;
mod lock;
mod owner_calls;
mod projection;
mod session;
mod store;
pub mod utf16;

pub use host::{CompositionEnd, TextStoreHost, TextStoreHostError, commit_composition_in_place};
pub use lock::{
    CommitGate, DEFERRED_LOCK_CAPACITY, LockGrant, LockKind, LockOutcome, LockTiming,
    TextStoreError,
};
pub use owner_calls::{OwnerCalls, RetainOnFailure};
pub use projection::project_ime_event;
pub use session::{
    Composition, PointMode, RangeRect, Selection, TextChange, TextInputPurpose, TextStoreEdit,
    TextStoreRead, TextStoreStatus,
};
pub use store::{TextStore, TextStoreObserver};
pub use utf16::{OffsetError, Utf16Offset, Utf16Range};
