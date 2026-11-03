//! A widget made by its public constructors reports the line that called the constructor, not a
//! line inside the widget's own file, whichever constructor made it.

use flui_foundation::CreationSite;
use flui_foundation::geometry::{EdgeInsets, Matrix4, Size};
use flui_rendering::delegates::CenterLayoutDelegate;
use flui_view::{BoxedView, View};
use flui_widgets::prelude::*;

/// Where the widget says it was made, as `(file, line)`.
fn made_at(view: &dyn View) -> Option<(&'static str, u32)> {
    let site = view.creation_site();
    Some((site.file()?, site.line()?))
}

/// Each constructor of `Padding` records its caller.
///
/// Fails when a constructor that delegates to another loses `#[track_caller]`: the site then
/// names a line in `padding.rs`.
#[test]
fn every_padding_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    let cases: [(u32, Padding); 4] = [
        (line!(), Padding::new(EdgeInsets::all(1.0))),
        (line!(), Padding::all(1.0)),
        (line!(), Padding::symmetric(1.0, 2.0)),
        (line!(), Padding::only(1.0, 2.0, 3.0, 4.0)),
    ];
    for (line, padding) in cases {
        let (file, at) = made_at(&padding).expect("a constructor records a site");
        assert!(
            file.ends_with("creation_site.rs"),
            "{file}:{at} is not the caller"
        );
        assert_eq!(at, line);
    }
}

/// The view reports a site in this file on `line`, the line of the constructor call.
fn assert_caller(line: u32, view: &dyn View) {
    let (file, at) = made_at(view).expect("a constructor records a site");
    assert!(
        file.ends_with("creation_site.rs"),
        "{file}:{at} is not the caller"
    );
    assert_eq!(at, line);
}

#[test]
fn every_aspect_ratio_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &AspectRatio::new(2.0));
}

#[test]
fn every_constrained_box_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &ConstrainedBox::new(BoxConstraints::UNCONSTRAINED));
}

#[test]
fn every_baseline_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &Baseline::new(1.0, TextBaseline::Alphabetic));
}

#[test]
fn every_limited_box_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &LimitedBox::new(1.0, 2.0));
}

#[test]
fn every_rotated_box_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &RotatedBox::new(1));
}

#[test]
fn every_sized_overflow_box_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &SizedOverflowBox::new(Size::new(1.0, 2.0)));
}

#[test]
fn every_fractional_translation_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &FractionalTranslation::new(0.5, 0.5));
}

#[test]
fn every_align_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &Align::new(Alignment::CENTER));
}

#[test]
fn every_center_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &Center::new());
}

#[test]
fn every_intrinsic_width_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &IntrinsicWidth::new());
}

#[test]
fn every_intrinsic_height_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &IntrinsicHeight::new());
}

#[test]
fn every_fitted_box_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &FittedBox::new());
}

#[test]
fn every_fractionally_sized_box_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &FractionallySizedBox::new());
}

#[test]
fn every_unconstrained_box_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &UnconstrainedBox::new());
}

#[test]
fn every_overflow_box_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &OverflowBox::new());
}

#[test]
fn every_ignore_baseline_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &IgnoreBaseline::new());
}

#[test]
fn every_custom_single_child_layout_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    let delegate = CenterLayoutDelegate;
    assert_caller(line!(), &CustomSingleChildLayout::new(delegate));
}

#[test]
fn every_sized_box_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &SizedBox::new(1.0, 2.0));
    assert_caller(line!(), &SizedBox::square(1.0));
    assert_caller(line!(), &SizedBox::width(1.0));
    assert_caller(line!(), &SizedBox::height(1.0));
    assert_caller(line!(), &SizedBox::expand());
    assert_caller(line!(), &SizedBox::shrink());
}

#[test]
fn every_transform_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &Transform::new(Matrix4::identity()));
    assert_caller(line!(), &Transform::translate(1.0, 2.0));
    assert_caller(line!(), &Transform::scale(1.0, 2.0));
    assert_caller(line!(), &Transform::rotation(1.0));
}

#[test]
fn every_preferred_size_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    let size = Size::new(1.0, 2.0);
    assert_caller(line!(), &PreferredSize::new(size, SizedBox::shrink()));
}

#[test]
fn every_table_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &Table::new(Vec::new()));
}

#[test]
fn every_list_body_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &ListBody::new(Vec::<BoxedView>::new()));
}
