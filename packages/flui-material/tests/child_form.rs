//! The child rule of `docs/extending.md` for this catalog: a child Flutter
//! leaves nullable is set with `.child(…)`, and the widget lays out without
//! one.
//!
//! Each row mounts its widget once without a child and once with a marker
//! child, the same shape as `flui-widgets`' `child_form` table.

use crate::common::{lay_out, loose};
use flui_material::{Card, Dialog, FloatingActionButton, InkWell, Theme, ThemeData};
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::SizedBox;

/// The child each row is given: a view type no widget of this catalog
/// builds, so counting it counts only what the row was handed.
#[derive(Clone, StatelessView)]
struct Marker;

impl StatelessView for Marker {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        SizedBox::new(10.0, 10.0)
    }
}

/// One widget's two forms under the light theme: built with no child, and
/// built around `child`.
struct Row {
    name: &'static str,
    build: fn(Option<Marker>) -> BoxedView,
}

/// Sets the child when the row is given one.
macro_rules! row {
    ($name:literal, $widget:expr) => {
        Row {
            name: $name,
            build: |child| match child {
                Some(child) => Theme::new(ThemeData::light(), $widget.child(child)).boxed(),
                None => Theme::new(ThemeData::light(), $widget).boxed(),
            },
        }
    };
}

fn rows() -> [Row; 4] {
    [
        row!("InkWell", InkWell::new()),
        row!("Card", Card::new()),
        row!("Dialog", Dialog::new()),
        row!("FloatingActionButton", FloatingActionButton::new()),
    ]
}

/// Every optional-child widget of the catalog lays out without its child
/// and mounts the one it is given.
pub fn optional_child_is_a_builder_and_may_be_absent() {
    let mut failures = Vec::<String>::new();
    for row in rows() {
        let mut empty = lay_out((row.build)(None), loose(200.0));
        let empty_markers = empty.count_elements_by_view_type::<Marker>();
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

        let mut full = lay_out((row.build)(Some(Marker)), loose(200.0));
        let markers = full.count_elements_by_view_type::<Marker>();
        if markers != 1 {
            failures.push(format!(
                "{}: {markers} child(ren) mounted, expected the one given",
                row.name
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
