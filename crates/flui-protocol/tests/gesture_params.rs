//! The constructors of the gesture methods' parameters: each names what its method needs and
//! leaves the rest to the method's defaults, so a client outside the crate sends what an agent
//! reads as the default gesture.

use flui_protocol::input::{
    Direction, DragParams, GestureOptions, LongPressParams, PressKeysParams, ScrollIntoViewParams,
    SwipeParams, TypeTextParams,
};
use flui_protocol::locator::{Locator, Target};
use flui_protocol::tree::WindowId;

fn named(name: &str) -> Target {
    Target::Locator(Locator::named(name))
}

#[test]
fn each_gesture_constructor_names_its_inputs_and_leaves_the_rest_default() {
    let long_press = LongPressParams::new(Locator::named("Save"));
    assert_eq!(long_press.target, named("Save"));
    assert_eq!(long_press.duration_ms, None, "the platform's delay");
    assert_eq!(long_press.options, GestureOptions::default());

    let drag = DragParams::new(Locator::named("Card"), Locator::named("Bin"));
    assert_eq!(
        (drag.target, drag.to),
        (named("Card"), named("Bin")),
        "what is picked up, then where it is dropped"
    );
    assert_eq!(drag.options, GestureOptions::default());

    let swipe = SwipeParams::new(Locator::named("List"), Direction::Up);
    assert_eq!(swipe.target, named("List"));
    assert_eq!(swipe.direction, Direction::Up);
    assert_eq!((swipe.distance, swipe.velocity), (None, None));
    assert_eq!(swipe.options, GestureOptions::default());

    let scroll = ScrollIntoViewParams::new(Locator::named("Row 40"));
    assert_eq!(scroll.target, named("Row 40"));
    assert_eq!(scroll.options, GestureOptions::default());

    let typed = TypeTextParams::new(Locator::named("Title"), "Notes");
    assert_eq!(typed.target, named("Title"));
    assert_eq!(typed.text, "Notes");
    assert_eq!(typed.options, GestureOptions::default());

    let window = WindowId::from_u64(3).expect("a window handle is numbered from 1");
    let keys = PressKeysParams::new(window, vec!["Control+a".to_owned()]);
    assert_eq!(keys.target, None, "the window's focused element");
    assert_eq!(keys.window, Some(window));
    assert_eq!(keys.keys, ["Control+a"]);
    assert_eq!(keys.options, GestureOptions::default());
}
