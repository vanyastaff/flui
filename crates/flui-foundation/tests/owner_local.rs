//! The owner thread's seams carry no thread bound (ADR-0323 §4): a clock and a tree observer
//! may keep `Rc` and `Cell` state, and are held as `Rc<dyn …>`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use flui_foundation::ElementId;
use flui_foundation::MonotonicClock;
use flui_foundation::generation::Generation;
use flui_foundation::observe::{ElementUnmounted, TreeObserver};

#[derive(Debug)]
struct SharedClock {
    now: Rc<Cell<Instant>>,
}

impl MonotonicClock for SharedClock {
    fn now(&self) -> Instant {
        self.now.get()
    }
}

#[derive(Default)]
struct Recorder {
    unmounted: RefCell<Vec<ElementId>>,
}

impl TreeObserver for Recorder {
    fn element_unmounted(&self, event: &ElementUnmounted) {
        self.unmounted.borrow_mut().push(event.element);
    }
}

#[test]
fn a_clock_and_an_observer_hold_owner_local_state() {
    let start = Instant::now();
    let now = Rc::new(Cell::new(start));
    let clock: Rc<dyn MonotonicClock> = Rc::new(SharedClock {
        now: Rc::clone(&now),
    });
    now.set(start + Duration::from_millis(16));
    assert_eq!(clock.now() - start, Duration::from_millis(16));

    let recorder = Rc::new(Recorder::default());
    let observer: Rc<dyn TreeObserver> = Rc::clone(&recorder) as Rc<dyn TreeObserver>;
    let element = ElementId::new_gen(0, Generation::FIRST);
    observer.element_unmounted(&ElementUnmounted::new(element));
    assert_eq!(*recorder.unmounted.borrow(), vec![element]);
}
