//! Recording is explicit: the production headless host keeps current state,
//! while its testing kit installs call recorders on request.

use flui_native::testing::FakeSystemChrome;
use flui_native::{HeadlessPlatform, HeadlessWindow};
use flui_platform::{WindowCommand, WindowMaterial, WindowOptions};
use flui_testing::window_commands::AdmitAndApply as _;

use crate::Opened;

fn open(platform: &HeadlessPlatform) -> Opened {
    Opened::on(platform, WindowOptions::default()).expect("a headless window opens")
}

fn headless(window: &Opened) -> &HeadlessWindow {
    window
        .as_any()
        .downcast_ref::<HeadlessWindow>()
        .expect("the headless platform opens its production window")
}

#[test]
fn production_headless_keeps_current_state_without_recording() {
    let opened = open(&HeadlessPlatform::new());
    let window = headless(&opened);

    assert_eq!(window.materials(), Vec::<WindowMaterial>::new());
    let chrome = opened.system_chrome().expect("headless system chrome");
    assert!(chrome.as_any().downcast_ref::<FakeSystemChrome>().is_none());

    opened
        .admit_and_apply(WindowCommand::SetMaterial(WindowMaterial::System))
        .expect("the headless window accepts a material");

    assert_eq!(window.window_system().material, WindowMaterial::System);
    assert_eq!(window.materials(), Vec::<WindowMaterial>::new());
}

#[test]
fn test_doubles_record_exact_histories_when_explicitly_installed() {
    let opened = open(&HeadlessPlatform::new().with_test_doubles());
    let window = headless(&opened);
    let chrome = opened.system_chrome().expect("headless system chrome");
    let recorder = chrome
        .as_any()
        .downcast_ref::<FakeSystemChrome>()
        .expect("test doubles install the system-chrome recorder");
    let style = flui_platform::SystemUiOverlayStyle::default();
    chrome.set_overlay_style(style);
    assert_eq!(recorder.styles(), vec![style]);

    for material in [WindowMaterial::System, WindowMaterial::Opaque] {
        opened
            .admit_and_apply(WindowCommand::SetMaterial(material))
            .expect("the headless window accepts a material");
    }

    assert_eq!(window.window_system().material, WindowMaterial::Opaque);
    assert_eq!(
        window.materials(),
        vec![WindowMaterial::System, WindowMaterial::Opaque]
    );
}
