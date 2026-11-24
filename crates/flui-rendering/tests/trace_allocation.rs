//! Process-isolated allocator witness: the frame path's tracing costs nothing at a level that
//! is not enabled.
//!
//! Spans and events at DEBUG and TRACE sit on the layout path. A subscriber that admits INFO
//! only must leave the allocation count of a relayout where a subscriber-free run puts it, and a
//! subscriber that admits TRACE and formats the fields must raise it, which shows the witness
//! sees the instrumentation at all.

use std::fmt::Write as _;

use flui_foundation::geometry::Size;
use flui_foundation::testing::heap::{CountingAllocator, HeapCounts};
use flui_objects::{RenderColoredBox, RenderPadding};
use flui_rendering::{constraints::BoxConstraints, pipeline::PipelineOwner};
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Dispatch, Event, Level, Metadata, Subscriber};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

/// Admits events and spans up to `ceiling`; a subscriber that formats what it admits allocates.
struct FormattingSubscriber {
    ceiling: Level,
}

struct Format(String);

impl Visit for Format {
    fn record_debug(&mut self, field: &Field, value: &dyn core::fmt::Debug) {
        let _ = write!(self.0, "{}={value:?} ", field.name());
    }
}

impl Subscriber for FormattingSubscriber {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        *metadata.level() <= self.ceiling
    }

    fn new_span(&self, attributes: &Attributes<'_>) -> Id {
        let mut text = Format(String::new());
        attributes.record(&mut text);
        Id::from_u64(1)
    }

    fn record(&self, _: &Id, _: &Record<'_>) {}

    fn record_follows_from(&self, _: &Id, _: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let mut text = Format(String::new());
        event.record(&mut text);
    }

    fn enter(&self, _: &Id) {}

    fn exit(&self, _: &Id) {}
}

/// Allocations of eight relayouts of a warm three-deep tree under `dispatch`.
fn relayout_allocations(dispatch: &Dispatch) -> u64 {
    let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
    let root = owner.set_root_render_object(Box::new(RenderPadding::all(1.0)));
    let middle = owner
        .insert_child_render_object(root, Box::new(RenderPadding::all(1.0)))
        .expect("padding attaches");
    owner
        .insert_child_render_object(middle, Box::new(RenderColoredBox::red(40.0, 40.0)))
        .expect("leaf attaches");
    owner.set_root_constraints(Some(BoxConstraints::loose(Size::new(1024.0, 1024.0))));
    let mut owner = owner.into_layout();
    tracing::dispatcher::with_default(dispatch, || {
        for _ in 0..2 {
            owner.mark_needs_layout(root);
            owner.run_layout().expect("the tree lays out");
        }
        let mut total = 0;
        for _ in 0..8 {
            owner.mark_needs_layout(root);
            let before = HeapCounts::now().allocations;
            owner.run_layout().expect("the tree lays out");
            total += HeapCounts::now().allocations - before;
        }
        total
    })
}

#[test]
fn a_level_that_is_not_enabled_adds_no_allocation_to_a_relayout() {
    let none = relayout_allocations(&Dispatch::none());
    let info = relayout_allocations(&Dispatch::new(FormattingSubscriber {
        ceiling: Level::INFO,
    }));
    let trace = relayout_allocations(&Dispatch::new(FormattingSubscriber {
        ceiling: Level::TRACE,
    }));
    assert_eq!(
        info, none,
        "DEBUG and TRACE instrumentation allocated while only INFO was enabled"
    );
    assert!(
        trace > none,
        "the witness did not see the instrumentation: {trace} allocations with TRACE, {none} without"
    );
}
