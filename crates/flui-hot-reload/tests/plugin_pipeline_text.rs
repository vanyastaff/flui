//! A plugin pipeline measures its text through the context it is mounted
//! with, never through one it builds for itself (ADR-0092 §10 step 3).

use flui_painting::testing::text_context_lends;
use flui_rendering::TextContextHandle;
use flui_view::{BuildContext, IntoView, StatelessView, View, element::ElementKind};

#[derive(Clone)]
struct Label;

impl StatelessView for Label {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        flui_sdk::widgets::Text::new("measured in the plugin")
    }
}

impl View for Label {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateless(self)
    }
}

/// Fails if the plugin pipeline measures on any context but the one handed
/// to `mount`: the paragraph's layout would leave this one unlent.
#[test]
fn a_plugin_pipeline_measures_through_the_context_it_is_given() {
    let text = TextContextHandle::standalone();
    let mut pipeline = flui_hot_reload::PluginPipeline::mount(Label, 320.0, 240.0, text.clone());

    let _scene = pipeline.draw_frame();

    assert!(
        text.with(|text| text_context_lends(text)) > 0,
        "the plugin's paragraph measured through the context it was mounted with"
    );
}
