//! ADR-0074 phase 2: realm-scoped signals driven through the real widget
//! pipeline (`lay_out` mounts a tree in a `HeadlessBinding`; `tick` pumps one
//! frame without dirtying anything itself).
//!
//! Every test counts `build` calls per element, because the claim under test
//! is *which elements rebuild*, not what they render.

#![cfg(feature = "signals")]

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::common::{lay_out, loose, size};
use flui_view::{Computed, Effect, Reactive, Signal, SignalError};
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

/// Reads a `Computed<usize>`.
#[derive(Clone, StatefulView)]
struct MemoReader {
    memo: Computed<usize>,
    builds: Builds,
}

struct MemoReaderState {
    view: MemoReader,
}

impl StatefulView for MemoReader {
    type State = MemoReaderState;
    fn create_state(&self) -> Self::State {
        MemoReaderState { view: self.clone() }
    }
}

impl ViewState<MemoReader> for MemoReaderState {
    fn build(&self, _view: &MemoReader, ctx: &dyn BuildContext) -> impl IntoView {
        self.view.builds.fetch_add(1, Ordering::Relaxed);
        SizedBox::square(1.0 + self.view.memo.get(ctx) as f32)
    }
}

/// Owns an effect: whenever `src` changes, writes `src * 10` into `dst`.
#[derive(Clone, StatefulView)]
struct EffectHost {
    src: Signal<u32>,
    dst: Signal<u32>,
}

struct EffectHostState {
    src: Signal<u32>,
    dst: Signal<u32>,
    effect: Option<Effect>,
}

impl StatefulView for EffectHost {
    type State = EffectHostState;
    fn create_state(&self) -> Self::State {
        EffectHostState {
            src: self.src,
            dst: self.dst,
            effect: None,
        }
    }
}

impl ViewState<EffectHost> for EffectHostState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        let (src, dst) = (self.src, self.dst);
        self.effect = Some(ctx.reactive().effect(move |r: &Reactive| {
            let v = src.track(r, |v| *v).expect("src is alive");
            dst.set(r, v * 10).expect("dst is alive");
        }));
    }

    fn build(&self, _view: &EffectHost, _ctx: &dyn BuildContext) -> impl IntoView {
        SizedBox::square(1.0)
    }
}

/// Creates a signal owned by its own element in `init_state` and publishes
/// the handle so the test can probe it after the element unmounts.
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
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        let own = ctx.reactive().signal_owned_by(ctx.element_id(), 7u32);
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
fn a_memo_rebuilds_its_readers_only_when_its_output_changes() {
    let mut laid = lay_out(SizedBox::square(1.0), loose(1000.0));
    let r = laid.build_owner_mut().reactive().clone();
    let fields: [Signal<u32>; 3] = std::array::from_fn(|_| r.signal(0u32));
    let empty_count = r.computed(move |r: &Reactive| {
        fields
            .iter()
            .filter(|f| f.track(r, |v| *v == 0).expect("field is alive"))
            .count()
    });
    let (b0, b1, b_save) = (builds(), builds(), builds());
    laid.pump_widget(Column::new((
        Reader {
            sig: fields[0],
            builds: Arc::clone(&b0),
        },
        Reader {
            sig: fields[1],
            builds: Arc::clone(&b1),
        },
        MemoReader {
            memo: empty_count,
            builds: Arc::clone(&b_save),
        },
    )));
    let (base0, base1, base_save) = (count(&b0), count(&b1), count(&b_save));
    let root = laid.current_root();
    assert_eq!(
        laid.size(laid.child(root, 2)),
        size(4.0, 4.0),
        "3 empty fields → 1 + 3"
    );

    // Field 0 goes 0 → 5: the memo flips 3 → 2, so field 0's reader AND the
    // memo's reader rebuild; field 1's reader does not.
    fields[0].set(&r, 5).unwrap();
    laid.tick();
    assert_eq!(count(&b0), base0 + 1);
    assert_eq!(count(&b1), base1);
    assert_eq!(count(&b_save), base_save + 1);
    assert_eq!(laid.size(laid.child(root, 2)), size(3.0, 3.0));

    // Field 0 goes 5 → 6: the memo recomputes to the same 2, so only field 0's
    // reader rebuilds.
    fields[0].set(&r, 6).unwrap();
    laid.tick();
    assert_eq!(count(&b0), base0 + 2);
    assert_eq!(count(&b1), base1);
    assert_eq!(
        count(&b_save),
        base_save + 1,
        "an unchanged memo output marks nobody"
    );
}

#[test]
fn effects_run_after_build_and_their_writes_land_in_the_next_frame() {
    let mut laid = lay_out(SizedBox::square(1.0), loose(1000.0));
    let r = laid.build_owner_mut().reactive().clone();
    let src = r.signal(1u32);
    let dst = r.signal(0u32);
    let dst_builds = builds();
    laid.pump_widget(Column::new((
        EffectHost { src, dst },
        Reader {
            sig: dst,
            builds: Arc::clone(&dst_builds),
        },
    )));
    // The effect registered in `init_state` ran in that frame's effects phase
    // (after the build that read dst = 0) and wrote dst = 10; the reader is
    // queued for the next frame.
    assert_eq!(dst.peek(&r, |v| *v).unwrap(), 10);
    laid.tick();
    let base = count(&dst_builds);
    let root = laid.current_root();
    assert_eq!(laid.size(laid.child(root, 1)), size(10.0, 10.0));

    src.set(&r, 2).unwrap();
    laid.tick();
    assert_eq!(
        dst.peek(&r, |v| *v).unwrap(),
        20,
        "the effect ran in this frame"
    );
    assert_eq!(
        count(&dst_builds),
        base,
        "the reader of dst was marked by the effect, after this frame's build phase"
    );
    assert_eq!(laid.size(laid.child(root, 1)), size(10.0, 10.0));

    laid.tick();
    assert_eq!(
        count(&dst_builds),
        base + 1,
        "…and rebuilds in the next frame"
    );
    assert_eq!(laid.size(laid.child(root, 1)), size(20.0, 20.0));
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

    laid.pump_widget(OwnerSwitch {
        show: false,
        published: Rc::clone(&published),
    });

    assert!(
        matches!(own.peek(&r, |v| *v), Err(SignalError::Released { .. })),
        "the owning element unmounted, so its signal is released"
    );
    assert!(r.readers_of(own.slot()).is_empty());
}
