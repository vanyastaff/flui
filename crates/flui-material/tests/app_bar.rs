//! `AppBar` widget-level integration coverage — mounts a real app bar through
//! the full render pipeline (`tests/common/mod.rs`, matching
//! `tests/material.rs`/`tests/elevated_button.rs`'s established pattern).
//!
//! `AppBar` composes `Theme::of` (M3 token defaults) and `MediaQuery::of`
//! (the top safe-area inset) — both ambient reads that only resolve through
//! a real mount, so these tests prove the composition end to end rather than
//! re-checking `app_bar.rs`'s own unit-tested `resolve_style` formula.

mod common;

use common::{lay_out, loose, tight};
use flui_material::{AppBar, AppBarThemeData, Theme, ThemeData, ThemeDataOverrides};
use flui_types::geometry::px;
use flui_types::{EdgeInsets, Size};
use flui_view::prelude::*;
use flui_widgets::{
    MediaQuery, MediaQueryData, Navigator, NavigatorHandle, PreferredSize, SimpleRoute, SizedBox,
    Text,
};

/// `_ElevatedButtonDefaultsM3`'s sibling formatting helper (see
/// `tests/elevated_button.rs`'s `color_property`): the exact `Debug` string
/// `RenderPhysicalShape` writes into its `"color"` diagnostics property, so a
/// test can compare against a resolved `Color` without downcasting.
fn color_property(color: flui_types::Color) -> String {
    format!("{color:?}")
}

#[test]
fn standalone_app_bar_consumes_the_top_padding_itself() {
    // No Scaffold at all: an AppBar mounted directly under a MediaQuery that
    // reports a 24px top safe-area inset (a notch/status bar) must reserve
    // that inset on its own — the "consumes the top inset itself" contract
    // (`app_bar.rs`'s module docs).
    let media_query = MediaQueryData {
        padding: EdgeInsets::new(px(24.0), px(0.0), px(0.0), px(0.0)),
        ..MediaQueryData::default()
    };
    let laid = lay_out(
        Theme::new(
            ThemeData::light(),
            MediaQuery::new(media_query, AppBar::new().title(Text::new("Title"))),
        ),
        loose(400.0),
    );

    let root = laid.root();
    assert_eq!(
        laid.size(root).height,
        px(56.0 + 24.0),
        "a primary AppBar must add the ambient MediaQuery top padding to its own \
         toolbar_height, unassisted by any Scaffold",
    );
}

#[test]
fn app_bar_with_no_top_padding_is_exactly_the_toolbar_height() {
    let laid = lay_out(
        Theme::new(
            ThemeData::light(),
            MediaQuery::new(
                MediaQueryData::default(),
                AppBar::new().title(Text::new("Title")),
            ),
        ),
        loose(400.0),
    );

    let root = laid.root();
    assert_eq!(
        laid.size(root).height,
        px(56.0),
        "with a zero MediaQuery padding, the app bar's height must be exactly \
         the default toolbar_height",
    );
}

#[test]
fn theme_defaults_apply_surface_background_and_zero_elevation() {
    let theme = ThemeData::light();
    let colors = theme.color_scheme;
    let laid = lay_out(
        Theme::new(
            theme,
            MediaQuery::new(
                MediaQueryData::default(),
                AppBar::new().title(Text::new("Title")),
            ),
        ),
        loose(400.0),
    );

    let material = laid
        .find_by_render_type("RenderPhysicalShape")
        .expect("AppBar must compose a Material (RenderPhysicalShape) surface");

    assert_eq!(
        laid.render_property(material, "color"),
        Some(color_property(colors.surface)),
        "an AppBar with no background_color override must resolve _AppBarDefaultsM3's \
         ColorScheme.surface",
    );
    assert_eq!(
        laid.render_property(material, "elevation"),
        Some("0".to_string()),
        "an AppBar with no elevation override must resolve _AppBarDefaultsM3's 0.0",
    );
}

#[test]
fn background_color_override_replaces_the_theme_default() {
    let overridden = flui_types::Color::rgb(10, 20, 30);
    let laid = lay_out(
        Theme::new(
            ThemeData::light(),
            MediaQuery::new(
                MediaQueryData::default(),
                AppBar::new()
                    .title(Text::new("Title"))
                    .background_color(overridden),
            ),
        ),
        loose(400.0),
    );

    let material = laid
        .find_by_render_type("RenderPhysicalShape")
        .expect("AppBar must compose a Material (RenderPhysicalShape) surface");
    assert_eq!(
        laid.render_property(material, "color"),
        Some(color_property(overridden)),
        "an explicit background_color must win over the theme default",
    );
}

/// The middle cascade tier, proven end to end: a `ThemeData.app_bar_theme`
/// with a custom `background_color` reaches the mounted `Material`, with no
/// widget-level `background_color` in the way.
#[test]
fn app_bar_theme_slot_reaches_the_mounted_materials_background_color() {
    let themed_background = flui_types::Color::rgb(44, 55, 66);
    let theme = ThemeData::light().copy_with(ThemeDataOverrides {
        app_bar_theme: Some(AppBarThemeData {
            background_color: Some(themed_background),
            ..Default::default()
        }),
        ..Default::default()
    });

    let laid = lay_out(
        Theme::new(
            theme,
            MediaQuery::new(
                MediaQueryData::default(),
                AppBar::new().title(Text::new("Title")),
            ),
        ),
        loose(400.0),
    );

    let material = laid
        .find_by_render_type("RenderPhysicalShape")
        .expect("AppBar must compose a Material (RenderPhysicalShape) surface");
    assert_eq!(
        laid.render_property(material, "color"),
        Some(color_property(themed_background)),
        "a configured app_bar_theme.background_color must reach the mounted Material",
    );
}

/// Regression guard for a real bug: a theme-configured `title_text_style`
/// must style ONLY `AppBar::title`, never leak onto other toolbar children
/// (`leading`/`actions`) — Flutter parity: `titleTextStyle` styles the title
/// widget specifically (`app_bar.dart`, oracle tag `3.44.0`), not the whole
/// `NavigationToolbar`. Before the fix, `resolve_style`'s `title_style` fed
/// the OUTER `DefaultTextStyle` wrapping the entire toolbar, so a bare
/// `Text` action inherited the themed title style too.
///
/// Proven via measured height, not a `TextStyle` equality check — an
/// ambient style change only visibly reaches a bare `Text` run through
/// re-layout, so this is the mutation-honest way to prove the scoping
/// (see `flui-widgets/tests/text.rs`'s identical
/// `a_larger_font_size_measures_to_a_taller_box` technique).
#[test]
fn themed_title_text_style_does_not_leak_into_toolbar_actions() {
    use flui_types::typography::TextStyle;
    use flui_widgets::DefaultTextStyle;

    let themed_font_size = 40.0;
    let theme = ThemeData::light().copy_with(ThemeDataOverrides {
        app_bar_theme: Some(AppBarThemeData {
            title_text_style: Some(TextStyle {
                font_size: Some(themed_font_size),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    });

    // Reference heights for the SAME text under each ambient style in
    // isolation — what the action's Text SHOULD measure as (the unthemed
    // toolbar ambient) vs. what it WOULD measure as if the bug were still
    // present (the themed title style leaking in).
    let default_ambient = theme.text_theme.title_large.clone().unwrap_or_default();
    let default_reference = lay_out(
        DefaultTextStyle::new(default_ambient, Text::new("Action")),
        loose(1000.0),
    );
    let themed_reference = lay_out(
        DefaultTextStyle::new(
            TextStyle {
                font_size: Some(themed_font_size),
                ..Default::default()
            },
            Text::new("Action"),
        ),
        loose(1000.0),
    );
    let default_height = default_reference.size(default_reference.root()).height;
    let themed_height = themed_reference.size(themed_reference.root()).height;
    assert_ne!(
        default_height, themed_height,
        "the themed font_size must measure differently from the default — otherwise this test \
         cannot distinguish a leak from no leak",
    );

    let laid = lay_out(
        Theme::new(
            theme,
            MediaQuery::new(
                MediaQueryData::default(),
                AppBar::new()
                    .title(Text::new("Title"))
                    .actions(vec![Text::new("Action").boxed()]),
            ),
        ),
        loose(400.0),
    );

    let paragraphs = laid.find_all_by_render_type("RenderParagraph");
    assert_eq!(
        paragraphs.len(),
        2,
        "expected exactly one title Text and one action Text"
    );
    let heights: Vec<_> = paragraphs.iter().map(|id| laid.size(*id).height).collect();

    assert!(
        heights.contains(&themed_height),
        "the title's Text must measure at the THEMED font_size — the theme's \
         title_text_style must reach the title slot",
    );
    assert!(
        heights.contains(&default_height),
        "the action's Text must measure at the DEFAULT (unthemed) toolbar ambient, NOT the \
         theme's title_text_style — a themed title style must not leak into other toolbar \
         children",
    );
}

/// Flutter parity: `_AppBarState.build` wraps `leading` in
/// `ConstrainedBox(BoxConstraints.tightFor(width: _kLeadingWidth))` — a
/// fixed 56px-wide slot, independent of whatever the leading widget's own
/// intrinsic width is. A 10×10 `SizedBox` as `leading` proves the wrap is
/// really pinning the SLOT, not just happening to match a same-sized
/// widget: `SizedBox` and `ConstrainedBox` both mount as
/// `RenderConstrainedBox` (see `flui-objects`' own `sized_box.rs` module
/// docs), so finding a 56×56 one distinct from the 10×10 leaf is the
/// mutation-honest check — deleting the wrap would collapse both to 10×10
/// and this assertion would find nothing.
#[test]
fn explicit_leading_is_pinned_to_the_56px_wide_slot_regardless_of_its_own_size() {
    let laid = lay_out(
        Theme::new(
            ThemeData::light(),
            MediaQuery::new(
                MediaQueryData::default(),
                AppBar::new()
                    .leading(SizedBox::square(10.0))
                    .title(Text::new("Title")),
            ),
        ),
        loose(400.0),
    );

    let candidates = laid.find_all_by_render_type("RenderConstrainedBox");
    let leaf_present = candidates
        .iter()
        .any(|&id| laid.size(id) == common::size(10.0, 10.0));
    assert!(
        leaf_present,
        "the 10x10 leading SizedBox itself must still be present in the tree",
    );
    let slot_present = candidates
        .iter()
        .any(|&id| laid.size(id) == common::size(56.0, 56.0));
    assert!(
        slot_present,
        "the leading slot's own wrapping ConstrainedBox must report exactly 56x56 \
         (_kLeadingWidth square), not collapse to the 10x10 leaf it wraps",
    );
}

// ── Implied leading: a BackButton synthesized when the navigator can pop ──
//
// Mounted through a real `Navigator` (`flui_widgets::Navigator`/
// `NavigatorHandle`), not a hand-built `BuildContext` — `resolve_leading`'s
// navigator-consulting branches only run through `NavigatorHandle::maybe_of`,
// which needs a live ancestor to find. `app_bar.rs`'s own unit tests cover
// the ctx-independent short-circuits (explicit `leading`,
// `automatically_imply_leading: false`); these three prove the wiring this
// module's docs describe end to end: no leading with one route on the
// stack, a leading once a second route makes the stack poppable, and a tap
// on that leading actually pops.

fn home_route() -> SimpleRoute<()> {
    SimpleRoute::new(|_ctx| {
        MediaQuery::new(
            MediaQueryData::default(),
            Theme::new(ThemeData::light(), AppBar::new().title(Text::new("Home"))),
        )
        .boxed()
    })
}

fn details_route() -> SimpleRoute<()> {
    SimpleRoute::new(|_ctx| {
        MediaQuery::new(
            MediaQueryData::default(),
            Theme::new(
                ThemeData::light(),
                AppBar::new().title(Text::new("Details")),
            ),
        )
        .boxed()
    })
}

#[test]
fn implied_leading_is_absent_when_the_navigator_cannot_pop() {
    let handle = NavigatorHandle::new();
    handle.seed_initial(home_route());
    assert!(!handle.can_pop());

    let laid = lay_out(Navigator::new(handle), tight(400.0, 800.0));

    assert_eq!(
        laid.find_all_by_render_type("RenderPhysicalShape").len(),
        1,
        "with a single route on the stack (can_pop == false), the AppBar must mount no implied \
         leading IconButton — only its own Material surface",
    );
}

/// A leading `IconButton`'s own `Material` (`RenderPhysicalShape`) among
/// every such node in the tree — one sized exactly 40×40 (its
/// `_IconButtonDefaultsM3.minimumSize`, see `icon_button.rs`), distinct from
/// an `AppBar`'s own full-size `Material`. More than one may match (see
/// `implied_leading_appears_once_the_navigator_can_pop`'s doc comment for
/// why two mounted routes yield two leading buttons) — any one of them taps
/// the same underlying `NavigatorHandle`, so the first is as good as any.
/// Panics with a diagnostic size list if none match at all.
fn find_leading_icon_button_material(laid: &common::LaidOut) -> flui_foundation::RenderId {
    let candidates = laid.find_all_by_render_type("RenderPhysicalShape");
    let leading_size = common::size(40.0, 40.0);
    candidates
        .iter()
        .copied()
        .find(|&id| laid.size(id) == leading_size)
        .unwrap_or_else(|| {
            panic!(
                "expected at least one 40x40 RenderPhysicalShape (a leading IconButton's Material) \
                 among {} candidates: sizes = {:?}",
                candidates.len(),
                candidates
                    .iter()
                    .map(|&id| laid.size(id))
                    .collect::<Vec<_>>(),
            )
        })
}

/// Every mounted route's `AppBar` currently shows an implied leading:
/// `resolve_leading`'s `NavigatorHandle::can_pop()` check is navigator-global
/// (see `app_bar.rs`'s "Implied leading" module docs), and this substrate's
/// `Navigator`/`Overlay` does not offstage a covered, non-current route's
/// subtree the way Flutter's `ModalRoute`-aware `Overlay` does — both the
/// seeded `home` route and the pushed `details` route stay mounted, and
/// both see the same `can_pop() == true`. So with two routes on the stack,
/// FOUR `RenderPhysicalShape`s mount: each `AppBar`'s own surface, plus each
/// one's leading `IconButton`'s surface.
#[test]
fn implied_leading_appears_once_the_navigator_can_pop() {
    let handle = NavigatorHandle::new();
    handle.seed_initial(home_route());
    let _details = handle.push(details_route());
    assert!(handle.can_pop());

    let laid = lay_out(Navigator::new(handle), tight(400.0, 800.0));

    assert_eq!(
        laid.find_all_by_render_type("RenderPhysicalShape").len(),
        4,
        "see this test's doc comment for why two routes yield four Material surfaces, not two",
    );
}

#[test]
fn tapping_the_implied_back_button_pops_the_route() {
    let handle = NavigatorHandle::new();
    handle.seed_initial(home_route());
    let _details = handle.push(details_route());
    assert!(handle.can_pop());

    let laid = lay_out(Navigator::new(handle.clone()), tight(400.0, 800.0));

    // Both mounted routes' leadings sit at the same geometry (see the
    // previous test's doc comment) — which one the tap lands on doesn't
    // matter: either fires `NavigatorHandle::maybe_pop()` against the SAME
    // `handle`, so either one popping is the behavior under test.
    let leading = find_leading_icon_button_material(&laid);
    let leading_size = laid.size(leading);
    let leading_origin = laid.absolute_offset(leading);
    let tap_x = leading_origin.dx.get() + leading_size.width.get() / 2.0;
    let tap_y = leading_origin.dy.get() + leading_size.height.get() / 2.0;

    laid.dispatch_pointer_down(tap_x, tap_y);
    laid.dispatch_pointer_up(tap_x, tap_y);

    assert!(
        !handle.can_pop(),
        "tapping the implied back button must pop the pushed route via NavigatorHandle::maybe_pop, \
         leaving only the seeded initial route on the stack",
    );
}

// ── `AppBar.bottom` — Flexible-toolbar/fixed-bottom Column layout ──
//
// `bottom.rs`'s own module docs and `app_bar.rs`'s `builders_set_the_expected_fields`/
// `preferred_size_adds_the_bottom_slots_height_when_set` cover the pure
// preferred-size math in isolation; these prove the mounted geometry end to
// end: the toolbar and bottom slot actually stack at their expected sizes,
// and a height shortfall shrinks the toolbar, never the bottom slot.

/// A `bottom` slot whose `preferred_size` and actual mounted height always
/// agree — matching how a real [`flui_material::TabBar`] behaves (its own
/// `build` returns exactly the height it advertises via
/// [`flui_widgets::PreferredSizeView::preferred_size`]).
fn fixed_height_bottom(height: f32) -> PreferredSize {
    PreferredSize::new(
        Size::new(px(f32::INFINITY), px(height)),
        SizedBox::height(height),
    )
}

const BOTTOM_SLOT_HEIGHT: f32 = 48.0;

/// The bottom slot's own mounted [`RenderConstrainedBox`](flui_objects::RenderConstrainedBox)
/// — the only one in this tree sized to exactly [`BOTTOM_SLOT_HEIGHT`] (the
/// toolbar's own `SizedBox` is 56px tall or, under a shortfall, shrunk below
/// it; the outer `SizedBox` wrapping the whole `Column` sums to a different
/// total again) — so filtering on that exact height disambiguates it without
/// walking tree structure.
fn find_bottom_slot_box(laid: &common::LaidOut) -> flui_foundation::RenderId {
    let candidates: Vec<_> = laid
        .find_all_by_render_type("RenderConstrainedBox")
        .into_iter()
        .filter(|&id| laid.size(id).height == px(BOTTOM_SLOT_HEIGHT))
        .collect();
    assert_eq!(
        candidates.len(),
        1,
        "expected exactly one RenderConstrainedBox at the bottom slot's own {BOTTOM_SLOT_HEIGHT}px \
         height"
    );
    candidates[0]
}

#[test]
fn app_bar_with_a_bottom_slot_mounts_the_toolbar_then_the_bottom_at_their_full_heights() {
    let laid = lay_out(
        Theme::new(
            ThemeData::light(),
            MediaQuery::new(
                MediaQueryData::default(),
                AppBar::new()
                    .title(Text::new("Title"))
                    .bottom(fixed_height_bottom(BOTTOM_SLOT_HEIGHT)),
            ),
        ),
        loose(400.0),
    );

    let root = laid.root();
    assert_eq!(
        laid.size(root).height,
        px(56.0 + BOTTOM_SLOT_HEIGHT),
        "with no height shortfall, the AppBar's total height must be toolbar_height + the \
         bottom slot's own preferred height",
    );

    let bottom = find_bottom_slot_box(&laid);
    assert_eq!(
        laid.absolute_offset(bottom).dy,
        px(56.0),
        "the bottom slot must sit directly below the toolbar, at y == toolbar_height"
    );
}

/// A caller-imposed height shortfall (here: an outer constraint that leaves
/// no extra room for the `MediaQuery` top inset the `SafeArea` still
/// consumes internally) must shrink the **toolbar**, not the bottom slot —
/// see `app_bar.rs`'s module docs' `bottom` section on why `Flexible`
/// (loose fit) wraps the toolbar specifically. `56 (toolbar) + 48 (bottom)
/// == 104`, the exact outer constraint below; the 40px top inset eats into
/// that 104 internally, leaving only `104 - 40 - 48 == 16px` for the toolbar.
///
/// Red-check: swap `Flexible`/`ConstrainedBox(max_height: toolbar_height)`
/// for a plain (unwrapped) toolbar in `app_bar.rs`'s `build` — the Column
/// would then try to give the toolbar its full, un-shrinkable natural size
/// under `MainAxisSize::Max`, overflowing the available height instead of
/// this test's exact 16px.
#[test]
fn a_height_shortfall_shrinks_the_toolbar_and_leaves_the_bottom_slot_at_its_full_height() {
    let media = MediaQueryData {
        padding: EdgeInsets::new(px(40.0), px(0.0), px(0.0), px(0.0)),
        ..MediaQueryData::default()
    };
    let laid = lay_out(
        Theme::new(
            ThemeData::light(),
            MediaQuery::new(
                media,
                AppBar::new()
                    .title(Text::new("Title"))
                    .bottom(fixed_height_bottom(BOTTOM_SLOT_HEIGHT)),
            ),
        ),
        tight(400.0, 56.0 + BOTTOM_SLOT_HEIGHT),
    );

    let bottom = find_bottom_slot_box(&laid);
    assert_eq!(
        laid.size(bottom).height,
        px(BOTTOM_SLOT_HEIGHT),
        "the bottom slot must keep its full preferred height under a shortfall"
    );

    let toolbar_boxes: Vec<_> = laid
        .find_all_by_render_type("RenderConstrainedBox")
        .into_iter()
        .filter(|&id| laid.size(id).height == px(56.0))
        .collect();
    assert!(
        toolbar_boxes.is_empty(),
        "the toolbar must have shrunk below its 56px preference under the shortfall"
    );

    // Both the toolbar's own `SizedBox` and the `ConstrainedBox(max_height:
    // toolbar_height)` wrapping it end up reporting the SAME shrunk height
    // (the inner one is fully absorbed by the outer's tighter constraint) —
    // so this is `>= 1`, not `== 1`; the count is an implementation detail,
    // the shrunk height itself is the behavior under test.
    let shrunk_toolbar_boxes: Vec<_> = laid
        .find_all_by_render_type("RenderConstrainedBox")
        .into_iter()
        .filter(|&id| laid.size(id).height == px(16.0))
        .collect();
    assert!(
        !shrunk_toolbar_boxes.is_empty(),
        "the toolbar must shrink to exactly the 16px left over (104 total - 40 top inset - \
         48 bottom slot)"
    );
}

/// A standalone `AppBar` (no `Scaffold`) with a `bottom` slot still consumes
/// its own top safe-area inset unassisted — the same "consumes the top inset
/// itself" contract `standalone_app_bar_consumes_the_top_padding_itself`
/// proves for a bare toolbar, now with `bottom` in the mix and ample room
/// (no shortfall), so the top inset adds cleanly on top of the full
/// toolbar + bottom sum rather than eating into either.
#[test]
fn standalone_app_bar_with_a_bottom_slot_still_consumes_its_own_top_padding() {
    let media = MediaQueryData {
        padding: EdgeInsets::new(px(24.0), px(0.0), px(0.0), px(0.0)),
        ..MediaQueryData::default()
    };
    let laid = lay_out(
        Theme::new(
            ThemeData::light(),
            MediaQuery::new(
                media,
                AppBar::new()
                    .title(Text::new("Title"))
                    .bottom(fixed_height_bottom(BOTTOM_SLOT_HEIGHT)),
            ),
        ),
        loose(400.0),
    );

    let root = laid.root();
    assert_eq!(
        laid.size(root).height,
        px(56.0 + BOTTOM_SLOT_HEIGHT + 24.0),
        "a primary AppBar with a bottom slot must add the ambient MediaQuery top padding to \
         toolbar_height + the bottom slot's own preferred height, unassisted by any Scaffold",
    );

    let bottom = find_bottom_slot_box(&laid);
    assert_eq!(
        laid.size(bottom).height,
        px(BOTTOM_SLOT_HEIGHT),
        "with ample room (no shortfall), the bottom slot must mount at its full preferred height"
    );
}
