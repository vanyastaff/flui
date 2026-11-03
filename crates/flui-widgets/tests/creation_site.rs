//! A widget made by its public constructors reports the line that called the constructor, not a
//! line inside the widget's own file, whichever constructor made it.

use std::rc::Rc;

use flui_animation::ConstantAnimation;
use flui_foundation::CreationSite;
use flui_foundation::geometry::{EdgeInsets, Matrix4, Size};
use flui_interaction::focus::FocusNode;
use flui_objects::ClipSourceToken;
use flui_painting::paint::{Image as PixelImage, ImageFilter, Path, TextureId};
use flui_painting::styling::{BoxDecoration, Color};
use flui_painting::typography::{TextSpan, TextStyle};
use flui_rendering::delegates::CenterLayoutDelegate;
use flui_rendering::semantics::SemanticsConfiguration;
use flui_view::{BoxedView, View};
use flui_widgets::prelude::*;
use flui_widgets::{
    AnimatedDefaultTextStyle, AnimatedIcon, AnimatedIconData, BackdropFilter, BackdropGroup,
    ClipOval, ClipPath, ClipRRect, ClipRSuperellipse, ClipRect, ColoredBox, Column, CustomPaint,
    DecoratedBox, DefaultSelectionControls, EditableText, ExcludeSemantics, Flex, IconData,
    IgnorePointer, Image, IndexedSemantics, MemoryImage, MergeSemantics, Opacity, PhysicalModel,
    PhysicalShape, RawTextField, RepaintBoundary, RichText, Row, SelectionArea, SelectionContainer,
    Semantics, SliverSemantics, Spacer, Text, TextEditingController, Texture, ToolbarItems, Wrap,
};

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

#[test]
fn every_clip_rect_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &ClipRect::new());
}

#[test]
fn every_clip_oval_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &ClipOval::new());
}

#[test]
fn every_clip_rrect_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &ClipRRect::new());
    assert_caller(line!(), &ClipRRect::circular(4.0));
}

#[test]
fn every_clip_rsuperellipse_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &ClipRSuperellipse::new());
    assert_caller(line!(), &ClipRSuperellipse::circular(4.0));
}

#[test]
fn every_clip_path_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    fn clip(_: Size) -> Path {
        Path::new()
    }
    let source = ClipSourceToken::fresh();
    assert_caller(line!(), &ClipPath::new(clip));
    assert_caller(line!(), &ClipPath::with_source(source, clip));
}

#[test]
fn every_decorated_box_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &DecoratedBox::new(BoxDecoration::default()));
}

#[test]
fn every_colored_box_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &ColoredBox::new(Color::BLACK));
}

#[test]
fn every_custom_paint_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &CustomPaint::new());
}

#[test]
fn every_opacity_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &Opacity::new(0.5));
}

#[test]
fn every_repaint_boundary_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &RepaintBoundary::new());
}

#[test]
fn every_ignore_pointer_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &IgnorePointer::new());
}

#[test]
fn every_backdrop_filter_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &BackdropFilter::new(ImageFilter::blur(1.0)));
}

#[test]
fn every_backdrop_group_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &BackdropGroup::new(SizedBox::shrink()));
}

#[test]
fn every_spacer_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &Spacer::new());
}

#[test]
fn every_flex_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    let (direction, children) = (
        flui_objects::FlexDirection::Vertical,
        Vec::<BoxedView>::new(),
    );
    assert_caller(line!(), &Flex::new(direction, children));
}

#[test]
fn every_row_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &Row::new(Vec::<BoxedView>::new()));
}

#[test]
fn every_column_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &Column::new(Vec::<BoxedView>::new()));
}

#[test]
fn every_wrap_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &Wrap::new(Vec::<BoxedView>::new()));
}

#[test]
fn every_physical_model_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &PhysicalModel::new(Color::BLACK));
}

#[test]
fn every_physical_shape_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &PhysicalShape::new(|_| Path::new(), Color::BLACK));
}

#[test]
fn every_image_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &Image::new(MemoryImage::new(vec![0_u8])));
    let pixels = PixelImage::from_rgba8(1, 1, vec![255_u8; 4]);
    assert_caller(line!(), &Image::from_image(pixels));
    assert_caller(line!(), &Image::memory(vec![0_u8]));
    assert_caller(line!(), &Image::file("missing.png"));
}

#[test]
fn every_texture_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &Texture::new(TextureId::new(1)));
}

#[test]
fn every_icon_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &Icon::new(IconData::new(0xe000)));
    assert_caller(line!(), &Icon::none());
}

#[test]
fn every_animated_icon_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    const DATA: AnimatedIconData =
        AnimatedIconData::from_static(24.0, &[0.0, 1.0], &[], &[], false);
    let progress = ConstantAnimation::new(0.0);
    assert_caller(line!(), &AnimatedIcon::new(DATA, progress));
    let shared = Rc::new(ConstantAnimation::new(0.0));
    assert_caller(line!(), &AnimatedIcon::shared(DATA, shared));
}

#[test]
fn every_text_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &Text::new("hello"));
}

#[test]
fn every_rich_text_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &RichText::new(TextSpan::new("hello")));
}

#[test]
fn every_animated_default_text_style_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    let (style, child) = (TextStyle::default(), SizedBox::shrink());
    assert_caller(line!(), &AnimatedDefaultTextStyle::new(style, child));
}

#[test]
fn every_editable_text_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    let (controller, focus) = (TextEditingController::new(), FocusNode::new());
    assert_caller(line!(), &EditableText::new(controller, focus));
}

#[test]
fn every_raw_text_field_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &RawTextField::new(TextEditingController::new()));
}

#[test]
fn every_selection_area_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &SelectionArea::new(SizedBox::shrink()));
}

#[test]
fn every_selection_container_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &SelectionContainer::disabled(SizedBox::shrink()));
}

#[test]
fn every_default_selection_controls_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &DefaultSelectionControls::new(SizedBox::shrink()));
}

#[test]
fn every_toolbar_items_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &ToolbarItems::new(Vec::new()));
}

#[test]
fn every_semantics_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &Semantics::new());
    let config = SemanticsConfiguration::new();
    assert_caller(line!(), &Semantics::from_configuration(config));
}

#[test]
fn every_sliver_semantics_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &SliverSemantics::new());
    let config = SemanticsConfiguration::new();
    assert_caller(line!(), &SliverSemantics::from_configuration(config));
}

#[test]
fn every_merge_semantics_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &MergeSemantics::new());
}

#[test]
fn every_indexed_semantics_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &IndexedSemantics::new(0));
}

#[test]
fn every_exclude_semantics_constructor_records_its_caller() {
    if !CreationSite::KEPT {
        return;
    }
    assert_caller(line!(), &ExcludeSemantics::new());
}
