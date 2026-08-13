//! Collapsing-sliver demo — a pinned [`SliverAppBar`] with a
//! [`FlexibleSpaceBar`] (title + colored background) over a list of labeled
//! rows, scrolled live by drag/fling.
//!
//! Run with: cargo run -p flui --features material --example sliver_demo
//!
//! The same tree, pre-scrolled to fixed offsets, is captured headlessly by
//! `examples/screenshot.rs`'s `sliver` | `sliver-mid` | `sliver-collapsed`
//! arms (which `#[path]`-include this file), so the screenshots and this
//! window always show the same widget tree.
//!
//! No local `tracing_subscriber` init, matching every other `run_app`-based
//! example: `run_app` installs the process-global subscriber itself.

use std::rc::Rc;

use flui::material::{FlexibleSpaceBar, SliverAppBar, Theme, ThemeData};
use flui::widgets::{
    ColoredBox, CustomScrollView, MediaQuery, MediaQueryData, Padding, ScrollController,
    Scrollable, SizedBox, SliverToBoxAdapter, Text, Viewport,
};
use flui_types::geometry::px;
use flui_types::{Color, EdgeInsets};
use flui_view::view::ViewExt;
use flui_view::{BoxedView, BuildContext, IntoView, StatelessView, View};

/// The demo slivers: the collapsing bar plus 30 labeled rows.
fn demo_slivers() -> Vec<BoxedView> {
    let bar = SliverAppBar::new()
        .expanded_height(220.0)
        .pinned(true)
        .flexible_space(
            FlexibleSpaceBar::new()
                .title(Text::new("FLUI Slivers"))
                .background(ColoredBox::new(Color::rgb(21, 101, 192))),
        );

    let mut slivers: Vec<BoxedView> = vec![bar.into_view().boxed()];
    for i in 0..30 {
        let shade = if i % 2 == 0 { 245 } else { 232 };
        slivers.push(
            SliverToBoxAdapter::new()
                .child(ColoredBox::new(Color::rgb(shade, shade, shade)).child(
                    SizedBox::height(56.0).child(Padding::new(EdgeInsets::all(px(16.0))).child(
                        Text::new(format!("Row {i} — scrolled under a pinned SliverAppBar")),
                    )),
                ))
                .into_view()
                .boxed(),
        );
    }
    slivers
}

/// The screenshot tree: the demo slivers at a FIXED programmatic offset —
/// `CustomScrollView`'s offset mode carries no gestures by design (see its
/// module doc), which is exactly right for a deterministic capture.
pub fn tree(offset: f32) -> impl IntoView {
    Theme::new(
        ThemeData::light(),
        MediaQuery::new(
            MediaQueryData::default(),
            CustomScrollView::new(demo_slivers()).offset(offset),
        ),
    )
}

/// The LIVE tree: the same slivers behind a gesture-driven [`Scrollable`]
/// (drag and fling; the collapsing bar reacts to the shared position).
fn live_tree() -> impl IntoView {
    let controller = ScrollController::new();
    let slivers = demo_slivers();
    Theme::new(
        ThemeData::light(),
        MediaQuery::new(
            MediaQueryData::default(),
            Scrollable::new()
                .controller(controller)
                .viewport_builder(Rc::new(move |position| {
                    Viewport::new(slivers.clone()).position(position).boxed()
                })),
        ),
    )
}

/// Stateless root: the demo tree, fully expanded, scrolled live.
#[derive(Clone, Debug)]
pub struct SliverDemoApp;

impl StatelessView for SliverDemoApp {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        live_tree()
    }
}

impl View for SliverDemoApp {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

fn main() {
    flui::run_app(SliverDemoApp);
}
