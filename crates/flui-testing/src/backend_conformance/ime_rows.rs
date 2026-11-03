//! The IME rows (ADR-0090): a composition reaches the field in one order on
//! every input method path.
//!
//! One scenario drives each path: a preedit grows, a candidate is picked, the
//! composition commits; a second composition is cancelled. After each step
//! the field holds the same text, composing range and caret whichever path
//! made it, and each step is one read-write session that changes the field's
//! store, so the field's owner hears of it as one change.

use std::cell::Cell;
use std::rc::Rc;

use flui_interaction::testing::InMemoryTextStore;
use flui_platform::text_store::{
    CommitGate, LockGrant, LockOutcome, LockTiming, TextStore, TextStoreError, TextStoreObserver,
    TextStoreRead, TextStoreStatus, Utf16Offset, Utf16Range,
};

use super::rows::Outcome;
use super::{BackendUnderTest, CompositionStep, RowOutcome, Step, TextInputPath};

/// The field every composition starts in: two characters, the caret after
/// them.
const FIELD: &str = "ab";

/// What the field holds after a step: its text, its composing range and its
/// selection, its anchor and its active end, in UTF-16 code units.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Field {
    text: String,
    composing: Option<(usize, usize)>,
    selection: (usize, usize),
}

/// What a step leaves in the field: [`Field`] as the table writes it.
#[derive(Debug, Clone, Copy)]
struct Expected {
    text: &'static str,
    composing: Option<(usize, usize)>,
    caret: usize,
}
const fn field(text: &'static str, composing: Option<(usize, usize)>, caret: usize) -> Expected {
    Expected {
        text,
        composing,
        caret,
    }
}

/// The composition and the field after each of its steps: に, にほ, the
/// candidate 日本, then its commit.
const COMMITTED: [(CompositionStep, Expected); 4] = [
    (
        CompositionStep::Preedit {
            text: "に",
            caret: 1,
        },
        field("abに", Some((2, 3)), 3),
    ),
    (
        CompositionStep::Preedit {
            text: "にほ",
            caret: 2,
        },
        field("abにほ", Some((2, 4)), 4),
    ),
    (
        CompositionStep::Preedit {
            text: "日本",
            caret: 2,
        },
        field("ab日本", Some((2, 4)), 4),
    ),
    (
        CompositionStep::Commit { text: "日本" },
        field("ab日本", None, 4),
    ),
];

/// A composition of か, cancelled.
const CANCELLED: [(CompositionStep, Expected); 2] = [
    (
        CompositionStep::Preedit {
            text: "か",
            caret: 1,
        },
        field("abか", Some((2, 3)), 3),
    ),
    (CompositionStep::Cancel, field("ab", None, 2)),
];

/// The reference store, counting the read-write sessions that change the
/// field: a session that leaves it as it was is no edit of it.
struct Counted {
    store: Rc<InMemoryTextStore>,
    edits: Rc<Cell<usize>>,
}

impl TextStore for Counted {
    fn status(&self) -> TextStoreStatus {
        self.store.status()
    }

    fn request_lock(
        &self,
        grant: LockGrant,
        timing: LockTiming,
    ) -> Result<LockOutcome, TextStoreError> {
        let grant = match grant {
            LockGrant::ReadWrite(body) => {
                let edits = Rc::clone(&self.edits);
                LockGrant::read_write(move |session| {
                    let before = read(&*session);
                    body(session);
                    if read(&*session) != before {
                        edits.set(edits.get() + 1);
                    }
                })
            }
            read @ LockGrant::Read(_) => read,
        };
        self.store.request_lock(grant, timing)
    }

    fn run_deferred_grants(&self) -> usize {
        self.store.run_deferred_grants()
    }

    fn set_commit_gate(&self, gate: CommitGate) {
        self.store.set_commit_gate(gate);
    }

    fn set_observer(&self, observer: Option<Rc<dyn TextStoreObserver>>) {
        self.store.set_observer(observer);
    }

    fn screen_moved(&self) {
        self.store.screen_moved();
    }
}

/// A field focused on `backend`'s input method path, or the outcome that
/// stands for the path's absence.
fn focused<B: BackendUnderTest>(
    backend: &mut B,
) -> Result<(Rc<InMemoryTextStore>, Rc<Cell<usize>>), RowOutcome> {
    let store = InMemoryTextStore::new(FIELD);
    let edits = Rc::new(Cell::new(0));
    let counted = Rc::new(Counted {
        store: Rc::clone(&store),
        edits: Rc::clone(&edits),
    });
    match backend.focus_text_store(counted) {
        TextInputPath::Focused => Ok((store, edits)),
        TextInputPath::Gap(gap) => Err(RowOutcome::Gap(gap)),
        TextInputPath::None(why) => Err(RowOutcome::NotApplicable(why)),
    }
}

/// What `store` holds now.
fn held(store: &InMemoryTextStore) -> Field {
    let selection = store.selection();
    Field {
        text: store.text(),
        composing: store.composition().map(|composition| {
            (
                composition.range.start().get(),
                composition.range.end().get(),
            )
        }),
        selection: (selection.anchor.get(), selection.active.get()),
    }
}

/// What a session of the field's store reads of it.
fn read(session: &dyn TextStoreRead) -> Field {
    let len = session.document_len();
    let all = Utf16Range::try_new(Utf16Offset::new(0), len).expect("BUG: a document starts at 0");
    let selection = session.selection();
    Field {
        text: session.text(all).unwrap_or_default(),
        composing: session.composition().map(|composition| {
            (
                composition.range.start().get(),
                composition.range.end().get(),
            )
        }),
        selection: (selection.anchor.get(), selection.active.get()),
    }
}

/// Run `steps` on a newly focused field: after each, the field holds what the
/// step names, and when `one_session` is set, the step was one edit of it.
fn compose<B: BackendUnderTest>(
    backend: &mut B,
    steps: &[(CompositionStep, Expected)],
    one_session: bool,
) -> Outcome {
    let (store, edits) = match focused(backend) {
        Ok(focused) => focused,
        Err(outcome) => return Ok(outcome),
    };
    for &(
        step,
        Expected {
            text,
            composing,
            caret,
        },
    ) in steps
    {
        let Some(reading) = backend.reading(Step::Composition(step)) else {
            return Err(format!("{step:?}: the focused path has no reading for it"));
        };
        let before = edits.get();
        let inputs = backend.decode(reading).count();
        if inputs != 0 {
            return Err(format!(
                "{step:?}: decoded into {inputs} inputs, not edits of the field"
            ));
        }
        let expected = Field {
            text: text.to_owned(),
            composing,
            selection: (caret, caret),
        };
        let got = held(&store);
        if got != expected {
            return Err(format!(
                "{step:?}: the field holds {got:?}, not {expected:?}"
            ));
        }
        let ran = edits.get() - before;
        if one_session && ran != 1 {
            return Err(format!(
                "{step:?}: reached the field in {ran} edits, not one"
            ));
        }
    }
    Ok(RowOutcome::Held)
}

/// A preedit that grows, a picked candidate and a commit leave the field as
/// they do on every path: the composing range covers the preedit, the caret
/// sits where the input method put it, and the commit ends the composition
/// with the committed text in its place.
pub(super) fn a_composition_reaches_the_field_in_order<B: BackendUnderTest>(
    backend: &mut B,
) -> Outcome {
    compose(backend, &COMMITTED, false)
}

/// A cancelled composition takes its text out of the field and leaves the
/// caret where the composition started.
pub(super) fn a_cancelled_composition_leaves_the_field_as_it_was<B: BackendUnderTest>(
    backend: &mut B,
) -> Outcome {
    compose(backend, &CANCELLED, false)
}

/// Each step of a composition is one read-write session that changes the
/// field's store: its owner hears of each as one change, never of a
/// half-made one.
pub(super) fn each_composition_step_is_one_edit_of_the_field<B: BackendUnderTest>(
    backend: &mut B,
) -> Outcome {
    let committed = compose(backend, &COMMITTED, true)?;
    if committed != RowOutcome::Held {
        return Ok(committed);
    }
    backend.restart();
    // The cancelled composition on a field of its own.
    compose(backend, &CANCELLED, true)
}
