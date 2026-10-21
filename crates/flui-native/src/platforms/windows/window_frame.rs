//! The rules of a Win32 window's frame and first show that need no window:
//! which style bits make a frame, which command gives a window its opening mode,
//! what is still owed of its first show, and how a screen rectangle becomes the
//! workspace rectangle `WINDOWPLACEMENT` takes.

use windows::Win32::{
    Foundation::RECT,
    UI::WindowsAndMessaging::{
        SW_SHOW, SW_SHOWMAXIMIZED, SW_SHOWMINIMIZED, WS_MAXIMIZEBOX, WS_OVERLAPPEDWINDOW, WS_POPUP,
        WS_THICKFRAME,
    },
};

use flui_platform::WindowMode;

/// What is still owed of a window's first show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FirstShow {
    /// Nothing: the window has been shown, or was hidden by the app.
    Done,
    /// The window waits for an explicit show, which gives it this mode.
    OnShow(WindowMode),
    /// The window waits for the embedder's first presented frame, which gives
    /// it this mode. Until then it is hidden for the system and visible for
    /// the runtime, whose frames must run to earn the reveal.
    AfterFirstFrame(WindowMode),
}

impl FirstShow {
    /// The mode the first show gives the window; `None` once it has happened.
    pub(super) fn mode(self) -> Option<WindowMode> {
        match self {
            Self::Done => None,
            Self::OnShow(mode) | Self::AfterFirstFrame(mode) => Some(mode),
        }
    }

    /// Whether the first presented frame shows the window.
    pub(super) fn awaits_frame(self) -> bool {
        matches!(self, Self::AfterFirstFrame(_))
    }

    /// The same wait with another mode; nothing to change once shown.
    pub(super) fn staged(self, mode: WindowMode) -> Self {
        match self {
            Self::Done => Self::Done,
            Self::OnShow(_) => Self::OnShow(mode),
            Self::AfterFirstFrame(_) => Self::AfterFirstFrame(mode),
        }
    }

    /// An explicit show still gives the mode; the first frame no longer does.
    pub(super) fn without_frame_wait(self) -> Self {
        match self {
            Self::AfterFirstFrame(mode) => Self::OnShow(mode),
            other => other,
        }
    }
}

/// The style bits that make up a window's frame: a title bar and borders when
/// it is decorated, a bare popup when it is not; and, when resizable, the
/// sizing border and the maximize box. A window without a frame cannot be
/// resized by its user, so `resizable` has no bit to set on a popup.
pub(super) fn frame_style(decorated: bool, resizable: bool) -> u32 {
    if !decorated {
        WS_POPUP.0
    } else if resizable {
        WS_OVERLAPPEDWINDOW.0
    } else {
        WS_OVERLAPPEDWINDOW.0 & !(WS_THICKFRAME.0 | WS_MAXIMIZEBOX.0)
    }
}

/// The style a window has after its frame changes to `decorated`/`resizable`:
/// `style` with its frame bits replaced and everything else (visible,
/// maximized, minimized, clip bits) kept.
pub(super) fn restyled(style: u32, decorated: bool, resizable: bool) -> u32 {
    let frame_bits = WS_OVERLAPPEDWINDOW.0 | WS_POPUP.0;
    (style & !frame_bits) | frame_style(decorated, resizable)
}

/// The show command that gives a window that has never been shown the mode it
/// opened in. Fullscreen is entered while the window is still hidden, so it
/// is shown like a normal one.
pub(super) fn first_show_command(
    mode: WindowMode,
) -> windows::Win32::UI::WindowsAndMessaging::SHOW_WINDOW_CMD {
    match mode {
        WindowMode::Maximized => SW_SHOWMAXIMIZED,
        WindowMode::Minimized => SW_SHOWMINIMIZED,
        _ => SW_SHOW,
    }
}

/// A rectangle in screen coordinates as the workspace coordinates
/// `WINDOWPLACEMENT::rcNormalPosition` uses: measured from the work area,
/// which a taskbar on the top or left edge moves away from the monitor's origin
/// by `origin`.
///
/// Saturating: a rectangle at the edge of the range stays there instead of
/// wrapping to the other side of the desktop.
pub(super) fn to_workspace(rect: RECT, origin: (i32, i32)) -> RECT {
    RECT {
        left: rect.left.saturating_sub(origin.0),
        top: rect.top.saturating_sub(origin.1),
        right: rect.right.saturating_sub(origin.0),
        bottom: rect.bottom.saturating_sub(origin.1),
    }
}

/// The inverse of [`to_workspace`].
pub(super) fn to_screen(rect: RECT, origin: (i32, i32)) -> RECT {
    RECT {
        left: rect.left.saturating_add(origin.0),
        top: rect.top.saturating_add(origin.1),
        right: rect.right.saturating_add(origin.0),
        bottom: rect.bottom.saturating_add(origin.1),
    }
}

impl super::platform::WindowContext {
    /// Before a window is first shown in `mode`: a window shown minimized is
    /// already minimized when the system announces the show, so the show does
    /// not read as the window appearing.
    pub(super) fn begin_first_show(&self, mode: WindowMode) {
        if mode == WindowMode::Minimized {
            self.mode.set(super::window::NativeMode::Minimized {
                from_fullscreen: None,
            });
        }
    }

    /// After a window was first shown. A window that came up minimized is not
    /// visible to the runtime, whatever it was told before (a deferred window
    /// is told it is visible so that its frames run). Whether it did is read
    /// from the window, not from the mode asked for: a launcher can choose the
    /// first show state.
    pub(super) fn end_first_show(&self, minimized: bool) {
        if minimized {
            self.report_surface_visibility(false);
        }
    }

    /// Tell the runtime the window's real visibility if what it was told after
    /// a muted restore disagrees with it (a show that came in meanwhile).
    pub(super) fn reconcile_visibility(&self, visible: bool) {
        self.report_surface_visibility(visible);
    }

    /// Note that the app asked for the window to be shown, so a restore that
    /// hides it again afterwards can tell it was overtaken.
    pub(super) fn note_show_request(&self) {
        self.show_requests
            .set(self.show_requests.get().wrapping_add(1));
    }
}

#[cfg(test)]
mod frame_tests {
    use windows::Win32::{
        Foundation::RECT,
        UI::WindowsAndMessaging::{
            SW_SHOW, SW_SHOWMAXIMIZED, SW_SHOWMINIMIZED, WS_CAPTION, WS_MAXIMIZE, WS_MAXIMIZEBOX,
            WS_POPUP, WS_THICKFRAME, WS_VISIBLE,
        },
    };

    use super::{WindowMode, first_show_command, frame_style, restyled, to_screen, to_workspace};

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT {
            left,
            top,
            right,
            bottom,
        }
    }

    fn same(a: RECT, b: RECT) -> bool {
        (a.left, a.top, a.right, a.bottom) == (b.left, b.top, b.right, b.bottom)
    }

    /// A task bar on the top edge moves the work area down from the monitor's
    /// origin, so workspace coordinates are screen coordinates shifted up.
    fn a_taskbar_on_the_top_edge_shifts_workspace_coordinates() {
        let screen = rect(100, 200, 800, 700);
        let workspace = to_workspace(screen, (0, 48));
        assert!(same(workspace, rect(100, 152, 800, 652)), "{workspace:?}");
        assert!(same(to_screen(workspace, (0, 48)), screen));
    }

    /// A task bar on the left edge moves the work area right.
    fn a_taskbar_on_the_left_edge_shifts_workspace_coordinates() {
        let screen = rect(100, 200, 800, 700);
        let workspace = to_workspace(screen, (72, 0));
        assert!(same(workspace, rect(28, 200, 728, 700)), "{workspace:?}");
        assert!(same(to_screen(workspace, (72, 0)), screen));
    }

    fn a_rectangle_at_the_edge_of_the_range_saturates_in_both_directions() {
        let edge = rect(i32::MIN, i32::MIN, i32::MIN + 10, i32::MIN + 10);
        let moved = to_workspace(edge, (72, 48));
        assert_eq!((moved.left, moved.top), (i32::MIN, i32::MIN));
        let high = rect(i32::MAX - 5, i32::MAX - 5, i32::MAX, i32::MAX);
        let back = to_screen(high, (72, 48));
        assert_eq!((back.right, back.bottom), (i32::MAX, i32::MAX));
    }

    fn a_taskbar_on_the_bottom_or_right_leaves_the_origin_alone() {
        let screen = rect(-1920, 10, -1000, 500);
        assert!(same(to_workspace(screen, (0, 0)), screen));
    }

    fn the_first_show_gives_the_opening_mode() {
        assert_eq!(first_show_command(WindowMode::Maximized), SW_SHOWMAXIMIZED);
        assert_eq!(first_show_command(WindowMode::Minimized), SW_SHOWMINIMIZED);
        // Fullscreen is entered while hidden and shown like a normal window.
        assert_eq!(first_show_command(WindowMode::Normal), SW_SHOW);
        assert_eq!(first_show_command(WindowMode::Fullscreen), SW_SHOW);
    }

    fn a_frame_has_a_caption_and_sizing_only_as_asked() {
        let sizing = WS_THICKFRAME.0 | WS_MAXIMIZEBOX.0;
        let resizable = frame_style(true, true);
        assert_eq!(resizable & WS_CAPTION.0, WS_CAPTION.0);
        assert_eq!(resizable & sizing, sizing);
        let fixed = frame_style(true, false);
        assert_eq!(fixed & WS_CAPTION.0, WS_CAPTION.0);
        assert_eq!(fixed & sizing, 0);
        for resizable in [true, false] {
            let bare = frame_style(false, resizable);
            assert_eq!(bare, WS_POPUP.0, "a popup has neither caption nor sizing");
        }
    }

    fn a_new_frame_keeps_the_state_bits_of_the_style() {
        let style = frame_style(true, true) | WS_VISIBLE.0 | WS_MAXIMIZE.0;
        let bare = restyled(style, false, true);
        assert_eq!(
            bare & (WS_VISIBLE.0 | WS_MAXIMIZE.0),
            WS_VISIBLE.0 | WS_MAXIMIZE.0
        );
        assert_eq!(bare & WS_CAPTION.0, 0);
        let back = restyled(bare, true, false);
        assert_eq!(back & WS_CAPTION.0, WS_CAPTION.0);
        assert_eq!(back & (WS_THICKFRAME.0 | WS_MAXIMIZEBOX.0), 0);
        assert_eq!(back & WS_POPUP.0, 0, "the popup bit does not linger");
        assert_eq!(
            back & (WS_VISIBLE.0 | WS_MAXIMIZE.0),
            WS_VISIBLE.0 | WS_MAXIMIZE.0
        );
    }

    #[test]
    fn window_frame_rules() {
        crate::table_test::run_table(
            "window_frame_rules",
            &[
                (
                    "a_taskbar_on_the_top_edge_shifts_workspace_coordinates",
                    a_taskbar_on_the_top_edge_shifts_workspace_coordinates as fn(),
                ),
                (
                    "a_taskbar_on_the_left_edge_shifts_workspace_coordinates",
                    a_taskbar_on_the_left_edge_shifts_workspace_coordinates as fn(),
                ),
                (
                    "a_rectangle_at_the_edge_of_the_range_saturates_in_both_directions",
                    a_rectangle_at_the_edge_of_the_range_saturates_in_both_directions as fn(),
                ),
                (
                    "a_taskbar_on_the_bottom_or_right_leaves_the_origin_alone",
                    a_taskbar_on_the_bottom_or_right_leaves_the_origin_alone as fn(),
                ),
                (
                    "the_first_show_gives_the_opening_mode",
                    the_first_show_gives_the_opening_mode as fn(),
                ),
                (
                    "a_frame_has_a_caption_and_sizing_only_as_asked",
                    a_frame_has_a_caption_and_sizing_only_as_asked as fn(),
                ),
                (
                    "a_new_frame_keeps_the_state_bits_of_the_style",
                    a_new_frame_keeps_the_state_bits_of_the_style as fn(),
                ),
            ],
        );
    }
}
