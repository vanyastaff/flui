//! `window.placement` as the agent serves it (ADR-0400 §5): the platform whose conventions the
//! presentation follows, by its wire name, beside the window's logical size and scale.

use flui_platform::TargetPlatform;
use flui_protocol::method;
use flui_protocol::session::{Platform, WindowParams};

use super::agent_semantics::{answer, counter};
use super::*;

/// A runtime whose presentation follows `platform`, one frame in.
fn following(platform: TargetPlatform) -> UiRuntime {
    let ui_runtime = UiRuntime::try_new(
        test_window().with_platform(platform),
        1.0,
        crate::runtime_services::RuntimeHostServices::new(
            Arc::new(|| {}),
            Arc::new(AtomicBool::new(false)),
            &flui_painting::FontCollection::new(),
            flui_scheduler::ClockSource::Platform,
        ),
    )
    .expect("runtime");
    let _presses = counter(&ui_runtime, None);
    ui_runtime
}

#[test]
fn a_placement_names_the_platform_the_presentation_follows() {
    let table = [
        (TargetPlatform::Android, Platform::Android),
        (TargetPlatform::iOS, Platform::Ios),
        (TargetPlatform::Linux, Platform::Linux),
        (TargetPlatform::MacOS, Platform::Macos),
        (TargetPlatform::Windows, Platform::Windows),
        (TargetPlatform::Fuchsia, Platform::Fuchsia),
        (TargetPlatform::Unknown, Platform::Neutral),
    ];
    for (platform, named) in table {
        let ui_runtime = following(platform);
        let window = ui_runtime
            .dev_agent_window(ui_runtime.presentation_id())
            .expect("the ui_runtime hosts its primary presentation");
        let reply = window
            .call::<method::WindowPlacement>(WindowParams::new(window.id()))
            .expect("the inbox has room");
        let placement = answer(&ui_runtime, reply).expect("the placement is served");
        assert_eq!(placement.platform, Some(named), "{platform:?}");
        assert!(placement.scale > 0.0, "{placement:?}");
    }
}
