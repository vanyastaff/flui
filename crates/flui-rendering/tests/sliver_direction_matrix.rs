//! Pure-math regression matrix for sliver direction composition (Wave 3.1).
//!
//! Covers 4 axis directions × 2 growth directions × 3 concerns:
//! effective axis, paint/sign sizing, and scroll-direction composition.

use flui_rendering::{
    constraints::{GrowthDirection, apply_growth_direction_to_scroll_direction, right_way_up},
    traits::RenderSliver,
    view::ScrollDirection,
};
use flui_types::{Size, geometry::px, layout::AxisDirection::*};

struct DirectionProbe {
    constraints: flui_rendering::constraints::SliverConstraints,
}

impl std::fmt::Debug for DirectionProbe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DirectionProbe").finish_non_exhaustive()
    }
}

impl DirectionProbe {
    fn new(axis_direction: flui_types::layout::AxisDirection, growth: GrowthDirection) -> Self {
        use flui_rendering::constraints::SliverConstraints;
        use flui_rendering::view::ScrollDirection;

        let cross_axis_direction = match axis_direction {
            TopToBottom | BottomToTop => LeftToRight,
            LeftToRight | RightToLeft => TopToBottom,
        };

        Self {
            constraints: SliverConstraints::new(
                axis_direction,
                growth,
                ScrollDirection::Idle,
                0.0,
                0.0,
                0.0,
                100.0,
                40.0,
                cross_axis_direction,
                100.0,
                100.0,
                0.0,
            ),
        }
    }
}

impl RenderSliver for DirectionProbe {
    type Arity = flui_tree::Leaf;
    type ParentData = flui_rendering::parent_data::SliverParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut flui_rendering::context::SliverLayoutContext<'_, Self::Arity, Self::ParentData>,
    ) {
        let _ = ctx;
    }

    fn geometry(&self) -> &flui_rendering::constraints::SliverGeometry {
        static ZERO: flui_rendering::constraints::SliverGeometry =
            flui_rendering::constraints::SliverGeometry::ZERO;
        &ZERO
    }

    fn constraints(&self) -> &flui_rendering::constraints::SliverConstraints {
        &self.constraints
    }

    fn set_geometry(&mut self, _: flui_rendering::constraints::SliverGeometry) {}

    fn hit_test(
        &self,
        _: &mut flui_rendering::context::SliverHitTestContext<'_, Self::Arity, Self::ParentData>,
    ) -> bool {
        false
    }
}

impl flui_foundation::Diagnosticable for DirectionProbe {}
impl flui_rendering::traits::PaintEffectsCapability for DirectionProbe {}
impl flui_rendering::traits::SemanticsCapability for DirectionProbe {}
impl flui_rendering::traits::HotReloadCapability for DirectionProbe {}

#[test]
fn sliver_direction_matrix_eight_by_three() {
    let cases = [
        (
            TopToBottom,
            GrowthDirection::Forward,
            TopToBottom,
            Size::new(px(40.0), px(25.0)),
            true,
        ),
        (
            TopToBottom,
            GrowthDirection::Reverse,
            BottomToTop,
            Size::new(px(40.0), px(-25.0)),
            false,
        ),
        (
            BottomToTop,
            GrowthDirection::Forward,
            BottomToTop,
            Size::new(px(40.0), px(-25.0)),
            false,
        ),
        (
            BottomToTop,
            GrowthDirection::Reverse,
            TopToBottom,
            Size::new(px(40.0), px(25.0)),
            true,
        ),
        (
            LeftToRight,
            GrowthDirection::Forward,
            LeftToRight,
            Size::new(px(25.0), px(40.0)),
            true,
        ),
        (
            LeftToRight,
            GrowthDirection::Reverse,
            RightToLeft,
            Size::new(px(-25.0), px(40.0)),
            false,
        ),
        (
            RightToLeft,
            GrowthDirection::Forward,
            RightToLeft,
            Size::new(px(-25.0), px(40.0)),
            false,
        ),
        (
            RightToLeft,
            GrowthDirection::Reverse,
            LeftToRight,
            Size::new(px(25.0), px(40.0)),
            true,
        ),
    ];

    for (axis, growth, expected_axis, expected_size, expected_right_way_up) in cases {
        let probe = DirectionProbe::new(axis, growth);

        assert_eq!(
            growth.apply_to_axis_direction(axis),
            expected_axis,
            "effective axis: axis={axis:?}, growth={growth:?}",
        );
        assert_eq!(
            probe.get_absolute_size_relative_to_origin(25.0),
            expected_size,
            "absolute size: axis={axis:?}, growth={growth:?}",
        );
        assert_eq!(
            right_way_up(axis, growth),
            expected_right_way_up,
            "right_way_up: axis={axis:?}, growth={growth:?}",
        );
        assert_eq!(
            apply_growth_direction_to_scroll_direction(ScrollDirection::Forward, growth),
            if growth.is_reverse() {
                ScrollDirection::Reverse
            } else {
                ScrollDirection::Forward
            },
            "scroll direction from Forward user scroll: axis={axis:?}, growth={growth:?}",
        );
        assert_eq!(
            apply_growth_direction_to_scroll_direction(ScrollDirection::Reverse, growth),
            if growth.is_reverse() {
                ScrollDirection::Forward
            } else {
                ScrollDirection::Reverse
            },
            "scroll direction from Reverse user scroll: axis={axis:?}, growth={growth:?}",
        );
    }
}
