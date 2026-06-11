//! Sans-IO fragment-recording paint context.
//!
//! Paint is a pure encoder pass: a render object's `paint` records a
//! node-local [`PaintFragment`] — draw-command runs, child splice
//! markers, and clip-layer scopes — without ever touching the live
//! render tree, the layer tree, or the GPU. The pipeline owner replays
//! fragments top-down into a `LayerTree` (see
//! `PipelineOwner::run_paint`), recursing at child markers and
//! splitting repaint-boundary subtrees into their own `OffsetLayer`s.
//!
//! Why this shape (design amendments D1/D9):
//!
//! * **No recording state machine leaks into user code.** The open
//!   picture buffer is sealed at deterministic points (scope
//!   enter/exit, child markers, `finish`) inside [`FragmentRecorder`];
//!   render objects cannot observe or corrupt it.
//! * **No live recursion.** `paint_child` records a marker instead of
//!   re-entering the pipeline, so `paint` borrows nothing but the
//!   recorder — a self-contained encode that a later change can run
//!   per-boundary on the data plane.
//! * **Local coordinates.** The recorder pre-translates every run to
//!   the node's origin, so paint code draws in the node's own space —
//!   no manual offset arithmetic (a recurring Flutter paint-bug class).
//!
//! # Coordinate model
//!
//! Inline children bake into the parent's picture space: the composer
//! merges adjacent runs into one `DisplayList` (commands carry their
//! own transforms). Repaint-boundary children are rebased to
//! `Offset::ZERO` under an `OffsetLayer` by the composer. Clip scopes
//! do **not** rebase — the clip shape is recorded in the current layer
//! space.
//!
//! # Canvas state is run-local
//!
//! `canvas().save()/clip_*()` affect only the current run: a child
//! marker seals the run, and the child's commands replay in a fresh
//! one. A clip that must cover children goes through
//! [`PaintCx::with_clip_rect`] / `with_clip_rrect` / `with_clip_path`,
//! which produce real clip layers. (A composer-side fast path may later
//! lower non-composited clip layers back into canvas clips —
//! correctness is identical, so the API does not expose the choice.)

use std::marker::PhantomData;

use flui_painting::{Canvas, DisplayList, DisplayListCore};
use flui_tree::{Arity, Optional, Single, Variable};
use flui_types::{Offset, Pixels, Rect, painting::Clip};

// ============================================================================
// Fragment ops
// ============================================================================

/// One step of a recorded paint fragment.
///
/// Crate-private: only the recorder writes ops and only the pipeline
/// composer reads them.
#[derive(Debug)]
pub(crate) enum FragmentOp {
    /// A sealed run of draw commands in the current layer space
    /// (node origin already baked into each command's transform).
    Run(DisplayList),

    /// Splice point for child `index`'s subtree.
    ///
    /// `offset_override` replaces the child's `RenderState.offset`
    /// when the parent paints the child somewhere other than its
    /// laid-out position (`paint_child_at`).
    Child {
        index: usize,
        offset_override: Option<Offset>,
    },

    /// Opens a clip-layer scope; balanced by a matching [`Self::Pop`].
    /// Boxed: clip shapes (especially paths) dwarf the other variants,
    /// and scope ops are rare relative to runs/markers.
    Push(Box<FragmentClip>),

    /// Closes the innermost open scope.
    Pop,
}

/// Clip shape for a fragment layer scope.
///
/// Mapped 1:1 onto `flui-layer`'s `ClipRectLayer` / `ClipRRectLayer` /
/// `ClipPathLayer` by the composer.
#[derive(Debug, Clone)]
pub(crate) enum FragmentClip {
    Rect {
        rect: Rect<Pixels>,
        behavior: Clip,
    },
    RRect {
        rrect: flui_types::RRect,
        behavior: Clip,
    },
    Path {
        /// Boxed: a path's command buffer dwarfs the other clip shapes.
        path: Box<flui_types::painting::Path>,
        behavior: Clip,
    },
}

/// An immutable recorded paint fragment — the output of one render
/// object's `paint`.
#[derive(Debug, Default)]
pub struct PaintFragment {
    pub(crate) ops: Vec<FragmentOp>,
}

impl PaintFragment {
    /// `true` when the fragment recorded nothing at all — no draws, no
    /// child markers, no scopes. (An offstage subtree, for example.)
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }
}

// ============================================================================
// FragmentRecorder
// ============================================================================

/// Accumulates fragment ops for one render object's `paint` call.
///
/// Constructed by the pipeline per node (origin = the node's position
/// in the current layer space); handed to `paint` wrapped in the typed
/// [`PaintCx`]. Sealing is deterministic: the open canvas is finished
/// into a sealed run whenever a child marker or scope
/// boundary needs ordering, and at [`Self::finish`].
#[derive(Debug)]
pub struct FragmentRecorder {
    ops: Vec<FragmentOp>,
    open: Option<Canvas>,
    origin: Offset,
    /// Currently open `Push` scopes; must be 0 at `finish` (the
    /// closure-based `with_*` API makes imbalance unreachable from
    /// safe user code, the counter turns an internal bug into a loud
    /// debug failure instead of a silently malformed layer tree).
    open_scopes: usize,
    dpr: f32,
}

impl FragmentRecorder {
    /// Creates a recorder for a node positioned at `origin` within the
    /// current layer space.
    pub fn new(origin: Offset, dpr: f32) -> Self {
        Self {
            ops: Vec::new(),
            open: None,
            origin,
            open_scopes: 0,
            dpr,
        }
    }

    /// Device pixel ratio for this paint pass (text shaping and
    /// hairline snapping need it).
    pub fn dpr(&self) -> f32 {
        self.dpr
    }

    /// The open recording canvas, pre-translated to the node origin.
    fn canvas(&mut self) -> &mut Canvas {
        self.open.get_or_insert_with(|| {
            let mut canvas = Canvas::new();
            if self.origin != Offset::ZERO {
                canvas.translate(self.origin.dx.get(), self.origin.dy.get());
            }
            canvas
        })
    }

    /// Seals the open canvas into a `Run` op (dropped when empty).
    fn seal(&mut self) {
        if let Some(canvas) = self.open.take() {
            let list = canvas.finish();
            if !list.is_empty() {
                self.ops.push(FragmentOp::Run(list));
            }
        }
    }

    fn push_scope(&mut self, clip: FragmentClip) {
        self.seal();
        self.ops.push(FragmentOp::Push(Box::new(clip)));
        self.open_scopes += 1;
    }

    fn pop_scope(&mut self) {
        self.seal();
        self.ops.push(FragmentOp::Pop);
        debug_assert!(
            self.open_scopes > 0,
            "FragmentRecorder scope underflow: Pop without matching Push — \
             only the closure-scoped with_clip_* API may emit scope ops",
        );
        self.open_scopes = self.open_scopes.saturating_sub(1);
    }

    fn child(&mut self, index: usize, offset_override: Option<Offset>) {
        self.seal();
        self.ops.push(FragmentOp::Child {
            index,
            offset_override,
        });
    }

    /// Finishes recording, sealing any open run.
    pub fn finish(mut self) -> PaintFragment {
        self.seal();
        debug_assert_eq!(
            self.open_scopes, 0,
            "FragmentRecorder finished with unbalanced clip scopes — \
             a with_clip_* closure must have leaked its scope",
        );
        PaintFragment { ops: self.ops }
    }
}

// ============================================================================
// PaintCx
// ============================================================================

/// Typed paint context handed to `RenderBox::paint`.
///
/// Wraps a [`FragmentRecorder`] with the node's child count and an
/// arity parameter that gates the child-painting surface at compile
/// time: `Leaf` objects have **no** `paint_child` method at all.
///
/// ```compile_fail
/// use flui_rendering::context::{FragmentRecorder, PaintCx};
/// use flui_tree::Leaf;
/// use flui_types::Offset;
///
/// let mut rec = FragmentRecorder::new(Offset::ZERO, 1.0);
/// let mut cx = PaintCx::<Leaf>::new(&mut rec, 0);
/// cx.paint_child(); // Leaf has no children to paint
/// ```
pub struct PaintCx<'a, A: Arity> {
    rec: &'a mut FragmentRecorder,
    child_count: usize,
    _arity: PhantomData<fn() -> A>,
}

impl<'a, A: Arity> PaintCx<'a, A> {
    /// Creates a typed context over a recorder.
    ///
    /// Called by the protocol blanket impl (`paint_raw`); render
    /// objects never construct their own context.
    pub fn new(rec: &'a mut FragmentRecorder, child_count: usize) -> Self {
        Self {
            rec,
            child_count,
            _arity: PhantomData,
        }
    }

    /// The recording canvas, pre-translated to this node's origin —
    /// draw in local coordinates.
    ///
    /// Canvas `save`/`clip_*` state is run-local: it does not extend
    /// across `paint_child` markers. Clips that must cover children go
    /// through [`Self::with_clip_rect`] and friends.
    pub fn canvas(&mut self) -> &mut Canvas {
        self.rec.canvas()
    }

    /// Device pixel ratio for this paint pass.
    pub fn dpr(&self) -> f32 {
        self.rec.dpr()
    }

    /// Number of children attached to this node.
    pub fn child_count(&self) -> usize {
        self.child_count
    }

    /// Records child markers for every child in tree order.
    ///
    /// This is the default `RenderBox::paint` body — a pass-through
    /// node (Padding, Flex without overflow clip, …) paints nothing
    /// itself and splices its children in order, matching Flutter's
    /// `RenderProxyBox.paint`. An override that does NOT call any
    /// child-painting method hides its subtree (offstage semantics).
    pub fn paint_children_in_order(&mut self) {
        for index in 0..self.child_count {
            self.rec.child(index, None);
        }
    }

    /// Clips everything recorded inside `f` — self draws AND child
    /// subtrees — to `rect` (local coordinates).
    pub fn with_clip_rect(
        &mut self,
        rect: Rect<Pixels>,
        behavior: Clip,
        f: impl FnOnce(&mut Self),
    ) {
        self.rec.push_scope(FragmentClip::Rect { rect, behavior });
        f(self);
        self.rec.pop_scope();
    }

    /// Clips everything recorded inside `f` to a rounded rect
    /// (local coordinates).
    pub fn with_clip_rrect(
        &mut self,
        rrect: flui_types::RRect,
        behavior: Clip,
        f: impl FnOnce(&mut Self),
    ) {
        self.rec.push_scope(FragmentClip::RRect { rrect, behavior });
        f(self);
        self.rec.pop_scope();
    }

    /// Clips everything recorded inside `f` to an arbitrary path
    /// (local coordinates).
    pub fn with_clip_path(
        &mut self,
        path: flui_types::painting::Path,
        behavior: Clip,
        f: impl FnOnce(&mut Self),
    ) {
        self.rec.push_scope(FragmentClip::Path {
            path: Box::new(path),
            behavior,
        });
        f(self);
        self.rec.pop_scope();
    }
}

// ============================================================================
// Arity-gated child painting
// ============================================================================

impl PaintCx<'_, Single> {
    /// Splices the single child at its laid-out offset
    /// (`RenderState.offset`).
    pub fn paint_child(&mut self) {
        if self.child_count > 0 {
            self.rec.child(0, None);
        }
    }

    /// Splices the single child at a custom offset instead of its
    /// laid-out position.
    pub fn paint_child_at(&mut self, offset: Offset) {
        if self.child_count > 0 {
            self.rec.child(0, Some(offset));
        }
    }
}

impl PaintCx<'_, Optional> {
    /// Splices the child if one is attached.
    pub fn paint_child_if_present(&mut self) {
        if self.child_count > 0 {
            self.rec.child(0, None);
        }
    }

    /// `true` when a child is attached.
    pub fn has_child(&self) -> bool {
        self.child_count > 0
    }
}

impl PaintCx<'_, Variable> {
    /// Splices child `index` at its laid-out offset. Out-of-range
    /// indices record nothing.
    pub fn paint_child(&mut self, index: usize) {
        if index < self.child_count {
            self.rec.child(index, None);
        }
    }

    /// Splices child `index` at a custom offset.
    pub fn paint_child_at(&mut self, index: usize, offset: Offset) {
        if index < self.child_count {
            self.rec.child(index, Some(offset));
        }
    }

    /// Splices all children in order (first to last).
    pub fn paint_children(&mut self) {
        for index in 0..self.child_count {
            self.rec.child(index, None);
        }
    }

    /// Splices all children in reverse order (last to first).
    pub fn paint_children_reverse(&mut self) {
        for index in (0..self.child_count).rev() {
            self.rec.child(index, None);
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use flui_tree::Leaf;
    use flui_types::{Point, Size, geometry::px, painting::Paint, styling::Color};

    use super::*;

    fn rect(w: f32, h: f32) -> Rect<Pixels> {
        Rect::from_origin_size(Point::ZERO, Size::new(px(w), px(h)))
    }

    fn fill() -> Paint {
        Paint::fill(Color::RED)
    }

    #[test]
    fn draws_between_child_markers_split_into_ordered_runs() {
        let mut rec = FragmentRecorder::new(Offset::ZERO, 1.0);
        let mut cx = PaintCx::<Variable>::new(&mut rec, 2);

        cx.canvas().draw_rect(rect(10.0, 10.0), &fill()); // background
        cx.paint_child(0);
        cx.canvas().draw_rect(rect(5.0, 5.0), &fill()); // between
        cx.paint_child(1);
        cx.canvas().draw_rect(rect(2.0, 2.0), &fill()); // foreground

        let frag = rec.finish();
        let kinds: Vec<&str> = frag
            .ops
            .iter()
            .map(|op| match op {
                FragmentOp::Run(_) => "run",
                FragmentOp::Child { .. } => "child",
                FragmentOp::Push(_) => "push",
                FragmentOp::Pop => "pop",
            })
            .collect();
        assert_eq!(
            kinds,
            vec!["run", "child", "run", "child", "run"],
            "draw / child interleave must preserve z-order as run-child-run-child-run",
        );
        let indices: Vec<usize> = frag
            .ops
            .iter()
            .filter_map(|op| match op {
                FragmentOp::Child { index, .. } => Some(*index),
                _ => None,
            })
            .collect();
        assert_eq!(indices, vec![0, 1]);
    }

    #[test]
    fn origin_is_baked_into_run_transforms() {
        let mut rec = FragmentRecorder::new(Offset::new(px(7.0), px(3.0)), 1.0);
        let mut cx = PaintCx::<Leaf>::new(&mut rec, 0);
        cx.canvas().draw_rect(rect(10.0, 10.0), &fill());

        let frag = rec.finish();
        let FragmentOp::Run(list) = &frag.ops[0] else {
            panic!("expected a single sealed run, got {:?}", frag.ops);
        };
        // The run's cached bounds reflect the origin translation —
        // local (0,0,10,10) lands at (7,3,17,13) in layer space.
        assert_eq!(
            list.bounds(),
            Rect::from_ltrb(px(7.0), px(3.0), px(17.0), px(13.0)),
            "record-time bounds must include the node-origin translation",
        );
    }

    #[test]
    fn clip_scope_brackets_children_and_balances() {
        let mut rec = FragmentRecorder::new(Offset::ZERO, 1.0);
        let mut cx = PaintCx::<Variable>::new(&mut rec, 2);

        cx.with_clip_rect(rect(50.0, 50.0), Clip::HardEdge, |cx| {
            cx.canvas().draw_rect(rect(10.0, 10.0), &fill());
            cx.paint_children();
        });

        let frag = rec.finish();
        let kinds: Vec<&str> = frag
            .ops
            .iter()
            .map(|op| match op {
                FragmentOp::Run(_) => "run",
                FragmentOp::Child { .. } => "child",
                FragmentOp::Push(_) => "push",
                FragmentOp::Pop => "pop",
            })
            .collect();
        assert_eq!(kinds, vec!["push", "run", "child", "child", "pop"]);
    }

    #[test]
    fn empty_paint_records_empty_fragment() {
        let rec = FragmentRecorder::new(Offset::ZERO, 1.0);
        let frag = rec.finish();
        assert!(frag.is_empty(), "no draws and no markers → empty fragment");
    }

    #[test]
    fn paint_child_at_records_offset_override() {
        let mut rec = FragmentRecorder::new(Offset::ZERO, 1.0);
        let mut cx = PaintCx::<Single>::new(&mut rec, 1);
        cx.paint_child_at(Offset::new(px(4.0), px(6.0)));

        let frag = rec.finish();
        assert!(matches!(
            frag.ops.as_slice(),
            [FragmentOp::Child {
                index: 0,
                offset_override: Some(o),
            }] if *o == Offset::new(px(4.0), px(6.0)),
        ));
    }

    #[test]
    fn out_of_range_child_indices_record_nothing() {
        let mut rec = FragmentRecorder::new(Offset::ZERO, 1.0);
        let mut cx = PaintCx::<Variable>::new(&mut rec, 1);
        cx.paint_child(5);
        let mut cx0 = PaintCx::<Single>::new(&mut rec, 0);
        cx0.paint_child();

        let frag = rec.finish();
        assert!(frag.is_empty());
    }

    #[test]
    fn default_passthrough_records_all_children_in_order() {
        let mut rec = FragmentRecorder::new(Offset::ZERO, 1.0);
        let mut cx = PaintCx::<Variable>::new(&mut rec, 3);
        cx.paint_children_in_order();

        let frag = rec.finish();
        let indices: Vec<usize> = frag
            .ops
            .iter()
            .filter_map(|op| match op {
                FragmentOp::Child { index, .. } => Some(*index),
                _ => None,
            })
            .collect();
        assert_eq!(indices, vec![0, 1, 2]);
    }
}
