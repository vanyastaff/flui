//! A widget reaches its window through the escape hatch's capability
//! (ADR-0254 §8): it acquires `NativeWindowCapability` in `create_state`, and
//! the handle runs a closure on the window as the core host hands it out. On
//! the headless platform that window has no operating system's view, the
//! typed absence a widget meets wherever no extension applies. An
//! application's test stands in for the built-in provider with a window of
//! its own.

use std::cell::OnceCell;
use std::rc::Rc;

use flui_app::Application;
use flui_app::native::{HostWindow, NativeWindow, NativeWindowCapability};
use flui_app::testing::{HeadlessRun, HeadlessRunHandle};
use flui_native::HeadlessPlatform;
use flui_platform::Unsupported;
use flui_view::prelude::*;

/// A root that acquires its window in `create_state`, as the forms ask.
#[derive(Clone, StatefulView)]
struct Root {
    acquired: Rc<OnceCell<Result<NativeWindow, Unsupported>>>,
}

struct RootState;

impl StatefulView for Root {
    type State = RootState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        self.acquired
            .set(ctx.capability::<NativeWindowCapability>())
            .expect("the root acquires its window once, in create_state");
        RootState
    }
}

impl ViewState<Root> for RootState {
    fn build(&self, _view: &Root, _ctx: &dyn BuildContext) -> impl IntoView {
        flui_widgets::SizedBox::shrink()
    }
}

/// `application` up on the headless platform, and what its root acquired.
fn acquired_by(application: Application) -> (HeadlessRunHandle, Result<NativeWindow, Unsupported>) {
    let acquired = Rc::new(OnceCell::new());
    let root = Root {
        acquired: Rc::clone(&acquired),
    };
    let run = HeadlessRun::start_on(application, root, HeadlessPlatform::new());
    run.run_owner_turn();
    let handle = acquired
        .get()
        .expect("the owner turn mounted the root")
        .clone();
    (run, handle)
}

/// Whether `window` has no operating system's view: no Win32 view on
/// Windows, no AppKit view on macOS.
fn has_no_os_view(window: &dyn HostWindow) -> bool {
    #[cfg(windows)]
    {
        use flui_app::native::windows::WindowsWindowExt as _;
        if window.win32().is_some() {
            return false;
        }
    }
    #[cfg(target_os = "macos")]
    {
        use flui_app::native::macos::MacOSWindowExt as _;
        if window.appkit().is_some() {
            return false;
        }
    }
    let _ = window;
    true
}

/// The built-in provider hands a windowed widget its window: the handle
/// reaches the headless window the root is presented in, which has no
/// operating system's view.
pub(crate) fn a_widget_acquires_its_window_in_create_state() {
    let (run, handle) = acquired_by(Application::new());
    let handle = handle.expect("a windowed widget acquires its window");
    assert_eq!(
        handle.with(has_no_os_view),
        Some(true),
        "the headless window is reached, and has no operating system's view"
    );
    drop(run);
}

/// An application's test stands in for the built-in provider: the override
/// hands out a handle on a window the test opened, and the widget's handle
/// reaches that window and no other.
pub(crate) fn a_test_stands_in_with_a_window_of_its_own() {
    let window: Rc<dyn HostWindow> = flui_native::headless_platform()
        .open_window(
            flui_platform::WindowOptions::default().visible(false),
            flui_testing::window_handler::RecordingWindowHandler::new(),
        )
        .expect("a headless window opens");
    let stand_in = Rc::clone(&window);
    let application = Application::new().capability::<NativeWindowCapability>(
        move |_: &flui_platform::capability::ProviderContext<'_>| Ok(NativeWindow::new(&stand_in)),
    );
    let (run, handle) = acquired_by(application);
    let handle = handle.expect("the override hands out its handle");
    assert_eq!(
        handle.with(|reached| std::ptr::addr_eq(std::ptr::from_ref(reached), Rc::as_ptr(&window))),
        Some(true),
        "the stand-in's window"
    );
    drop(run);
}
