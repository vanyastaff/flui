//! Composition parity and state-stability tests for [`Container`].

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::common::{lay_out, loose, offset, size};
use flui_foundation::geometry::{EdgeInsets, Matrix4};
use flui_painting::Alignment;
use flui_painting::styling::BoxDecoration;
use flui_painting::styling::Color;
use flui_view::prelude::{BuildContext, StatefulView};
use flui_view::{IntoView, ViewState};
use flui_widgets::{Container, IntrinsicWidth, LayoutBuilder, SizedBox};

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

/// Counts `create_state` / `dispose` so optional-layer toggles can assert the
/// unkeyed child was neither recreated nor disposed.
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
            .padding(EdgeInsets::all(4.0))
            .child(child.clone()),
    );
    assert_child_state_preserved(&creates, &disposes, "padding");

    laid.pump_widget(
        Container::new()
            .padding(EdgeInsets::all(4.0))
            .margin(EdgeInsets::all(2.0))
            .child(child.clone()),
    );
    assert_child_state_preserved(&creates, &disposes, "margin");

    laid.pump_widget(
        Container::new()
            .padding(EdgeInsets::all(4.0))
            .margin(EdgeInsets::all(2.0))
            .decoration(BoxDecoration::with_color(Color::rgb(9, 8, 7)))
            .child(child.clone()),
    );
    assert_child_state_preserved(&creates, &disposes, "decoration");

    laid.pump_widget(
        Container::new()
            .padding(EdgeInsets::all(4.0))
            .margin(EdgeInsets::all(2.0))
            .decoration(BoxDecoration::with_color(Color::rgb(9, 8, 7)))
            .transform(Matrix4::identity())
            .child(child.clone()),
    );
    assert_child_state_preserved(&creates, &disposes, "transform");

    laid.pump_widget(Container::new().child(child));
    assert_child_state_preserved(&creates, &disposes, "strip all optional layers");
}

/// A tight additional width must answer an intrinsic query without asking the
/// child — matching `RenderConstrainedBox`. `LayoutBuilder` logs (Flutter
/// throws) if asked; the control below proves that log is reachable.
#[test]
fn container_tight_width_does_not_query_layout_builder_intrinsics() {
    const NEEDLE: &str = "does not support intrinsic dimensions";

    let ((), log) = flui_testing::log_capture::capture(|| {
        let _ = lay_out(
            IntrinsicWidth::new().child(LayoutBuilder::new(|_ctx, _c| SizedBox::square(10.0))),
            loose(200.0),
        );
    });
    assert!(
        log.count_containing(NEEDLE) >= 1,
        "control: LayoutBuilder under IntrinsicWidth must emit the unsupported-intrinsics \
         error, got {log}"
    );

    let (laid, log) = flui_testing::log_capture::capture(|| {
        lay_out(
            IntrinsicWidth::new().child(
                Container::new()
                    .width(100.0)
                    .child(LayoutBuilder::new(|_ctx, _c| SizedBox::square(10.0))),
            ),
            loose(200.0),
        )
    });
    assert_eq!(laid.size(laid.root()).width, 100.0);
    assert_eq!(
        log.count_containing(NEEDLE),
        0,
        "a tight Container width must answer the intrinsic without asking LayoutBuilder: {log}"
    );
}
