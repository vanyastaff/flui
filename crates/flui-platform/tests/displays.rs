//! The headless platform's injectable displays, driven through the public
//! `Platform` surface the way a host test would.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use flui_platform::{
    DisplayChange, DisplayChangeError, DisplaySpec, HeadlessPlatform, MockWindow, Platform as _,
    WindowOptions,
};

fn two_displays() -> HeadlessPlatform {
    HeadlessPlatform::new().with_displays(vec![
        DisplaySpec::new("Left", 1920, 1080).with_refresh_period(Duration::from_millis(16)),
        DisplaySpec::new("Right", 2560, 1440).with_scale_factor(2.0),
    ])
}

fn injected_displays_are_laid_out_and_the_first_is_primary() {
    let platform = two_displays();
    let snapshot = platform.display_snapshot().expect("snapshot");
    let [left, right] = snapshot.displays() else {
        panic!("two displays expected, got {:?}", snapshot.displays());
    };
    assert_ne!(left.id, right.id);
    assert!(left.is_primary && !right.is_primary);
    assert_eq!(left.bounds.origin.x, 0);
    assert_eq!(
        right.bounds.origin.x, 1920,
        "displays lay out left to right"
    );
    assert_eq!(right.scale_factor, 2.0);
    assert_eq!(
        left.refresh_period.map(flui_platform::RefreshPeriod::get),
        Some(Duration::from_millis(16))
    );
    assert_eq!(
        right.refresh_period, None,
        "an unspecified period is unknown"
    );
    assert_eq!(snapshot.primary().map(|d| d.id), Some(left.id));
}

fn display_change_advances_the_revision_once() {
    let platform = two_displays();
    let controls = platform.display_controls();
    let before = platform.display_snapshot().expect("snapshot");
    let id = before.displays()[0].id;

    controls
        .simulate_display_change(
            id,
            DisplayChange::RefreshPeriod(Some(Duration::from_micros(8333))),
        )
        .expect("change");
    let after = platform.display_snapshot().expect("snapshot");
    assert_eq!(
        after.revision().get(),
        before.revision().get() + 1,
        "one change is one revision"
    );
    assert_eq!(
        after
            .get(id)
            .and_then(|d| d.refresh_period)
            .map(flui_platform::RefreshPeriod::get),
        Some(Duration::from_micros(8333))
    );

    // Setting the value it already has publishes nothing new.
    controls
        .simulate_display_change(
            id,
            DisplayChange::RefreshPeriod(Some(Duration::from_micros(8333))),
        )
        .expect("change");
    assert_eq!(
        platform.display_snapshot().expect("snapshot").revision(),
        after.revision()
    );

    // A period that describes no cadence is unknown, not stored raw.
    controls
        .simulate_display_change(id, DisplayChange::RefreshPeriod(Some(Duration::ZERO)))
        .expect("change");
    assert_eq!(
        platform
            .display_snapshot()
            .expect("snapshot")
            .get(id)
            .and_then(|d| d.refresh_period),
        None
    );
}

fn an_id_the_platform_never_issued_is_refused() {
    let platform = two_displays();
    let controls = platform.display_controls();
    // Ids are issued per platform instance; one from another registry that
    // this platform did not issue is refused rather than aliased.
    let unissued = {
        let registry = flui_platform::DisplayRegistry::new();
        let _ = registry.mint(&flui_platform::DisplayKey::Unkeyed);
        let _ = registry.mint(&flui_platform::DisplayKey::Unkeyed);
        registry
            .mint(&flui_platform::DisplayKey::Unkeyed)
            .expect("third id")
    };
    assert_eq!(
        controls.simulate_display_change(unissued, DisplayChange::Name("x".into())),
        Err(DisplayChangeError::UnknownDisplay)
    );
}

fn window_move_names_the_new_display_and_hints_once() {
    let platform = two_displays();
    let snapshot = platform.display_snapshot().expect("snapshot");
    let (left, right) = (snapshot.displays()[0].id, snapshot.displays()[1].id);
    let window = platform
        .open_window(WindowOptions::default())
        .expect("window");
    let window: Arc<dyn flui_platform::HostWindow> = window;
    let mock = window
        .as_any()
        .downcast_ref::<MockWindow>()
        .expect("headless window")
        .clone();
    assert_eq!(
        window.display_id(),
        Some(left),
        "a window opens on the primary"
    );

    let hints = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&hints);
    window.on_display_changed(Box::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    }));
    mock.simulate_move_to(right).expect("move");
    assert_eq!(window.display_id(), Some(right));
    assert_eq!(hints.load(Ordering::SeqCst), 1);
    mock.simulate_move_to(right)
        .expect("move to where it already is");
    assert_eq!(
        hints.load(Ordering::SeqCst),
        1,
        "staying on the same display is not a change"
    );
}

#[test]
fn headless_displays_contract() {
    crate::run_table(
        "headless_displays_contract",
        &[
            (
                "injected_displays_are_laid_out_and_the_first_is_primary",
                injected_displays_are_laid_out_and_the_first_is_primary as fn(),
            ),
            (
                "display_change_advances_the_revision_once",
                display_change_advances_the_revision_once as fn(),
            ),
            (
                "an_id_the_platform_never_issued_is_refused",
                an_id_the_platform_never_issued_is_refused as fn(),
            ),
            (
                "window_move_names_the_new_display_and_hints_once",
                window_move_names_the_new_display_and_hints_once as fn(),
            ),
        ],
    );
}
