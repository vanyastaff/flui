//! `CarouselView`: M3 cards that follow each item's mask, the theme cascade, taps and button
//! semantics only with `on_tap`, and the snap a layout implies.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use flui_material::{CarouselView, CarouselViewThemeData, Theme, ThemeData, ThemeDataOverrides};
use flui_sdk::animation::Vsync;
use flui_sdk::foundation::inspect::Rgba;
use flui_sdk::geometry::Offset;
use flui_sdk::painting::{Alignment, Color};
use flui_sdk::view::ViewExt;
use flui_sdk::widgets::animated::VsyncScope;
use flui_sdk::widgets::{CarouselItemScope, CarouselLayout, ColoredBox, KeylineSnap};
use flui_testing::TouchScript;
use flui_testing::a11y::Role;

use crate::common::{LaidOut, lay_out_animated, rgba, tight};

const WIDTH: f64 = 360.0;
const HEIGHT: f64 = 200.0;
/// The M3 inset of a card inside its mask, on each side.
const PADDING: f64 = 4.0;

type Scopes = Rc<RefCell<Vec<Option<CarouselItemScope>>>>;

/// A view of `count` coloured items that records their scopes.
fn view(count: usize, scopes: &Scopes) -> CarouselView {
    scopes.borrow_mut().resize(count, None);
    let scopes = Rc::clone(scopes);
    CarouselView::builder(count, move |_ctx, scope, index| {
        scopes.borrow_mut()[index] = Some(scope.clone());
        ColoredBox::new(Color::rgb(30, 60, 90))
    })
}

fn mount_with(theme: ThemeData, view: CarouselView) -> LaidOut {
    let vsync = Vsync::new();
    let tree = VsyncScope::new(vsync.clone(), Theme::new(theme, view)).boxed();
    let mut laid = lay_out_animated(tree, tight(WIDTH, HEIGHT), vsync);
    laid.tick();
    laid
}

fn mount(view: CarouselView) -> LaidOut {
    mount_with(ThemeData::light(), view)
}

fn settle(laid: &mut LaidOut) {
    for _ in 0..120 {
        laid.pump_for(Duration::from_millis(16));
    }
}

/// A slow touch drag of `distance` to the left that comes to rest before it lifts.
fn slow_drag(laid: &mut LaidOut, distance: f64) {
    let at = |x: f64| Offset::new(x, HEIGHT / 2.0);
    let mut script = TouchScript::new("drag").down(1, at(WIDTH / 2.0));
    for step in 1..=10 {
        let x = WIDTH / 2.0 - distance * f64::from(step) / 10.0;
        script = script.wait(Duration::from_millis(16)).move_to(1, at(x));
    }
    let script = script
        .wait(Duration::from_millis(300))
        .up(1)
        .build()
        .expect("one whole contact");
    laid.replay(&script);
    settle(laid);
}

fn shown(scopes: &Scopes, index: usize) -> Option<f64> {
    scopes.borrow()[index]
        .clone()?
        .mask()
        .map(|mask| mask.width())
}

/// The widths of the cards drawn, widest first.
fn card_widths(laid: &LaidOut) -> Vec<f64> {
    let mut widths: Vec<f64> = laid
        .find_all_by_render_type("RenderPhysicalShape")
        .into_iter()
        .map(|card| laid.size(card).width)
        .collect();
    widths.sort_by(|a, b| b.total_cmp(a));
    widths
}

pub fn every_layout_implies_its_snap() {
    let cases = [
        (
            "multi-browse",
            CarouselLayout::default(),
            Some(KeylineSnap::SingleAdvance),
        ),
        (
            "hero",
            CarouselLayout::hero(Alignment::CENTER).expect("in range"),
            Some(KeylineSnap::SingleAdvance),
        ),
        (
            "full screen",
            CarouselLayout::full_screen(),
            Some(KeylineSnap::SingleAdvance),
        ),
        (
            "weighted",
            CarouselLayout::weighted([3.0, 2.0, 1.0]).expect("positive weights"),
            Some(KeylineSnap::SingleAdvance),
        ),
        (
            "uncontained",
            CarouselLayout::uncontained(150.0).expect("a valid extent"),
            None,
        ),
    ];
    for (name, layout, snap) in cases {
        assert_eq!(KeylineSnap::for_layout(&layout), snap, "{name}");
    }
}

pub fn the_implied_snap_settles_a_release_and_none_scrolls_freely() {
    // The default multi-browse view returns to its first item after a short slow drag.
    let scopes = Scopes::default();
    let mut laid = mount(view(10, &scopes));
    let large = shown(&scopes, 0).expect("item 0 shows");
    slow_drag(&mut laid, 30.0);
    assert_eq!(
        shown(&scopes, 0),
        Some(large),
        "snapped back onto the first item"
    );

    // Set to none, the same drag leaves the first item part way off its keyline.
    let scopes = Scopes::default();
    let mut laid = mount(view(10, &scopes).snap(None));
    slow_drag(&mut laid, 30.0);
    let free = shown(&scopes, 0).expect("item 0 shows");
    assert!(free < large, "scrolled freely: {free} < {large}");

    // An uncontained view scrolls freely by default.
    let layout = CarouselLayout::uncontained(150.0).expect("a valid extent");
    let scopes = Scopes::default();
    let mut laid = mount(view(10, &scopes).layout(layout));
    let progress = |scopes: &Scopes| scopes.borrow()[1].clone().expect("item 1 built").progress();
    let resting = progress(&scopes);
    slow_drag(&mut laid, 30.0);
    let moved = progress(&scopes);
    assert!(
        moved != resting,
        "left between keylines: {moved} vs {resting}"
    );
}

pub fn cards_cover_the_shown_part_of_each_item() {
    let scopes = Scopes::default();
    let laid = mount(view(10, &scopes));
    let expected: Vec<f64> = (0..3)
        .map(|index| shown(&scopes, index).expect("a shown item") - 2.0 * PADDING)
        .collect();
    let widths = card_widths(&laid);
    assert_eq!(&widths[..3], &expected[..], "{widths:?}");
    // The content behind each card keeps the item's full extent, the large item's, and does
    // not reflow as its card narrows.
    let large = shown(&scopes, 0).expect("item 0 shows");
    for content in laid.find_all_by_render_type("RenderColoredBox") {
        assert_eq!(laid.size(content).width, large);
    }
}

pub fn the_widget_beats_the_theme_which_beats_the_tokens() {
    let themed = Color::rgb(11, 22, 33);
    let mine = Color::rgb(44, 55, 66);
    let theme = ThemeData::light().copy_with(ThemeDataOverrides {
        carousel_view_theme: Some(CarouselViewThemeData {
            background_color: Some(themed),
            elevation: Some(3.0),
            ..Default::default()
        }),
        ..Default::default()
    });
    let surface = ThemeData::light().color_scheme.surface;
    let cases = [
        ("tokens", Box::new(ThemeData::light()), None, surface, 0.0),
        ("theme", Box::new(theme.clone()), None, themed, 3.0),
        ("widget", Box::new(theme), Some(mine), mine, 3.0),
    ];
    for (name, theme, color, expected, elevation) in cases {
        let scopes = Scopes::default();
        let mut carousel = view(4, &scopes);
        if let Some(color) = color {
            carousel = carousel.background_color(color);
        }
        let laid = mount_with(*theme, carousel);
        let card = laid.find_all_by_render_type("RenderPhysicalShape")[0];
        let card_node = laid.render_node(card);
        assert_eq!(card_node.get::<Rgba>("color"), Ok(rgba(expected)), "{name}");
        assert_eq!(card_node.get::<f64>("elevation"), Ok(elevation), "{name}");
    }
}

pub fn a_tap_reports_its_card_and_cards_are_buttons_only_with_on_tap() {
    let taps = Rc::new(RefCell::new(Vec::new()));
    let scopes = Scopes::default();
    let tapped = {
        let taps = Rc::clone(&taps);
        view(10, &scopes).on_tap(move |_cx, index| taps.borrow_mut().push(index))
    };
    let mut laid = mount(tapped);
    laid.enable_semantics();
    laid.tick();
    let large = shown(&scopes, 0).expect("item 0 shows");
    laid.dispatch_pointer_down(large / 2.0, HEIGHT / 2.0);
    laid.dispatch_pointer_up(large / 2.0, HEIGHT / 2.0);
    settle(&mut laid);
    assert_eq!(*taps.borrow(), [0]);
    let buttons = laid
        .a11y_tree()
        .expect("semantics")
        .find_all(Role::Button)
        .len();
    assert!(buttons >= 3, "each shown card is a button: {buttons}");

    let scopes = Scopes::default();
    let mut laid = mount(view(10, &scopes));
    laid.enable_semantics();
    laid.tick();
    let buttons = laid
        .a11y_tree()
        .expect("semantics")
        .find_all(Role::Button)
        .len();
    assert_eq!(buttons, 0, "without on_tap a card is a plain surface");
}
