//! Integration tests for [`CupertinoNavigationBar`] — the 44pt persistent
//! height contract, the hairline border's alpha, background
//! resolution against the theme, the top safe-area inset, and that
//! leading/middle/trailing all actually reach the mounted render tree.
//!
//! Every mount wraps the bar in a [`MediaQuery`] ancestor: `SafeArea`
//! (`nav_bar.rs`'s own self-padding, matching `flui-material`'s `AppBar`
//! and its identical contract) reads `MediaQuery::of` unconditionally and panics
//! with no ancestor — see `flui-material/tests/app_bar.rs` for the same
//! precedent.

use crate::common;

use crate::page_scaffold::find_by_size;
use common::{lay_out, tight};
use flui_cupertino::CupertinoNavigationBar;
use flui_sdk::view::environment::{MediaQuery, MediaQueryData};
use flui_sdk::widgets::{SizedBox, Text};
use flui_testing::a11y::Role;

/// `leading`/`middle`/`trailing` all reach the mounted render tree, not just
/// the constructor's stored fields — proven by a delta against a bar with
/// none of the three set (whose own outer `SizedBox` already contributes one
/// `RenderConstrainedBox`, so an absolute count would be misleading).
pub fn leading_middle_and_trailing_all_mount() {
    let empty = lay_out(
        MediaQuery::new(MediaQueryData::default(), CupertinoNavigationBar::new()),
        tight(400.0, 44.0),
    );
    let empty_constrained_box_count = empty.find_all_by_render_type("RenderConstrainedBox").len();

    let laid = lay_out(
        MediaQuery::new(
            MediaQueryData::default(),
            CupertinoNavigationBar::new()
                .leading(SizedBox::new(20.0, 20.0))
                .middle(Text::new("Settings"))
                .trailing(SizedBox::new(20.0, 20.0)),
        ),
        tight(400.0, 44.0),
    );

    assert!(
        laid.try_find_by_render_type("RenderParagraph").is_some(),
        "the middle Text must mount as a RenderParagraph"
    );
    assert_eq!(
        laid.find_all_by_render_type("RenderConstrainedBox").len(),
        empty_constrained_box_count + 2,
        "both the leading and trailing SizedBox must mount, on top of the bar's own"
    );
}

/// The middle is the page title, so a screen reader's heading navigation
/// reaches it as a level-one heading named by its text.
pub fn the_middle_announces_as_a_level_one_heading() {
    let mut laid = lay_out(
        MediaQuery::new(
            MediaQueryData::default(),
            CupertinoNavigationBar::new().middle(Text::new("Settings")),
        ),
        tight(400.0, 44.0),
    );
    laid.enable_semantics();
    laid.pump();
    let tree = laid
        .a11y_tree()
        .expect("semantics enabled before the frame");

    let heading = tree
        .find(Role::Heading)
        .unwrap_or_else(|error| panic!("{error}\n{}", tree.describe()));
    assert_eq!(heading.label(), Some("Settings"), "{}", tree.describe());
    assert_eq!(heading.heading_level(), Some(1), "{}", tree.describe());
}

/// The default bottom border is one device pixel: `1 / device_pixel_ratio`
/// logical pixels tall, along the bar's bottom edge, in `0x4D000000` at every
/// ratio. Flutter spells it as width `0.0`
/// (`flutter/flutter@3.50.1 packages/flutter/lib/src/cupertino/nav_bar.dart:111`).
pub fn the_default_border_is_one_device_pixel() {
    use flui_sdk::painting::DrawOp;
    use flui_sdk::painting::styling::Color;

    for ratio in [1.0, 2.0, 3.0] {
        let media = MediaQueryData {
            device_pixel_ratio: ratio,
            ..MediaQueryData::default()
        };
        let laid = lay_out(
            MediaQuery::new(media, CupertinoNavigationBar::new()),
            tight(400.0, 44.0),
        );
        let borders: Vec<_> = laid
            .draw_ops()
            .into_iter()
            .filter_map(|command| match command.op {
                DrawOp::Rect { rect, paint } if paint.color == Color::from_argb(0x4D00_0000) => {
                    Some(rect)
                }
                _ => None,
            })
            .collect();
        let [border] = borders.as_slice() else {
            panic!("at ratio {ratio} the bar paints one border side: {borders:?}");
        };
        assert!(
            (border.height() - 1.0 / ratio).abs() < 1e-9,
            "at ratio {ratio} the border is one device pixel tall: {border:?}"
        );
        assert!(
            (border.max.y - 44.0).abs() < 1e-9 && (border.width() - 400.0).abs() < 1e-9,
            "at ratio {ratio} the border runs along the bar's bottom edge: {border:?}"
        );
    }
}

/// One layout of the toolbar: the slot widths, the direction, and where each
/// slot's box must land.
struct ToolbarCase {
    name: &'static str,
    direction: flui_sdk::foundation::TextDirection,
    leading_width: f64,
    middle_width: f64,
    trailing_width: f64,
    /// The left edges of the leading, middle and trailing boxes, and the
    /// middle box's laid-out width.
    expected_left: (f64, f64, f64),
    expected_middle_width: f64,
}

/// `middle` is centred on the bar's full width while it keeps 6 points from
/// `leading` and `trailing` (each 16 points in from its edge), moves toward
/// the side with room when centring would reach a side, and is limited to
/// the space between them when wider than that; in a right-to-left context
/// `leading` is on the right. The laid-out boxes never overlap. Flutter:
/// `NavigationToolbar` with `middleSpacing: 6.0`
/// (`flutter/flutter@3.50.1 packages/flutter/lib/src/cupertino/nav_bar.dart:1845`).
pub fn the_middle_is_centred_until_it_would_reach_a_side() {
    use flui_sdk::foundation::TextDirection;
    use flui_sdk::widgets::Directionality;

    // Heights tell the three boxes apart from each other and from the bar's
    // own 400x44 box: leading 10, middle 14, trailing 12.
    let cases = [
        ToolbarCase {
            name: "a short middle is centred on the full width",
            direction: TextDirection::Ltr,
            leading_width: 100.0,
            middle_width: 60.0,
            trailing_width: 20.0,
            // Centred: (400 - 60) / 2.
            expected_left: (16.0, 170.0, 364.0),
            expected_middle_width: 60.0,
        },
        ToolbarCase {
            name: "a middle that would reach leading starts after it",
            direction: TextDirection::Ltr,
            leading_width: 100.0,
            middle_width: 200.0,
            trailing_width: 20.0,
            // Leading's slot is 16 + 100, plus the 6-point spacing.
            expected_left: (16.0, 122.0, 364.0),
            expected_middle_width: 200.0,
        },
        ToolbarCase {
            name: "a middle that would reach trailing ends before it",
            direction: TextDirection::Ltr,
            leading_width: 20.0,
            middle_width: 200.0,
            trailing_width: 100.0,
            // Trailing's slot starts at 400 - 116; 6 points before it.
            expected_left: (16.0, 78.0, 284.0),
            expected_middle_width: 200.0,
        },
        ToolbarCase {
            name: "a middle wider than the gap is limited to it",
            direction: TextDirection::Ltr,
            leading_width: 100.0,
            middle_width: 300.0,
            trailing_width: 20.0,
            // 400 - 116 - 36 - 2 * 6.
            expected_left: (16.0, 122.0, 364.0),
            expected_middle_width: 236.0,
        },
        ToolbarCase {
            name: "right to left puts leading on the right",
            direction: TextDirection::Rtl,
            leading_width: 100.0,
            middle_width: 200.0,
            trailing_width: 20.0,
            // The left-to-right layout mirrored: 400 - 122 - 200 for middle.
            expected_left: (284.0, 78.0, 16.0),
            expected_middle_width: 200.0,
        },
    ];

    for case in cases {
        let name = case.name;
        let laid = lay_out(
            Directionality::new(
                case.direction,
                MediaQuery::new(
                    MediaQueryData::default(),
                    CupertinoNavigationBar::new()
                        .leading(SizedBox::new(case.leading_width, 10.0))
                        .middle(SizedBox::new(case.middle_width, 14.0))
                        .trailing(SizedBox::new(case.trailing_width, 12.0)),
                ),
            ),
            tight(400.0, 44.0),
        );
        let leading = find_by_size(&laid, case.leading_width, 10.0);
        let middle = find_by_size(&laid, case.expected_middle_width, 14.0);
        let trailing = find_by_size(&laid, case.trailing_width, 12.0);
        let rect = |id| {
            let offset = laid.absolute_offset(id);
            let size = laid.size(id);
            (offset.dx, offset.dy, offset.dx + size.width)
        };
        let (leading_left, leading_top, leading_right) = rect(leading);
        let (middle_left, middle_top, middle_right) = rect(middle);
        let (trailing_left, trailing_top, trailing_right) = rect(trailing);

        let (expected_leading, expected_middle, expected_trailing) = case.expected_left;
        for (slot, left, expected) in [
            ("leading", leading_left, expected_leading),
            ("middle", middle_left, expected_middle),
            ("trailing", trailing_left, expected_trailing),
        ] {
            assert!(
                (left - expected).abs() < 0.01,
                "{name}: {slot} starts at x={left}, expected {expected}"
            );
        }
        for (slot, top, height) in [
            ("leading", leading_top, 10.0),
            ("middle", middle_top, 14.0),
            ("trailing", trailing_top, 12.0),
        ] {
            assert!(
                (top - (44.0 - height) / 2.0).abs() < 0.01,
                "{name}: {slot} is centred vertically, at y={top}"
            );
        }
        let (left_side, right_side) = match case.direction {
            TextDirection::Ltr => (leading_right, trailing_left),
            TextDirection::Rtl => (trailing_right, leading_left),
        };
        assert!(
            middle_left >= left_side && middle_right <= right_side,
            "{name}: middle [{middle_left}, {middle_right}] overlaps a side \
             (sides end at {left_side} and start at {right_side})"
        );
    }
}

/// Behind a translucent background (the theme's default) each bar blurs its
/// backdrop with a sigma-10 Gaussian blur; behind an opaque one the same
/// filter stays mounted, switched off, and pushes no backdrop-filter layer.
/// No bar adds a clip. Flutter:
/// `flutter/flutter@3.50.1 packages/flutter/lib/src/cupertino/nav_bar.dart:252`.
pub fn a_translucent_bar_blurs_its_backdrop_and_an_opaque_one_does_not() {
    use flui_cupertino::{CupertinoTabBar, CupertinoTabBarItem};
    use flui_sdk::painting::styling::Color;
    use flui_sdk::view::BoxedView;
    use flui_sdk::view::prelude::*;
    use flui_sdk::widgets::{Icon, IconData};

    let opaque = Color::from_argb(0xFF20_2020);
    let tab_bar = || {
        CupertinoTabBar::new(vec![
            CupertinoTabBarItem::new(Icon::new(IconData::new(0xF3A1))).label("Home"),
            CupertinoTabBarItem::new(Icon::new(IconData::new(0xF3A2))).label("Settings"),
        ])
    };
    let cases: [(&str, BoxedView, bool, f64); 4] = [
        (
            "translucent navigation bar",
            CupertinoNavigationBar::new().boxed(),
            true,
            44.0,
        ),
        (
            "opaque navigation bar",
            CupertinoNavigationBar::new()
                .background_color(opaque)
                .boxed(),
            false,
            44.0,
        ),
        ("translucent tab bar", tab_bar().boxed(), true, 50.0),
        (
            "opaque tab bar",
            tab_bar().background_color(opaque).boxed(),
            false,
            50.0,
        ),
    ];
    for (name, bar, blurs, height) in cases {
        let laid = lay_out(
            MediaQuery::new(MediaQueryData::default(), bar),
            tight(400.0, height),
        );
        let filters = laid.find_all_by_render_type("RenderBackdropFilter");
        let [filter] = filters.as_slice() else {
            panic!("{name}: one backdrop filter, found {}", filters.len());
        };
        let size = laid.size(*filter);
        assert!(
            (size.width - 400.0).abs() < 0.01 && (size.height - height).abs() < 0.01,
            "{name}: the filter covers the bar: {size:?}"
        );
        let described = laid
            .render_debug_field(*filter, "filter")
            .unwrap_or_else(|| panic!("{name}: the filter describes its image filter"));
        assert!(
            described.contains("sigma_x: 10.0") && described.contains("sigma_y: 10.0"),
            "{name}: a sigma-10 blur: {described}"
        );
        let kinds = laid.layer_kinds();
        let filter_layers = kinds
            .iter()
            .filter(|&&kind| kind == "BackdropFilter")
            .count();
        assert_eq!(
            filter_layers,
            usize::from(blurs),
            "{name}: the filter is switched {}: {kinds:?}",
            if blurs { "on" } else { "off" }
        );
        assert!(
            laid.find_all_by_render_type("RenderClipRect").is_empty(),
            "{name}: the filter already covers only the bar; no clip"
        );
    }
}

/// A `leading` whose `create_state` is counted.
#[derive(Clone)]
struct CountedLeading(std::rc::Rc<std::cell::Cell<u32>>);

impl flui_sdk::view::View for CountedLeading {
    fn create_element(self: std::rc::Rc<Self>) -> flui_sdk::view::element::ElementKind {
        flui_sdk::view::element::ElementKind::stateful(self)
    }
}

impl flui_sdk::view::prelude::StatefulView for CountedLeading {
    type State = CountedLeadingState;

    fn create_state(&self, _ctx: &dyn flui_sdk::view::prelude::LifecycleContext) -> Self::State {
        self.0.set(self.0.get() + 1);
        CountedLeadingState
    }
}

struct CountedLeadingState;

impl flui_sdk::view::prelude::ViewState<CountedLeading> for CountedLeadingState {
    fn build(
        &self,
        _view: &CountedLeading,
        _ctx: &dyn flui_sdk::view::prelude::BuildContext,
    ) -> impl flui_sdk::view::prelude::IntoView {
        SizedBox::new(20.0, 20.0)
    }
}

/// A bar whose background crosses full opacity, either way, keeps its
/// slots' elements: the leading slot's state is created once, and the filter
/// is switched rather than mounted or removed.
pub fn a_bar_whose_background_crosses_full_opacity_keeps_its_slots_state() {
    use flui_sdk::painting::styling::Color;

    let created = std::rc::Rc::new(std::cell::Cell::new(0));
    let bar = |background: Color| {
        MediaQuery::new(
            MediaQueryData::default(),
            CupertinoNavigationBar::new()
                .leading(CountedLeading(std::rc::Rc::clone(&created)))
                .background_color(background),
        )
    };
    let translucent = Color::from_argb(0xF0F9_F9F9);
    let opaque = Color::from_argb(0xFFF9_F9F9);
    let mut laid = lay_out(bar(translucent), tight(400.0, 44.0));
    for (step, background) in [("opaque", opaque), ("translucent again", translucent)] {
        laid.pump_widget(bar(background));
        assert_eq!(
            created.get(),
            1,
            "{step}: the leading slot keeps the state it was created with"
        );
    }
}
