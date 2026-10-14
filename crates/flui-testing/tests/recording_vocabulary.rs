//! The recording DTO keeps up with the vocabulary (ADR-0215 §2): every
//! variant in the generated table has a sample here, and every sample
//! survives a write and a read unchanged.

use std::num::{NonZeroI32, NonZeroU8, NonZeroU32};
use std::sync::Arc;
use std::time::Duration;

use flui_foundation::DataTransferId;
use flui_foundation::geometry::{DevicePixelRatio, EdgeInsets, Point, Size};
use flui_interaction::GestureDisposition;
use flui_platform_api::data_transfer::{
    DataTransferOffer, Mime, RepresentationDescriptor, TransferActions, TransferFormat,
};
use flui_platform_api::input_capabilities::{Availability, InputCapabilities, PointerKindSet};
use flui_platform_api::keyboard::{
    Code, ImeComposition, Key, KeyEvent, KeyRepeat, KeyState, Location, Modifiers, NamedKey,
};
use flui_platform_api::pointer::{
    ButtonChange, CancelReason, ContactSize, DeviceId, HoverDistance, PanZoomEvent, PanZoomPhase,
    PanZoomTransform, PenOrientation, PenSqueeze, PenSqueezePhase, PenTool, PointerButton,
    PointerButtons, PointerCancel, PointerDeviceChange, PointerEvent, PointerId, PointerInfo,
    PointerKind, PointerMove, PointerPosition, PointerPress, PointerRelease, PointerRevision,
    PointerRole, PointerSample, PointerSignal, Pressure, ScrollDelta, ScrollEvent, ScrollPhase,
    ScrollPrecision, ScrollUnit, TangentialPressure, Twist,
};
use flui_platform_api::{
    Brightness, DragDropEvent, EventTime, GesturePreferences, ImeEvent, Locale, MotionPreference,
    NativeMouseGeometry, NativeTouchGeometry, PlatformInput, SystemPreferences,
    TextWeightPreference, WheelPreferences, WheelStep, WindowExecutionState, WindowId,
};
use flui_runtime::owner::{InputOutcome, WindowObservation};
use flui_runtime::{DecisionRow, HostInput, InputRecord, InputRow, PreferencesRow};
use flui_testing::recording::{
    Privacy, RecordingClock, RecordingHeader, RecordingReader, RecordingWriter,
};

#[path = "generated/recording_variants.rs"]
#[rustfmt::skip]
mod generated;

fn at(nanos: u64) -> EventTime {
    EventTime::from_nanos(nanos)
}

fn row(input: HostInput, outcome: InputOutcome) -> InputRecord {
    InputRecord::Input(InputRow::new(WindowId(1), at(5), at(9), input, outcome))
}

fn host(input: HostInput) -> InputRecord {
    row(input, InputOutcome::Handled)
}

fn platform(input: PlatformInput) -> InputRecord {
    host(HostInput::Platform(input))
}

fn pointer_event(event: PointerEvent) -> InputRecord {
    platform(PlatformInput::Pointer(event))
}

fn info(kind: PointerKind, role: PointerRole) -> PointerInfo {
    PointerInfo::new(PointerId::try_from(4_u64).expect("nonzero"), kind)
        .with_device(DeviceId::try_from(9_u64).expect("nonzero"))
        .with_role(role)
}

fn touch() -> PointerInfo {
    info(PointerKind::Touch, PointerRole::Primary)
}

fn position(x: f64, y: f64) -> PointerPosition {
    PointerPosition::try_new(Point::new(x, y)).expect("finite")
}

/// A sample with every sensor a pen reports.
fn rich_sample(t: u64) -> PointerSample {
    PointerSample::new(at(t), position(1.25, -3.5))
        .with_pressure(Pressure::try_new(0.5).expect("in range"))
        .with_tangential_pressure(TangentialPressure::try_new(-0.25).expect("in range"))
        .with_orientation(PenOrientation::try_new(0.75, 1.5).expect("in range"))
        .with_twist(Twist::try_new(2.0).expect("in range"))
        .with_hover_distance(HoverDistance::try_new(0.125).expect("in range"))
        .with_contact_size(ContactSize::try_new(Size::new(3.0, 4.5)).expect("in range"))
}

fn plain_sample(t: u64) -> PointerSample {
    PointerSample::new(at(t), position(10.0, 20.0))
}

fn press(pointer: PointerInfo) -> PointerPress {
    PointerPress::new(
        pointer,
        PointerButton::SECONDARY,
        PointerButtons::only(PointerButton::PRIMARY),
        rich_sample(10),
    )
    .with_modifiers(Modifiers::SHIFT | Modifiers::CAPS_LOCK)
    .with_click_count(NonZeroU8::new(2).expect("nonzero"))
}

fn release(pointer: PointerInfo) -> PointerRelease {
    PointerRelease::new(
        pointer,
        PointerButton::BACK,
        PointerButtons::NONE.with(PointerButton::BACK),
        plain_sample(11),
    )
}

fn down_with(pointer: PointerInfo) -> InputRecord {
    pointer_event(PointerEvent::Down(press(pointer)))
}

fn signal(pointer: PointerInfo) -> PointerSignal {
    PointerSignal::new(pointer, at(12)).with_position(position(0.5, 0.25))
}

fn cancel(reason: CancelReason) -> InputRecord {
    pointer_event(PointerEvent::Cancel(PointerCancel::new(
        touch(),
        at(13),
        reason,
    )))
}

fn scroll(unit: ScrollUnit, precision: ScrollPrecision, phase: Option<ScrollPhase>) -> InputRecord {
    let mut event = ScrollEvent::new(
        info(PointerKind::Mouse, PointerRole::Primary),
        at(14),
        position(5.0, 6.0),
        ScrollDelta::try_new(unit, -1.5, 3.0).expect("finite"),
    )
    .with_precision(precision)
    .with_modifiers(Modifiers::CONTROL);
    if let Some(phase) = phase {
        event = event.with_phase(phase);
    }
    pointer_event(PointerEvent::Scroll(event))
}

fn pan_zoom(phase: PanZoomPhase) -> InputRecord {
    pointer_event(PointerEvent::PanZoom(
        PanZoomEvent::new(touch(), at(15), position(7.0, 8.0), phase)
            .with_modifiers(Modifiers::META),
    ))
}

fn squeeze(phase: PenSqueezePhase) -> InputRecord {
    pointer_event(PointerEvent::PenSqueeze(
        PenSqueeze::new(
            info(
                PointerKind::Pen { tool: PenTool::Tip },
                PointerRole::Primary,
            ),
            at(16),
            phase,
        )
        .with_hover(rich_sample(16)),
    ))
}

fn key(state: KeyState, key: Key, location: Location, repeat: KeyRepeat) -> InputRecord {
    platform(PlatformInput::Keyboard(
        KeyEvent::new(state, key, Code::KeyA, at(17))
            .with_location(location)
            .with_modifiers(Modifiers::ALT | Modifiers::FN_LOCK)
            .with_repeat(repeat)
            .with_composition(ImeComposition::Active),
    ))
}

fn plain_key(state: KeyState) -> InputRecord {
    key(
        state,
        Key::Named(NamedKey::Enter),
        Location::Standard,
        KeyRepeat::First,
    )
}

fn composing_key(composition: ImeComposition) -> InputRecord {
    platform(PlatformInput::Keyboard(
        KeyEvent::new(KeyState::Down, Key::character("é"), Code::Quote, at(18))
            .with_composition(composition),
    ))
}

fn ime(event: ImeEvent) -> InputRecord {
    platform(PlatformInput::Ime(event))
}

fn transfer_id() -> DataTransferId {
    DataTransferId::new_gen(3, NonZeroU32::new(2).expect("nonzero"))
}

fn entered(format: TransferFormat) -> InputRecord {
    platform(PlatformInput::DragDrop(DragDropEvent::Entered {
        offer: DataTransferOffer::new(
            transfer_id(),
            Arc::from([RepresentationDescriptor {
                format,
                declared_len: Some(128),
            }]),
        ),
        allowed: TransferActions::COPY | TransferActions::LINK,
        position: Some(Point::new(1.0, 2.0)),
    }))
}

fn capabilities(availability: Availability) -> InputRecord {
    platform(PlatformInput::Capabilities(
        InputCapabilities::builder()
            .pen(availability)
            .precise_scroll(Availability::Native)
            .trackpad_gestures(Availability::Emulated)
            .present(PointerKindSet::TOUCH | PointerKindSet::PEN)
            .build(),
    ))
}

fn window(observation: WindowObservation) -> InputRecord {
    host(HostInput::Window(observation))
}

fn execution(state: WindowExecutionState) -> InputRecord {
    window(WindowObservation::Snapshot {
        execution: state,
        focused: true,
        visible: false,
    })
}

fn preferences(values: SystemPreferences) -> InputRecord {
    InputRecord::Preferences(PreferencesRow::new(7, Arc::new(values)))
}

/// Preferences with every field observed.
fn full_preferences() -> SystemPreferences {
    let ratio = DevicePixelRatio::new(1.5).expect("positive");
    SystemPreferences::default()
        .with_text_scale(1.25)
        .expect("in range")
        .with_motion(MotionPreference::Reduce)
        .with_high_contrast(true)
        .with_haptics(false)
        .with_text_weight(TextWeightPreference::Bold)
        .with_locales(vec![
            Locale::en_us(),
            Locale::from_language_tag("ar-EG").expect("tag"),
        ])
        .with_gestures(
            GesturePreferences::default()
                .with_double_click_interval(Duration::from_millis(500))
                .with_double_tap_interval(Duration::from_millis(300))
                .with_long_press_timeout(Duration::from_millis(450))
                .with_native_mouse_geometry(
                    NativeMouseGeometry::new(Size::new(4, 4), Size::new(2, 3), ratio)
                        .expect("valid"),
                )
                .with_native_touch_geometry(
                    NativeTouchGeometry::new(8, 100, 50, 8000, ratio).expect("valid"),
                ),
        )
        .with_wheel(
            WheelPreferences::default()
                .with_vertical(WheelStep::Lines(3))
                .with_horizontal_characters(2),
        )
}

fn action(action: accesskit::Action, data: Option<accesskit::ActionData>) -> InputRecord {
    host(HostInput::Accessibility(accesskit::ActionRequest {
        action,
        target_tree: accesskit::TreeId(accesskit::Uuid::from_u128(
            0x0123_4567_89ab_cdef_0123_4567_89ab_cdef,
        )),
        target_node: accesskit::NodeId(42),
        data,
    }))
}

fn decision(disposition: GestureDisposition) -> InputRecord {
    InputRecord::Decision(DecisionRow::new(
        WindowId(2),
        PointerId::try_from(6_u64).expect("nonzero"),
        at(19),
        disposition,
        "flui_interaction::TapArenaMember",
    ))
}

/// The records that carry `variant` of `enumeration`.
#[expect(
    clippy::match_same_arms,
    clippy::too_many_lines,
    reason = "one arm per generated variant: rows that share a sample stay listed by name"
)]
fn sample(enumeration: &str, variant: &str) -> Option<Vec<InputRecord>> {
    use accesskit::{Action, ActionData};
    let one = |record| Some(vec![record]);
    match (enumeration, variant) {
        ("InputRecord", "Input") | ("HostInput", "Platform") | ("PlatformInput", "Pointer") => {
            one(down_with(touch()))
        }
        ("InputRecord", "Decision") => one(decision(GestureDisposition::Accepted)),
        ("InputRecord", "Preferences") => one(preferences(full_preferences())),
        ("InputRecord", "Gap") => one(InputRecord::Gap { rows: 3 }),
        ("InputRecord", "End") => one(InputRecord::End { lost_rows: 3 }),
        ("HostInput", "Window") => one(window(WindowObservation::Moved)),
        ("HostInput", "Frame") => one(host(HostInput::Frame {
            vsync: at(16_666_667),
        })),
        ("HostInput", "Accessibility") => one(action(Action::Click, None)),
        ("InputOutcome", "Handled") => one(host(HostInput::Frame { vsync: at(1) })),
        ("InputOutcome", "Unhandled") => one(row(
            HostInput::Platform(PlatformInput::Keyboard(KeyEvent::new(
                KeyState::Up,
                Key::Named(NamedKey::F4),
                Code::F4,
                at(1),
            ))),
            InputOutcome::Unhandled,
        )),
        ("InputOutcome", "Queued") => one(row(
            HostInput::Platform(PlatformInput::Ime(ImeEvent::Enabled)),
            InputOutcome::Queued,
        )),
        ("WindowObservation", "Metrics") => one(window(WindowObservation::Metrics {
            size: Size::new(800.5, 600.25),
            scale_factor: 1.75,
        })),
        ("WindowObservation", "Focus") => one(window(WindowObservation::Focus(false))),
        ("WindowObservation", "Visibility") => one(window(WindowObservation::Visibility(true))),
        ("WindowObservation", "Execution") => one(window(WindowObservation::Execution(
            WindowExecutionState::Suspended,
        ))),
        ("WindowObservation", "Snapshot") => one(execution(WindowExecutionState::Running)),
        ("WindowObservation", "Hover") => one(window(WindowObservation::Hover(true))),
        ("WindowObservation", "Moved") => one(window(WindowObservation::Moved)),
        ("WindowObservation", "SafeArea") => one(window(WindowObservation::SafeArea(
            EdgeInsets::new(44.0, 0.0, 34.0, 0.5),
        ))),
        ("WindowObservation", "Brightness") => {
            one(window(WindowObservation::Brightness(Brightness::Dark)))
        }
        ("PlatformInput", "Keyboard") => one(plain_key(KeyState::Down)),
        ("PlatformInput", "Ime") => one(ime(ImeEvent::Enabled)),
        ("PlatformInput", "DragDrop") | ("DragDropEvent", "Entered") => {
            one(entered(TransferFormat::Text))
        }
        ("PlatformInput", "Capabilities") => one(capabilities(Availability::Native)),
        ("DragDropEvent", "Moved") => {
            one(platform(PlatformInput::DragDrop(DragDropEvent::Moved {
                id: transfer_id(),
                allowed: TransferActions::MOVE,
                position: Point::new(3.0, 4.0),
            })))
        }
        ("DragDropEvent", "Dropped") => {
            one(platform(PlatformInput::DragDrop(DragDropEvent::Dropped {
                id: transfer_id(),
                action: TransferActions::COPY,
                position: None,
            })))
        }
        ("DragDropEvent", "Exited") => {
            one(platform(PlatformInput::DragDrop(DragDropEvent::Exited {
                id: transfer_id(),
            })))
        }
        ("PointerEvent", "Down") => one(down_with(touch())),
        ("PointerEvent", "ButtonChange") | ("ButtonChange", "Pressed") => one(pointer_event(
            PointerEvent::ButtonChange(ButtonChange::Pressed(press(touch()))),
        )),
        ("ButtonChange", "Released") => one(pointer_event(PointerEvent::ButtonChange(
            ButtonChange::Released(release(touch())),
        ))),
        ("PointerEvent", "Up") => one(pointer_event(PointerEvent::Up(release(touch())))),
        ("PointerEvent", "Move") => one(pointer_event(PointerEvent::Move(
            PointerMove::new(
                touch(),
                PointerButtons::only(PointerButton::PRIMARY),
                rich_sample(30),
            )
            .with_modifiers(Modifiers::SYMBOL)
            .with_coalesced(vec![plain_sample(20), plain_sample(25)])
            .with_predicted(vec![plain_sample(35)]),
        ))),
        ("PointerEvent", "Cancel") => one(cancel(CancelReason::Platform)),
        ("PointerEvent", "Enter") => one(pointer_event(PointerEvent::Enter(signal(touch())))),
        ("PointerEvent", "Leave") => one(pointer_event(PointerEvent::Leave(PointerSignal::new(
            touch(),
            at(12),
        )))),
        ("PointerEvent", "Scroll") => {
            one(scroll(ScrollUnit::Pixels, ScrollPrecision::Precise, None))
        }
        ("PointerEvent", "ScrollInertiaCancel") => one(pointer_event(
            PointerEvent::ScrollInertiaCancel(signal(touch())),
        )),
        ("PointerEvent", "PanZoom") => one(pan_zoom(PanZoomPhase::Start)),
        ("PointerEvent", "Revise") => one(pointer_event(PointerEvent::Revise(
            PointerRevision::new(touch(), rich_sample(40)),
        ))),
        ("PointerEvent", "PenSqueeze") => one(squeeze(PenSqueezePhase::Began)),
        ("PointerEvent", "DeviceAdded") => one(pointer_event(PointerEvent::DeviceAdded(
            PointerDeviceChange::new(
                DeviceId::try_from(9_u64).expect("nonzero"),
                PointerKind::Pen {
                    tool: PenTool::Eraser,
                },
                at(41),
            ),
        ))),
        ("PointerEvent", "DeviceRemoved") => one(pointer_event(PointerEvent::DeviceRemoved(
            PointerDeviceChange::new(DeviceId::KEYBOARD, PointerKind::Virtual, at(42)),
        ))),
        ("PointerKind", "Mouse") => one(down_with(info(PointerKind::Mouse, PointerRole::Primary))),
        ("PointerKind", "Touch") => one(down_with(touch())),
        ("PointerKind", "Pen") | ("PenTool", "Tip") => one(down_with(info(
            PointerKind::Pen { tool: PenTool::Tip },
            PointerRole::Primary,
        ))),
        ("PenTool", "Eraser") => one(down_with(info(
            PointerKind::Pen {
                tool: PenTool::Eraser,
            },
            PointerRole::Additional,
        ))),
        ("PointerKind", "Trackpad") => one(pan_zoom(PanZoomPhase::End)),
        ("PointerKind", "Virtual") => {
            one(down_with(info(PointerKind::Virtual, PointerRole::Primary)))
        }
        ("PointerKind", "Unknown") => one(down_with(info(
            PointerKind::Unknown,
            PointerRole::Additional,
        ))),
        ("PointerRole", "Primary") => one(down_with(touch())),
        ("PointerRole", "Additional") => {
            one(down_with(info(PointerKind::Touch, PointerRole::Additional)))
        }
        ("CancelReason", "Platform") => one(cancel(CancelReason::Platform)),
        ("CancelReason", "CaptureLost") => one(cancel(CancelReason::CaptureLost)),
        ("CancelReason", "FocusLost") => one(cancel(CancelReason::FocusLost)),
        ("CancelReason", "DeviceRemoved") => one(cancel(CancelReason::DeviceRemoved)),
        ("CancelReason", "InvalidInput") => one(cancel(CancelReason::InvalidInput)),
        ("CancelReason", "Claimed") => one(cancel(CancelReason::Claimed)),
        ("PenSqueezePhase", "Began") => one(squeeze(PenSqueezePhase::Began)),
        ("PenSqueezePhase", "Changed") => one(squeeze(PenSqueezePhase::Changed)),
        ("PenSqueezePhase", "Ended") => one(squeeze(PenSqueezePhase::Ended)),
        ("PenSqueezePhase", "Cancelled") => one(squeeze(PenSqueezePhase::Cancelled)),
        ("ScrollUnit", "Lines") => one(scroll(ScrollUnit::Lines, ScrollPrecision::Notched, None)),
        ("ScrollUnit", "Pixels") => one(scroll(ScrollUnit::Pixels, ScrollPrecision::Unknown, None)),
        ("ScrollUnit", "Pages") => one(scroll(ScrollUnit::Pages, ScrollPrecision::Notched, None)),
        ("ScrollUnit", "Detents") => {
            one(scroll(ScrollUnit::Detents, ScrollPrecision::Notched, None))
        }
        ("ScrollPrecision", "Unknown") => {
            one(scroll(ScrollUnit::Pixels, ScrollPrecision::Unknown, None))
        }
        ("ScrollPrecision", "Notched") => {
            one(scroll(ScrollUnit::Lines, ScrollPrecision::Notched, None))
        }
        ("ScrollPrecision", "Precise") => {
            one(scroll(ScrollUnit::Pixels, ScrollPrecision::Precise, None))
        }
        ("ScrollPhase", phase) => {
            let phase = match phase {
                "Began" => ScrollPhase::Began,
                "Changed" => ScrollPhase::Changed,
                "Ended" => ScrollPhase::Ended,
                "Cancelled" => ScrollPhase::Cancelled,
                "MomentumBegan" => ScrollPhase::MomentumBegan,
                "MomentumChanged" => ScrollPhase::MomentumChanged,
                "MomentumEnded" => ScrollPhase::MomentumEnded,
                _ => return None,
            };
            one(scroll(
                ScrollUnit::Pixels,
                ScrollPrecision::Precise,
                Some(phase),
            ))
        }
        ("PanZoomPhase", "Start") => one(pan_zoom(PanZoomPhase::Start)),
        ("PanZoomPhase", "Update") => one(pan_zoom(PanZoomPhase::Update(
            PanZoomTransform::try_new(1.5, -0.25).expect("valid"),
        ))),
        ("PanZoomPhase", "End") => one(pan_zoom(PanZoomPhase::End)),
        ("PanZoomPhase", "Cancelled") => one(pan_zoom(PanZoomPhase::Cancelled)),
        ("Key", "Character") => one(key(
            KeyState::Down,
            Key::character("ß"),
            Location::Left,
            KeyRepeat::AutoRepeat,
        )),
        ("Key", "Named") => one(plain_key(KeyState::Down)),
        ("KeyState", "Down") => one(plain_key(KeyState::Down)),
        ("KeyState", "Up") => one(plain_key(KeyState::Up)),
        ("Location", location) => {
            let location = match location {
                "Standard" => Location::Standard,
                "Left" => Location::Left,
                "Right" => Location::Right,
                "Numpad" => Location::Numpad,
                _ => return None,
            };
            one(key(
                KeyState::Down,
                Key::Named(NamedKey::Shift),
                location,
                KeyRepeat::First,
            ))
        }
        ("KeyRepeat", "First") => one(plain_key(KeyState::Down)),
        ("KeyRepeat", "AutoRepeat") => one(key(
            KeyState::Down,
            Key::Named(NamedKey::ArrowDown),
            Location::Standard,
            KeyRepeat::AutoRepeat,
        )),
        ("ImeComposition", "Inactive") => one(composing_key(ImeComposition::Inactive)),
        ("ImeComposition", "Active") => one(composing_key(ImeComposition::Active)),
        ("ImeEvent", "Enabled") => one(ime(ImeEvent::Enabled)),
        ("ImeEvent", "Preedit") => Some(vec![
            ime(ImeEvent::Preedit {
                text: "かな".to_owned(),
                cursor: Some((3, 6)),
            }),
            ime(ImeEvent::Preedit {
                text: String::new(),
                cursor: None,
            }),
        ]),
        ("ImeEvent", "Commit") => one(ime(ImeEvent::Commit("漢字".to_owned()))),
        ("ImeEvent", "Disabled") => one(ime(ImeEvent::Disabled)),
        ("TransferFormat", "Text") => one(entered(TransferFormat::Text)),
        ("TransferFormat", "Html") => one(entered(TransferFormat::Html)),
        ("TransferFormat", "UriList") => one(entered(TransferFormat::UriList)),
        ("TransferFormat", "Image") => one(entered(TransferFormat::Image {
            mime: Mime::new("image/png"),
        })),
        ("TransferFormat", "Custom") => one(entered(TransferFormat::Custom {
            mime: Mime::new("application/x-flui; v=\"2\""),
        })),
        ("Availability", "Native") => one(capabilities(Availability::Native)),
        ("Availability", "Emulated") => one(capabilities(Availability::Emulated)),
        ("Availability", "Unavailable") => one(capabilities(Availability::Unavailable)),
        ("WindowExecutionState", "Running") => one(execution(WindowExecutionState::Running)),
        ("WindowExecutionState", "Suspended") => one(execution(WindowExecutionState::Suspended)),
        ("WindowExecutionState", "Detached") => one(execution(WindowExecutionState::Detached)),
        ("Brightness", "Light") => one(window(WindowObservation::Brightness(Brightness::Light))),
        ("Brightness", "Dark") => one(window(WindowObservation::Brightness(Brightness::Dark))),
        ("MotionPreference", "NoPreference") => one(preferences(
            SystemPreferences::default().with_motion(MotionPreference::NoPreference),
        )),
        ("MotionPreference", "Reduce") => one(preferences(full_preferences())),
        ("MotionPreference", "Scaled") => one(preferences(
            SystemPreferences::default()
                .with_motion(MotionPreference::from_duration_scale(0.5).expect("valid")),
        )),
        ("TextWeightPreference", "NoPreference") => one(preferences(
            SystemPreferences::default().with_text_weight(TextWeightPreference::NoPreference),
        )),
        ("TextWeightPreference", "Bold") => one(preferences(full_preferences())),
        ("TextWeightPreference", "Adjustment") => {
            one(preferences(SystemPreferences::default().with_text_weight(
                TextWeightPreference::Adjustment(NonZeroI32::new(-100).expect("nonzero")),
            )))
        }
        ("WheelStep", "Lines") => one(preferences(full_preferences())),
        ("WheelStep", "Page") => one(preferences(
            SystemPreferences::default()
                .with_wheel(WheelPreferences::default().with_vertical(WheelStep::Page)),
        )),
        ("GestureDisposition", "Accepted") => one(decision(GestureDisposition::Accepted)),
        ("GestureDisposition", "Rejected") => one(decision(GestureDisposition::Rejected)),
        ("accesskit::Action", name) => {
            let action = [
                Action::Click,
                Action::Focus,
                Action::Blur,
                Action::Collapse,
                Action::Expand,
                Action::CustomAction,
                Action::Decrement,
                Action::Increment,
                Action::HideTooltip,
                Action::ShowTooltip,
                Action::ReplaceSelectedText,
                Action::ScrollDown,
                Action::ScrollLeft,
                Action::ScrollRight,
                Action::ScrollUp,
                Action::ScrollIntoView,
                Action::ScrollToPoint,
                Action::SetScrollOffset,
                Action::SetTextSelection,
                Action::SetSequentialFocusNavigationStartingPoint,
                Action::SetValue,
                Action::ShowContextMenu,
            ]
            .into_iter()
            .find(|action| format!("{action:?}") == name)?;
            one(self::action(action, None))
        }
        ("accesskit::ActionData", data) => {
            let data = match data {
                "CustomAction" => ActionData::CustomAction(-3),
                "Value" => ActionData::Value("café".into()),
                "NumericValue" => ActionData::NumericValue(0.1),
                "ScrollUnit" => ActionData::ScrollUnit(accesskit::ScrollUnit::Page),
                "ScrollHint" => ActionData::ScrollHint(accesskit::ScrollHint::TopLeft),
                "ScrollToPoint" => ActionData::ScrollToPoint(accesskit::Point { x: 1.5, y: -2.0 }),
                "SetScrollOffset" => {
                    ActionData::SetScrollOffset(accesskit::Point { x: 0.0, y: 300.25 })
                }
                "SetTextSelection" => ActionData::SetTextSelection(accesskit::TextSelection {
                    anchor: accesskit::TextPosition {
                        node: accesskit::NodeId(5),
                        character_index: 2,
                    },
                    focus: accesskit::TextPosition {
                        node: accesskit::NodeId(6),
                        character_index: 0,
                    },
                }),
                _ => return None,
            };
            one(action(Action::SetValue, Some(data)))
        }
        ("accesskit::ScrollUnit", "Item") => one(action(
            Action::ScrollDown,
            Some(ActionData::ScrollUnit(accesskit::ScrollUnit::Item)),
        )),
        ("accesskit::ScrollUnit", "Page") => one(action(
            Action::ScrollDown,
            Some(ActionData::ScrollUnit(accesskit::ScrollUnit::Page)),
        )),
        ("accesskit::ScrollHint", hint) => {
            let hint = [
                accesskit::ScrollHint::TopLeft,
                accesskit::ScrollHint::BottomRight,
                accesskit::ScrollHint::TopEdge,
                accesskit::ScrollHint::BottomEdge,
                accesskit::ScrollHint::LeftEdge,
                accesskit::ScrollHint::RightEdge,
            ]
            .into_iter()
            .find(|candidate| format!("{candidate:?}") == hint)?;
            one(action(
                Action::ScrollIntoView,
                Some(ActionData::ScrollHint(hint)),
            ))
        }
        _ => None,
    }
}

/// `records` written with text kept and read back.
fn round_trip(records: &[InputRecord]) -> Vec<InputRecord> {
    let mut header = RecordingHeader::new(RecordingClock::Monotonic);
    header.privacy = Privacy::KeepText;
    let mut writer = RecordingWriter::new(Vec::new(), &header).expect("header");
    for record in records {
        writer.write(record).expect("row");
    }
    let bytes = writer.finish().expect("flush");
    RecordingReader::new(&bytes[..])
        .expect("header")
        .map(|row| row.expect("every row reads back").record)
        .collect()
}

#[test]
fn every_vocabulary_variant_round_trips_through_the_recording_dto() {
    let mut missing = Vec::new();
    let mut changed = Vec::new();
    for &(enumeration, variant) in generated::VARIANTS {
        let Some(records) = sample(enumeration, variant) else {
            missing.push(format!("{enumeration}::{variant}"));
            continue;
        };
        let written = format!("{records:?}");
        assert!(
            written.contains(variant),
            "the sample for {enumeration}::{variant} does not carry it: {written}"
        );
        let read = format!("{:?}", round_trip(&records));
        if read != written {
            changed.push(format!(
                "{enumeration}::{variant}\n  wrote {written}\n  read  {read}"
            ));
        }
    }
    assert!(
        missing.is_empty(),
        "variants without a sample (mirror them in the recording DTO, then add one here): {missing:#?}"
    );
    assert!(
        changed.is_empty(),
        "variants the DTO changed:\n{}",
        changed.join("\n")
    );
}
