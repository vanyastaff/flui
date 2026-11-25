//! What the agent composes for a gesture whose call names no duration, velocity or distance
//! (ADR-0400 §2): what the window it plays in recognises as that gesture, from the window's own
//! gesture settings, and aimed at the part of its target the window shows.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use flui_interaction::gesture::{GestureProfiles, GestureSettings};
use flui_platform::input::pointer::PointerKind;
use flui_platform::{GesturePreferences, SystemPreferences, TargetPlatform};
use flui_protocol::method::{self, Method, WindowMethod};
use flui_protocol::{
    act::ActOutcome, error::ErrorCode, error::ErrorObject, input::JournalParams,
    input::LongPressParams, input::PinchParams, input::SwipeParams, input::TapParams,
    locator::Locator, locator::Target,
};
use flui_testing::host::{HeadlessDevAgent, HeadlessHost, HeadlessWindow};
use flui_view::dev_agent::{AgentAnswer, AgentWindow, DevAgentHook, InputLease};
use flui_view::prelude::*;
use flui_widgets::{ColoredBox, Column, DragSlot, GestureDetector, Row, Semantics, SizedBox};
use parking_lot::Mutex;
use serde_json::json;

/// Keeps every window the host hands it.
struct Windows(Arc<Mutex<Vec<AgentWindow>>>);

impl DevAgentHook for Windows {
    fn attach(&mut self) -> bool {
        true
    }

    fn window_opened(&mut self, window: AgentWindow) {
        self.0.lock().push(window);
    }
}

/// `view` shown in `window`, its agent window and a lease on its input, after its first frames.
fn shown(
    window: HeadlessWindow,
    view: &(impl View + Clone + 'static),
) -> (HeadlessHost, HeadlessDevAgent, AgentWindow, InputLease) {
    let windows = Arc::new(Mutex::new(Vec::new()));
    let mut agent = HeadlessDevAgent::attach(Windows(Arc::clone(&windows)));
    let mut host = HeadlessHost::new(window).with_dev_agent(&mut agent);
    host.attach(view).expect("mount");
    let _ = host.pump(Duration::from_millis(16));
    let window = windows
        .lock()
        .pop()
        .expect("the host handed the agent its window");
    let _ = host.pump(Duration::from_millis(16));
    let lease = window
        .input_lease()
        .expect("an open window leases its input");
    (host, agent, window, lease)
}

/// Pumps 16 ms frames until `answer` is answered, at most `frames` of them; the frames it took.
fn answered<T>(
    host: &mut HeadlessHost,
    answer: &mut AgentAnswer<T>,
    frames: u32,
) -> (Result<T, ErrorObject>, u32) {
    for frame in 1..=frames {
        let _ = host.pump(Duration::from_millis(16));
        if let Some(answer) = answer.try_take() {
            return (answer, frame);
        }
    }
    panic!("no answer within {frames} frames");
}

fn named(name: &str) -> Target {
    Target::Locator(Locator::named(name))
}

fn ask<M: Method<Output = ActOutcome> + WindowMethod>(
    lease: &InputLease,
    params: M::Params,
) -> AgentAnswer<ActOutcome> {
    lease.call::<M>(params).expect("the inbox has room")
}

/// What the pad's recognisers heard.
#[derive(Clone, Default)]
struct Heard {
    long_presses: Rc<Cell<u32>>,
    double_taps: Rc<Cell<u32>>,
    drags: Rc<Cell<u32>>,
    /// The speed each drag ended at, in logical pixels a second.
    releases: Rc<RefCell<Vec<f64>>>,
}

/// "Pad", a surface that recognises a long press, a double tap and a drag; and "Drop" below it.
#[derive(Clone, StatelessView)]
struct Pad(Heard);

impl StatelessView for Pad {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let heard = self.0.clone();
        let (long_presses, double_taps) = (heard.long_presses, heard.double_taps);
        let (drags, releases) = (heard.drags, heard.releases);
        let pad = GestureDetector::new()
            .on_long_press(move |_| long_presses.set(long_presses.get() + 1))
            .on_double_tap(move |_| double_taps.set(double_taps.get() + 1))
            .drag(
                DragSlot::pan()
                    .on_start(move |_, _| drags.set(drags.get() + 1))
                    .on_end(move |_, details| {
                        releases.borrow_mut().push(details.velocity.magnitude());
                    }),
            )
            .child(
                SizedBox::new(200.0, 200.0)
                    .child(ColoredBox::new(flui_painting::styling::Color::WHITE)),
            );
        Column::new(vec![
            Semantics::new()
                .label("Pad")
                .container(true)
                .child(pad)
                .into_view()
                .boxed(),
            Semantics::new()
                .label("Drop")
                .container(true)
                .child(
                    SizedBox::new(200.0, 100.0)
                        .child(ColoredBox::new(flui_painting::styling::Color::WHITE)),
                )
                .into_view()
                .boxed(),
        ])
    }
}

/// How the window's gesture settings were set: a platform's preset, and the system's long-press
/// and double-tap timeouts where its preferences name them.
#[derive(Clone, Copy, Debug)]
struct Setting {
    platform: TargetPlatform,
    long_press: Option<Duration>,
    double_tap: Option<Duration>,
}

impl Setting {
    const fn preset(platform: TargetPlatform) -> Self {
        Self {
            platform,
            long_press: None,
            double_tap: None,
        }
    }

    fn window(self) -> HeadlessWindow {
        HeadlessWindow::new(200, 400)
            .with_platform(self.platform)
            .with_gesture_preset(GestureProfiles::preset(self.platform))
    }

    fn apply(self, host: &HeadlessHost) {
        let mut preferences = GesturePreferences::default();
        if let Some(long_press) = self.long_press {
            preferences = preferences.with_long_press_timeout(long_press);
        }
        if let Some(double_tap) = self.double_tap {
            preferences = preferences
                .with_double_tap_interval(double_tap)
                .with_double_click_interval(double_tap);
        }
        host.set_system_preferences(SystemPreferences::default().with_gestures(preferences));
    }

    /// The least speed a touch release flings at in this window.
    fn min_fling(self) -> f64 {
        GestureSettings::preset(self.platform, PointerKind::Touch).min_fling_velocity()
    }
}

/// The gestures a call naming no duration, velocity or distance asks for.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Unnamed {
    LongPress,
    DoubleTap,
    Fling,
    Swipe,
    DragTo,
}

fn swipe_params() -> SwipeParams {
    serde_json::from_value(json!({
        "target": { "locator": { "name": { "match": "exact", "text": "Pad" } } },
        "direction": "right",
    }))
    .expect("a swipe reads")
}

/// The gesture `default` asks of the pad, as its call names nothing but the target.
fn ask_default(lease: &InputLease, default: Unnamed) -> AgentAnswer<ActOutcome> {
    match default {
        Unnamed::LongPress => {
            let params: LongPressParams = serde_json::from_value(json!({
                "target": { "locator": { "name": { "match": "exact", "text": "Pad" } } },
            }))
            .expect("a long press reads");
            ask::<method::ElementLongPress>(lease, params)
        }
        Unnamed::DoubleTap => ask::<method::ElementDoubleTap>(lease, TapParams::new(named("Pad"))),
        Unnamed::Fling => ask::<method::ElementFling>(lease, swipe_params()),
        Unnamed::Swipe => ask::<method::ElementSwipe>(lease, swipe_params()),
        Unnamed::DragTo => {
            let params = serde_json::from_value(json!({
                "target": { "locator": { "name": { "match": "exact", "text": "Pad" } } },
                "to": { "locator": { "name": { "match": "exact", "text": "Drop" } } },
            }))
            .expect("a drag reads");
            ask::<method::ElementDragTo>(lease, params)
        }
    }
}

/// A gesture whose call names no duration, velocity or distance is composed from the window's
/// own gesture settings, so the window's recognisers take it as that gesture: on every
/// platform's preset, and on a window whose long-press and double-tap timeouts the system's
/// preferences raised or lowered. A long press is a long press, a double tap a double tap, a
/// fling flings and a swipe does not, and a drag drags.
#[test]
fn a_default_gesture_is_recognised_as_that_gesture_in_every_window() {
    let platforms = [
        TargetPlatform::Android,
        TargetPlatform::iOS,
        TargetPlatform::Linux,
        TargetPlatform::MacOS,
        TargetPlatform::Windows,
        TargetPlatform::Fuchsia,
        TargetPlatform::Unknown,
    ];
    let mut settings: Vec<Setting> = platforms.into_iter().map(Setting::preset).collect();
    settings.push(Setting {
        long_press: Some(Duration::from_millis(1_500)),
        double_tap: Some(Duration::from_millis(700)),
        ..Setting::preset(TargetPlatform::Android)
    });
    settings.push(Setting {
        long_press: Some(Duration::from_millis(300)),
        double_tap: Some(Duration::from_millis(150)),
        ..Setting::preset(TargetPlatform::iOS)
    });
    let defaults = [
        Unnamed::LongPress,
        Unnamed::DoubleTap,
        Unnamed::Fling,
        Unnamed::Swipe,
        Unnamed::DragTo,
    ];
    for setting in settings {
        for default in defaults {
            let heard = Heard::default();
            let (mut host, _agent, _window, lease) = shown(setting.window(), &Pad(heard.clone()));
            setting.apply(&host);
            let _ = host.pump(Duration::from_millis(16));
            let mut asked = ask_default(&lease, default);
            let outcome = answered(&mut host, &mut asked, 400)
                .0
                .unwrap_or_else(|error| panic!("{setting:?} {default:?}: {error:?}"));
            assert_eq!(outcome.effect.kind, flui_protocol::act::EffectKind::Ran);
            let case = format!("{setting:?} {default:?}");
            match default {
                Unnamed::LongPress => {
                    assert_eq!(heard.long_presses.get(), 1, "{case}: a long press");
                }
                Unnamed::DoubleTap => {
                    assert_eq!(heard.double_taps.get(), 1, "{case}: a double tap");
                }
                Unnamed::Fling | Unnamed::Swipe => {
                    assert_eq!(heard.drags.get(), 1, "{case}: a drag");
                    let releases = heard.releases.borrow();
                    let [release] = releases.as_slice() else {
                        panic!("{case}: one release: {releases:?}");
                    };
                    let flung = *release >= setting.min_fling();
                    assert_eq!(
                        flung,
                        default == Unnamed::Fling,
                        "{case}: released at {release} px/s, the window flings from {}",
                        setting.min_fling()
                    );
                }
                Unnamed::DragTo => assert_eq!(heard.drags.get(), 1, "{case}: a drag"),
            }
        }
    }
}

/// A long press naming no duration holds at most the ceiling the reply bound counts: a window
/// whose long-press timeout leaves no room for one within it refuses the call, naming the
/// duration to give, and a long press that names its duration plays.
#[test]
fn a_long_press_past_the_ceiling_is_refused_until_its_duration_is_named() {
    let setting = Setting {
        long_press: Some(Duration::from_millis(3_900)),
        ..Setting::preset(TargetPlatform::Android)
    };
    let heard = Heard::default();
    let (mut host, _agent, _window, lease) = shown(setting.window(), &Pad(heard.clone()));
    setting.apply(&host);
    let _ = host.pump(Duration::from_millis(16));
    let mut refused = ask_default(&lease, Unnamed::LongPress);
    let refused = answered(&mut host, &mut refused, 60)
        .0
        .expect_err("past the ceiling");
    assert_eq!(refused.code, ErrorCode::InvalidArgument, "{refused:?}");
    assert!(refused.message.contains("name its duration"), "{refused:?}");
    let params: LongPressParams = serde_json::from_value(json!({
        "target": { "locator": { "name": { "match": "exact", "text": "Pad" } } },
        "duration_ms": 4_200,
    }))
    .expect("a long press reads");
    let mut named_press = ask::<method::ElementLongPress>(&lease, params);
    answered(&mut host, &mut named_press, 400)
        .0
        .expect("a named long press plays");
    assert_eq!(
        heard.long_presses.get(),
        1,
        "the named press is a long press"
    );
}

/// "Long", 2000 px wide and as tall as the window, which shows its first 200 px.
#[derive(Clone, StatelessView)]
struct Clipped;

impl StatelessView for Clipped {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        Row::new(vec![
            Semantics::new()
                .label("Long")
                .container(true)
                .child(
                    SizedBox::new(2_000.0, 400.0)
                        .child(ColoredBox::new(flui_painting::styling::Color::WHITE)),
                )
                .into_view()
                .boxed(),
        ])
    }
}

/// A swipe naming no distance and a pinch on an element far wider than the window stay inside
/// the part the window shows, where their contact lands: their extent is that part's, not the
/// element's whole bounds.
#[test]
fn a_gesture_on_a_clipped_element_stays_inside_the_part_shown() {
    type Ask = fn(&InputLease) -> AgentAnswer<ActOutcome>;
    let rows: [(&str, Ask); 2] = [
        ("a swipe right naming no distance", |lease| {
            let params: SwipeParams = serde_json::from_value(json!({
                "target": { "locator": { "name": { "match": "exact", "text": "Long" } } },
                "direction": "right",
            }))
            .expect("a swipe reads");
            ask::<method::ElementSwipe>(lease, params)
        }),
        ("a pinch to twice", |lease| {
            let params: PinchParams = serde_json::from_value(json!({
                "target": { "locator": { "name": { "match": "exact", "text": "Long" } } },
                "scale": 2.0,
            }))
            .expect("a pinch reads");
            ask::<method::ElementPinch>(lease, params)
        }),
    ];
    for (case, call) in rows {
        let (mut host, _agent, window, lease) = shown(HeadlessWindow::new(200, 400), &Clipped);
        let mut asked = call(&lease);
        let outcome = answered(&mut host, &mut asked, 200)
            .0
            .unwrap_or_else(|error| panic!("{case}: {error:?}"));
        let span = outcome.observed.expect("its input went out");
        let mut read = window
            .call::<method::InputJournal>(JournalParams::new(window.id()))
            .expect("the inbox has room");
        let journal = answered(&mut host, &mut read, 2)
            .0
            .expect("the journal is read");
        let points: Vec<_> = journal
            .entries
            .iter()
            .filter(|entry| (span.from..=span.to).contains(&entry.seq))
            .filter_map(|entry| entry.at)
            .collect();
        assert!(!points.is_empty(), "{case}: pointer input went out");
        for at in points {
            assert!(
                (0.0..=200.0).contains(&at.x) && (0.0..=400.0).contains(&at.y),
                "{case}: {at:?} is outside the 200 x 400 window"
            );
        }
    }
}
