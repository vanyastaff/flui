//! A plugin pipeline lays its root out at the surface size of each frame the
//! host asks for, not only at the size it was mounted with.

use flui_layer::Scene;
use flui_rendering::TextContextHandle;
use flui_sdk::geometry::{Rect, Size};
use flui_sdk::painting::Color;
use flui_sdk::widgets::ColoredBox;
use flui_view::{BuildContext, IntoView, StatelessView, View, element::ElementKind};

/// Fills whatever the root is given, so the painted area is the root's size.
#[derive(Clone)]
struct Fill;

impl StatelessView for Fill {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        ColoredBox::new(Color::rgb(10, 20, 30))
    }
}

impl View for Fill {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateless(self)
    }
}

/// The area the scene paints: the union of every layer's bounds.
fn painted_size(scene: &Scene) -> Size {
    let area = scene
        .tree()
        .iter()
        .filter_map(|(_, node)| node.layer().bounds())
        .reduce(|a: Rect<f64>, b| a.union(&b))
        .expect("the scene paints something");
    area.size()
}

/// Mounted at 320x240, each row draws one frame at its size and checks the
/// root filled exactly that. A pipeline that kept its mount-time constraints
/// paints 320x240 on every row after the first.
#[test]
fn a_plugin_pipeline_lays_out_at_the_size_of_each_frame() {
    let mut pipeline =
        flui_hot_reload::PluginPipeline::mount(Fill, 320.0, 240.0, TextContextHandle::standalone());

    for (width, height) in [
        (320.0, 240.0),
        (800.0, 600.0),
        (200.0, 100.0),
        (200.0, 100.0),
    ] {
        let scene = pipeline.draw_frame(width, height);
        assert_eq!(
            painted_size(&scene),
            Size::new(width, height),
            "the root filled the {width}x{height} surface of this frame"
        );
    }
}
