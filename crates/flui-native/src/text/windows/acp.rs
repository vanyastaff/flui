//! TSF's `ITextStoreACP` contract over a [`TextStore`], in plain values.
//!
//! Free of `windows` types and built for tests on every host, so its tables
//! run there; the Win32 text services (`tsf::document`) are its one
//! consumer and only move values between COM's pointers and these
//! functions. What it decides:
//!
//! - **positions**: an ACP is a UTF-16 offset as an `i32`. A negative one,
//!   one past the end, one between the halves of a surrogate pair and a
//!   range whose start lies after its end are invalid positions
//!   (`TS_E_INVALIDPOS`); CR and LF are a position each, like any code unit;
//! - **locks**: `TS_LF_*` flags as the store's lock kind and timing, and the
//!   store's answer as the pair `RequestLock` returns: the method's own
//!   result, and the session result (`*phrSession`) it writes when it
//!   succeeds (ADR-0135 §2);
//! - **the session slot**: a method reads or edits only inside a granted
//!   lock; outside one it answers `TS_E_NOLOCK`, and so does an edit under a
//!   read lock;
//! - **errors**: each [`TextStoreError`] as the HRESULT TSF expects.
//!
//! The codes are the numbers `windows` names; `tsf::document` asserts at
//! compile time that each matches.

use std::cell::Cell;
use std::rc::Rc;

use flui_platform::text_store::{
    LockGrant, LockKind, LockOutcome, LockTiming, OffsetError, PointMode, Selection, TextChange,
    TextStore, TextStoreEdit, TextStoreError, TextStoreRead, Utf16Offset, Utf16Range,
};

/// An HRESULT, as the number TSF reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Hresult(i32);

impl Hresult {
    /// Success.
    pub(crate) const S_OK: Self = Self::from_bits(0);
    /// The lock is queued and granted later.
    pub(crate) const TS_S_ASYNC: Self = Self::from_bits(0x0004_0300);
    /// Not a position in the document.
    pub(crate) const TS_E_INVALIDPOS: Self = Self::from_bits(0x8004_0200);
    /// The call needs a lock the caller does not hold.
    pub(crate) const TS_E_NOLOCK: Self = Self::from_bits(0x8004_0201);
    /// The text has no layout yet.
    pub(crate) const TS_E_NOLAYOUT: Self = Self::from_bits(0x8004_0206);
    /// The point is not on the text.
    pub(crate) const TS_E_INVALIDPOINT: Self = Self::from_bits(0x8004_0207);
    /// A synchronous lock cannot be granted now.
    pub(crate) const TS_E_SYNCHRONOUS: Self = Self::from_bits(0x8004_0208);
    /// The call came at a time it cannot be served.
    pub(crate) const E_UNEXPECTED: Self = Self::from_bits(0x8000_FFFF);
    /// The call failed.
    pub(crate) const E_FAIL: Self = Self::from_bits(0x8000_4005);
    /// An argument is not valid.
    pub(crate) const E_INVALIDARG: Self = Self::from_bits(0x8007_0057);

    const fn from_bits(bits: u32) -> Self {
        Self(bits.cast_signed())
    }

    /// The HRESULT's value.
    pub(crate) const fn value(self) -> i32 {
        self.0
    }

    /// The HRESULT whose value is `value`.
    pub(crate) const fn new(value: i32) -> Self {
        Self(value)
    }
}

/// `TS_LF_SYNC`: the caller needs the lock before `RequestLock` returns.
pub(crate) const TS_LF_SYNC: u32 = 0x1;
/// `TS_LF_READWRITE`: a lock that reads and edits; it includes `TS_LF_READ`.
pub(crate) const TS_LF_READWRITE: u32 = 0x6;
/// `GXFPF_ROUND_NEAREST`: a point inside a character's box takes the box's
/// nearer edge.
pub(crate) const GXFPF_ROUND_NEAREST: u32 = 0x1;
/// `GXFPF_NEAREST`: a point outside every character's box takes the nearest
/// position instead of failing with `TS_E_INVALIDPOINT`.
pub(crate) const GXFPF_NEAREST: u32 = 0x2;
/// `TS_IAS_QUERYONLY`: `InsertTextAtSelection` inserts nothing and answers
/// the range an insertion would replace.
pub(crate) const TS_IAS_QUERYONLY: u32 = 0x2;

/// A `RequestLock`'s flags as the store's request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LockRequest {
    pub(crate) kind: LockKind,
    pub(crate) timing: LockTiming,
    /// The flags `OnLockGranted` receives: the request's, less `TS_LF_SYNC`.
    pub(crate) granted_flags: u32,
}

/// The request `flags` make: read-write when every bit of
/// `TS_LF_READWRITE` is set, read otherwise; synchronous with `TS_LF_SYNC`.
pub(crate) const fn lock_request(flags: u32) -> LockRequest {
    LockRequest {
        kind: if flags & TS_LF_READWRITE == TS_LF_READWRITE {
            LockKind::ReadWrite
        } else {
            LockKind::Read
        },
        timing: if flags & TS_LF_SYNC == 0 {
            LockTiming::Async
        } else {
            LockTiming::Sync
        },
        granted_flags: flags & !TS_LF_SYNC,
    }
}

/// `RequestLock` on `store`: `grant` runs the lock with its session and the
/// flags `OnLockGranted` receives, and answers what `OnLockGranted` returned.
///
/// `Ok` carries the session result `RequestLock` writes to `*phrSession`
/// when it returns `S_OK`; `Err` is the result `RequestLock` itself returns
/// (see [`lock_answer`]).
pub(crate) fn request_lock(
    store: &dyn TextStore,
    flags: u32,
    grant: impl FnOnce(Session<'_>, u32) -> Hresult + 'static,
) -> Result<Hresult, Hresult> {
    let request = lock_request(flags);
    let granted = Rc::new(Cell::new(None));
    let result = Rc::clone(&granted);
    let granted_flags = request.granted_flags;
    let lock = match request.kind {
        LockKind::Read => LockGrant::read(move |session| {
            result.set(Some(grant(Session::Read(session), granted_flags)));
        }),
        LockKind::ReadWrite => LockGrant::read_write(move |session| {
            result.set(Some(grant(Session::Write(session), granted_flags)));
        }),
    };
    lock_answer(store.request_lock(lock, request.timing), granted.get())
}

/// The store's answer to a lock request as `RequestLock`'s (ADR-0135 §2):
///
/// - granted: the session result is what `OnLockGranted` returned
///   (`session`); a store that reports a grant it never ran gets `E_FAIL`,
///   as no session took place;
/// - deferred: `TS_S_ASYNC`, the grant runs at the store's next anchor;
/// - no synchronous lock now: `RequestLock` succeeds, and the session result
///   is `TS_E_SYNCHRONOUS`;
/// - any other refusal fails `RequestLock` itself ([`store_error`]).
pub(crate) fn lock_answer(
    outcome: Result<LockOutcome, TextStoreError>,
    session: Option<Hresult>,
) -> Result<Hresult, Hresult> {
    match outcome {
        Ok(LockOutcome::Granted) => Ok(session.unwrap_or(Hresult::E_FAIL)),
        Ok(LockOutcome::Deferred) => Ok(Hresult::TS_S_ASYNC),
        Err(TextStoreError::SyncLockUnavailable) => Ok(Hresult::TS_E_SYNCHRONOUS),
        Err(error) => Err(store_error(error)),
    }
}

/// A store error as the HRESULT TSF expects.
pub(crate) const fn store_error(error: TextStoreError) -> Hresult {
    match error {
        TextStoreError::SyncLockUnavailable => Hresult::TS_E_SYNCHRONOUS,
        TextStoreError::Offset(_) => Hresult::TS_E_INVALIDPOS,
        TextStoreError::NoLayout => Hresult::TS_E_NOLAYOUT,
        TextStoreError::PointOutside => Hresult::TS_E_INVALIDPOINT,
        TextStoreError::Detached => Hresult::E_UNEXPECTED,
        _ => Hresult::E_FAIL,
    }
}

/// The offset an ACP names; a negative one is no position.
pub(crate) fn offset(acp: i32) -> Result<Utf16Offset, Hresult> {
    usize::try_from(acp)
        .map(Utf16Offset::new)
        .map_err(|_| Hresult::TS_E_INVALIDPOS)
}

/// The ACP of `offset`; an offset past `i32::MAX` answers `i32::MAX`, which
/// no store reaches (TSF cannot address it).
pub(crate) fn acp(offset: Utf16Offset) -> i32 {
    i32::try_from(offset.get()).unwrap_or(i32::MAX)
}

/// The range from `start` to `end`; a negative or inverted one is no range.
pub(crate) fn range(start: i32, end: i32) -> Result<Utf16Range, Hresult> {
    Utf16Range::try_new(offset(start)?, offset(end)?).ok_or(Hresult::TS_E_INVALIDPOS)
}

/// A text change as `TS_TEXTCHANGE`'s three ACPs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AcpChange {
    pub(crate) start: i32,
    pub(crate) old_end: i32,
    pub(crate) new_end: i32,
}

/// `change` in ACPs.
pub(crate) fn text_change(change: TextChange) -> AcpChange {
    AcpChange {
        start: acp(change.start),
        old_end: acp(change.old_end),
        new_end: acp(change.new_end),
    }
}

/// A selection as `TS_SELECTION_ACP` holds it: an ordered span, and which
/// end is active (`TS_AE_START` or `TS_AE_END`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AcpSelection {
    pub(crate) start: i32,
    pub(crate) end: i32,
    /// Whether the active end is the start (`TS_AE_START`), as in a
    /// selection made backwards.
    pub(crate) active_at_start: bool,
}

/// The session's selection in ACPs.
pub(crate) fn selection(session: &dyn TextStoreRead) -> AcpSelection {
    let selection = session.selection();
    let span = selection.range();
    AcpSelection {
        start: acp(span.start()),
        end: acp(span.end()),
        active_at_start: selection.active < selection.anchor,
    }
}

/// Set the session's selection to `wanted`.
///
/// # Errors
///
/// `TS_E_INVALIDPOS` for an end that is no position in the document, and
/// the store's refusal otherwise.
pub(crate) fn set_selection(
    session: &mut dyn TextStoreEdit,
    wanted: AcpSelection,
) -> Result<(), Hresult> {
    let (start, end) = (offset(wanted.start)?, offset(wanted.end)?);
    let selection = if wanted.active_at_start {
        Selection {
            anchor: end,
            active: start,
        }
    } else {
        Selection {
            anchor: start,
            active: end,
        }
    };
    session.set_selection(selection).map_err(store_error)
}

/// `GetText`'s text: the UTF-16 units from `start` to `end` (`-1` for the
/// document's end), at most `room` of them, the room the caller's buffer
/// has. Only what fits is read. A cap that falls between the halves of a
/// surrogate pair stops before the pair, which the caller reads next from
/// the ACP after the last unit returned; a cap anywhere else, between CR and
/// LF included, is kept.
///
/// While there is room and text left the answer is never empty, so a caller
/// reading on from the ACP after it always moves: a buffer of one unit at a
/// pair gets the pair's high surrogate alone, since `GetText` counts units,
/// not characters.
///
/// # Errors
///
/// `TS_E_INVALIDPOS` for a range that is not in the document, and the
/// store's refusal otherwise.
pub(crate) fn text(
    session: &dyn TextStoreRead,
    start: i32,
    end: i32,
    room: u32,
) -> Result<Vec<u16>, Hresult> {
    let end = if end == -1 {
        acp(session.document_len())
    } else {
        end
    };
    let room = i32::try_from(room).unwrap_or(i32::MAX);
    let capped = end.min(start.saturating_add(room));
    let text = match session.text(range(start, capped)?) {
        Ok(text) => text,
        Err(TextStoreError::Offset(OffsetError::SplitsSurrogatePair(split)))
            if capped < end && offset(capped).is_ok_and(|cap| cap.get() == split) =>
        {
            if capped - 1 > start {
                session
                    .text(range(start, capped - 1)?)
                    .map_err(store_error)?
            } else {
                // Room for one unit, and the pair starts at `start`.
                let pair = session
                    .text(range(start, capped.saturating_add(1))?)
                    .map_err(store_error)?;
                return Ok(pair.encode_utf16().take(1).collect());
            }
        }
        Err(error) => return Err(store_error(error)),
    };
    Ok(text.encode_utf16().collect())
}

/// Replace the text from `start` to `end` with `text`.
///
/// # Errors
///
/// `TS_E_INVALIDPOS` for a range that is not in the document, and the
/// store's refusal otherwise.
pub(crate) fn replace(
    session: &mut dyn TextStoreEdit,
    start: i32,
    end: i32,
    text: &str,
) -> Result<AcpChange, Hresult> {
    let span = range(start, end)?;
    session
        .replace(span, text)
        .map(text_change)
        .map_err(store_error)
}

/// What `InsertTextAtSelection` answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SelectionInsert {
    /// `TS_IAS_QUERYONLY`: the span an insertion would replace, the
    /// selection; nothing changed.
    Query { start: i32, end: i32 },
    /// The text replaced the selection.
    Inserted(AcpChange),
}

/// `InsertTextAtSelection` on the open session with `flags`. `text` is read
/// only when the call inserts: a query passes no text to decode.
///
/// A query only reads, so any lock serves it, and an input method that
/// reconverts committed text asks one before it marks that text; the
/// document and its selection stay as they were.
///
/// # Errors
///
/// `TS_E_NOLOCK` outside a lock, and for an insertion under a read lock;
/// `text`'s refusal; the store's refusal otherwise.
pub(crate) fn insert_at_selection(
    slot: &SessionSlot,
    flags: u32,
    text: impl FnOnce() -> Result<String, Hresult>,
) -> Result<SelectionInsert, Hresult> {
    if flags & TS_IAS_QUERYONLY != 0 {
        return slot.read(|session| {
            let span = selection(session);
            Ok(SelectionInsert::Query {
                start: span.start,
                end: span.end,
            })
        });
    }
    let text = text()?;
    slot.write(|session| {
        session
            .insert_at_selection(&text)
            .map(|change| SelectionInsert::Inserted(text_change(change)))
            .map_err(store_error)
    })
}

/// `cch` UTF-16 units TSF passed as a `String`; an unpaired surrogate is an
/// invalid argument, not a lossy replacement.
pub(crate) fn wide_text(units: &[u16]) -> Result<String, Hresult> {
    String::from_utf16(units).map_err(|_| Hresult::E_INVALIDARG)
}

/// `GetACPFromPoint`'s flags as the store's point mode: the two flags are
/// independent, and each sets one of the mode's two choices
/// (<https://learn.microsoft.com/en-us/windows/win32/api/textstor/nf-textstor-itextstoreacp-getacpfrompoint>).
pub(crate) const fn point_mode(flags: u32) -> PointMode {
    PointMode {
        round_to_nearest_edge: flags & GXFPF_ROUND_NEAREST != 0,
        nearest_when_outside: flags & GXFPF_NEAREST != 0,
    }
}

/// The session a granted lock opened.
pub(crate) enum Session<'a> {
    Read(&'a dyn TextStoreRead),
    Write(&'a mut dyn TextStoreEdit),
}

/// A session with its lifetime erased, valid only while
/// [`SessionSlot::with_open`]'s frame is on the stack.
#[derive(Clone, Copy)]
enum ErasedSession {
    Read(*const (dyn TextStoreRead + 'static)),
    Write(*mut (dyn TextStoreEdit + 'static)),
}

/// Where a granted lock's session waits for the TSF calls made under it.
///
/// TSF reads and edits by calling the text store's methods from inside
/// `OnLockGranted`; those calls reach the session through this slot. It is
/// open only while [`Self::with_open`] runs, and at most one method borrows
/// the session at a time (`in_use`): a method reached reentrantly while
/// another holds it answers `E_UNEXPECTED` instead of taking a second
/// `&mut`. Owner-thread only (`Cell`).
#[derive(Default)]
pub(crate) struct SessionSlot {
    session: Cell<Option<ErasedSession>>,
    in_use: Cell<bool>,
}

/// Restores the slot's previous value when `with_open`'s frame ends,
/// unwinding included.
struct SlotRestore<'a> {
    slot: &'a SessionSlot,
    previous: Option<ErasedSession>,
}

impl Drop for SlotRestore<'_> {
    fn drop(&mut self) {
        self.slot.session.set(self.previous);
    }
}

/// Clears the slot's `in_use` flag when a method's borrow ends.
struct InUse<'a>(&'a Cell<bool>);

impl Drop for InUse<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

#[expect(
    unsafe_code,
    reason = "the slot erases the session's lifetime for the TSF calls made under the grant"
)]
impl SessionSlot {
    /// Run `f` with `session` reachable through the slot.
    #[expect(
        clippy::transmute_ptr_to_ptr,
        reason = "only the trait object's lifetime bound changes, which a cast cannot do"
    )]
    pub(crate) fn with_open<R>(&self, session: Session<'_>, f: impl FnOnce() -> R) -> R {
        let erased = match session {
            // SAFETY: only the trait object's lifetime bound changes (the
            // pointer's layout is the same). This frame keeps the borrow the
            // pointer came from alive and unused until `SlotRestore` closes
            // the slot again, before this function returns or unwinds, so
            // the pointer never outlives the session it points to.
            Session::Read(read) => ErasedSession::Read(unsafe {
                std::mem::transmute::<
                    *const (dyn TextStoreRead + '_),
                    *const (dyn TextStoreRead + 'static),
                >(read)
            }),
            // SAFETY: as above, for the read-write session.
            Session::Write(write) => ErasedSession::Write(unsafe {
                std::mem::transmute::<
                    *mut (dyn TextStoreEdit + '_),
                    *mut (dyn TextStoreEdit + 'static),
                >(write)
            }),
        };
        let _restore = SlotRestore {
            slot: self,
            previous: self.session.replace(Some(erased)),
        };
        f()
    }

    fn borrow(&self) -> Result<(ErasedSession, InUse<'_>), Hresult> {
        let session = self.session.get().ok_or(Hresult::TS_E_NOLOCK)?;
        if self.in_use.replace(true) {
            tracing::warn!(target: "flui_native::tsf", "nested session borrow refused");
            return Err(Hresult::E_UNEXPECTED);
        }
        Ok((session, InUse(&self.in_use)))
    }

    /// Run `f` on the open session, read or read-write.
    ///
    /// # Errors
    ///
    /// `TS_E_NOLOCK` with no session open, `E_UNEXPECTED` while another
    /// method borrows it, and `f`'s own.
    pub(crate) fn read<R, E: From<Hresult>>(
        &self,
        f: impl FnOnce(&dyn TextStoreRead) -> Result<R, E>,
    ) -> Result<R, E> {
        let (session, _in_use) = self.borrow()?;
        match session {
            // SAFETY: the slot holds a pointer only while `with_open`'s frame
            // keeps its session alive (see there); `in_use` makes this the
            // only borrow of it; the slot is owner-thread only (`Cell`).
            ErasedSession::Read(read) => f(unsafe { &*read }),
            // SAFETY: as above.
            ErasedSession::Write(write) => f(unsafe { &*write }),
        }
    }

    /// Run `f` on the open session, which must be read-write.
    ///
    /// # Errors
    ///
    /// As [`Self::read`], and `TS_E_NOLOCK` for a read-only session.
    pub(crate) fn write<R, E: From<Hresult>>(
        &self,
        f: impl FnOnce(&mut dyn TextStoreEdit) -> Result<R, E>,
    ) -> Result<R, E> {
        let (session, _in_use) = self.borrow()?;
        match session {
            ErasedSession::Read(_) => Err(Hresult::TS_E_NOLOCK.into()),
            // SAFETY: as in `read`; `in_use` rules out a second `&mut`.
            ErasedSession::Write(write) => f(unsafe { &mut *write }),
        }
    }
}

#[cfg(test)]
mod tests;
