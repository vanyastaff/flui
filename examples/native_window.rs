//! Native window — what one operating system's window offers beyond the
//! window contract (`flui::native`, ADR-0254 §8), reached from a widget.
//!
//! The view acquires its window as a capability in `create_state` and keeps
//! the handle in its state; a button's press runs a closure on the window
//! (`NativeWindow::with`), which narrows it to its system's view: `win32()`
//! on Windows, `appkit()` on macOS. Each command does what it says or answers
//! why not, and the answer is shown under the buttons. On any other system
//! the example says there is nothing to show.
//!
//! Run with: cargo run --example native_window --features native

use flui::native::{NativeWindow, NativeWindowCapability};
use flui::platform::Unsupported;
use flui::prelude::*;
use flui::widgets::SafeArea;

#[derive(Clone, StatefulView)]
struct NativeWindowView;

struct NativeWindowState {
    /// The window this view is presented in, or why it has none.
    window: Result<NativeWindow, Unsupported>,
    /// What the last command answered.
    status: Signal<String>,
}

impl StatefulView for NativeWindowView {
    type State = NativeWindowState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        NativeWindowState {
            window: ctx.capability::<NativeWindowCapability>(),
            status: ctx.signal(String::from("Press a button.")),
        }
    }
}

impl ViewState<NativeWindowView> for NativeWindowState {
    fn build(&self, _view: &NativeWindowView, ctx: &dyn BuildContext) -> impl IntoView {
        let status = self.status;
        let controls: BoxedView = match &self.window {
            Ok(window) => system::controls(window, status),
            Err(why) => Text::new(format!("This view has no window: {why}")).boxed(),
        };
        SafeArea::new(
            Center::new().child(
                Column::new((controls, SizedBox::height(16.0), Text::new(status.get(ctx))))
                    .main_axis_alignment(MainAxisAlignment::Center),
            ),
        )
    }
}

/// A button that runs `command` on the window and shows what it answered.
#[cfg(any(windows, target_os = "macos"))]
fn command_button(
    label: &str,
    window: &NativeWindow,
    status: Signal<String>,
    command: impl Fn(&dyn flui::native::HostWindow) -> String + 'static,
) -> RawButton {
    let window = window.clone();
    RawButton::new(Text::new(label.to_owned())).on_pressed(move |ctx| {
        let answer = window
            .with(&command)
            .unwrap_or_else(|| String::from("The window has closed."));
        status.set(ctx, answer)
    })
}

#[cfg(windows)]
mod system {
    use std::cell::Cell;
    use std::rc::Rc;

    use flui::native::NativeWindow;
    use flui::native::windows::{WindowsBackdrop, WindowsWindowExt};
    use flui::prelude::*;

    /// What a Win32 window answered, as a line to show.
    fn answered<T: std::fmt::Debug>(
        result: Result<T, flui::native::windows::Win32WindowError>,
    ) -> String {
        match result {
            Ok(value) => format!("Done: {value:?}"),
            Err(why) => format!("Refused: {why}"),
        }
    }

    pub(super) fn controls(window: &NativeWindow, status: Signal<String>) -> BoxedView {
        let colours = [
            Some(Color::rgb(0x1F, 0x4E, 0x79)),
            Some(Color::rgb(0x7A, 0x1F, 0x3D)),
            None,
        ];
        let next_colour = Rc::new(Cell::new(0_usize));
        let mica = Rc::new(Cell::new(true));
        Column::new((
            super::command_button("Title bar colour", window, status, move |host| {
                let index = next_colour.get();
                next_colour.set((index + 1) % colours.len());
                host.win32().map_or_else(
                    || String::from("Not a Win32 window."),
                    |win32| answered(win32.set_title_bar_color(colours[index])),
                )
            }),
            SizedBox::height(8.0),
            super::command_button("Mica or none", window, status, move |host| {
                let backdrop = if mica.replace(!mica.get()) {
                    WindowsBackdrop::None
                } else {
                    WindowsBackdrop::Mica
                };
                host.win32().map_or_else(
                    || String::from("Not a Win32 window."),
                    |win32| answered(win32.set_backdrop(backdrop).map(|()| backdrop)),
                )
            }),
        ))
        .boxed()
    }
}

#[cfg(target_os = "macos")]
mod system {
    use std::cell::Cell;
    use std::rc::Rc;

    use flui::native::NativeWindow;
    use flui::native::macos::{MacOSWindowAlpha, MacOSWindowExt, MacOSWindowLevel};
    use flui::prelude::*;

    /// What an AppKit window answered, as a line to show.
    fn answered<T: std::fmt::Debug>(
        result: Result<T, flui::native::macos::AppKitWindowError>,
    ) -> String {
        match result {
            Ok(value) => format!("Done: {value:?}"),
            Err(why) => format!("Refused: {why}"),
        }
    }

    pub(super) fn controls(window: &NativeWindow, status: Signal<String>) -> BoxedView {
        let floating = Rc::new(Cell::new(false));
        let translucent = MacOSWindowAlpha::try_new(0.8).expect("0.8 is an alpha");
        Column::new((
            super::command_button("Float above other windows", window, status, move |host| {
                let level = if floating.replace(!floating.get()) {
                    MacOSWindowLevel::Normal
                } else {
                    MacOSWindowLevel::Floating
                };
                host.appkit().map_or_else(
                    || String::from("Not an AppKit window."),
                    |appkit| answered(appkit.set_window_level(level).map(|()| level)),
                )
            }),
            SizedBox::height(8.0),
            super::command_button("Translucent or opaque", window, status, move |host| {
                host.appkit().map_or_else(
                    || String::from("Not an AppKit window."),
                    |appkit| {
                        let alpha = match appkit.alpha() {
                            Ok(alpha) if alpha == MacOSWindowAlpha::OPAQUE => translucent,
                            Ok(_) => MacOSWindowAlpha::OPAQUE,
                            Err(why) => return answered(Err::<(), _>(why)),
                        };
                        answered(appkit.set_alpha(alpha).map(|()| alpha.get()))
                    },
                )
            }),
        ))
        .boxed()
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod system {
    use flui::native::NativeWindow;
    use flui::prelude::*;

    pub(super) fn controls(_window: &NativeWindow, _status: Signal<String>) -> BoxedView {
        Text::new("flui::native has no view of this system's windows.").boxed()
    }
}

fn main() -> Result<(), LaunchError> {
    run_app(NativeWindowView)
}
