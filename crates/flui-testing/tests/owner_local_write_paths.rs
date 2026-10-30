//! Writes opened through a captured `WriterSource` (ADR-0086). A listener, an
//! animation-status listener and a post-frame callback each write from the
//! owner and the reader rebuilds on the next frame. A nested
//! `write` inside a live `EventCx` opens its own writer, applies at once and is
//! not a transaction with the outer one, and a panic inside it reaches the
//! outer callback with the writes before it still applied.

use std::panic::{AssertUnwindSafe, catch_unwind};

use flui_view::{BuildOwner, SignalWriteExt};

pub(crate) fn nested_writer_source_inside_an_event_cx() {
    let owner = BuildOwner::new();
    let graph = owner.signal_graph();
    let source = owner.writer_source();
    let a = graph.signal(0u32);
    let b = graph.signal(0u32);

    let seen = source.write(|outer| {
        a.set(outer, 1).expect("outer write");
        let inner_saw = source.write(|inner| {
            let saw = a.peek(inner, |n| *n);
            b.set(inner, 2).expect("nested write");
            saw
        });
        (inner_saw, b.peek(outer, |n| *n))
    });

    assert_eq!(
        seen,
        (Ok(1), Ok(2)),
        "each write is visible to the next read, across the nesting"
    );
    assert_eq!(a.peek(graph, |n| *n), Ok(1));
    assert_eq!(b.peek(graph, |n| *n), Ok(2));
}

pub(crate) fn nested_write_panics_inside_an_event_cx() {
    let owner = BuildOwner::new();
    let graph = owner.signal_graph();
    let source = owner.writer_source();
    let a = graph.signal(0u32);
    let b = graph.signal(0u32);

    let failure = catch_unwind(AssertUnwindSafe(|| {
        source.write(|outer| {
            a.set(outer, 1).expect("outer write");
            source.write(|inner| {
                b.set(inner, 2).expect("nested write");
                panic!("nested callback failure");
            });
        });
    }))
    .expect_err("the nested panic reaches the outer callback");

    assert_eq!(
        flui_foundation::panic::payload_text(&*failure),
        Some("nested callback failure"),
        "the nested panic is the outer callback's first panic"
    );
    assert_eq!(
        a.peek(graph, |n| *n),
        Ok(1),
        "the outer write stays applied"
    );
    assert_eq!(
        b.peek(graph, |n| *n),
        Ok(2),
        "the nested write stays applied"
    );
    assert_eq!(
        source.write(|cx| a.set(cx, 3)),
        Ok(()),
        "the source opens the next write"
    );
}

/// What hands the reader's callback control: a listener, an animation-status
/// listener or a post-frame callback, each owner-local.
#[derive(Clone)]
enum Trigger {
    Listener(flui_foundation::ChangeNotifier),
    Status(flui_animation::AnimationController),
    PostFrame,
}

/// Writes a signal from its trigger's callback through the `WriterSource` it
/// captured in `init_state`, and records what each build reads.
#[derive(Clone, flui_view::prelude::StatefulView)]
struct WriteReader {
    trigger: Trigger,
    seen: std::rc::Rc<std::cell::Cell<Option<u32>>>,
}

struct WriteReaderState {
    view: WriteReader,
    written: Option<flui_view::Signal<u32>>,
    keep: Vec<Box<dyn std::any::Any>>,
}

impl flui_view::StatefulView for WriteReader {
    type State = WriteReaderState;

    fn create_state(&self) -> Self::State {
        WriteReaderState {
            view: self.clone(),
            written: None,
            keep: Vec::new(),
        }
    }
}

impl flui_view::ViewState<WriteReader> for WriteReaderState {
    fn init_state(&mut self, ctx: &dyn flui_view::LifecycleContext) {
        use flui_view::BuildContextExt as _;

        let written = ctx.signal(0u32);
        let writer = ctx.writer_source();
        let write = move |value: u32| {
            writer
                .write(|cx| written.set(cx, value))
                .expect("the callback's write lands on the owner");
        };
        match &self.view.trigger {
            Trigger::Listener(notifier) => {
                use flui_foundation::Listenable as _;
                let id = notifier.add_listener(std::rc::Rc::new(move || write(1)));
                self.keep.push(Box::new(id));
            }
            Trigger::Status(controller) => {
                use flui_animation::Animation as _;
                let subscription = controller.subscribe_status(std::rc::Rc::new(move |status| {
                    if status == flui_animation::AnimationStatus::ForwardSettled {
                        write(2);
                    }
                }));
                self.keep.push(Box::new(subscription));
            }
            Trigger::PostFrame => {
                ctx.post_frame_handle()
                    .expect("a mounted presentation has a post-frame lane")
                    .schedule(move |_timing| write(3))
                    .expect("the lane admits the callback");
            }
        }
        self.written = Some(written);
    }

    fn build(
        &self,
        view: &WriteReader,
        ctx: &dyn flui_view::BuildContext,
    ) -> impl flui_view::IntoView {
        let written = self.written.expect("init_state ran");
        view.seen.set(Some(written.get(ctx)));
        flui_widgets::SizedBox::new(10.0, 10.0)
    }
}

fn mount_reader(
    trigger: Trigger,
) -> (
    flui_testing::widgets::LaidOut,
    std::rc::Rc<std::cell::Cell<Option<u32>>>,
) {
    let seen = std::rc::Rc::new(std::cell::Cell::new(None));
    let view = WriteReader {
        trigger,
        seen: std::rc::Rc::clone(&seen),
    };
    let laid = flui_testing::widgets::lay_out(view, flui_testing::widgets::loose(100.0));
    (laid, seen)
}

/// A `ChangeNotifier` listener writes through its captured `WriterSource`;
/// the reader rebuilds on the next frame reading the write.
pub(crate) fn listener_writes_through_a_captured_writer_source() {
    let notifier = flui_foundation::ChangeNotifier::new();
    let (mut laid, seen) = mount_reader(Trigger::Listener(notifier.clone()));
    assert_eq!(seen.get(), Some(0));

    notifier.notify_listeners();
    laid.tick();

    assert_eq!(
        seen.get(),
        Some(1),
        "the next frame rebuilt with the listener's write"
    );
}

/// An animation-status listener writes when its run completes; the reader
/// rebuilds on the next frame.
pub(crate) fn status_listener_writes_through_a_captured_writer_source() {
    let vsync = flui_animation::Vsync::new();
    let controller =
        flui_animation::AnimationController::builder(std::time::Duration::from_millis(100))
            .build_on(Some(&vsync));
    let seen = std::rc::Rc::new(std::cell::Cell::new(None));
    let mut laid = flui_testing::widgets::lay_out_animated(
        WriteReader {
            trigger: Trigger::Status(controller.controller().clone()),
            seen: std::rc::Rc::clone(&seen),
        },
        flui_testing::widgets::loose(100.0),
        vsync,
    );
    assert_eq!(seen.get(), Some(0));

    let _run = controller.controller().forward().expect("the run starts");
    laid.pump_for(std::time::Duration::ZERO);
    laid.pump_for(std::time::Duration::from_millis(100));
    laid.tick();

    assert_eq!(
        seen.get(),
        Some(2),
        "the reader rebuilt with the status listener's write"
    );
}

/// A post-frame callback scheduled in `init_state` writes after the first
/// frame; the reader rebuilds on the next one.
pub(crate) fn post_frame_callback_writes_through_a_captured_writer_source() {
    let (mut laid, seen) = mount_reader(Trigger::PostFrame);
    laid.tick();
    laid.tick();

    assert_eq!(
        seen.get(),
        Some(3),
        "the reader rebuilt with the post-frame write"
    );
}
