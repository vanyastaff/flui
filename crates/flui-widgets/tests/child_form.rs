//! The child rule of `docs/extending.md`: a child Flutter leaves nullable is
//! set with `.child(…)`, and the widget lays out without one.
//!
//! Each row mounts its widget once without a child and once with a marker
//! child. A widget whose child is required takes it in `new`, which the
//! compiler holds; it has no row here.

use std::rc::Rc;

use crate::common::{lay_out, loose};
use flui_animation::{Animation, ConstantAnimation};
use flui_foundation::geometry::{Angle, EdgeInsets};
use flui_objects::TranslationFraction;
use flui_painting::Alignment;
use flui_view::{BoxedView, ViewExt};
use flui_widgets::{
    AnimatedAlign, AnimatedContainer, AnimatedOpacity, AnimatedPadding, AnimatedRotation,
    AnimatedScale, AnimatedSize, AnimatedSlide, FadeTransition, RotationTransition,
    ScaleTransition, SizeTransition, SizedBox, SlideTransition,
};

/// One widget's two forms: built with no child, and built around `child`.
struct Row {
    name: &'static str,
    build: fn(Option<BoxedView>) -> BoxedView,
}

fn scalar(value: f64) -> Rc<dyn Animation<f64>> {
    Rc::new(ConstantAnimation::new(value))
}

/// Sets the child when the row is given one.
macro_rules! row {
    ($name:literal, $widget:expr) => {
        Row {
            name: $name,
            build: |child| match child {
                Some(child) => $widget.child(child).boxed(),
                None => $widget.boxed(),
            },
        }
    };
}

fn rows() -> [Row; 13] {
    [
        row!("AnimatedOpacity", AnimatedOpacity::new(0.5)),
        row!("AnimatedScale", AnimatedScale::new(2.0)),
        row!(
            "AnimatedRotation",
            AnimatedRotation::new(Angle::from_turns(0.25))
        ),
        row!("AnimatedAlign", AnimatedAlign::new(Alignment::CENTER)),
        row!(
            "AnimatedPadding",
            AnimatedPadding::new(EdgeInsets::all(4.0))
        ),
        row!(
            "AnimatedSlide",
            AnimatedSlide::new(TranslationFraction::new(0.5, 0.0))
        ),
        row!("AnimatedContainer", AnimatedContainer::new()),
        row!("AnimatedSize", AnimatedSize::new()),
        row!("FadeTransition", FadeTransition::new(scalar(0.5))),
        row!("ScaleTransition", ScaleTransition::new(scalar(2.0))),
        row!("RotationTransition", RotationTransition::new(scalar(0.25))),
        row!("SizeTransition", SizeTransition::new(scalar(0.5))),
        row!(
            "SlideTransition",
            SlideTransition::new(Rc::new(ConstantAnimation::new(TranslationFraction::new(
                0.5, 0.0
            ))))
        ),
    ]
}

/// Every optional-child widget lays out without its child and mounts the one
/// it is given.
pub(crate) fn optional_child_is_a_builder_and_may_be_absent() {
    let mut failures = Vec::<String>::new();
    for row in rows() {
        let mut empty = lay_out((row.build)(None), loose(100.0));
        let empty_markers = empty.count_elements_by_view_type::<SizedBox>();
        if empty_markers != 0 {
            failures.push(format!(
                "{}: {empty_markers} child(ren) mounted with none given",
                row.name
            ));
        }
        let size = empty.size(empty.root());
        if !(size.width.is_finite() && size.height.is_finite()) {
            failures.push(format!("{}: childless size {size:?}", row.name));
        }

        let mut full = lay_out(
            (row.build)(Some(SizedBox::new(10.0, 10.0).boxed())),
            loose(100.0),
        );
        let markers = full.count_elements_by_view_type::<SizedBox>();
        if markers != 1 {
            failures.push(format!(
                "{}: {markers} child(ren) mounted, expected the one given",
                row.name
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
