//! `LifecycleContext::timer` through the testing kit: a task sleeps on the
//! UI runtime's virtual clock, and the kit's pumps advance it.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use flui_testing::widgets::{lay_out, tight};
use flui_view::prelude::*;
use flui_view::{TaskToken, TimerHandle};

/// Sleeps `after` from `init_state`, then sets `done`; `dropped` is set when
/// the task's future is dropped, finished or not.
#[derive(Clone, StatefulView)]
struct Sleeper {
    after: Duration,
    done: Rc<Cell<bool>>,
    dropped: Rc<Cell<bool>>,
}

struct SleeperState {
    view: Sleeper,
    task: Option<TaskToken>,
}

/// Sets its cell when dropped.
struct SetOnDrop(Rc<Cell<bool>>);

impl Drop for SetOnDrop {
    fn drop(&mut self) {
        self.0.set(true);
    }
}

impl StatefulView for Sleeper {
    type State = SleeperState;

    fn create_state(&self) -> Self::State {
        SleeperState {
            view: self.clone(),
            task: None,
        }
    }
}

impl ViewState<Sleeper> for SleeperState {
    fn init_state(&mut self, cx: &dyn LifecycleContext) {
        let view = &self.view;
        let timer: TimerHandle = cx.timer().expect("a hosted tree has a timer");
        let sleep = timer.sleep(view.after);
        let done = Rc::clone(&view.done);
        let guard = SetOnDrop(Rc::clone(&view.dropped));
        self.task = cx.async_driver().map(|driver| {
            driver.spawn_local(Box::pin(async move {
                let _guard = guard;
                sleep.await;
                done.set(true);
            }))
        });
    }

    fn build(&self, _view: &Sleeper, _cx: &dyn BuildContext) -> impl IntoView {
        flui_widgets::SizedBox::new(10.0, 10.0)
    }
}

fn sleeper(after: Duration) -> (Sleeper, Rc<Cell<bool>>, Rc<Cell<bool>>) {
    let (done, dropped) = (Rc::new(Cell::new(false)), Rc::new(Cell::new(false)));
    let view = Sleeper {
        after,
        done: Rc::clone(&done),
        dropped: Rc::clone(&dropped),
    };
    (view, done, dropped)
}

/// A sleep resolves when the kit's clock reaches its deadline, not a
/// millisecond before.
#[test]
fn a_task_sleep_follows_the_kit_clock() {
    let (view, done, _dropped) = sleeper(Duration::from_millis(50));
    let mut tree = lay_out(view, tight(10.0, 10.0));
    tree.pump_for(Duration::from_millis(49));
    assert!(!done.get(), "49 ms: not yet");
    tree.pump_for(Duration::from_millis(1));
    assert!(done.get(), "50 ms: resolved");
}

/// Tearing the tree down while a task sleeps drops the task and its sleep;
/// nothing is left armed or woken.
#[test]
fn teardown_drops_a_sleeping_task() {
    let (view, done, dropped) = sleeper(Duration::from_secs(60));
    let mut tree = lay_out(view, tight(10.0, 10.0));
    tree.pump_for(Duration::from_millis(1));
    assert!(!dropped.get(), "the task sleeps");
    drop(tree);
    assert!(dropped.get(), "teardown dropped the sleeping task");
    assert!(!done.get(), "and never resolved its sleep");
}
