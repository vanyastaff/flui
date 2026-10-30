//! Per-device input scripts (ADR-0215 §6): every device builds its script on
//! virtual time with its contacts paired, and a script's frames say which
//! events share a frame.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use flui_foundation::geometry::Offset;
use flui_interaction::PointerId;
use flui_interaction::events::PointerEvent;
use flui_platform::pointer::DeviceId;
use flui_platform::{GesturePreferences, SystemPreferences};
use flui_runtime::HostInput;
use flui_testing::replay::{
    MouseScript, PenScript, PointerScript, ScriptError, TouchScript, TrackpadScript, VirtualScript,
};
use flui_testing::{HeadlessHost, HeadlessWindow};
use flui_widgets::{GestureDetector, HitTestBehavior, Listener};

fn at(x: f64, y: f64) -> Offset<f64> {
    Offset::new(x, y)
}

fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}

fn contact(id: u64) -> PointerId {
    PointerId::try_from(id).expect("nonzero")
}

fn device() -> DeviceId {
    DeviceId::try_from(5_u64).expect("nonzero")
}

#[test]
fn a_script_with_an_unpaired_down_fails_to_build() {
    let point = at(1.0, 2.0);
    let refused: [(&str, Result<PointerScript, ScriptError>, ScriptError); 7] = [
        (
            "touch down, never up",
            TouchScript::new("t").down(1, point).build(),
            ScriptError::UnpairedDown {
                pointer: contact(1),
            },
        ),
        (
            "mouse press, never released",
            MouseScript::new("m").press(point).build(),
            ScriptError::UnpairedDown {
                pointer: contact(1),
            },
        ),
        (
            "pen down, never up",
            PenScript::new("p").down(point, 0.5).build(),
            ScriptError::UnpairedDown {
                pointer: contact(1),
            },
        ),
        (
            "virtual press, never released",
            VirtualScript::new("v", device()).press(point).build(),
            ScriptError::UnpairedDown {
                pointer: contact(1),
            },
        ),
        (
            "touch up without a down",
            TouchScript::new("t").up(2).build(),
            ScriptError::NotDown {
                pointer: contact(2),
            },
        ),
        (
            "touch down twice",
            TouchScript::new("t")
                .down(1, point)
                .down(1, point)
                .up(1)
                .build(),
            ScriptError::AlreadyDown {
                pointer: contact(1),
            },
        ),
        (
            "trackpad pinch never ended",
            TrackpadScript::new("tp", point)
                .pinch_start()
                .pinch_to(1.5, 0.0)
                .build(),
            ScriptError::UnpairedPinch,
        ),
    ];
    for (case, built, expected) in refused {
        assert_eq!(built.err(), Some(expected), "{case}");
    }

    // The same shapes, paired, build — one per device.
    let paired = [
        TouchScript::new("t").down(1, point).cancel(1).build(),
        MouseScript::new("m")
            .move_to(point)
            .click(point)
            .wheel(3.0)
            .build(),
        PenScript::new("p")
            .hover(point)
            .down(point, 0.25)
            .move_to(at(4.0, 4.0), 0.75)
            .up()
            .build(),
        TrackpadScript::new("tp", point)
            .pan(at(0.0, 30.0), 3, ms(8))
            .pinch_start()
            .pinch_to(1.5, 0.25)
            .pinch_end()
            .build(),
        VirtualScript::new("v", device())
            .move_to(point)
            .press(point)
            .release()
            .build(),
    ];
    for script in paired {
        let script = script.expect("a paired script builds");
        assert_eq!(script.inputs().len(), script.len());
    }
}

#[test]
fn a_script_is_host_input_on_virtual_time_with_a_readable_form() {
    let script = TouchScript::new("drag")
        .down(1, at(10.0, 10.0))
        .wait(ms(8))
        .move_to(1, at(30.0, 10.0))
        .frame()
        .up(1)
        .build()
        .expect("paired");
    let times: Vec<u64> = script
        .inputs()
        .iter()
        .map(|input| match input {
            HostInput::Platform(flui_platform::PlatformInput::Pointer(event)) => {
                flui_interaction::PointerEventExt::time(event)
                    .expect("a contact event carries its time")
                    .as_nanos()
            }
            HostInput::Frame { vsync, .. } => vsync.as_nanos(),
            other => panic!("a touch script delivers pointer input and frames: {other:?}"),
        })
        .collect();
    assert_eq!(times, [0, 8_000_000, 8_000_000, 8_000_000]);
    assert_eq!(
        script.to_string(),
        "script drag\n     0.000 ms  touch #1 down at (10, 10)\n     8.000 ms  touch #1 move at \
         (30, 10)\n     8.000 ms  frame\n     8.000 ms  touch #1 up at (30, 10)\n"
    );

    let virtual_pointer = VirtualScript::new("v", device())
        .press(at(1.0, 1.0))
        .release()
        .build()
        .expect("paired");
    let HostInput::Platform(flui_platform::PlatformInput::Pointer(PointerEvent::Down(press))) =
        &virtual_pointer.inputs()[0]
    else {
        panic!("a press is a down");
    };
    assert_eq!(
        press.pointer.device,
        Some(device()),
        "a virtual pointer names its device"
    );

    // The JSON Lines form beside it is a complete recording.
    let bytes = script
        .write_recording(Vec::new(), flui_platform::WindowId(1))
        .expect("written");
    let recording = flui_testing::recording::ReplayableRecording::read(
        flui_testing::recording::RecordingReader::try_new(&bytes[..]).expect("header"),
    )
    .expect("a whole recording");
    assert_eq!(recording.rows().len(), script.len());
}

/// A box over the whole window that counts what its detector recognized.
struct Counts {
    taps: Rc<Cell<u32>>,
    double_taps: Rc<Cell<u32>>,
}

fn detector_host() -> (HeadlessHost, Counts) {
    let mut host = HeadlessHost::new(HeadlessWindow::new(200, 200));
    let counts = Counts {
        taps: Rc::new(Cell::new(0)),
        double_taps: Rc::new(Cell::new(0)),
    };
    let (taps, double_taps) = (Rc::clone(&counts.taps), Rc::clone(&counts.double_taps));
    host.attach(
        &GestureDetector::new()
            .on_tap(move |_| taps.set(taps.get() + 1))
            .on_double_tap(move |_| double_taps.set(double_taps.get() + 1))
            .child(flui_widgets::ColoredBox::new(
                flui_painting::styling::Color::RED,
            )),
    )
    .expect("attach");
    let _ = host.pump(ms(16));
    (host, counts)
}

/// Row "double click within one frame": a stalled frame spans both clicks,
/// and they still make one double tap, decided by their own times rather
/// than by the frames around them.
#[test]
fn a_double_click_within_one_frame_is_one_double_tap() {
    let (mut host, counts) = detector_host();
    let point = at(100.0, 100.0);
    host.run_script(
        &MouseScript::new("double click in one stalled frame")
            .frame()
            .wait(ms(10))
            .press(point)
            .wait(ms(20))
            .release()
            .wait(ms(60))
            .press(point)
            .wait(ms(20))
            .release()
            .wait(ms(10))
            .frame()
            .build()
            .expect("paired"),
    );
    host.pump_until_settled(ms(16), 120)
        .expect("the gesture settles");
    assert_eq!(counts.double_taps.get(), 1, "one double tap");
    assert_eq!(
        counts.taps.get(),
        0,
        "the clicks were not taps of their own"
    );
}

/// Row "N events in one frame": every move between two frames reaches the
/// tree, in order, before the second frame.
#[test]
fn every_event_in_one_frame_is_delivered_in_order() {
    let mut host = HeadlessHost::new(HeadlessWindow::new(200, 200));
    let moves = Rc::new(RefCell::new(Vec::new()));
    let seen = Rc::clone(&moves);
    host.attach(
        &Listener::new()
            .behavior(HitTestBehavior::Opaque)
            .on_pointer_move(move |_, event| {
                if let PointerEvent::Move(event) = event.global {
                    seen.borrow_mut().push(event.current().position.get().x);
                }
            })
            .child(flui_widgets::SizedBox::new(200.0, 200.0)),
    )
    .expect("attach");
    let _ = host.pump(ms(16));
    let mut script = TouchScript::new("five moves")
        .frame()
        .down(1, at(10.0, 50.0));
    for step in 1..=5_u32 {
        script = script
            .wait(ms(2))
            .move_to(1, at(10.0 + 10.0 * f64::from(step), 50.0));
    }
    host.run_script(&script.up(1).wait(ms(4)).frame().build().expect("paired"));
    assert_eq!(*moves.borrow(), [20.0, 30.0, 40.0, 50.0, 60.0]);
}

/// Row "an event between vsyncs": a contact that lands between two frames
/// keeps its own time, so its deadline counts from it and not from a vsync.
#[test]
fn an_event_between_vsyncs_keeps_its_own_time() {
    let mut host = HeadlessHost::new(HeadlessWindow::new(200, 200));
    host.set_system_preferences(
        SystemPreferences::default()
            .with_gestures(GesturePreferences::default().with_long_press_timeout(ms(490))),
    );
    let fired_at = Rc::new(Cell::new(None));
    let clock = host.clock().clone();
    let start = Rc::new(Cell::new(Duration::ZERO));
    let (fired, origin) = (Rc::clone(&fired_at), Rc::clone(&start));
    host.attach(
        &GestureDetector::new()
            .on_long_press(move |_| fired.set(clock.elapsed().checked_sub(origin.get())))
            .child(flui_widgets::ColoredBox::new(
                flui_painting::styling::Color::RED,
            )),
    )
    .expect("attach");
    let _ = host.pump(ms(16));
    start.set(host.clock().elapsed());
    // Frames every 16 ms from the script's start; the finger lands at 10 ms,
    // between the first two vsyncs. Its 490 ms deadline is due at 500 ms, so
    // the frame at 512 ms fires it; counted from the vsync before it, the
    // deadline would fire on the frame at 496 ms.
    let mut script = TouchScript::new("press between vsyncs")
        .frame()
        .wait(ms(10))
        .down(1, at(100.0, 100.0))
        .wait(ms(6))
        .frame();
    for _ in 0..40 {
        script = script.wait(ms(16)).frame();
    }
    host.run_script(&script.up(1).build().expect("paired"));
    assert_eq!(fired_at.get(), Some(ms(512)));
}

/// A script finds its target as assistive technology does, by role and
/// label, on a window at a fractional scale; a run until settled stops at
/// its frame limit when something never settles.
#[test]
fn a_script_finds_its_target_by_role_and_label_and_settles_within_a_limit() {
    use flui_view::ViewExt as _;
    use flui_widgets::{Column, Semantics, SemanticsRole, SizedBox};

    let mut host = HeadlessHost::new(HeadlessWindow::new(200, 200));
    let window = host.primary_window();
    host.set_scale_factor(window, 1.5);
    let pressed = Rc::new(RefCell::new(Vec::new()));
    let button = |label: &'static str| {
        let pressed = Rc::clone(&pressed);
        Semantics::new()
            .role(SemanticsRole::Button)
            .label(label)
            .child(
                GestureDetector::new()
                    .on_tap(move |_| pressed.borrow_mut().push(label))
                    .child(
                        SizedBox::new(200.0, 60.0).child(flui_widgets::ColoredBox::new(
                            flui_painting::styling::Color::RED,
                        )),
                    ),
            )
            .boxed()
    };
    host.attach(&Column::new(vec![button("Cancel"), button("Save")]))
        .expect("attach");
    host.enable_semantics();
    host.pump_until_settled(ms(16), 10)
        .expect("a static tree settles");

    let save = host
        .find_center(window, accesskit::Role::Button, "Save")
        .expect("one Save button");
    assert_eq!(save, at(100.0, 90.0), "logical center of the second row");
    assert!(matches!(
        host.find_center(window, accesskit::Role::Button, "Delete"),
        Err(flui_testing::A11yQueryError::NotFound { .. })
    ));
    host.run_script(
        &MouseScript::new("press save")
            .move_to(save)
            .wait(ms(16))
            .press(save)
            .wait(ms(40))
            .release()
            .build()
            .expect("paired"),
    );
    host.pump_until_settled(ms(16), 30)
        .expect("the tap settles");
    assert_eq!(*pressed.borrow(), ["Save"]);

    let mut spinning = HeadlessHost::new(HeadlessWindow::new(40, 40));
    spinning
        .attach(&flui_widgets::ActivityIndicator::new())
        .expect("attach");
    assert_eq!(
        spinning.pump_until_settled(ms(16), 12),
        Err(flui_testing::NotSettled { frames: 12 }),
        "a repeating animation never settles; the limit ends the run"
    );
}
