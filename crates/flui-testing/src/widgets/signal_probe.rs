//! [`SignalProbe`] — the state an event-callback test writes into (ADR-0086).
//!
//! A callback test builds its widget inside the probe, wires the widget's
//! callback to one of the probe's signals, drives an event, and then asserts
//! the value the write left and that the probe (the signal's reader) rebuilt:
//!
//! ```rust,ignore
//! let probe = SignalProbe::new(|ProbeSignals { count, .. }| {
//!     GestureDetector::new()
//!         .on_tap(move |cx| count.update(cx, |n| *n += 1))
//!         .child(ColoredBox::new(Color::WHITE))
//! });
//! let mut app = lay_out(probe.view(), tight(100.0, 100.0));
//! app.dispatch_pointer_down(50.0, 50.0);
//! app.dispatch_pointer_up(50.0, 50.0);
//! app.pump();
//! assert_eq!(probe.value(), Ok(1));
//! assert_eq!(probe.reads(), [0, 1]);
//! ```

use std::cell::RefCell;
use std::rc::Rc;

use flui_view::prelude::*;
use flui_view::{BoxedView, Reactive, SignalError};

/// The signals a [`SignalProbe`] hands its builder.
#[derive(Clone, Copy, Debug)]
pub struct ProbeSignals {
    /// A live signal, read by the probe in every `build`.
    pub count: Signal<u32>,
    /// A signal whose slot is already released, as after its owning element
    /// unmounted: every write to it is refused with
    /// [`SignalError::Released`], which a callback reports.
    pub released: Signal<u32>,
}

/// What the probe has seen: the signal and graph once mounted, and every
/// value it read.
#[derive(Default)]
struct Seen {
    count: RefCell<Option<(Signal<u32>, Reactive)>>,
    reads: RefCell<Vec<u32>>,
    writer: RefCell<Option<WriterSource>>,
}

type Builder = Rc<dyn Fn(ProbeSignals) -> BoxedView>;

/// A test root that owns the signals an event callback writes, reads the
/// live one in `build`, and builds the widget under test below itself.
#[derive(Clone)]
pub struct SignalProbe {
    builder: Builder,
    seen: Rc<Seen>,
}

impl std::fmt::Debug for SignalProbe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignalProbe")
            .field("reads", &self.seen.reads.borrow())
            .finish_non_exhaustive()
    }
}

impl SignalProbe {
    /// A probe that builds `builder(signals)` below itself.
    pub fn new<V, B>(builder: B) -> Self
    where
        V: IntoView,
        B: Fn(ProbeSignals) -> V + 'static,
    {
        Self {
            builder: Rc::new(move |signals| builder(signals).into_view().boxed()),
            seen: Rc::new(Seen::default()),
        }
    }

    /// The root view to mount. Every clone shares what the probe has seen.
    #[must_use]
    pub fn view(&self) -> ProbeRoot {
        ProbeRoot {
            probe: self.clone(),
        }
    }

    /// The live signal's current value, read without a frame.
    ///
    /// # Errors
    ///
    /// The signal's error when it cannot be read.
    ///
    /// # Panics
    ///
    /// Before the probe has built once.
    pub fn value(&self) -> Result<u32, SignalError> {
        let count = self.seen.count.borrow();
        let (signal, graph) = count.as_ref().expect("the probe built before it is read");
        signal.peek(graph, |v| *v)
    }

    /// Every value the probe's `build` read, oldest first: one entry per
    /// build, so a write that rebuilt the reader adds exactly one.
    #[must_use]
    pub fn reads(&self) -> Vec<u32> {
        self.seen.reads.borrow().clone()
    }

    /// Run `f` with an [`EventCx`] opened from the probe's writer source,
    /// as an event callback would: for a test that calls an API taking the
    /// caller's `cx` (`FormHandle::save`, `FormFieldHandle::reset`) outside
    /// any widget's callback.
    ///
    /// # Panics
    ///
    /// Before the probe has built once.
    pub fn write<R>(&self, f: impl FnOnce(&mut EventCx<'_>) -> R) -> R {
        let writer = self
            .seen
            .writer
            .borrow()
            .clone()
            .expect("the probe built before it writes");
        writer.write(f)
    }
}

/// The mounted form of a [`SignalProbe`].
#[derive(Clone, StatefulView)]
pub struct ProbeRoot {
    probe: SignalProbe,
}

impl std::fmt::Debug for ProbeRoot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProbeRoot")
            .field("probe", &self.probe)
            .finish()
    }
}

/// The state of a [`ProbeRoot`]: the two signals and the writer source,
/// acquired in `init_state`.
#[derive(Debug, Default)]
pub struct ProbeRootState {
    count: Signal<u32>,
    released: Signal<u32>,
    writer: Option<WriterSource>,
}

impl StatefulView for ProbeRoot {
    type State = ProbeRootState;

    fn create_state(&self) -> Self::State {
        ProbeRootState::default()
    }
}

impl ViewState<ProbeRoot> for ProbeRootState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.count = ctx.signal(0);
        let graph = ctx.reactive();
        self.released = graph.signal(0);
        graph.release(self.released.slot());
        self.writer = Some(ctx.writer_source());
    }

    fn build(&self, view: &ProbeRoot, ctx: &dyn BuildContext) -> impl IntoView {
        let seen = &view.probe.seen;
        seen.count
            .borrow_mut()
            .get_or_insert_with(|| (self.count, ctx.reactive()));
        seen.writer.borrow_mut().clone_from(&self.writer);
        seen.reads.borrow_mut().push(self.count.get(ctx));
        (view.probe.builder)(ProbeSignals {
            count: self.count,
            released: self.released,
        })
    }
}
