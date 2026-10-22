//! The displays' part of the system's broadcasts, which the owner's broadcast
//! receiver tells its subscribers.

use std::cell::Cell;
use std::sync::{Arc, Weak};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    SPI_SETWORKAREA, WM_DISPLAYCHANGE, WM_SETTINGCHANGE,
};

use crate::owner::signal::OwnerSignal;
use crate::owner::windows::HostDisplays;
use crate::owner::windows::broadcast::Subscriber;

/// The displays' part of the system's broadcasts: `WM_DISPLAYCHANGE` (a
/// monitor was attached, removed or switched mode) and a work-area change
/// (`WM_SETTINGCHANGE` with `SPI_SETWORKAREA`: the taskbar moved or was
/// resized, which a display's usable bounds carry). Each marks the monitors
/// stale and wakes the owner, which samples them on its next turn; nothing is
/// sampled inside the message, which can arrive inside a COM call of ours.
///
/// It only observes: Windows reads no result from either notification, so
/// the subscriber answers neither and every subscriber after it hears the
/// same message, whatever order the host lists them in.
///
/// It admits wakes of its own accord until the receiver detaches it, so a
/// message the receiver still hears while it is destroyed, or in a window the
/// operating system kept after a failed destruction, wakes nobody. The mark
/// stays: it is a flag the next sample clears.
pub(crate) struct DisplayBroadcasts {
    displays: Arc<dyn HostDisplays>,
    signal: Weak<OwnerSignal>,
    /// Cleared when the receiver detaches this subscriber. The receiver runs
    /// on the owner thread only.
    attached: Cell<bool>,
}

impl DisplayBroadcasts {
    /// The subscriber that marks `displays` stale and wakes the owner behind
    /// `signal`.
    pub(crate) fn new(displays: &Arc<dyn HostDisplays>, signal: &Arc<OwnerSignal>) -> Self {
        Self {
            displays: Arc::clone(displays),
            signal: Arc::downgrade(signal),
            attached: Cell::new(true),
        }
    }

    /// The monitors may have changed: mark them stale and wake the owner. A
    /// wake that cannot be posted leaves the signal's debt for the next turn,
    /// and the mark for the next sample.
    fn notify(&self) {
        self.displays.invalidate();
        if self.attached.get()
            && let Some(signal) = self.signal.upgrade()
            && signal.accepting()
        {
            let _ = signal.wake();
        }
    }
}

impl Subscriber for DisplayBroadcasts {
    fn detach(&self, _receiver: HWND) {
        self.attached.set(false);
    }

    fn message(
        &self,
        _hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        _lparam: LPARAM,
    ) -> Option<LRESULT> {
        let work_area = message == WM_SETTINGCHANGE && wparam.0 == SPI_SETWORKAREA.0 as usize;
        if message == WM_DISPLAYCHANGE || work_area {
            self.notify();
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use windows::Win32::UI::WindowsAndMessaging::{
        SPI_SETWORKAREA, WM_DISPLAYCHANGE, WM_SETTINGCHANGE,
    };

    use super::DisplayBroadcasts;
    use crate::display::windows::WindowsDisplays;
    use crate::owner::signal::OwnerSignal;
    use crate::owner::windows::HostDisplays;
    use crate::owner::windows::broadcast::{Receiver, Subscriber};

    /// An owner signal that counts its wakes.
    fn counting_signal() -> (Arc<OwnerSignal>, Arc<AtomicUsize>) {
        let wakes = Arc::new(AtomicUsize::new(0));
        let signal = OwnerSignal::new({
            let wakes = Arc::clone(&wakes);
            Arc::new(move || {
                wakes.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
        });
        signal.start().expect("the owner signal starts");
        (signal, wakes)
    }

    /// Whether `displays` samples again on its next snapshot, which costs it
    /// that sample.
    fn samples_again(displays: &Arc<dyn HostDisplays>) -> bool {
        let before = displays.sample_count();
        displays.snapshot().expect("a snapshot");
        displays.sample_count() > before
    }

    /// Whom the subscriber wakes: a display change and a work-area change mark
    /// the monitors stale and wake the owner, any other setting is not the
    /// displays'; once detached, it still marks the monitors stale but wakes
    /// nobody. It answers none of them. The owner's turn runs after each wake,
    /// so the next one is posted again.
    #[test]
    fn display_broadcasts_mark_the_monitors_stale_and_wake_until_detached() {
        let (signal, wakes) = counting_signal();
        let displays: Arc<dyn HostDisplays> = Arc::new(WindowsDisplays::new());
        let subscriber = Rc::new(DisplayBroadcasts::new(&displays, &signal));
        let receiver =
            Receiver::new([Rc::clone(&subscriber) as Rc<dyn Subscriber>]).expect("a receiver");
        displays.snapshot().expect("the first sample");

        receiver.send_with_for_test(WM_DISPLAYCHANGE, 32, 0);
        assert!(samples_again(&displays), "a display change marked nothing");
        assert_eq!(wakes.load(Ordering::SeqCst), 1);
        signal.drive();

        receiver.send_with_for_test(WM_SETTINGCHANGE, SPI_SETWORKAREA.0 as usize, 0);
        assert!(
            samples_again(&displays),
            "a work-area change marked nothing"
        );
        assert_eq!(wakes.load(Ordering::SeqCst), 2);
        signal.drive();

        receiver.send_with_for_test(WM_SETTINGCHANGE, 0, 0);
        assert!(
            !samples_again(&displays),
            "another setting is not the displays'"
        );
        assert_eq!(wakes.load(Ordering::SeqCst), 2);

        let receiver_window = receiver.hwnd_for_test();
        drop(receiver);
        let answer = subscriber.message(
            receiver_window,
            WM_DISPLAYCHANGE,
            windows::Win32::Foundation::WPARAM(32),
            windows::Win32::Foundation::LPARAM(0),
        );
        assert_eq!(answer, None, "the subscriber answered a display change");
        assert!(
            samples_again(&displays),
            "a detached subscriber still marks the monitors"
        );
        assert_eq!(
            wakes.load(Ordering::SeqCst),
            2,
            "a detached subscriber woke the owner"
        );
    }
}
