//! [`SharedSignal`] — a value a test and the view that reads it both hold.
//!
//! A test builds its views around a model it also drives from outside:
//! the test sets an input, pumps, and asserts what the view rebuilt from. The
//! model is a `Signal` (ADR-0074); a signal belongs to an element's graph, so
//! the reading view creates it in `init_state` and the test reaches it through
//! the writer source the view took there.
//!
//! ```rust,ignore
//! let shown = SharedSignal::new(true);
//! // in the view's state:
//! //   init_state(&mut self, ctx)  { self.shown.bind(ctx); }
//! //   build(&self, _, ctx)        { if self.shown.get(ctx) { .. } }
//! let mut app = lay_out(view(shown.clone()), tight(100.0, 100.0));
//! shown.set(false);
//! app.pump();
//! ```

use std::cell::RefCell;
use std::rc::Rc;

use flui_view::prelude::*;
use flui_view::{BuildContext, EventCx, LifecycleContext, SignalError};

struct Bound<T: 'static> {
    signal: Signal<T>,
    writer: WriterSource,
}

struct Shared<T: 'static> {
    /// The value before a view has bound it: what `bind` seeds the signal with.
    initial: RefCell<T>,
    bound: RefCell<Option<Bound<T>>>,
}

/// A test-owned value held in a signal its reading view creates.
pub struct SharedSignal<T: 'static> {
    shared: Rc<Shared<T>>,
}

impl<T: 'static> Clone for SharedSignal<T> {
    fn clone(&self) -> Self {
        Self {
            shared: Rc::clone(&self.shared),
        }
    }
}

impl<T: std::fmt::Debug + 'static> std::fmt::Debug for SharedSignal<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedSignal")
            .field("bound", &self.shared.bound.borrow().is_some())
            .finish_non_exhaustive()
    }
}

impl<T: Clone + 'static> SharedSignal<T> {
    /// A value that seeds the signal the first reading view creates.
    #[must_use]
    pub fn new(initial: T) -> Self {
        Self {
            shared: Rc::new(Shared {
                initial: RefCell::new(initial),
                bound: RefCell::new(None),
            }),
        }
    }

    /// Create the signal in the calling element's graph. Call from
    /// `init_state`; a later `bind` (a remount) replaces the signal and seeds
    /// it with the value last set before the first bind.
    pub fn bind(&self, ctx: &dyn LifecycleContext) {
        let signal = ctx.signal(self.shared.initial.borrow().clone());
        *self.shared.bound.borrow_mut() = Some(Bound {
            signal,
            writer: ctx.writer_source(),
        });
    }

    /// The signal, for a view that passes it to its children.
    ///
    /// # Panics
    ///
    /// Before a view has bound it.
    #[must_use]
    pub fn signal(&self) -> Signal<T> {
        self.shared
            .bound
            .borrow()
            .as_ref()
            .expect("the reading view bound the signal in init_state")
            .signal
    }

    /// Read in `build`: the element depends on the value.
    pub fn with<R>(&self, ctx: &dyn BuildContext, f: impl FnMut(&T) -> R) -> R {
        self.signal().with(ctx, f)
    }

    /// The value, read in `build`.
    #[must_use]
    pub fn get(&self, ctx: &dyn BuildContext) -> T {
        self.with(ctx, T::clone)
    }

    /// Read outside any frame, without depending on the value.
    pub fn peek<R>(&self, mut f: impl FnMut(&T) -> R) -> R {
        let bound = self.shared.bound.borrow();
        match bound.as_ref() {
            Some(bound) => bound
                .writer
                .write(|cx| bound.signal.peek(cx, &mut f))
                .expect("the bound signal is alive while its view is mounted"),
            None => f(&self.shared.initial.borrow()),
        }
    }

    /// The value, read outside any frame.
    #[must_use]
    pub fn value(&self) -> T {
        self.peek(T::clone)
    }

    /// Change the value from the test, outside any frame. Before a view binds
    /// it, this changes the seed; after, it writes the signal and schedules
    /// the readers.
    pub fn update<R>(&self, mut f: impl FnMut(&mut T) -> R) -> R {
        let bound = self.shared.bound.borrow();
        match bound.as_ref() {
            Some(bound) => bound
                .writer
                .write(|cx| bound.signal.update(cx, &mut f))
                .expect("the bound signal is alive while its view is mounted"),
            None => f(&mut self.shared.initial.borrow_mut()),
        }
    }

    /// [`update`](Self::update) with a new value.
    pub fn set(&self, value: T) {
        let mut value = Some(value);
        self.update(|current| {
            *current = value.take().expect("BUG: the closure runs once");
        });
    }

    /// Change the value from an event callback that holds the caller's `cx`.
    ///
    /// # Errors
    ///
    /// The signal's error when it cannot be written.
    ///
    /// # Panics
    ///
    /// Before a view has bound it.
    pub fn update_in<R>(
        &self,
        cx: &mut EventCx<'_>,
        f: impl FnMut(&mut T) -> R,
    ) -> Result<R, SignalError> {
        self.signal().update(cx, f)
    }

    /// [`update_in`](Self::update_in) with a new value.
    ///
    /// # Errors
    ///
    /// The signal's error when it cannot be written.
    ///
    /// # Panics
    ///
    /// Before a view has bound it.
    pub fn set_in(&self, cx: &mut EventCx<'_>, value: T) -> Result<(), SignalError> {
        let mut value = Some(value);
        self.update_in(cx, |current| {
            *current = value.take().expect("BUG: the closure runs once");
        })
    }
}
