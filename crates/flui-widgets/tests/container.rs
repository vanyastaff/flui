//! Composition parity and state-stability tests for [`Container`].

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use crate::common::{lay_out, lay_out_animated, loose, offset, size};
use flui_animation::Vsync;
use flui_geometry::{EdgeInsets, Matrix4};
use flui_types::styling::BoxDecoration;
use flui_types::{Alignment, Color};
use flui_view::prelude::{BuildContext, StatefulView};
use flui_view::{IntoView, ViewState};
use flui_widgets::{AnimatedContainer, Container, SizedBox, VsyncScope};

#[test]
fn container_padding_shrink_wraps_child() {
    // Padding(10) around a 50×50 child, no forced size → 70×70.
    let laid = lay_out(
        Container::new()
            .padding(EdgeInsets::all(flui_geometry::px(10.0)))
            .child(SizedBox::square(50.0)),
        loose(1000.0),
    );
    assert_eq!(laid.size(laid.root()), size(70.0, 70.0));
}

#[test]
fn container_width_height_force_size_regardless_of_child() {
    let laid = lay_out(
        Container::new()
            .width(200.0)
            .height(120.0)
            .child(SizedBox::square(10.0)),
        loose(1000.0),
    );
    assert_eq!(laid.size(laid.root()), size(200.0, 120.0));
}

#[test]
fn container_aligns_child_within_forced_size() {
    let laid = lay_out(
        Container::new()
            .width(100.0)
            .height(100.0)
            .alignment(Alignment::CENTER)
            .child(SizedBox::square(20.0)),
        loose(1000.0),
    );
    assert_eq!(laid.size(laid.root()), size(100.0, 100.0));

    let inner = laid.only_child(laid.root());
    assert_eq!(laid.size(inner), size(20.0, 20.0));
    // Centered in 100×100: (100-20)/2 = 40 on each axis.
    assert_eq!(laid.offset(inner), offset(40.0, 40.0));
}

#[test]
fn container_childless_with_size_fills_to_size() {
    // No child + forced size: the childless placeholder is pinned by the
    // ConstrainedBox layer to the requested size.
    let laid = lay_out(Container::new().width(80.0).height(40.0), loose(1000.0));
    assert_eq!(laid.size(laid.root()), size(80.0, 40.0));
}

/// Counts `create_state` / `dispose` so optional-layer toggles can assert the
/// unkeyed child was reparented rather than recreated.
#[derive(Clone, StatefulView)]
struct StateProbe {
    creates: Arc<AtomicUsize>,
    disposes: Arc<AtomicUsize>,
}

struct StateProbeState {
    disposes: Arc<AtomicUsize>,
}

impl StatefulView for StateProbe {
    type State = StateProbeState;

    fn create_state(&self) -> Self::State {
        self.creates.fetch_add(1, Ordering::SeqCst);
        StateProbeState {
            disposes: Arc::clone(&self.disposes),
        }
    }
}

impl ViewState<StateProbe> for StateProbeState {
    fn build(&self, _view: &StateProbe, _ctx: &dyn BuildContext) -> impl IntoView {
        SizedBox::square(10.0)
    }

    fn dispose(&mut self) {
        self.disposes.fetch_add(1, Ordering::SeqCst);
    }
}

fn assert_child_state_preserved(creates: &AtomicUsize, disposes: &AtomicUsize, label: &str) {
    assert_eq!(
        creates.load(Ordering::SeqCst),
        1,
        "{label}: optional Container layer must not recreate the unkeyed child state"
    );
    assert_eq!(
        disposes.load(Ordering::SeqCst),
        0,
        "{label}: optional Container layer must not dispose the unkeyed child state"
    );
}

#[test]
fn container_optional_color_preserves_unkeyed_child_state() {
    let creates = Arc::new(AtomicUsize::new(0));
    let disposes = Arc::new(AtomicUsize::new(0));
    let child = StateProbe {
        creates: Arc::clone(&creates),
        disposes: Arc::clone(&disposes),
    };

    let mut laid = lay_out(Container::new().child(child.clone()), loose(1000.0));
    assert_eq!(creates.load(Ordering::SeqCst), 1);

    laid.pump_widget(
        Container::new()
            .color(Color::rgb(1, 2, 3))
            .child(child.clone()),
    );
    assert_child_state_preserved(&creates, &disposes, "color None→Some");

    laid.pump_widget(Container::new().child(child));
    assert_child_state_preserved(&creates, &disposes, "color Some→None");
}

#[test]
fn container_optional_alignment_preserves_unkeyed_child_state() {
    let creates = Arc::new(AtomicUsize::new(0));
    let disposes = Arc::new(AtomicUsize::new(0));
    let child = StateProbe {
        creates: Arc::clone(&creates),
        disposes: Arc::clone(&disposes),
    };

    let mut laid = lay_out(
        Container::new()
            .width(100.0)
            .height(100.0)
            .child(child.clone()),
        loose(1000.0),
    );
    assert_eq!(creates.load(Ordering::SeqCst), 1);

    laid.pump_widget(
        Container::new()
            .width(100.0)
            .height(100.0)
            .alignment(Alignment::CENTER)
            .child(child.clone()),
    );
    assert_child_state_preserved(&creates, &disposes, "alignment None→Some");

    laid.pump_widget(Container::new().width(100.0).height(100.0).child(child));
    assert_child_state_preserved(&creates, &disposes, "alignment Some→None");
}

#[test]
fn container_optional_padding_margin_decoration_transform_preserve_unkeyed_child_state() {
    let creates = Arc::new(AtomicUsize::new(0));
    let disposes = Arc::new(AtomicUsize::new(0));
    let child = StateProbe {
        creates: Arc::clone(&creates),
        disposes: Arc::clone(&disposes),
    };

    let mut laid = lay_out(Container::new().child(child.clone()), loose(1000.0));
    assert_eq!(creates.load(Ordering::SeqCst), 1);

    laid.pump_widget(
        Container::new()
            .padding(EdgeInsets::all(flui_geometry::px(4.0)))
            .child(child.clone()),
    );
    assert_child_state_preserved(&creates, &disposes, "padding");

    laid.pump_widget(
        Container::new()
            .padding(EdgeInsets::all(flui_geometry::px(4.0)))
            .margin(EdgeInsets::all(flui_geometry::px(2.0)))
            .child(child.clone()),
    );
    assert_child_state_preserved(&creates, &disposes, "margin");

    laid.pump_widget(
        Container::new()
            .padding(EdgeInsets::all(flui_geometry::px(4.0)))
            .margin(EdgeInsets::all(flui_geometry::px(2.0)))
            .decoration(BoxDecoration::with_color(Color::rgb(9, 8, 7)))
            .child(child.clone()),
    );
    assert_child_state_preserved(&creates, &disposes, "decoration");

    laid.pump_widget(
        Container::new()
            .padding(EdgeInsets::all(flui_geometry::px(4.0)))
            .margin(EdgeInsets::all(flui_geometry::px(2.0)))
            .decoration(BoxDecoration::with_color(Color::rgb(9, 8, 7)))
            .transform(Matrix4::identity())
            .child(child.clone()),
    );
    assert_child_state_preserved(&creates, &disposes, "transform");

    laid.pump_widget(Container::new().child(child));
    assert_child_state_preserved(&creates, &disposes, "strip all optional layers");
}

#[test]
fn container_optional_constraints_preserve_unkeyed_child_state() {
    let creates = Arc::new(AtomicUsize::new(0));
    let disposes = Arc::new(AtomicUsize::new(0));
    let child = StateProbe {
        creates: Arc::clone(&creates),
        disposes: Arc::clone(&disposes),
    };

    let mut laid = lay_out(Container::new().child(child.clone()), loose(1000.0));
    assert_eq!(creates.load(Ordering::SeqCst), 1);

    laid.pump_widget(
        Container::new()
            .width(80.0)
            .height(40.0)
            .child(child.clone()),
    );
    assert_child_state_preserved(&creates, &disposes, "width/height");

    laid.pump_widget(Container::new().child(child));
    assert_child_state_preserved(&creates, &disposes, "clear width/height");
}

#[test]
fn animated_container_optional_color_preserves_unkeyed_child_state() {
    let creates = Arc::new(AtomicUsize::new(0));
    let disposes = Arc::new(AtomicUsize::new(0));
    let child = StateProbe {
        creates: Arc::clone(&creates),
        disposes: Arc::clone(&disposes),
    };
    let vsync = Vsync::new();

    let mut laid = lay_out_animated(
        VsyncScope::new(
            vsync.clone(),
            AnimatedContainer::new(child.clone()).duration(Duration::from_millis(200)),
        ),
        loose(1000.0),
        vsync.clone(),
    );
    assert_eq!(creates.load(Ordering::SeqCst), 1);

    laid.pump_widget(VsyncScope::new(
        vsync.clone(),
        AnimatedContainer::new(child.clone())
            .color(Color::rgb(10, 20, 30))
            .duration(Duration::from_millis(200)),
    ));
    assert_child_state_preserved(&creates, &disposes, "AnimatedContainer color None→Some");

    laid.pump_widget(VsyncScope::new(
        vsync,
        AnimatedContainer::new(child).duration(Duration::from_millis(200)),
    ));
    assert_child_state_preserved(&creates, &disposes, "AnimatedContainer color Some→None");
}
