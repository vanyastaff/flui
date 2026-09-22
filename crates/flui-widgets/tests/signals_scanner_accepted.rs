//! The **accepted** fixture of refusal trigger 24 (`scripts/check-signal-write-scope.sh
//! --self-test` scans this file): every legal shape the scanner must not
//! report, as real code that compiles and runs under the `signals` feature.
//!
//! - reads in `build` (`get`, `with`, `try_get`, `peek`) are the subscription path;
//! - one-argument `Cell::set(x)` is not a signal write;
//! - a write inside a callback closure *defined* in `build` runs later, from an event;
//! - creation and writes in `init_state` / `did_update_view` are outside the frame phases.

#![cfg(feature = "signals")]

use std::cell::Cell;
use std::rc::Rc;

use crate::common::{lay_out, loose};
use flui_view::{Reactive, Signal};
use flui_widgets::prelude::*;
use flui_widgets::{Column, SizedBox};

#[derive(Clone, StatefulView)]
struct Counter {
    seed: u32,
    tapped: Rc<Cell<u32>>,
}

struct CounterState {
    view: Counter,
    count: Option<Signal<u32>>,
    other: Option<Signal<u32>>,
    cell: Cell<u32>,
    flag: Cell<bool>,
}

impl StatefulView for Counter {
    type State = CounterState;
    fn create_state(&self) -> Self::State {
        CounterState {
            view: self.clone(),
            count: None,
            other: None,
            cell: Cell::new(0),
            flag: Cell::new(false),
        }
    }
}

impl ViewState<Counter> for CounterState {
    fn init_state(&mut self, cx: &dyn BuildContext) {
        // Creation and a write from a lifecycle hook: legal.
        let count = cx.signal(self.view.seed);
        let other = cx.signal(0u32);
        other
            .set(&cx.reactive(), self.view.seed)
            .expect("fresh slot");
        self.count = Some(count);
        self.other = Some(other);
    }

    fn build(&self, _v: &Counter, cx: &dyn BuildContext) -> impl IntoView {
        let count = self.count.expect("init_state ran");
        let other = self.other.expect("init_state ran");
        // Reads in build: the subscription path.
        let n = count.get(cx);
        let t = count.with(cx, |c| *c * 2);
        let p = other.try_get(cx).expect("alive");
        let q = count.peek(&cx.reactive(), |c| *c).expect("alive");
        // One-argument Cell::set is not a signal write.
        self.cell.set(n);
        self.flag.set(true);
        // Writes inside callback closures defined here run later, from an event.
        let tapped = Rc::clone(&self.view.tapped);
        let tap: Box<dyn Fn(&Reactive)> =
            Box::new(move |r: &Reactive| count.update(r, |c| *c += 1).map(|_| ()).unwrap_or(()));
        let _hold = tap;
        let _tapped = tapped;
        let _later: Box<dyn Fn(&Reactive)> = Box::new(move |r: &Reactive| {
            count.update(r, |c| *c -= 1).map(|_| ()).unwrap_or(());
            other.set(r, 0).unwrap_or(());
        });
        Column::new(vec![
            flui_view::ViewExt::boxed(SizedBox::square(1.0 + n as f32)),
            flui_view::ViewExt::boxed(SizedBox::square(1.0 + t as f32)),
            flui_view::ViewExt::boxed(SizedBox::square(1.0 + p as f32)),
            flui_view::ViewExt::boxed(SizedBox::square(1.0 + q as f32)),
        ])
    }

    fn did_update_view(&mut self, _old: &Counter, new: &Counter) {
        // A write from a lifecycle hook: legal (the runtime allows it too —
        // no build is running here).
        if let Some(count) = self.count {
            let _ = new.seed;
            let _ = count;
        }
    }
}

#[test]
fn the_accepted_shapes_compile_and_run() {
    let tapped = Rc::new(Cell::new(0));
    let mut laid = lay_out(
        Counter {
            seed: 3,
            tapped: Rc::clone(&tapped),
        },
        loose(1000.0),
    );
    let root = laid.current_root();
    assert_eq!(laid.children(root).len(), 4);
    laid.tick();
    assert_eq!(laid.children(root).len(), 4);
}
