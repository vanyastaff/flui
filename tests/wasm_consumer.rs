//! An application on wasm32 keeps `Rc` state in owner-local callbacks: a
//! `ChangeNotifier` listener and a post-frame callback capture an
//! `Rc<Cell<_>>`, and nothing asks either to be `Send` (ADR-0086). `cargo
//! xtask wasm-check` compiles this file for `wasm32-unknown-unknown`, so a
//! bound that creeps back onto either callback breaks the web build here.

use std::any::Any;
use std::cell::Cell;
use std::rc::Rc;

use flui::foundation::{ChangeNotifier, Listenable as _};
use flui::prelude::*;

/// Counts its notifier's changes and its post-frame callbacks in one shared
/// `Rc<Cell<u32>>`.
#[derive(Clone, StatefulView)]
struct Counted {
    notifier: ChangeNotifier,
    count: Rc<Cell<u32>>,
}

struct CountedState {
    view: Counted,
    listener: Option<Box<dyn Any>>,
}

impl StatefulView for Counted {
    type State = CountedState;

    fn create_state(&self) -> Self::State {
        CountedState {
            view: self.clone(),
            listener: None,
        }
    }
}

impl ViewState<Counted> for CountedState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.listener = Some(Box::new(listen(&self.view.notifier, &self.view.count)));
        let count = Rc::clone(&self.view.count);
        ctx.post_frame_handle()
            .expect("a mounted presentation has a post-frame lane")
            .schedule(move |_timing| count.set(count.get() + 10))
            .expect("the lane admits the callback");
    }

    fn build(&self, _view: &Counted, _ctx: &dyn BuildContext) -> impl IntoView {
        SizedBox::new(10.0, 10.0)
    }
}

/// Registers a listener that captures `count` on `notifier`.
fn listen(notifier: &ChangeNotifier, count: &Rc<Cell<u32>>) -> impl Any {
    let count = Rc::clone(count);
    notifier.add_listener(Rc::new(move || count.set(count.get() + 1)))
}

#[test]
fn wasm_consumer_captures_rc_in_listener_and_post_frame() {
    let notifier = ChangeNotifier::new();
    let count = Rc::new(Cell::new(0));
    let _listener = listen(&notifier, &count);
    notifier.notify_listeners();
    assert_eq!(count.get(), 1, "the listener ran on the owner thread");
    // The view's `init_state` holds the post-frame half; compiling it on
    // wasm32 is that half's check.
    let _ = Counted { notifier, count };
}
