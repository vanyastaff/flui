//! Every animation controller a framework widget creates carries a debug
//! label, so a diagnostic names the widget that owns the controller.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use flui_animation::Vsync;
use flui_foundation::geometry::{Angle, EdgeInsets};
use flui_painting::Alignment;
use flui_painting::styling::Color;
use flui_view::{BoxedView, View, ViewExt};
use flui_widgets::{
    ActivityIndicator, AnimatedAlign, AnimatedContainer, AnimatedOpacity, AnimatedPadding,
    AnimatedRotation, AnimatedSize, Center, ColoredBox, DismissDirection, Dismissible,
    InteractiveViewer, MotionBuilder, RefreshController, RefreshIndicator, ScrollController,
    SizedBox, VsyncScope,
};
use tracing::Subscriber;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::prelude::*;

use crate::common::{lay_out_animated, loose};

/// The `label` of every `animation_controller` span opened while recording.
struct Labels(Arc<Mutex<Vec<String>>>);

#[derive(Default)]
struct Label(Option<String>);

impl tracing::field::Visit for Label {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "label" {
            self.0 = Some(format!("{value:?}"));
        }
    }
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "label" {
            self.0 = Some(value.to_owned());
        }
    }
}

impl<S: Subscriber> Layer<S> for Labels {
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        _: &tracing::span::Id,
        _: Context<'_, S>,
    ) {
        if attrs.metadata().name() == "animation_controller" {
            let mut label = Label::default();
            attrs.record(&mut label);
            self.0
                .lock()
                .expect("labels")
                .push(label.0.unwrap_or_default());
        }
    }
}

/// The labels of the controllers `view` creates when it mounts and runs a
/// first frame. An unlabeled controller records an empty label.
fn labels(view: impl FnOnce(Vsync) -> BoxedView, size: f64) -> Vec<String> {
    flui_testing::log_capture::disarm_interest_cache();
    let captured = Arc::new(Mutex::new(Vec::new()));
    let subscriber = tracing_subscriber::registry().with(Labels(Arc::clone(&captured)));
    tracing::subscriber::with_default(subscriber, || {
        let vsync = Vsync::new();
        let mut laid = lay_out_animated(view(vsync.clone()), loose(size), vsync);
        laid.pump_for(Duration::ZERO);
    });
    captured.lock().expect("labels").clone()
}

fn assert_labels(view: impl FnOnce(Vsync) -> BoxedView, expected: &[&str]) {
    let mut labels = labels(view, 400.0);
    labels.sort_unstable();
    let mut expected: Vec<String> = expected.iter().map(|label| (*label).to_owned()).collect();
    expected.sort_unstable();
    assert_eq!(labels, expected);
}

fn scoped(vsync: Vsync, child: impl View) -> BoxedView {
    VsyncScope::new(vsync, child).boxed()
}

fn activity_indicator() {
    assert_labels(
        |vsync| scoped(vsync, Center::new().child(ActivityIndicator::new())),
        &["ActivityIndicator"],
    );
}

fn animated_size() {
    assert_labels(
        |vsync| {
            scoped(
                vsync,
                AnimatedSize::new()
                    .duration(Duration::from_millis(200))
                    .child(SizedBox::new(20.0, 20.0)),
            )
        },
        &["AnimatedSize"],
    );
}

fn implicit_single_values() {
    assert_labels(
        |vsync| scoped(vsync, AnimatedOpacity::new(0.5, SizedBox::new(10.0, 10.0))),
        &["AnimatedOpacity"],
    );
    assert_labels(
        |vsync| {
            scoped(
                vsync,
                AnimatedPadding::new(EdgeInsets::all(4.0), SizedBox::new(10.0, 10.0)),
            )
        },
        &["AnimatedPadding"],
    );
    assert_labels(
        |vsync| {
            scoped(
                vsync,
                AnimatedRotation::new(Angle::from_turns(0.25), SizedBox::new(10.0, 10.0)),
            )
        },
        &["AnimatedRotation"],
    );
}

fn implicit_properties() {
    assert_labels(
        |vsync| {
            scoped(
                vsync,
                AnimatedAlign::new(Alignment::CENTER, SizedBox::new(10.0, 10.0))
                    .width_factor(1.0)
                    .height_factor(1.0),
            )
        },
        &[
            "AnimatedAlign.alignment",
            "AnimatedAlign.width_factor",
            "AnimatedAlign.height_factor",
        ],
    );
    assert_labels(
        |vsync| {
            scoped(
                vsync,
                AnimatedContainer::new(SizedBox::shrink())
                    .width(40.0)
                    .height(30.0)
                    .padding(EdgeInsets::all(2.0))
                    .margin(EdgeInsets::all(2.0))
                    .alignment(Alignment::CENTER)
                    .color(Color::rgb(1, 2, 3)),
            )
        },
        &[
            "AnimatedContainer.alignment",
            "AnimatedContainer.color",
            "AnimatedContainer.height",
            "AnimatedContainer.margin",
            "AnimatedContainer.padding",
            "AnimatedContainer.width",
        ],
    );
}

fn interactive_viewer() {
    let labels = labels(
        |vsync| {
            scoped(
                vsync,
                InteractiveViewer::new().child(SizedBox::new(300.0, 300.0)),
            )
        },
        300.0,
    );
    assert!(
        labels
            .iter()
            .all(|label| label == "InteractiveViewer.fling")
            && !labels.is_empty(),
        "{labels:?}"
    );
}

fn refresh_indicator() {
    let labels = labels(
        |vsync| {
            scoped(
                vsync,
                RefreshIndicator::new()
                    .controller(RefreshController::new())
                    .scroll_controller(ScrollController::new())
                    .child(SizedBox::new(300.0, 5000.0)),
            )
        },
        300.0,
    );
    assert_eq!(labels, ["RefreshIndicator.fling"]);
}

fn dismissible() {
    let labels = labels(
        |vsync| {
            scoped(
                vsync,
                Dismissible::new(ColoredBox::new(Color::rgb(10, 20, 30)))
                    .direction(DismissDirection::Horizontal),
            )
        },
        300.0,
    );
    assert_eq!(labels, ["Dismissible.move"]);
}

#[test]
fn framework_controllers_carry_their_widget_label() {
    crate::common::cases::run_cases(
        "framework_controllers_carry_their_widget_label",
        &[
            ("activity indicator", activity_indicator),
            ("animated size", animated_size),
            ("implicit single values", implicit_single_values),
            ("implicit properties", implicit_properties),
            ("interactive viewer", interactive_viewer),
            ("refresh indicator", refresh_indicator),
            ("dismissible", dismissible),
        ],
    );
}
