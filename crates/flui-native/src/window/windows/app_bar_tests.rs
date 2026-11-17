//! A maximized Win32 window without the system's title bar against the
//! auto-hide app bars of its monitor, with the shell's answer supplied where
//! the window reads it: the window's measured bars, which `WM_NCCALCSIZE` (an
//! application's title bar) and `WM_GETMINMAXINFO` (no frame) take before
//! they ask the shell. The machine's own taskbar setting is never changed.

#![expect(
    unsafe_code,
    reason = "reads the rectangles of the test's own Win32 windows"
)]

use std::rc::Rc;

use flui_platform::WindowOptions;
use flui_platform::{AppTitleBar, SystemControls, WindowCommand, WindowFrame, WindowMode};
use flui_testing::window_commands::AdmitAndApply as _;
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::UI::WindowsAndMessaging::{
    GetClientRect, GetWindowRect, SW_MAXIMIZE, SW_RESTORE, SendMessageW, ShowWindow,
    WM_SETTINGCHANGE,
};

use super::chrome::{AUTO_HIDE_BAR_REVEAL, AutoHideEdges};
use super::proc::{MonitorAppBars, with_window_context};
use super::window::WindowsWindow;
use crate::WindowsPlatform;
use crate::host_window::HostWindow;
use crate::platform::Platform as _;

fn open(platform: &WindowsPlatform, frame: WindowFrame) -> Rc<dyn HostWindow> {
    platform
        .open_window(
            WindowOptions::default()
                .title("app bars")
                .visible(false)
                .frame(frame),
            flui_testing::window_handler::RecordingWindowHandler::new(),
        )
        .expect("a window opens")
}

fn hwnd_of(window: &Rc<dyn HostWindow>) -> HWND {
    window
        .as_any()
        .downcast_ref::<WindowsWindow>()
        .expect("Win32 backend")
        .hwnd()
}

/// The client rectangle in screen coordinates.
fn client_on_screen(hwnd: HWND) -> RECT {
    let mut client = RECT::default();
    let mut origin = POINT::default();
    // SAFETY: stack locals of the live test window, on its thread.
    unsafe {
        GetClientRect(hwnd, &raw mut client).expect("the client rectangle");
        assert!(ClientToScreen(hwnd, &raw mut origin).as_bool());
    }
    RECT {
        left: origin.x,
        top: origin.y,
        right: origin.x + client.right,
        bottom: origin.y + client.bottom,
    }
}

/// The work area of the monitor `hwnd` is on.
fn work_area(hwnd: HWND) -> RECT {
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
    };

    let mut monitor = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: a stack local with `cbSize` set, of the live window's monitor.
    let read = unsafe {
        GetMonitorInfoW(
            MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST),
            &raw mut monitor,
        )
    };
    assert!(read.as_bool(), "the monitor is read");
    monitor.rcWork
}

/// Supply `edges` as the shell's answer for the window's monitor, maximize
/// the window again, and measure its client.
fn with_bars(hwnd: HWND, edges: AutoHideEdges) -> RECT {
    let mut window = RECT::default();
    // SAFETY: a stack local out-parameter of the live window.
    unsafe { GetWindowRect(hwnd, &raw mut window) }.expect("the window rectangle");
    let (monitor, _) = super::proc::monitor_at(&window).expect("the window's monitor");
    with_window_context(hwnd, "test app bars", |ctx| {
        ctx.app_bars.set(Some(MonitorAppBars { monitor, edges }));
    })
    .expect("the window's context");
    // SAFETY: plain show commands on the live test window, on its thread.
    unsafe {
        let _ = ShowWindow(hwnd, SW_RESTORE);
        let _ = ShowWindow(hwnd, SW_MAXIMIZE);
    }
    client_on_screen(hwnd)
}

/// For an application's title bar and for no frame at all, a maximized
/// window's client is the work area, the taskbar left uncovered (a window
/// without a frame covered the whole monitor), and it stands
/// `AUTO_HIDE_BAR_REVEAL` back from each edge the shell names, and from no
/// other.
#[test]
fn a_maximized_window_leaves_each_auto_hide_bar_its_edge() {
    let platform = WindowsPlatform::try_new().expect("native platform");
    let reveal = AUTO_HIDE_BAR_REVEAL;
    for frame in [
        WindowFrame::AppTitleBar(AppTitleBar::new().with_system_controls(SystemControls::Hidden)),
        WindowFrame::None,
    ] {
        let window = open(&platform, frame);
        window
            .admit_and_apply(WindowCommand::SetMode(WindowMode::Maximized))
            .expect("the window maximizes");
        let hwnd = hwnd_of(&window);
        let none = with_bars(hwnd, AutoHideEdges::default());
        assert_eq!(none, work_area(hwnd), "{frame:?}: the work area");
        for (name, edges, wanted) in [
            (
                "left",
                AutoHideEdges {
                    left: true,
                    ..AutoHideEdges::default()
                },
                RECT {
                    left: none.left + reveal,
                    ..none
                },
            ),
            (
                "top",
                AutoHideEdges {
                    top: true,
                    ..AutoHideEdges::default()
                },
                RECT {
                    top: none.top + reveal,
                    ..none
                },
            ),
            (
                "right",
                AutoHideEdges {
                    right: true,
                    ..AutoHideEdges::default()
                },
                RECT {
                    right: none.right - reveal,
                    ..none
                },
            ),
            (
                "bottom",
                AutoHideEdges {
                    bottom: true,
                    ..AutoHideEdges::default()
                },
                RECT {
                    bottom: none.bottom - reveal,
                    ..none
                },
            ),
        ] {
            assert_eq!(
                with_bars(hwnd, edges),
                wanted,
                "{frame:?}: a bar on the {name}"
            );
        }
        window.close_now();
    }
}

/// A settings change makes the window forget the bars it measured and ask
/// the shell again; a maximized window whose monitor's bars changed measures
/// its client again, though nothing resizes it.
#[test]
fn a_settings_change_measures_the_bars_again() {
    use windows::Win32::UI::Shell::{ABM_GETSTATE, ABS_AUTOHIDE, APPBARDATA, SHAppBarMessage};

    let mut query = APPBARDATA {
        cbSize: std::mem::size_of::<APPBARDATA>() as u32,
        ..Default::default()
    };
    // SAFETY: a stack local with `cbSize` set, read by the call.
    if unsafe { SHAppBarMessage(ABM_GETSTATE, &raw mut query) } & ABS_AUTOHIDE as usize != 0 {
        eprintln!("CANNOT_VERIFY a settings change: this machine's taskbar hides itself");
        return;
    }
    let platform = WindowsPlatform::try_new().expect("native platform");
    for frame in [
        WindowFrame::AppTitleBar(AppTitleBar::new().with_system_controls(SystemControls::Hidden)),
        WindowFrame::None,
    ] {
        let window = open(&platform, frame);
        window
            .admit_and_apply(WindowCommand::SetMode(WindowMode::Maximized))
            .expect("the window maximizes");
        let hwnd = hwnd_of(&window);
        let none = with_bars(hwnd, AutoHideEdges::default());
        let with_bottom_bar = with_bars(
            hwnd,
            AutoHideEdges {
                bottom: true,
                ..AutoHideEdges::default()
            },
        );
        assert_ne!(
            with_bottom_bar, none,
            "{frame:?}: the supplied bar is honoured"
        );
        // SAFETY: a message to the live window on its own thread, as the
        // shell broadcasts it.
        unsafe { SendMessageW(hwnd, WM_SETTINGCHANGE, None, None) };
        assert_eq!(
            client_on_screen(hwnd),
            none,
            "{frame:?}: the shell, asked again, names no auto-hide bar on this machine"
        );
        window.close_now();
    }
}
