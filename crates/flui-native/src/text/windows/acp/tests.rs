//! TSF's `ITextStoreACP` contract over a text store, run on every host:
//! positions, lock flags and answers, the session slot and error codes.

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use flui_foundation::geometry::Point;
use flui_platform::text_store::{
    CommitGate, DEFERRED_LOCK_CAPACITY, InMemoryTextStore, LockGrant, LockKind, LockOutcome,
    LockTiming, OffsetError, PointMode, TextStore, TextStoreEdit, TextStoreError, TextStoreRead,
    Utf16Offset,
};

use super::{
    AcpChange, AcpSelection, Hresult, LockRequest, Session, SessionSlot, acp, lock_answer,
    lock_request, offset, point_mode, range, replace, request_lock, selection, set_selection,
    store_error, text, wide_text,
};

/// `a`, an astral emoji (two units), `b`, CR, LF and `c`: seven units, the
/// pair at 1..3 and the line break at 4..6.
const DOCUMENT: &str = "a\u{1F600}b\r\nc";

/// Runs every row, each contained, and names the ones that failed.
fn run(rows: &[(&str, fn())]) {
    let failed: Vec<&str> = rows
        .iter()
        .filter(|(_, row)| catch_unwind(*row).is_err())
        .map(|(name, _)| *name)
        .collect();
    assert!(failed.is_empty(), "failing rows: {failed:?}");
}

fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// `f`'s answer, from inside a read lock on a store holding `DOCUMENT`.
fn reading<R: 'static>(f: impl FnOnce(&dyn TextStoreRead) -> R + 'static) -> R {
    let store = InMemoryTextStore::new(DOCUMENT);
    let answer = Rc::new(RefCell::new(None));
    let slot = Rc::clone(&answer);
    let outcome = store.request_lock(
        LockGrant::read(move |session| *slot.borrow_mut() = Some(f(session))),
        LockTiming::Sync,
    );
    assert_eq!(outcome, Ok(LockOutcome::Granted), "the read lock runs now");
    answer.take().expect("the grant ran")
}

/// `f`'s answer, from inside a read-write lock on a store holding
/// `DOCUMENT`, and the store's text and selection after the lock.
fn editing<R: 'static>(
    f: impl FnOnce(&mut dyn TextStoreEdit) -> R + 'static,
) -> (R, String, AcpSelection) {
    let store = InMemoryTextStore::new(DOCUMENT);
    let answer = Rc::new(RefCell::new(None));
    let slot = Rc::clone(&answer);
    let outcome = store.request_lock(
        LockGrant::read_write(move |session| *slot.borrow_mut() = Some(f(session))),
        LockTiming::Sync,
    );
    assert_eq!(outcome, Ok(LockOutcome::Granted), "the edit lock runs now");
    let answer = answer.take().expect("the grant ran");
    let sel = store.selection().range();
    let active_at_start = store.selection().active < store.selection().anchor;
    (
        answer,
        store.text(),
        AcpSelection {
            start: acp(sel.start()),
            end: acp(sel.end()),
            active_at_start,
        },
    )
}

fn caret(at: i32) -> AcpSelection {
    AcpSelection {
        start: at,
        end: at,
        active_at_start: false,
    }
}

#[test]
fn split_surrogate_offsets_are_invalid_positions() {
    run(&[
        ("a range ending inside the pair", || {
            assert_eq!(
                reading(|s| text(s, 0, 2, 16)),
                Err(Hresult::TS_E_INVALIDPOS)
            );
        }),
        ("a range starting inside the pair", || {
            assert_eq!(
                reading(|s| text(s, 2, 4, 16)),
                Err(Hresult::TS_E_INVALIDPOS)
            );
        }),
        ("a buffer ending inside the pair stops before it", || {
            assert_eq!(reading(|s| text(s, 0, -1, 2)), Ok(units("a")));
        }),
        (
            "a one-unit buffer at the pair gets its high surrogate",
            || {
                // An empty answer would leave the caller's next ACP where it was.
                assert_eq!(reading(|s| text(s, 1, -1, 1)), Ok(vec![0xD83D]));
            },
        ),
        ("a buffer ending between CR and LF keeps the CR", || {
            assert_eq!(reading(|s| text(s, 3, -1, 2)), Ok(units("b\r")));
        }),
        ("the pair and the line break count a unit each", || {
            assert_eq!(reading(|s| text(s, 0, -1, 16)), Ok(units(DOCUMENT)));
            assert_eq!(reading(|s| text(s, 1, 3, 16)), Ok(units("\u{1F600}")));
            assert_eq!(reading(|s| text(s, 4, 6, 16)), Ok(units("\r\n")));
        }),
        ("a range past the end", || {
            assert_eq!(
                reading(|s| text(s, 0, 8, 16)),
                Err(Hresult::TS_E_INVALIDPOS)
            );
        }),
        ("a start past the end of the document", || {
            assert_eq!(
                reading(|s| text(s, 8, -1, 16)),
                Err(Hresult::TS_E_INVALIDPOS)
            );
        }),
        ("a negative start", || {
            assert_eq!(
                reading(|s| text(s, -1, 2, 16)),
                Err(Hresult::TS_E_INVALIDPOS)
            );
        }),
        ("an inverted range", || {
            assert_eq!(
                reading(|s| text(s, 3, 1, 16)),
                Err(Hresult::TS_E_INVALIDPOS)
            );
        }),
        ("a buffer as large as an ACP can count", || {
            assert_eq!(reading(|s| text(s, 6, -1, u32::MAX)), Ok(units("c")));
        }),
        ("a caret inside the pair", || {
            let (answer, _, after) = editing(|s| set_selection(s, caret(2)));
            assert_eq!(answer, Err(Hresult::TS_E_INVALIDPOS));
            assert_eq!(after, caret(7), "the selection is unchanged");
        }),
        ("a caret between CR and LF", || {
            let (answer, _, after) = editing(|s| set_selection(s, caret(5)));
            assert_eq!(answer, Ok(()));
            assert_eq!(after, caret(5));
        }),
        ("a caret past the end", || {
            let (answer, _, after) = editing(|s| set_selection(s, caret(8)));
            assert_eq!(answer, Err(Hresult::TS_E_INVALIDPOS));
            assert_eq!(after, caret(7));
        }),
        ("a selection made backwards keeps its active end", || {
            let backwards = AcpSelection {
                start: 1,
                end: 3,
                active_at_start: true,
            };
            let (answer, _, after) = editing(move |s| {
                set_selection(s, backwards)?;
                Ok::<_, Hresult>(selection(s))
            });
            assert_eq!(answer, Ok(backwards));
            assert_eq!(after, backwards);
        }),
        ("a replacement ending inside the pair", || {
            let (answer, text, _) = editing(|s| replace(s, 1, 2, "x"));
            assert_eq!(answer, Err(Hresult::TS_E_INVALIDPOS));
            assert_eq!(text, DOCUMENT, "the document is unchanged");
        }),
        ("a replacement past the end", || {
            let (answer, text, _) = editing(|s| replace(s, 7, 8, "x"));
            assert_eq!(answer, Err(Hresult::TS_E_INVALIDPOS));
            assert_eq!(text, DOCUMENT);
        }),
        ("a replacement of the pair by another counts units", || {
            let (answer, text, _) = editing(|s| replace(s, 1, 3, "\u{2000B}e\u{301}"));
            assert_eq!(
                answer,
                Ok(AcpChange {
                    start: 1,
                    old_end: 3,
                    new_end: 5,
                })
            );
            assert_eq!(text, "a\u{2000B}e\u{301}b\r\nc");
        }),
        ("an unpaired surrogate from TSF", || {
            assert_eq!(wide_text(&[0xD83D]), Err(Hresult::E_INVALIDARG));
            assert_eq!(wide_text(&[0xDE00, 0x61]), Err(Hresult::E_INVALIDARG));
            assert_eq!(
                wide_text(&units("\u{1F600}\r\n")),
                Ok("\u{1F600}\r\n".into())
            );
        }),
    ]);
}

#[test]
fn reads_outside_a_lock_answer_no_lock() {
    fn read_now(slot: &SessionSlot) -> Result<i32, Hresult> {
        slot.read(|session| Ok(acp(session.document_len())))
    }
    fn write_now(slot: &SessionSlot) -> Result<(), Hresult> {
        slot.write(|session| set_selection(session, caret(0)))
    }
    run(&[
        ("a read with no lock", || {
            assert_eq!(read_now(&SessionSlot::default()), Err(Hresult::TS_E_NOLOCK));
        }),
        ("an edit with no lock", || {
            assert_eq!(
                write_now(&SessionSlot::default()),
                Err(Hresult::TS_E_NOLOCK)
            );
        }),
        ("an edit under a read lock", || {
            let answers = reading(|session| {
                let slot = SessionSlot::default();
                slot.with_open(Session::Read(session), || {
                    (read_now(&slot), write_now(&slot))
                })
            });
            assert_eq!(answers, (Ok(7), Err(Hresult::TS_E_NOLOCK)));
        }),
        ("a read and an edit under a read-write lock", || {
            let (answers, _, after) = editing(|session| {
                let slot = SessionSlot::default();
                slot.with_open(Session::Write(session), || {
                    (read_now(&slot), write_now(&slot))
                })
            });
            assert_eq!(answers, (Ok(7), Ok(())));
            assert_eq!(after, caret(0));
        }),
        ("a read once the lock ended", || {
            let answer = reading(|session| {
                let slot = SessionSlot::default();
                slot.with_open(Session::Read(session), || ());
                read_now(&slot)
            });
            assert_eq!(answer, Err(Hresult::TS_E_NOLOCK));
        }),
        ("a read once the lock unwound", || {
            let answer = reading(|session| {
                let slot = SessionSlot::default();
                let unwound = catch_unwind(AssertUnwindSafe(|| {
                    slot.with_open(Session::Read(session), || panic!("the grant panics"));
                }));
                assert!(unwound.is_err());
                read_now(&slot)
            });
            assert_eq!(answer, Err(Hresult::TS_E_NOLOCK));
        }),
        ("a read reached from inside another", || {
            let answers = reading(|session| {
                let slot = SessionSlot::default();
                slot.with_open(Session::Read(session), || {
                    let nested = slot.read(|_| read_now(&slot));
                    (nested, read_now(&slot))
                })
            });
            assert_eq!(
                answers,
                (Err(Hresult::E_UNEXPECTED), Ok(7)),
                "the nested read is refused, and the next one served"
            );
        }),
    ]);
}

/// A store holding `DOCUMENT` behind a gate the test holds.
fn gated() -> (Rc<InMemoryTextStore>, CommitGate) {
    let store = InMemoryTextStore::new(DOCUMENT);
    let gate = CommitGate::new();
    store.set_commit_gate(gate.clone());
    (store, gate)
}

/// What one grant saw: whether it could edit, and its flags.
type Seen = Rc<Cell<Option<(bool, u32)>>>;

/// A grant recording what it saw, answering `answer` as `OnLockGranted`.
fn recording(seen: &Seen, answer: Hresult) -> impl FnOnce(Session<'_>, u32) -> Hresult + 'static {
    let seen = Rc::clone(seen);
    move |session, flags| {
        seen.set(Some((matches!(session, Session::Write(_)), flags)));
        answer
    }
}

#[test]
fn a_lock_inside_the_frame_answers_async() {
    run(&[
        ("an async edit lock is queued", || {
            let (store, gate) = gated();
            gate.set_open(false);
            let seen = Seen::default();
            let answer = request_lock(&*store, 0x6, recording(&seen, Hresult::S_OK));
            assert_eq!(answer, Ok(Hresult::TS_S_ASYNC));
            assert_eq!(seen.get(), None, "nothing runs inside the frame");
            gate.set_open(true);
            assert_eq!(store.run_deferred_grants(), 1);
            assert_eq!(seen.get(), Some((true, 0x6)), "the anchor grants the edit");
        }),
        ("a sync lock is refused through the session result", || {
            let (store, gate) = gated();
            gate.set_open(false);
            let seen = Seen::default();
            let answer = request_lock(&*store, 0x7, recording(&seen, Hresult::S_OK));
            assert_eq!(answer, Ok(Hresult::TS_E_SYNCHRONOUS));
            gate.set_open(true);
            assert_eq!(
                store.run_deferred_grants(),
                0,
                "a refused lock is not queued"
            );
            assert_eq!(seen.get(), None);
        }),
        (
            "outside the frame a sync lock runs before the answer",
            || {
                let (store, _gate) = gated();
                let seen = Seen::default();
                let answer = request_lock(&*store, 0x3, recording(&seen, Hresult::E_FAIL));
                assert_eq!(
                    answer,
                    Ok(Hresult::E_FAIL),
                    "the session result is what OnLockGranted answered"
                );
                assert_eq!(seen.get(), Some((false, 0x2)), "a read, without TS_LF_SYNC");
            },
        ),
        ("a full queue fails the request and keeps the queue", || {
            let (store, gate) = gated();
            gate.set_open(false);
            let seen = Seen::default();
            for _ in 0..DEFERRED_LOCK_CAPACITY {
                let answer = request_lock(&*store, 0x2, recording(&seen, Hresult::S_OK));
                assert_eq!(answer, Ok(Hresult::TS_S_ASYNC));
            }
            let answer = request_lock(&*store, 0x2, recording(&seen, Hresult::S_OK));
            assert_eq!(answer, Err(Hresult::E_FAIL));
            gate.set_open(true);
            assert_eq!(store.run_deferred_grants(), DEFERRED_LOCK_CAPACITY);
        }),
    ]);
}

#[test]
fn lock_flags_and_errors_translate_to_tsf_codes() {
    run(&[
        ("lock flags", || {
            let rows = [
                (0x2, LockKind::Read, LockTiming::Async, 0x2),
                (0x3, LockKind::Read, LockTiming::Sync, 0x2),
                (0x6, LockKind::ReadWrite, LockTiming::Async, 0x6),
                (0x7, LockKind::ReadWrite, LockTiming::Sync, 0x6),
                // The write bit without the read bit is no read-write lock.
                (0x4, LockKind::Read, LockTiming::Async, 0x4),
            ];
            for (flags, kind, timing, granted_flags) in rows {
                assert_eq!(
                    lock_request(flags),
                    LockRequest {
                        kind,
                        timing,
                        granted_flags,
                    },
                    "flags {flags:#x}"
                );
            }
        }),
        ("store errors", || {
            let rows = [
                (
                    TextStoreError::SyncLockUnavailable,
                    Hresult::TS_E_SYNCHRONOUS,
                ),
                (TextStoreError::DeferredQueueFull, Hresult::E_FAIL),
                (
                    TextStoreError::Offset(OffsetError::SplitsSurrogatePair(2)),
                    Hresult::TS_E_INVALIDPOS,
                ),
                (
                    TextStoreError::Offset(OffsetError::PastEnd { offset: 8, len: 7 }),
                    Hresult::TS_E_INVALIDPOS,
                ),
                (TextStoreError::Protected, Hresult::E_FAIL),
                (TextStoreError::NoLayout, Hresult::TS_E_NOLAYOUT),
                (TextStoreError::PointOutside, Hresult::TS_E_INVALIDPOINT),
                (TextStoreError::Detached, Hresult::E_UNEXPECTED),
            ];
            for (error, code) in rows {
                assert_eq!(store_error(error), code, "{error:?}");
            }
        }),
        ("lock answers", || {
            let rows = [
                // Granted, yet the grant never ran: no session took place.
                (Ok(LockOutcome::Granted), None, Ok(Hresult::E_FAIL)),
                (
                    Ok(LockOutcome::Granted),
                    Some(Hresult::S_OK),
                    Ok(Hresult::S_OK),
                ),
                (
                    Ok(LockOutcome::Granted),
                    Some(Hresult::E_FAIL),
                    Ok(Hresult::E_FAIL),
                ),
                (Ok(LockOutcome::Deferred), None, Ok(Hresult::TS_S_ASYNC)),
                (
                    Err(TextStoreError::SyncLockUnavailable),
                    None,
                    Ok(Hresult::TS_E_SYNCHRONOUS),
                ),
                (
                    Err(TextStoreError::DeferredQueueFull),
                    None,
                    Err(Hresult::E_FAIL),
                ),
                (
                    Err(TextStoreError::Detached),
                    None,
                    Err(Hresult::E_UNEXPECTED),
                ),
            ];
            for (outcome, session, answer) in rows {
                assert_eq!(lock_answer(outcome, session), answer, "{outcome:?}");
            }
        }),
        ("positions", || {
            assert_eq!(offset(-1), Err(Hresult::TS_E_INVALIDPOS));
            assert_eq!(offset(i32::MIN), Err(Hresult::TS_E_INVALIDPOS));
            assert_eq!(offset(7), Ok(Utf16Offset::new(7)));
            assert_eq!(range(3, 1), Err(Hresult::TS_E_INVALIDPOS));
            assert_eq!(acp(Utf16Offset::new(usize::MAX)), i32::MAX);
        }),
        ("codes as the numbers TSF reads", || {
            let rows = [
                (Hresult::S_OK, 0x0000_0000_u32),
                (Hresult::TS_S_ASYNC, 0x0004_0300),
                (Hresult::TS_E_INVALIDPOS, 0x8004_0200),
                (Hresult::TS_E_NOLOCK, 0x8004_0201),
                (Hresult::TS_E_NOLAYOUT, 0x8004_0206),
                (Hresult::TS_E_INVALIDPOINT, 0x8004_0207),
                (Hresult::TS_E_SYNCHRONOUS, 0x8004_0208),
                (Hresult::E_UNEXPECTED, 0x8000_FFFF),
                (Hresult::E_FAIL, 0x8000_4005),
                (Hresult::E_INVALIDARG, 0x8007_0057),
            ];
            for (code, bits) in rows {
                assert_eq!(code.value().cast_unsigned(), bits, "{code:?}");
                assert_eq!(Hresult::new(code.value()), code);
            }
        }),
        ("GetACPFromPoint flags", || {
            let rows = [
                (0x0, false, false),
                (0x1, true, false),
                (0x2, false, true),
                (0x3, true, true),
            ];
            for (flags, round_to_nearest_edge, nearest_when_outside) in rows {
                assert_eq!(
                    point_mode(flags),
                    PointMode {
                        round_to_nearest_edge,
                        nearest_when_outside,
                    },
                    "flags {flags:#x}"
                );
            }
        }),
        ("a point outside the text, by its flags", || {
            // Left of the first cell, on its line.
            let outside = Point::new(-5.0, 1.0);
            let answer = |flags| {
                reading(move |session| {
                    session
                        .index_at_point(outside, point_mode(flags))
                        .map(acp)
                        .map_err(store_error)
                })
            };
            assert_eq!(answer(0x0), Err(Hresult::TS_E_INVALIDPOINT));
            assert_eq!(answer(0x1), Err(Hresult::TS_E_INVALIDPOINT));
            assert_eq!(answer(0x2), Ok(0));
            assert_eq!(answer(0x3), Ok(0));
        }),
    ]);
}
