//! `PageView` is a `Scrollable` with a page `SnapPolicy`: a release settles on
//! a page, the scroll keys turn pages, the pager admits the device kinds its
//! author names, and assistive technology reads which page is showing.

use std::time::Duration;

use flui_animation::{ArcCurve, Vsync, curves};
use flui_foundation::geometry::{Axis, Offset};
use flui_interaction::PointerKindSet;
use flui_interaction::events::{Code, Key, KeyState, Modifiers, NamedKey};
use flui_painting::styling::Color;
use flui_painting::typography::TextDirection;
use flui_testing::{MouseScript, PointerScript, TouchScript};
use flui_view::prelude::*;
use flui_view::{BoxedView, ViewExt};
use flui_widgets::{ColoredBox, Directionality, PageController, PageView, VsyncScope};

use crate::common::{LaidOut, lay_out_animated, tight};

const PAGE: f64 = 300.0;

fn pages(count: u8) -> Vec<BoxedView> {
    (0..count)
        .map(|index| {
            ColoredBox::new(Color::rgb(40 * index, 20, 30))
                .into_view()
                .boxed()
        })
        .collect()
}

fn mount(view: PageView) -> (LaidOut, Vsync) {
    mount_tree(|vsync| VsyncScope::new(vsync, view).boxed())
}

fn mount_tree(build: impl FnOnce(Vsync) -> BoxedView) -> (LaidOut, Vsync) {
    let vsync = Vsync::new();
    let tree = build(vsync.clone());
    (
        lay_out_animated(tree, tight(PAGE, PAGE), vsync.clone()),
        vsync,
    )
}

fn settle(laid: &mut LaidOut) {
    for _ in 0..120 {
        laid.pump_for(Duration::from_millis(16));
    }
}

fn at(x: f64, y: f64) -> Offset<f64> {
    Offset::new(x, y)
}

#[derive(Clone, Copy, Debug)]
enum Device {
    Touch,
    Mouse,
}

/// A drag from `from` to `to` that comes to rest before it lifts.
fn slow_drag(device: Device, from: Offset<f64>, to: Offset<f64>) -> PointerScript {
    let points: Vec<_> = (0..=10)
        .map(|step| {
            let t = f64::from(step) / 10.0;
            at(
                from.dx + (to.dx - from.dx) * t,
                from.dy + (to.dy - from.dy) * t,
            )
        })
        .collect();
    let sample = Duration::from_millis(16);
    let hold = Duration::from_millis(300);
    match device {
        Device::Touch => {
            let mut script = TouchScript::new("drag").down(1, points[0]);
            for point in &points[1..] {
                script = script.wait(sample).move_to(1, *point);
            }
            script.wait(hold).up(1).build().expect("one whole contact")
        }
        Device::Mouse => {
            let mut script = MouseScript::new("drag").move_to(points[0]).press(points[0]);
            for point in &points[1..] {
                script = script.wait(sample).move_to(*point);
            }
            script
                .wait(hold)
                .release()
                .build()
                .expect("one whole press")
        }
    }
}

struct Keys(std::rc::Rc<flui_interaction::FocusManager>);

impl Keys {
    fn press(&self, code: Code, key: NamedKey) -> bool {
        self.press_key(code, Key::Named(key), Modifiers::NONE)
    }

    fn press_key(&self, code: Code, key: Key, modifiers: Modifiers) -> bool {
        let down = flui_interaction::testing::input::KeyEventBuilder::new(code)
            .with_state(KeyState::Down)
            .with_key(key.clone())
            .with_modifiers(modifiers)
            .build();
        let handled = self.0.dispatch_key_event(&down).is_handled();
        let up = flui_interaction::testing::input::KeyEventBuilder::new(code)
            .with_state(KeyState::Up)
            .with_key(key)
            .build();
        let _ = self.0.dispatch_key_event(&up);
        handled
    }
}

fn near(pixels: f64, target: f64) -> bool {
    (pixels - target).abs() < 1.0
}

#[test]
fn a_release_settles_on_a_page() {
    for device in [Device::Touch, Device::Mouse] {
        let rests = |from: f64, to: f64| {
            let controller = PageController::new();
            let (mut laid, _vsync) = mount(PageView::new(pages(3)).controller(controller.clone()));
            laid.replay(&slow_drag(device, at(from, 150.0), at(to, 150.0)));
            settle(&mut laid);
            controller.position().extents_snapshot().pixels
        };
        let short = rests(250.0, 150.0);
        assert!(
            near(short, 0.0),
            "{device:?}: a short drag returns, at {short}"
        );
        let long = rests(280.0, 80.0);
        assert!(
            near(long, PAGE),
            "{device:?}: past half a page, the next one, at {long}"
        );
    }
}

#[test]
fn a_page_view_turns_pages_with_the_scroll_keys() {
    let controller = PageController::new();
    let (mut laid, _vsync) = mount(
        PageView::new(pages(4))
            .controller(controller.clone())
            .autofocus(true),
    );
    laid.tick();
    let keys = Keys(laid.focus_manager());
    let pixels = || controller.position().extents_snapshot().pixels;

    for (code, key, expected) in [
        (Code::ArrowRight, NamedKey::ArrowRight, PAGE),
        (Code::PageDown, NamedKey::PageDown, 2.0 * PAGE),
        (Code::ArrowLeft, NamedKey::ArrowLeft, PAGE),
        (Code::PageUp, NamedKey::PageUp, 0.0),
        (Code::End, NamedKey::End, 3.0 * PAGE),
        (Code::Home, NamedKey::Home, 0.0),
    ] {
        assert!(keys.press(code, key), "{code:?} is handled");
        settle(&mut laid);
        assert_eq!(pixels(), expected, "{code:?}");
    }
    assert!(
        !keys.press(Code::ArrowLeft, NamedKey::ArrowLeft),
        "before the first page, left moves nothing and bubbles"
    );
    assert!(
        !keys.press(Code::ArrowDown, NamedKey::ArrowDown),
        "an arrow across the axis is not a page key"
    );
    controller.jump_to_page(3);
    laid.tick();
    assert!(
        keys.press_key(Code::Space, Key::character(" "), Modifiers::SHIFT),
        "Shift+Space goes back a page"
    );
    settle(&mut laid);
    assert_eq!(pixels(), 2.0 * PAGE);
}

#[test]
fn arrow_keys_follow_the_reading_direction_and_the_axis() {
    for (direction, forward) in [
        (TextDirection::Ltr, (Code::ArrowRight, NamedKey::ArrowRight)),
        (TextDirection::Rtl, (Code::ArrowLeft, NamedKey::ArrowLeft)),
    ] {
        let controller = PageController::new();
        let (mut laid, _vsync) = mount_tree(|vsync| {
            VsyncScope::new(
                vsync,
                Directionality::new(
                    direction,
                    PageView::new(pages(3))
                        .controller(controller.clone())
                        .autofocus(true),
                ),
            )
            .boxed()
        });
        laid.tick();
        let keys = Keys(laid.focus_manager());
        assert!(keys.press(forward.0, forward.1), "{direction:?}");
        settle(&mut laid);
        assert_eq!(controller.page(), Some(1.0), "{direction:?}: forward");
    }

    let controller = PageController::new();
    let (mut laid, _vsync) = mount(
        PageView::new(pages(3))
            .controller(controller.clone())
            .scroll_direction(Axis::Vertical)
            .autofocus(true),
    );
    laid.tick();
    let keys = Keys(laid.focus_manager());
    assert!(keys.press(Code::ArrowDown, NamedKey::ArrowDown));
    settle(&mut laid);
    assert_eq!(controller.page(), Some(1.0), "a vertical pager: down");
    assert!(keys.press(Code::PageDown, NamedKey::PageDown));
    settle(&mut laid);
    assert_eq!(controller.page(), Some(2.0), "and PageDown");
}

#[test]
fn allowed_devices_name_who_may_turn_the_page() {
    let drag_page = |view: PageView, device: Device| {
        let controller = PageController::new();
        let (mut laid, _vsync) = mount(view.controller(controller.clone()));
        laid.replay(&slow_drag(device, at(280.0, 150.0), at(80.0, 150.0)));
        settle(&mut laid);
        controller.page()
    };
    let touch_only = || PageView::new(pages(3)).allowed_devices(PointerKindSet::TOUCH);
    assert_eq!(drag_page(touch_only(), Device::Touch), Some(1.0));
    assert_eq!(
        drag_page(touch_only(), Device::Mouse),
        Some(0.0),
        "a mouse drag is refused"
    );
    assert_eq!(
        drag_page(PageView::new(pages(3)), Device::Mouse),
        Some(1.0),
        "every device by default"
    );
}

#[test]
fn a_refused_device_does_not_stop_a_pager_in_motion() {
    let press = || {
        MouseScript::new("press")
            .move_to(at(150.0, 150.0))
            .press(at(150.0, 150.0))
            .wait(Duration::from_millis(50))
            .release()
            .build()
            .expect("one whole press")
    };
    let rests_at = |view: PageView| {
        let controller = PageController::new();
        let (mut laid, _vsync) = mount(view.controller(controller.clone()));
        laid.tick();
        controller.animate_to_page(
            1,
            Duration::from_millis(1000),
            ArcCurve::new(curves::LINEAR),
        );
        laid.pump_for(Duration::from_millis(1));
        laid.pump_for(Duration::from_millis(100));
        laid.replay(&press());
        settle(&mut laid);
        controller.page()
    };
    assert_eq!(
        rests_at(PageView::new(pages(3))),
        Some(0.0),
        "a press on the moving pager holds it, and it settles back"
    );
    assert_eq!(
        rests_at(PageView::new(pages(3)).allowed_devices(PointerKindSet::TOUCH)),
        Some(1.0),
        "a mouse press the pager refuses leaves its motion alone"
    );
}

#[test]
fn the_controller_jumps_and_animates_to_a_page() {
    let controller = PageController::new();
    let (mut laid, _vsync) = mount(PageView::new(pages(4)).controller(controller.clone()));
    laid.tick();
    assert_eq!(controller.page(), Some(0.0));

    controller.jump_to_page(2);
    assert_eq!(controller.page(), Some(2.0));

    controller.animate_to_page(1, Duration::from_millis(200), ArcCurve::new(curves::LINEAR));
    laid.pump_for(Duration::from_millis(1));
    laid.pump_for(Duration::from_millis(100));
    let midway = controller.page().expect("laid out");
    assert!(midway < 2.0 && midway > 1.0, "mid-flight at {midway}");
    settle(&mut laid);
    assert_eq!(controller.page(), Some(1.0));
}

#[test]
fn assistive_technology_reads_the_page_showing() {
    let controller = PageController::new();
    let (mut laid, _vsync) = mount(PageView::new(pages(3)).controller(controller.clone()));
    laid.enable_semantics();
    laid.tick();
    let tree = laid.a11y_tree().expect("published semantics");
    assert!(
        tree.find_by_label("Page 1 of 3").is_ok(),
        "the first page is showing"
    );

    controller.jump_to_page(2);
    laid.tick();
    laid.tick();
    let tree = laid.a11y_tree().expect("republished semantics");
    assert!(
        tree.find_by_label("Page 3 of 3").is_ok(),
        "and then the last"
    );
    assert!(tree.find_by_label("Page 1 of 3").is_err());
}

#[test]
fn the_page_label_comes_from_the_widgets_localizations() {
    use flui_platform_api::Locale;
    use flui_widgets::localization::{
        BoxedLocalizationsDelegate, BoxedWidgetsLocalizations, DefaultWidgetsLocalizations,
        Localizations, LocalizationsDelegate, WidgetsLocalizations,
    };

    #[derive(Debug)]
    struct German;
    impl WidgetsLocalizations for German {
        fn text_direction(&self) -> TextDirection {
            TextDirection::Ltr
        }
        fn reorder_item_to_start(&self) -> &'static str {
            DefaultWidgetsLocalizations.reorder_item_to_start()
        }
        fn reorder_item_to_end(&self) -> &'static str {
            DefaultWidgetsLocalizations.reorder_item_to_end()
        }
        fn reorder_item_up(&self) -> &'static str {
            DefaultWidgetsLocalizations.reorder_item_up()
        }
        fn reorder_item_down(&self) -> &'static str {
            DefaultWidgetsLocalizations.reorder_item_down()
        }
        fn reorder_item_left(&self) -> &'static str {
            DefaultWidgetsLocalizations.reorder_item_left()
        }
        fn reorder_item_right(&self) -> &'static str {
            DefaultWidgetsLocalizations.reorder_item_right()
        }
        fn copy_button_label(&self) -> &'static str {
            DefaultWidgetsLocalizations.copy_button_label()
        }
        fn cut_button_label(&self) -> &'static str {
            DefaultWidgetsLocalizations.cut_button_label()
        }
        fn paste_button_label(&self) -> &'static str {
            DefaultWidgetsLocalizations.paste_button_label()
        }
        fn select_all_button_label(&self) -> &'static str {
            DefaultWidgetsLocalizations.select_all_button_label()
        }
        fn look_up_button_label(&self) -> &'static str {
            DefaultWidgetsLocalizations.look_up_button_label()
        }
        fn search_web_button_label(&self) -> &'static str {
            DefaultWidgetsLocalizations.search_web_button_label()
        }
        fn share_button_label(&self) -> &'static str {
            DefaultWidgetsLocalizations.share_button_label()
        }
        fn radio_button_unselected_label(&self) -> &'static str {
            DefaultWidgetsLocalizations.radio_button_unselected_label()
        }
        fn page_label(&self, page: usize, page_count: usize) -> String {
            format!("Seite {page} von {page_count}")
        }
    }
    #[derive(Debug)]
    struct GermanDelegate;
    impl LocalizationsDelegate for GermanDelegate {
        type Resources = BoxedWidgetsLocalizations;
        fn is_supported(&self, _locale: &Locale) -> bool {
            true
        }
        fn load(&self, _locale: &Locale) -> Self::Resources {
            BoxedWidgetsLocalizations::new(German)
        }
    }

    let (mut laid, _vsync) = mount_tree(|vsync| {
        Localizations::new(
            Locale::new("de", None::<&str>).expect("valid German locale"),
            vec![BoxedLocalizationsDelegate::new(GermanDelegate)],
            VsyncScope::new(vsync, PageView::new(pages(3))),
        )
        .boxed()
    });
    laid.enable_semantics();
    laid.tick();
    let tree = laid.a11y_tree().expect("published semantics");
    assert!(tree.find_by_label("Seite 1 von 3").is_ok());
    assert!(tree.find_by_label("Page 1 of 3").is_err());
}
