//! The iOS display source (`UIScreen`).
//!
//! Each screen UIKit lists becomes a [`DisplayInfo`]. `bounds` are in points
//! (logical, top-left origin) and `nativeScale` is the device pixel ratio, so
//! bounds are scaled by it to the device pixels the snapshot carries.
//!
//! A screen is keyed by a number this source assigns to the `UIScreen` it
//! retains; the screen is recognised again by object identity while it is held
//! and never by address (`ScreenLedger`).
//!
//! The refresh period of a screen is the cadence its window's `CADisplayLink`
//! measures. The window reports it through [`IosDisplays::report_link_period`]
//! when it leaves the publish band, and the snapshot carries it for that
//! screen; a screen with no running link reports no period.
//!
//! `display_snapshot` runs on every owner wake, so the last sample is kept and
//! read again only after an event marked it stale: a screen or scene
//! notification, a window changing screen, or a new link period.

// Some screen APIs remain deprecated in SDK 26.2. Scene ownership is
// implemented; this allowance does not exempt new code from scene lifetime
// rules.
#![expect(deprecated)]
// UIKit's notification center is reached through Objective-C calls that have
// no safe wrapper; each block states the invariant it relies on.
#![expect(unsafe_code)]

use std::ptr::NonNull;
use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, Ordering},
};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{MainThreadMarker, Message};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSObjectProtocol, NSRect};
use objc2_ui_kit::{
    UISceneDidDisconnectNotification, UISceneWillConnectNotification, UIScreen,
    UIScreenDidConnectNotification, UIScreenDidDisconnectNotification,
    UIScreenModeDidChangeNotification,
};
use parking_lot::Mutex;

use flui_foundation::geometry::{Bounds, Point, Size};
use flui_platform::{DisplayInfo, DisplayKey, DisplaySnapshot, RefreshPeriod};

use objc2_quartz_core::{CADisplayLink, CAFrameRateRange};

use super::link_rate::{
    FrameRatePreference, FrameRateRange, FrameRateRangeProperty, ScreenPeriods,
    frame_rate_preference,
};
use super::screen::{SameObject, ScreenLedger};
use crate::display::source::DisplaySource;
use crate::error::PlatformError;
use crate::owner::apple::ios::{NativeOwner, ScreenIdentity, ScreenNumber};
use crate::owner::signal::OwnerSignal;
use crate::owner::watch::Watch;
use crate::version::gate::Since;
use crate::version::gate::apple::AppleVersion;
use flui_platform::WindowId;

type RetainedScreen = Retained<UIScreen>;

impl SameObject for RetainedScreen {
    fn same_object(&self, other: &Self) -> bool {
        Retained::as_ptr(self) == Retained::as_ptr(other)
    }
}

/// The displays of one iOS platform: their ids and the last snapshot (kept by
/// the [`DisplaySource`] until an event invalidates it), the link periods
/// windows measured, and the OS version.
pub struct IosDisplays {
    source: DisplaySource,
    version: AppleVersion,
    /// The retained screens and their numbers. Main thread only: a
    /// `UIScreen` is retained and compared there.
    screens: NativeOwner<std::cell::RefCell<ScreenLedger<RetainedScreen>>>,
    /// The period each window's display link last published, by screen.
    link_periods: Mutex<ScreenPeriods<WindowId>>,
}

impl std::fmt::Debug for IosDisplays {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IosDisplays").finish_non_exhaustive()
    }
}

impl IosDisplays {
    pub(crate) fn new(marker: MainThreadMarker) -> Self {
        Self {
            source: DisplaySource::new(),
            version: AppleVersion::read(),
            screens: NativeOwner::new(std::cell::RefCell::new(ScreenLedger::new()), marker),
            link_periods: Mutex::new(ScreenPeriods::new()),
        }
    }

    /// Read every screen UIKit lists.
    fn read(&self, marker: MainThreadMarker) -> Vec<DisplayInfo> {
        let listed = UIScreen::screens(marker);
        let live: Vec<RetainedScreen> = listed.iter().collect();
        let mut displays = Vec::with_capacity(live.len());
        let mut live_numbers = Vec::with_capacity(live.len());
        let mut ledger = self.screens.get(marker).borrow_mut();
        // The first listed screen is the device's own display.
        for (index, screen) in live.iter().enumerate() {
            let Some(number) = ledger.number_of(screen) else {
                continue;
            };
            live_numbers.push(number);
            let Some(id) = self.source.mint(&DisplayKey::IosScreen(number)) else {
                continue;
            };
            let scale = screen.nativeScale();
            let bounds = device_bounds_from_points(screen.bounds(), scale);
            let name = if index == 0 {
                "iOS Display".to_owned()
            } else {
                format!("External Display {index}")
            };
            // iOS has no per-display "usable bounds" separate from the frame:
            // safe-area insets carve out the notch and home indicator for
            // CONTENT, not for the display itself.
            let mut display = DisplayInfo::new(id, name, bounds, scale).with_primary(index == 0);
            display.refresh_period = self.link_periods.lock().period_of(number);
            displays.push(display);
        }
        ledger.release_unlisted(&live);
        drop(ledger);
        self.link_periods.lock().retain_screens(&live_numbers);
        displays
    }
}

/// The `(points, scale)` → device-pixel conversion, free-standing so it can
/// be tested without a display. `CGRect`/`CGSize` are plain C structs.
fn device_bounds_from_points(bounds: NSRect, scale: f64) -> Bounds<i32> {
    let to_device = |points: f64| (points * scale).round() as i32;
    Bounds {
        origin: Point::new(to_device(bounds.origin.x), to_device(bounds.origin.y)),
        size: Size::new(to_device(bounds.size.width), to_device(bounds.size.height)),
    }
}

/// Watches the UIKit notifications that change which screens exist or how they
/// run: a screen connecting, disconnecting or changing mode, and a scene
/// connecting or disconnecting (a scene can be shown on another screen). The
/// handler only marks the snapshot stale and wakes the owner; the owner
/// samples on its own turn.
pub(crate) struct ScreenObserver {
    center: Retained<NSNotificationCenter>,
    tokens: Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>,
    active: Arc<AtomicBool>,
}

impl ScreenObserver {
    pub(crate) fn new(displays: &Arc<IosDisplays>, signal: &Arc<OwnerSignal>) -> Self {
        let active = Arc::new(AtomicBool::new(true));
        let callback = {
            let active = Arc::clone(&active);
            let displays: Weak<IosDisplays> = Arc::downgrade(displays);
            let signal = Arc::downgrade(signal);
            RcBlock::new(move |_: NonNull<NSNotification>| {
                // Delivered on the posting thread; the notification is never
                // read. Dropping the upgraded leases can release the last
                // owner, so the whole body runs inside the owner-callback
                // panic boundary.
                crate::owner::panic_boundary::contain_owner_callback(|| {
                    if !active.load(Ordering::Acquire) {
                        return;
                    }
                    if let Some(displays) = displays.upgrade() {
                        displays.source.invalidate();
                    }
                    if let Some(signal) = signal.upgrade() {
                        let _ = signal.wake();
                    }
                });
            })
        };
        let center = NSNotificationCenter::defaultCenter();
        let mut observer = Self {
            center,
            tokens: Vec::with_capacity(5),
            active,
        };
        // SAFETY: UIKit owns these immutable notification-name constants.
        let names = unsafe {
            [
                UIScreenDidConnectNotification,
                UIScreenDidDisconnectNotification,
                UIScreenModeDidChangeNotification,
                UISceneWillConnectNotification,
                UISceneDidDisconnectNotification,
            ]
        };
        for name in names {
            // SAFETY: no object filter and no queue, so the block runs on the
            // posting thread. It captures only atomics and weak handles to
            // thread-safe state, which makes it sendable; it ignores its
            // argument. Each token is kept at once, so `Drop` removes it even
            // if a later registration unwinds.
            let token = unsafe {
                observer.center.addObserverForName_object_queue_usingBlock(
                    Some(name),
                    None,
                    None,
                    &callback,
                )
            };
            observer.tokens.push(token);
        }
        observer
    }
}

impl Drop for ScreenObserver {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Release);
        for token in self.tokens.drain(..) {
            let token: &ProtocolObject<dyn NSObjectProtocol> = &token;
            // SAFETY: `token` is the exact observer this center returned at
            // registration; a callback already running holds only inert
            // shared state.
            unsafe {
                self.center.removeObserver(token.as_ref());
            }
        }
    }
}

impl crate::owner::apple::ios::HostDisplays for IosDisplays {
    /// The last sample while nothing has invalidated it, otherwise a fresh one.
    fn snapshot(&self, marker: MainThreadMarker) -> Result<DisplaySnapshot, PlatformError> {
        self.source.snapshot_with(|| Ok(self.read(marker)))
    }

    /// One atomic store, so any thread may call it.
    fn invalidate(&self) {
        self.source.invalidate();
    }

    /// A screen `UIScreen.screens` does not list is not numbered: it answers no
    /// identity and records a miss, so the snapshot is read again once per
    /// revision and a later call may find the screen listed. A screen the
    /// published snapshot does not hold yet records a miss too.
    fn identify(&self, marker: MainThreadMarker, screen: &UIScreen) -> ScreenIdentity {
        let screen = screen.retain();
        let held = self.screens.get(marker).borrow().number_if_held(&screen);
        let number = if held.is_some() {
            held
        } else {
            let listed = UIScreen::screens(marker);
            if !listed
                .iter()
                .any(|screen_in_list| screen_in_list.same_object(&screen))
            {
                // The address stands only for "this missed object" in the
                // once-per-revision ledger; it names no display.
                self.source
                    .note_miss(Retained::as_ptr(&screen).addr() as u64);
                return ScreenIdentity::default();
            }
            self.screens.get(marker).borrow_mut().number_of(&screen)
        };
        let id = number.and_then(|number| self.source.mint(&DisplayKey::IosScreen(number)));
        if let Some(id) = id
            && self.source.published().get(id).is_none()
        {
            self.source.note_miss(id.get());
        }
        ScreenIdentity {
            id,
            number: number.map(ScreenNumber),
        }
    }

    fn report_link_period(&self, number: ScreenNumber, window: WindowId, period: RefreshPeriod) {
        self.link_periods.lock().report(number.0, window, period);
        self.source.invalidate();
    }

    /// Other windows' periods on the screen stay.
    fn forget_link_period(&self, number: ScreenNumber, window: WindowId) {
        let forgot = self.link_periods.lock().forget(number.0, window);
        if forgot {
            self.source.invalidate();
        }
    }

    /// A frame-rate range on iOS 15 and later, `preferredFramesPerSecond`
    /// before it.
    fn request_frame_rate(&self, link: &CADisplayLink, screen: &UIScreen) {
        let maximum = i64::try_from(screen.maximumFramesPerSecond()).unwrap_or(0);
        match frame_rate_preference(self.version, maximum) {
            FrameRatePreference::Range(property, range) => {
                set_frame_rate_range(link, property, range);
            }
            FrameRatePreference::FramesPerSecond(frames) => {
                link.setPreferredFramesPerSecond(
                    isize::try_from(frames).expect("BUG: a u16 frame rate fits isize"),
                );
            }
        }
    }

    fn observe(self: Arc<Self>, signal: &Arc<OwnerSignal>) -> Watch {
        Watch::new(ScreenObserver::new(&self, signal))
    }
}

/// `CADisplayLink.preferredFrameRateRange`, which exists from iOS 15. The
/// proof can only come from a version check, so the call cannot be written for
/// an older system.
fn set_frame_rate_range(
    link: &CADisplayLink,
    _available: Since<FrameRateRangeProperty>,
    range: FrameRateRange,
) {
    link.setPreferredFrameRateRange(CAFrameRateRange {
        minimum: range.minimum,
        maximum: range.maximum,
        preferred: range.preferred,
    });
}
