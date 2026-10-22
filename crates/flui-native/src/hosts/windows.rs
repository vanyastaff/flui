//! The Windows host: Win32 windows, OLE drag and drop, TSF text input and the
//! Win32 message loop.

use std::rc::Rc;
use std::sync::Arc;

#[cfg(feature = "a11y")]
use crate::accessibility::windows::WindowsAccessibility;
use crate::clipboard::windows::WindowsClipboard;
use crate::display::windows::{DisplayBroadcasts, WindowsDisplays};
use crate::drag::windows::{DropTargetRegistration, OleTransfer};
use crate::error::PlatformError;
use crate::haptics::windows::WindowsHaptics;
use crate::host::{Absent, Host, position};
use crate::input::windows::{InputMessages, Touchpad};
use crate::lifecycle::notice::SystemNotices;
use crate::lifecycle::windows::HostEvents;
use crate::owner::notices::NoticeChannel;
use crate::owner::windows::broadcast::Subscriber;
use crate::owner::windows::control::{OwnerLifecycle, OwnerObjects, OwnerPreferences, OwnerSeeds};
use crate::owner::windows::{HostDisplays, HostDrag, WindowHooks, WindowsHost, WindowsLoop};
use crate::platform::HostPlatform;
use crate::preferences::windows::PreferenceSource;
use crate::storage::UserDataDirs;
use crate::text::windows::{TextServices, Tsf};
use crate::window::config::WindowConfiguration;
use crate::window::windows::proc::WindowProc;
use crate::window::windows::{WindowSet, WindowsWindow};

/// The Windows host's table.
#[derive(Debug)]
pub struct Windows;

impl Host for Windows {
    const NAME: &'static str = "Windows";

    type Loop = WindowsLoop;
    type Window = WindowsWindow;
    type Displays = WindowsLoop;
    type Input = WindowsLoop;
    type Text = Rc<TextServices>;
    #[cfg(feature = "a11y")]
    type Accessibility = WindowsAccessibility;
    #[cfg(not(feature = "a11y"))]
    type Accessibility = Absent<position::Accessibility>;
    type Clipboard = WindowsClipboard;
    type Drag = WindowsLoop;
    type Haptics = WindowsHaptics;
    type Preferences = WindowsLoop;
    type Lifecycle = WindowsLoop;
    type Chrome = Absent<position::Chrome>;
    type Storage = UserDataDirs;
}

impl WindowsHost for Windows {
    /// The text services, which hear a window's moves before its procedure
    /// answers them and answer nothing themselves; the window's own messages;
    /// its input messages; the drop target, which only learns of each window
    /// created; and the precision touchpad's (its hit test and pacing timer,
    /// and the viewport each new window gets). The hooks list disjoint
    /// messages (`the_window_hooks_answer_disjoint_messages`); what none
    /// answers gets the default procedure. A created window activates TSF,
    /// then registers its drop target, then gets its touchpad viewport.
    const WINDOW_HOOKS: WindowHooks = &[
        &Tsf,
        &WindowProc,
        &InputMessages,
        &DropTargetRegistration,
        &Touchpad,
    ];

    fn preferences(owner: &OwnerSeeds<'_>) -> Result<Rc<dyn OwnerPreferences>, PlatformError> {
        Ok(Rc::new(PreferenceSource::new(owner.signal)?))
    }

    fn notices() -> Arc<dyn NoticeChannel> {
        Arc::new(SystemNotices::default())
    }

    fn lifecycle(owner: &OwnerSeeds<'_>) -> Rc<dyn OwnerLifecycle> {
        Rc::new(HostEvents::new(
            Rc::clone(owner.handlers),
            Arc::clone(owner.notices),
        ))
    }

    /// The displays (display and work-area changes) and the preferences
    /// (setting changes) only observe, so their order carries no meaning; the
    /// lifecycle (session, power, lock and memory) answers the session-end
    /// query and the power broadcast, which nothing else hears.
    fn broadcast_subscribers(owner: &OwnerObjects<'_>) -> Vec<Rc<dyn Subscriber>> {
        vec![
            Rc::new(DisplayBroadcasts::new(owner.displays, owner.signal)),
            owner.preferences.broadcasts(),
            Rc::clone(owner.lifecycle) as Rc<dyn Subscriber>,
        ]
    }

    fn displays() -> Arc<dyn HostDisplays> {
        Arc::new(WindowsDisplays::new())
    }

    /// OLE drag and drop, whose sessions run from the owner window.
    fn drag() -> Arc<dyn HostDrag> {
        OleTransfer::new()
    }
}

/// The native Windows platform: Win32 windows and message loop, OLE drag and
/// drop, TSF text input and the system's clipboard, displays and preferences.
pub type WindowsPlatform = HostPlatform<Windows>;

impl HostPlatform<Windows> {
    /// Create a new Windows platform instance with default configuration
    pub fn new() -> Result<Self, PlatformError> {
        Self::with_config(WindowConfiguration::default())
    }

    /// Create a new Windows platform instance with custom configuration
    ///
    /// # Arguments
    ///
    /// * `config` - Window configuration (hotkeys, debouncing, fullscreen
    ///   behavior)
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use flui_native::{WindowsPlatform, WindowConfiguration, FullscreenMonitor};
    ///
    /// // Disable F11 hotkey
    /// let config = WindowConfiguration::no_hotkey();
    /// let platform = WindowsPlatform::with_config(config)?;
    ///
    /// // Use primary monitor for fullscreen
    /// let config = WindowConfiguration {
    ///     fullscreen_monitor: FullscreenMonitor::Primary,
    ///     ..Default::default()
    /// };
    /// let platform = WindowsPlatform::with_config(config)?;
    /// ```
    pub fn with_config(config: WindowConfiguration) -> Result<Self, PlatformError> {
        Self::try_from_parts(WindowSet::new(config), |windows| {
            WindowsLoop::new::<Windows>(Arc::clone(windows) as _)
        })
    }
}

#[cfg(test)]
mod tests {
    use windows::Win32::Foundation::WPARAM;
    use windows::Win32::UI::WindowsAndMessaging::{DM_POINTERHITTEST, WM_SYSCOMMAND, WM_TIMER};

    use super::Windows;
    use crate::owner::windows::WindowsHost;

    /// How many of the table's window hooks answer `msg` with `wparam`.
    fn answering(msg: u32, wparam: usize) -> usize {
        <Windows as WindowsHost>::WINDOW_HOOKS
            .iter()
            .filter(|hook| hook.answers(msg, WPARAM(wparam)))
            .count()
    }

    /// The table's window hooks list disjoint messages, so the order the
    /// class's procedure offers them a message in decides nothing: no message
    /// number, and no timer or system command of the messages that carry one,
    /// is answered by two hooks. A `WM_TIMER` arm in the window's procedure
    /// that does not ask which timer fired would take the touchpad's.
    #[test]
    fn the_window_hooks_answer_disjoint_messages() {
        for msg in 0..=0xFFFF_u32 {
            assert!(
                answering(msg, 0) <= 1,
                "message {msg:#06x} is answered by more than one hook"
            );
        }
        for wparam in 0..=0xFFFF_usize {
            assert!(
                answering(WM_TIMER, wparam) <= 1,
                "timer {wparam:#06x} is answered by more than one hook"
            );
            assert!(
                answering(WM_SYSCOMMAND, wparam) <= 1,
                "system command {wparam:#06x} is answered by more than one hook"
            );
        }
        // What the touchpad answers still reaches it: the hit test, and two
        // timers in all, the window's modal deadline and the touchpad's pump.
        assert_eq!(answering(DM_POINTERHITTEST, 0), 1);
        let timers = (0..=0xFFFF_usize)
            .filter(|&timer| answering(WM_TIMER, timer) == 1)
            .count();
        assert_eq!(timers, 2, "the modal deadline timer and the touchpad's");
    }
}
