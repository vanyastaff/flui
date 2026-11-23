//! A custom painter shapes text through the text context its paint call
//! lends, and the loan records the painting node, so a font registered later
//! paints it again; a painter that never asks is not recorded.

use std::cell::Cell;
use std::rc::Rc;

use flui_foundation::TextDirection;
use flui_foundation::geometry::{Offset, Point, Rect, Size};
use flui_objects::{RenderCustomPaint, RenderFlex};
use flui_painting::paint::Paint;
use flui_painting::styling::Color;
use flui_painting::typography::TextSpan;
use flui_painting::{Canvas, TextPainter};
use flui_rendering::{
    PipelineOwner, TextContextHandle,
    constraints::BoxConstraints,
    delegates::{CustomPaintContext, CustomPainter},
    error::RenderError,
    testing::{DrawKind, SceneHost, box_node, collect_commands, tree},
};

const PROBE_MONO: &[u8] = include_bytes!("../../flui-painting/assets/fonts/probe-mono-100.ttf");

/// Paints its label, shaped through the context's text loan.
#[derive(Debug)]
struct LabelPainter(&'static str);

impl CustomPainter for LabelPainter {
    fn paint(&self, ctx: &mut CustomPaintContext<'_>) {
        let width = ctx.size().width;
        let mut label = TextPainter::new()
            .with_text(TextSpan::new(self.0))
            .with_text_direction(TextDirection::Ltr);
        ctx.text()
            .expect("the paint pass lends the text context")
            .measurement()
            .layout(&mut label, 0.0, width)
            .expect("the label lays out");
        label.paint(ctx.canvas(), Offset::ZERO);
    }

    fn should_repaint(&self, _old_delegate: &dyn CustomPainter) -> bool {
        false
    }
}

/// Fills its box and never asks for text.
#[derive(Debug)]
struct FillPainter;

impl CustomPainter for FillPainter {
    fn paint(&self, ctx: &mut CustomPaintContext<'_>) {
        let area = Rect::from_origin_size(Point::ZERO, ctx.size());
        ctx.canvas().draw_rect(area, &Paint::fill(Color::RED));
    }

    fn should_repaint(&self, _old_delegate: &dyn CustomPainter) -> bool {
        false
    }
}

/// Records whether its loan was refused as busy.
#[derive(Debug)]
struct LoanOutcome(Rc<Cell<Option<bool>>>);

impl CustomPainter for LoanOutcome {
    fn paint(&self, ctx: &mut CustomPaintContext<'_>) {
        let busy = matches!(ctx.text(), Err(RenderError::TextContextBusy));
        self.0.set(Some(busy));
    }

    fn should_repaint(&self, _old_delegate: &dyn CustomPainter) -> bool {
        false
    }
}

fn custom_paint(painter: impl CustomPainter) -> RenderCustomPaint {
    RenderCustomPaint::new(Some(Rc::new(painter)), None, Size::new(120.0, 24.0))
}

/// A pipeline over `text` with `spec` mounted under loose root constraints.
fn mount(
    text: &TextContextHandle,
    spec: tree::TreeNode,
) -> (PipelineOwner, tree::RenderLabelRegistry) {
    let mut owner = PipelineOwner::new(text.clone());
    let (root, labels) = tree::mount(&mut owner, spec);
    owner.set_root_id(Some(root));
    owner.set_root_constraints(Some(BoxConstraints::loose(Size::new(400.0, 400.0))));
    (owner, labels)
}

fn label_and_fill() -> tree::TreeNode {
    box_node(RenderFlex::column())
        .child(box_node(custom_paint(LabelPainter("42 %"))).label("label"))
        .child(box_node(custom_paint(FillPainter)).label("fill"))
}

fn needs_paint(owner: &PipelineOwner, id: flui_foundation::RenderId) -> bool {
    owner
        .render_tree()
        .get(id)
        .expect("the node is live")
        .needs_paint()
}

/// The painter's label reaches the frame as a text command. Fails if the
/// paint pass lends a painter no text context, or the shaped label is not
/// drawn on the node's canvas.
#[test]
fn a_custom_painter_draws_text_through_its_context() {
    let text = TextContextHandle::standalone();
    let (owner, _) = mount(&text, label_and_fill());
    let (_owner, result) = SceneHost::new().run_frame(owner);
    let tree = result
        .expect("the frame paints")
        .expect("the first frame produces a layer tree");
    let commands = collect_commands(&tree);
    assert!(
        commands
            .iter()
            .any(|command| command.kind == DrawKind::Text),
        "the label is drawn as text: {commands:?}"
    );
    assert!(
        commands
            .iter()
            .any(|command| command.kind == DrawKind::Rect),
        "the fill painter still draws: {commands:?}"
    );
}

/// Only the painter that asked for text is recorded, and registering a face
/// marks it, and not its sibling, for paint again. Fails if paint lends an
/// unrecorded source (the label would keep its old shaping), or if every
/// painter is recorded whether it asks or not.
#[test]
fn a_font_change_repaints_only_the_painter_that_shaped_text() {
    let text = TextContextHandle::standalone();
    let (owner, labels) = mount(&text, label_and_fill());
    let label = labels.get("label").expect("labelled");
    let fill = labels.get("fill").expect("labelled");
    let (mut owner, result) = SceneHost::new().run_frame(owner);
    result.expect("the first frame paints");
    assert_eq!(
        owner.text_measurer_count(),
        1,
        "the label painter alone measured text"
    );

    owner.drain_pending_dirty();
    assert!(!needs_paint(&owner, label), "no font changed yet");

    text.with(|text| text.fonts().register_font(PROBE_MONO))
        .expect("the probe face loads");
    owner.drain_pending_dirty();

    assert!(
        needs_paint(&owner, label),
        "the painter that shaped text repaints"
    );
    assert!(
        !needs_paint(&owner, fill),
        "a painter that never asked for text is not marked"
    );
}

/// A painter whose loan meets one already held gets the typed refusal, and
/// its node is not poisoned for it. Fails if the paint loan borrows the
/// shared context unconditionally (a panic) or ignores the competing loan.
#[test]
fn a_painter_that_finds_the_context_lent_is_refused_as_busy() {
    let text = TextContextHandle::standalone();
    let outcome = Rc::new(Cell::new(None));
    let (owner, _) = mount(&text, box_node(custom_paint(LoanOutcome(outcome.clone()))));
    let mut scene = SceneHost::new();
    let (_owner, result) = text.with(|_| scene.run_frame(owner));
    result.expect("a refused loan does not fail the frame");
    assert_eq!(outcome.get(), Some(true), "the loan was refused as busy");
}

/// A context built apart from a paint pass draws, and refuses text with a
/// typed error instead of panicking.
#[test]
fn a_context_without_text_refuses_the_loan() {
    let mut canvas = Canvas::new();
    let mut ctx = CustomPaintContext::without_text(&mut canvas, Size::new(10.0, 10.0));
    assert!(matches!(
        ctx.text(),
        Err(RenderError::TextContextUnavailable)
    ));
    FillPainter.paint(&mut ctx);
    assert_eq!(canvas.finish().iter().len(), 1, "the painter still draws");
}
