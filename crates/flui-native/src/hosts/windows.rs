//! The Windows host: Win32 windows, OLE drag and drop, TSF text input and the
//! Win32 message loop.

use std::rc::Rc;
use std::sync::Arc;

use crate::accessibility::windows::WindowsAccessibility;
use crate::clipboard::windows::WindowsClipboard;
use crate::display::windows::{DisplayBroadcasts, WindowsDisplays};
use crate::drag::windows::{DropTargetRegistration, OleTransfer};
use crate::error::PlatformError;
use crate::haptics::windows::WindowsHaptics;
use crate::host::{Absent, Host, position};
use crate::input::windows::InputWindows;
use crate::lifecycle::notice::SystemNotices;
use crate::lifecycle::windows::{HostEvents, SessionWindows};
use crate::owner::app_delivery::AppDelivery;
use crate::owner::notices::NoticeChannel;
use crate::owner::windows::broadcast::Subscriber;
use crate::owner::windows::control::{OwnerLifecycle, OwnerObjects, OwnerPreferences, OwnerSeeds};
use crate::owner::windows::{
    HostDisplays, HostDrag, WindowClass, WindowHooks, WindowMessages, WindowsHost, WindowsLoop,
};
use crate::platform::HostPlatform;
use crate::preferences::windows::PreferenceSource;
use crate::storage::UserDataDirs;
use crate::text::windows::Tsf;
use crate::version::gate::windows::WindowsBuild;
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
    type Text = Tsf;
    type Accessibility = WindowsAccessibility;
    type Clipboard = WindowsClipboard;
    type PrimarySelection = Absent<position::PrimarySelection>;
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
    /// its input (the mouse, pointers, wheel, keyboard and keyboard layout,
    /// and the precision touchpad's hit test and pacing timer); the session,
    /// whose lock and unlock it answers; and the drop target, which answers
    /// nothing. The hooks list disjoint messages
    /// (`the_window_hooks_answer_disjoint_messages`); what none answers gets
    /// the default procedure. A created window activates TSF, then gets its
    /// input with its touchpad viewport, then hears its session, then
    /// registers its drop target. The text services are the class's text,
    /// which the window hands out.
    fn window_class() -> WindowClass {
        let input = InputWindows::new();
        let text = Tsf::new();
        let hooks: WindowHooks = Rc::new([
            Rc::clone(&text) as Rc<dyn WindowMessages>,
            Rc::new(WindowProc),
            Rc::clone(&input) as Rc<dyn WindowMessages>,
            Rc::new(SessionWindows::default()),
            DropTargetRegistration::new(),
        ]);
        WindowClass { hooks, input, text }
    }

    fn preferences(owner: &OwnerSeeds<'_>) -> Result<Rc<dyn OwnerPreferences>, PlatformError> {
        Ok(Rc::new(PreferenceSource::try_new(owner.signal)?))
    }

    fn notices(app: &Arc<AppDelivery>) -> Arc<dyn NoticeChannel> {
        Arc::new(SystemNotices::new(Arc::clone(app)))
    }

    fn lifecycle(owner: &OwnerSeeds<'_>) -> Rc<dyn OwnerLifecycle> {
        Rc::new(HostEvents::new(
            Arc::clone(owner.app),
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
    /// Create a new Windows platform instance.
    ///
    /// # Errors
    /// [`PlatformError::UnsupportedOs`] on a build below FLUI's floor, which
    /// a Windows manifest does not keep from launching (ADR-0244 §3);
    /// [`PlatformError::Init`] when the owner thread's COM apartment, window
    /// class or owner window cannot be set up.
    pub fn try_new() -> Result<Self, PlatformError> {
        let build = WindowsBuild::read();
        crate::version::gate::refuse_below_floor(build)?;
        Self::at_build(build)
    }

    /// The platform as it runs on `build`, which its windows' version gates
    /// read: [`try_new`](Self::try_new) with the build the process runs on, and a
    /// test with the build it names.
    pub(crate) fn at_build(build: WindowsBuild) -> Result<Self, PlatformError> {
        Self::try_from_parts(
            |positions| WindowSet::new(WindowConfiguration::default(), positions, build),
            |windows| WindowsLoop::try_new::<Windows>(Arc::clone(windows) as _),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use windows::Win32::Foundation::WPARAM;
    use windows::Win32::UI::WindowsAndMessaging::{DM_POINTERHITTEST, WM_SYSCOMMAND, WM_TIMER};

    use super::Windows;
    use crate::owner::windows::{WindowMessages, WindowsHost};

    /// How many of `hooks` answer `msg` with `wparam`.
    fn answering(hooks: &[Rc<dyn WindowMessages>], msg: u32, wparam: usize) -> usize {
        hooks
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
        let hooks = <Windows as WindowsHost>::window_class().hooks;
        let answering = |msg, wparam| answering(&hooks, msg, wparam);
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
        // What the touchpad answers still reaches the input: the hit test,
        // and two timers in all, the window's modal deadline and the
        // touchpad's pump.
        assert_eq!(answering(DM_POINTERHITTEST, 0), 1);
        let timers = (0..=0xFFFF_usize)
            .filter(|&timer| answering(WM_TIMER, timer) == 1)
            .count();
        assert_eq!(timers, 2, "the modal deadline timer and the touchpad's");
    }
}
