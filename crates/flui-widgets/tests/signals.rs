//! ADR-0074: realm-scoped signals driven through the real widget pipeline
//! (`lay_out` mounts a tree in a `HeadlessBinding`; `tick` pumps one frame
//! without dirtying anything itself).
//!
//! Every test counts `build` calls per element, because the claim under test
//! is *which elements rebuild*, not what they render.

#![cfg(feature = "signals")]

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::common::{lay_out, loose, size};
use flui_view::{Signal, SignalError};
use flui_widgets::prelude::*;
use flui_widgets::{Column, SizedBox};

type Builds = Arc<AtomicU32>;

fn builds() -> Builds {
    Arc::new(AtomicU32::new(0))
}

fn count(b: &Builds) -> u32 {
    b.load(Ordering::Relaxed)
}

/// Reads one `Signal<u32>` and renders a square of that side.
#[derive(Clone, StatefulView)]
struct Reader {
    sig: Signal<u32>,
    builds: Builds,
}

struct ReaderState {
    sig: Signal<u32>,
    builds: Builds,
}

impl StatefulView for Reader {
    type State = ReaderState;
    fn create_state(&self) -> Self::State {
        ReaderState {
            sig: self.sig,
            builds: Arc::clone(&self.builds),
        }
    }
}

impl ViewState<Reader> for ReaderState {
    fn build(&self, _view: &Reader, ctx: &dyn BuildContext) -> impl IntoView {
        self.builds.fetch_add(1, Ordering::Relaxed);
        SizedBox::square(self.sig.get(ctx) as f32)
    }
}

/// Reads `sig` only while `flag` is true.
#[derive(Clone, StatefulView)]
struct ConditionalReader {
    flag: Signal<bool>,
    sig: Signal<u32>,
    builds: Builds,
}

struct ConditionalReaderState {
    view: ConditionalReader,
}

impl StatefulView for ConditionalReader {
    type State = ConditionalReaderState;
    fn create_state(&self) -> Self::State {
        ConditionalReaderState { view: self.clone() }
    }
}

impl ViewState<ConditionalReader> for ConditionalReaderState {
    fn build(&self, _view: &ConditionalReader, ctx: &dyn BuildContext) -> impl IntoView {
        self.view.builds.fetch_add(1, Ordering::Relaxed);
        let side = if self.view.flag.get(ctx) {
            self.view.sig.get(ctx)
        } else {
            1
        };
        SizedBox::square(side as f32)
    }
}

/// Creates a signal owned by its own element in `init_state` (the canonical
/// `cx.signal(..)` idiom) and publishes the handle so the test can probe it
/// after the element unmounts.
#[derive(Clone, StatefulView)]
struct SignalOwner {
    published: Rc<Cell<Option<Signal<u32>>>>,
}

struct SignalOwnerState {
    published: Rc<Cell<Option<Signal<u32>>>>,
    own: Option<Signal<u32>>,
}

impl StatefulView for SignalOwner {
    type State = SignalOwnerState;
    fn create_state(&self) -> Self::State {
        SignalOwnerState {
            published: Rc::clone(&self.published),
            own: None,
        }
    }
}

impl ViewState<SignalOwner> for SignalOwnerState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        let own = ctx.signal(7u32);
        self.published.set(Some(own));
        self.own = Some(own);
    }

    fn build(&self, _view: &SignalOwner, ctx: &dyn BuildContext) -> impl IntoView {
        SizedBox::square(self.own.expect("init_state ran").get(ctx) as f32)
    }
}

/// Root that shows a [`SignalOwner`] or a plain box.
#[derive(Clone, Debug, StatelessView)]
struct OwnerSwitch {
    show: bool,
    published: Rc<Cell<Option<Signal<u32>>>>,
}

impl StatelessView for OwnerSwitch {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        use flui_view::ViewExt;
        if self.show {
            SignalOwner {
                published: Rc::clone(&self.published),
            }
            .boxed()
        } else {
            SizedBox::square(2.0).boxed()
        }
    }
}

/// Reads a handle that may be stale through `try_get`, rendering the outcome
/// as a size (1 = error, value otherwise).
#[derive(Clone, StatefulView)]
struct TolerantReader {
    sig: Signal<u32>,
    outcome: Rc<Cell<Option<Result<u32, SignalError>>>>,
}

struct TolerantReaderState {
    view: TolerantReader,
}

impl StatefulView for TolerantReader {
    type State = TolerantReaderState;
    fn create_state(&self) -> Self::State {
        TolerantReaderState { view: self.clone() }
    }
}

impl ViewState<TolerantReader> for TolerantReaderState {
    fn build(&self, _view: &TolerantReader, ctx: &dyn BuildContext) -> impl IntoView {
        let outcome = self.view.sig.try_get(ctx);
        let side = outcome.as_ref().map_or(1.0, |v| *v as f32);
        self.view.outcome.set(Some(outcome));
        SizedBox::square(side)
    }
}

/// Tries to write and to create a signal from inside `build`: the runtime
/// refuses both.
#[derive(Clone, StatefulView)]
struct WriterInBuild {
    sig: Signal<u32>,
    outcome: Rc<Cell<Option<Result<(), SignalError>>>>,
    created: Rc<Cell<Option<Result<(), SignalError>>>>,
}

struct WriterInBuildState {
    view: WriterInBuild,
}

impl StatefulView for WriterInBuild {
    type State = WriterInBuildState;
    fn create_state(&self) -> Self::State {
        WriterInBuildState { view: self.clone() }
    }
}

impl ViewState<WriterInBuild> for WriterInBuildState {
    fn build(&self, _view: &WriterInBuild, ctx: &dyn BuildContext) -> impl IntoView {
        let r = ctx.reactive();
        // This test proves the run-time refusal of a write in build.
        self.view.outcome.set(Some(self.view.sig.set(&r, 99)));
        // Same for a slot creation in build.
        self.view.created.set(Some(r.try_signal(0u8).map(|_| ())));
        SizedBox::square(1.0)
    }
}

#[test]
fn writing_a_signal_rebuilds_exactly_its_readers() {
    // Mount a placeholder first: the tree's own graph is the one the build
    // contexts hand out, so the signals must be minted there.
    let mut laid = lay_out(SizedBox::square(1.0), loose(1000.0));
    let r = laid.build_owner_mut().reactive().clone();
    let a = r.signal(10u32);
    let b = r.signal(20u32);
    let (builds_a, builds_b) = (builds(), builds());
    laid.pump_widget(Column::new((
        Reader {
            sig: a,
            builds: Arc::clone(&builds_a),
        },
        Reader {
            sig: b,
            builds: Arc::clone(&builds_b),
        },
    )));
    let (before_a, before_b) = (count(&builds_a), count(&builds_b));
    assert_eq!(r.readers_of(a.slot()).len(), 1, "one element reads a");
    assert_eq!(r.readers_of(b.slot()).len(), 1, "one element reads b");

    a.set(&r, 30).unwrap();
    laid.tick();

    assert_eq!(
        count(&builds_a),
        before_a + 1,
        "the reader of a rebuilt once"
    );
    assert_eq!(
        count(&builds_b),
        before_b,
        "the reader of b did not rebuild"
    );
    let root = laid.current_root();
    assert_eq!(laid.size(laid.child(root, 0)), size(30.0, 30.0));
    assert_eq!(laid.size(laid.child(root, 1)), size(20.0, 20.0));

    // Equality is opt-in: an equal plain write still rebuilds the reader.
    a.set(&r, 30).unwrap();
    laid.tick();
    assert_eq!(
        count(&builds_a),
        before_a + 2,
        "a plain equal write marks readers"
    );

    assert!(!a.set_if_changed(&r, 30).unwrap());
    laid.tick();
    assert_eq!(
        count(&builds_a),
        before_a + 2,
        "set_if_changed on an equal value marks nobody"
    );
}

#[test]
fn a_rebuild_re_derives_the_read_set_so_a_dropped_read_stops_depending() {
    let builds = builds();
    let mut laid = lay_out(SizedBox::square(1.0), loose(1000.0));
    let r = laid.build_owner_mut().reactive().clone();
    let flag = r.signal(true);
    let sig = r.signal(5u32);
    laid.pump_widget(ConditionalReader {
        flag,
        sig,
        builds: Arc::clone(&builds),
    });
    assert_eq!(r.readers_of(sig.slot()).len(), 1);
    let base = count(&builds);

    flag.set(&r, false).unwrap();
    laid.tick();
    assert_eq!(
        count(&builds),
        base + 1,
        "flipping the flag rebuilds the reader"
    );
    assert!(
        r.readers_of(sig.slot()).is_empty(),
        "the rebuild no longer read sig, so the element is no longer its reader"
    );

    sig.set(&r, 6).unwrap();
    laid.tick();
    assert_eq!(count(&builds), base + 1, "writing sig now rebuilds nothing");
}

#[test]
fn a_signal_created_in_init_state_is_released_when_its_element_unmounts() {
    let published = Rc::new(Cell::new(None));
    let mut laid = lay_out(
        OwnerSwitch {
            show: true,
            published: Rc::clone(&published),
        },
        loose(1000.0),
    );
    let r = laid.build_owner_mut().reactive().clone();
    let own = published.get().expect("init_state published the handle");
    assert_eq!(own.peek(&r, |v| *v), Ok(7));
    assert_eq!(laid.size(laid.current_root()), size(7.0, 7.0));
    assert_eq!(r.live_slot_count(), 1);

    laid.pump_widget(OwnerSwitch {
        show: false,
        published: Rc::clone(&published),
    });

    assert!(
        matches!(own.peek(&r, |v| *v), Err(SignalError::Released { .. })),
        "the owning element unmounted, so its signal is released"
    );
    assert!(r.readers_of(own.slot()).is_empty());
    assert_eq!(r.live_slot_count(), 0, "no slot leaked");
}

#[test]
fn a_stale_handle_read_in_build_is_a_typed_error_through_try_get() {
    let mut laid = lay_out(SizedBox::square(1.0), loose(1000.0));
    let r = laid.build_owner_mut().reactive().clone();
    let sig = r.signal(4u32);
    let outcome = Rc::new(Cell::new(None));
    laid.pump_widget(TolerantReader {
        sig,
        outcome: Rc::clone(&outcome),
    });
    assert_eq!(outcome.get(), Some(Ok(4)));
    assert_eq!(laid.size(laid.current_root()), size(4.0, 4.0));

    // The handle goes stale while the reader stays mounted (eviction of the
    // owner, a popped route): the next build sees a typed error, not a panic.
    r.release(sig.slot());
    laid.pump();

    assert!(matches!(
        outcome.get(),
        Some(Err(SignalError::Released { .. }))
    ));
    assert_eq!(laid.size(laid.current_root()), size(1.0, 1.0));
}

#[test]
fn writes_and_creations_inside_build_are_refused_by_the_runtime() {
    let mut laid = lay_out(SizedBox::square(1.0), loose(1000.0));
    let r = laid.build_owner_mut().reactive().clone();
    let sig = r.signal(1u32);
    let outcome = Rc::new(Cell::new(None));
    let created = Rc::new(Cell::new(None));
    laid.pump_widget(WriterInBuild {
        sig,
        outcome: Rc::clone(&outcome),
        created: Rc::clone(&created),
    });

    assert!(
        matches!(
            outcome.get(),
            Some(Err(SignalError::WrittenDuringBuild { .. }))
        ),
        "a write from build is refused"
    );
    assert!(
        matches!(
            created.get(),
            Some(Err(SignalError::CreatedDuringBuild { .. }))
        ),
        "a creation from build is refused"
    );
    assert_eq!(
        sig.peek(&r, |v| *v),
        Ok(1),
        "the refused write changed nothing"
    );
    assert_eq!(
        r.live_slot_count(),
        1,
        "the refused creation leaked nothing"
    );

    sig.set(&r, 2).unwrap();
    assert_eq!(sig.peek(&r, |v| *v), Ok(2));
}
