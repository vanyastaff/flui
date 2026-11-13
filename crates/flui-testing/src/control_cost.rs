//! What a control costs to mount, counted rather than timed (ADR-0322).
//!
//! A scenario mounts [`MOUNT_ROWS`] controls, each over a `Text`, in one
//! eager column on a headless host with semantics off, pumps the first
//! frame, and records what each control adds to the `Text` it holds: its
//! elements and render nodes, and what its mount asked of the heap and left
//! live. The heap figures need the target to install
//! [`CountingAllocator`](crate::heap::CountingAllocator) (see
//! [`crate::heap`]); without it they read zero.
//!
//! The scenarios of `flui-widgets`' `perf` target (`raw_button_over_text`)
//! and of the catalogs' `perf` targets measure through
//! [`control_over_text`], so a control in any crate is held to the same
//! measure.

use std::time::Duration;

use flui_foundation::geometry::Size;
use flui_rendering::constraints::BoxConstraints;
use flui_view::prelude::*;
use flui_widgets::{Column, SingleChildScrollView, Text};

use crate::heap::HeapCounts;
use crate::{HeadlessHost, HeadlessWindow};

/// The rows a scenario mounts: enough that the app around them is noise in
/// a per-row figure.
pub const MOUNT_ROWS: usize = 1_000;

/// One 60 Hz frame of virtual time.
const FRAME: Duration = Duration::from_nanos(16_666_667);
const WIDTH: f64 = 800.0;
const HEIGHT: f64 = 600.0;

/// What [`MOUNT_ROWS`] rows of one kind cost to mount.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MountCost {
    /// Elements of the mounted tree.
    elements: usize,
    /// Render nodes of the mounted tree.
    render_nodes: usize,
    /// What the mount and its first frame asked of the heap, and what of it
    /// stays live after the frame.
    heap: HeapCounts,
}

/// `MOUNT_ROWS` rows in one eager column, each made by `row`, under the
/// ancestors `scope` puts around the column.
#[derive(Clone, StatelessView)]
struct Rows {
    row: fn(usize) -> BoxedView,
    scope: fn(BoxedView) -> BoxedView,
}

impl StatelessView for Rows {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let rows = (0..MOUNT_ROWS).map(self.row).collect::<Vec<BoxedView>>();
        (self.scope)(
            SingleChildScrollView::new()
                .child(Column::new(rows))
                .boxed(),
        )
    }
}

/// Counts what a column of [`MOUNT_ROWS`] rows made by `row` holds under
/// `scope`, then mounts it on a headless host with semantics off, pumps its
/// first frame, and measures what that allocated and what stays live.
fn mount_cost(row: fn(usize) -> BoxedView, scope: fn(BoxedView) -> BoxedView) -> MountCost {
    let (elements, render_nodes) = {
        let mut laid = crate::widgets::lay_out(
            Rows { row, scope },
            BoxConstraints::tight(Size::new(WIDTH, HEIGHT)),
        );
        (laid.element_node_count(), laid.render_node_count())
    };
    let before = HeapCounts::now();
    let mut host = HeadlessHost::new(HeadlessWindow::new(WIDTH as u32, HEIGHT as u32));
    host.attach(&Rows { row, scope })
        .expect("BUG: a fresh host takes a root");
    let _ = host.pump(FRAME);
    let cost = MountCost {
        elements,
        render_nodes,
        heap: HeapCounts::now().since(before),
    };
    drop(host);
    cost
}

/// What one control adds to the `Text` it holds, per control, beside what
/// the `Text` alone costs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControlCost {
    /// Elements the control adds.
    pub elements: u64,
    /// Render nodes the control adds.
    pub render_nodes: u64,
    /// Allocations its mount adds.
    pub allocations: u64,
    /// `realloc`s among them.
    pub reallocations: u64,
    /// Bytes it leaves live.
    pub bytes: u64,
    /// What one `Text` row asks of the heap and leaves live, on its own.
    pub text: HeapCounts,
}

impl ControlCost {
    /// The figures under the names a baseline file gives them.
    #[must_use]
    pub fn counts(&self) -> [(&'static str, u64); 5] {
        [
            ("elements", self.elements),
            ("render_nodes", self.render_nodes),
            ("allocations", self.allocations),
            ("reallocations", self.reallocations),
            ("bytes", self.bytes),
        ]
    }
}

/// What the control `control` makes of each row's `Text` adds to that
/// `Text`, per row. `control` builds row `row` over
/// `Text::new(format!("Row {row}"))`, the row the comparison mounts alone.
///
/// What the process and the thread set up once (the font system, lazy
/// tables) is paid by a mount that is not measured.
#[must_use]
pub fn control_over_text(control: fn(usize) -> BoxedView) -> ControlCost {
    control_over_text_under(|column| column, control)
}

/// [`control_over_text`] for a control that needs ancestors (a theme): both
/// columns, the controls' and the bare `Text`s', mount under what `scope`
/// puts around them, so the scope's cost is not the control's.
#[must_use]
pub fn control_over_text_under(
    scope: fn(BoxedView) -> BoxedView,
    control: fn(usize) -> BoxedView,
) -> ControlCost {
    let _warm_up = mount_cost(control, scope);
    let text = mount_cost(|row| Text::new(format!("Row {row}")).boxed(), scope);
    let with = mount_cost(control, scope);
    let rows = MOUNT_ROWS as u64;
    let per_row = |with: u64, without: u64| with.saturating_sub(without).div_ceil(rows);
    ControlCost {
        elements: per_row(with.elements as u64, text.elements as u64),
        render_nodes: per_row(with.render_nodes as u64, text.render_nodes as u64),
        allocations: per_row(with.heap.allocations, text.heap.allocations),
        reallocations: per_row(with.heap.reallocations, text.heap.reallocations),
        bytes: per_row(with.heap.live_bytes, text.heap.live_bytes),
        text: HeapCounts {
            allocations: text.heap.allocations / rows,
            reallocations: text.heap.reallocations / rows,
            allocated_bytes: text.heap.allocated_bytes / rows,
            live_bytes: text.heap.live_bytes / rows,
        },
    }
}
