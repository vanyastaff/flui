//! A window whose surface target is asked, while its close lets go of the
//! handler, to keep the native window past the close
//! (`SurfaceTarget::keep_past_close`, ADR-0305 §9), as `flui-app`'s raster
//! host asks when a raster thread did not take the window's slot down: the
//! window is closed to the application all the same, and the headless window
//! reports the keep.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use flui_native::{HeadlessHostWindow, HeadlessPlatform, HostWindow, Platform as _};
use flui_platform::{
    DispatchEventResult, ImeEvent, PlatformInput, SurfaceTarget, WindowCommand, WindowOptions,
};
use flui_testing::window_commands::AdmitAndApply as _;
use flui_testing::window_handler::RecordingWindowHandler;

/// Asks its target to keep the native window when dropped: what the
/// renderer a window's handler owns does when its raster thread did not
/// answer its detach.
struct KeepOnDrop(Arc<dyn SurfaceTarget>);

impl Drop for KeepOnDrop {
    fn drop(&mut self) {
        self.0.keep_past_close();
    }
}

/// A window of `platform`, the headless window behind it, and how many inputs
/// reached its handler. The handler is the window's alone: `keep` is dropped
/// with it, so the keep is asked while the close lets go of it.
fn open(
    platform: &HeadlessPlatform,
    keep: bool,
) -> (Rc<dyn HostWindow>, HeadlessHostWindow, Rc<Cell<usize>>) {
    let handler = RecordingWindowHandler::new();
    let window = platform
        .open_window(WindowOptions::default(), handler.clone())
        .expect("a headless window opens");
    let headless = window
        .as_any()
        .downcast_ref::<HeadlessHostWindow>()
        .expect("the headless platform opens headless windows")
        .clone();
    let inputs = Rc::new(Cell::new(0));
    let counted = Rc::clone(&inputs);
    let keep = keep.then(|| KeepOnDrop(window.surface_target()));
    handler.react_to_input(move |_| {
        let _released_with_the_handler = &keep;
        counted.set(counted.get() + 1);
        DispatchEventResult::default()
    });
    (window, headless, inputs)
}

/// A keep asked while the close lets go of the handler is reported, and the
/// window is closed to the application: out of the platform's windows, its
/// handler released, and input simulated for it reaching nothing. Red-check:
/// with `HeadlessSurface::keep_past_close` storing nothing, the window does
/// not report the keep.
fn a_window_kept_past_its_close_is_closed_to_the_application() {
    let platform = HeadlessPlatform::new();
    let open_windows = platform.open_windows();
    let (window, headless, inputs) = open(&platform, true);
    let id = window.id();
    assert!(open_windows.get(id).is_some(), "the window is open");
    let input = || PlatformInput::Ime(ImeEvent::Enabled);
    headless.inject_event(input());
    let before = inputs.get();
    assert!(before > 0, "an open window's input reaches its handler");

    window
        .admit_and_apply(WindowCommand::Close)
        .expect("the window closes");

    assert!(
        headless.kept_past_close(),
        "the keep asked during the close is reported"
    );
    assert!(
        open_windows.get(id).is_none(),
        "a kept window is in no window list"
    );
    assert_eq!(
        Rc::strong_count(&inputs),
        1,
        "the close let go of the handler, and of what it held"
    );
    headless.inject_event(input());
    assert_eq!(inputs.get(), before, "a kept window delivers no input");
}

/// A window closed with no keep reports none, and a keep asked after its
/// close let go of the handler, when the native window is gone, does
/// nothing. Red-check: with `complete_close` letting go of the handler by
/// `clear()` and never settling the window's fate, the late keep is
/// reported.
fn a_keep_after_the_close_does_nothing() {
    let platform = HeadlessPlatform::new();
    let (window, headless, _inputs) = open(&platform, false);
    let target = window.surface_target();

    window
        .admit_and_apply(WindowCommand::Close)
        .expect("the window closes");
    assert!(!headless.kept_past_close(), "nothing asked for a keep");

    target.keep_past_close();
    target.keep_past_close();
    assert!(
        !headless.kept_past_close(),
        "a keep after the close keeps nothing"
    );
}

#[test]
fn kept_past_close_contracts() {
    super::run_table(
        "kept_past_close",
        &[
            (
                "a_window_kept_past_its_close_is_closed_to_the_application",
                a_window_kept_past_its_close_is_closed_to_the_application,
            ),
            (
                "a_keep_after_the_close_does_nothing",
                a_keep_after_the_close_does_nothing,
            ),
        ],
    );
}
