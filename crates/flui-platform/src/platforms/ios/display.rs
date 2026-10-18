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

use std::collections::HashMap;
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
use flui_platform_api::{DisplayId, DisplayInfo, DisplayKey, DisplaySnapshot, RefreshPeriod};

use super::native_owner::NativeOwner;
use super::os_version;
use crate::error::PlatformError;
use crate::shared::apple_display_identity::{SameObject, ScreenLedger};
use crate::shared::apple_os_version::OsVersion;
use crate::shared::display_link_rate::{FrameRatePreference, frame_rate_preference};
use crate::shared::display_source::DisplaySource;
use crate::shared::owner_signal::OwnerSignal;

type RetainedScreen = Retained<UIScreen>;

impl SameObject for RetainedScreen {
    fn same_object(&self, other: &Self) -> bool {
        Retained::as_ptr(self) == Retained::as_ptr(other)
    }
}

/// The screen a window is on, as the display source names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct ScreenIdentity {
    /// The display's id; `None` when the source cannot issue one.
    pub(super) id: Option<DisplayId>,
    /// The number the source assigned to the retained screen, which keys the
    /// measured link period; `None` when the numbers are used up.
    pub(super) number: Option<u64>,
}

/// The displays of one iOS platform: their ids, the last snapshot, the link
/// periods windows measured, and the OS version.
pub(super) struct IosDisplays {
    source: DisplaySource,
    version: OsVersion,
    /// The retained screens and their numbers. Main thread only: a
    /// `UIScreen` is retained and compared there.
    screens: NativeOwner<std::cell::RefCell<ScreenLedger<RetainedScreen>>>,
    /// Set by anything that can change a display; cleared by the sample that
    /// answers it. Starts set, so the first read samples.
    stale: AtomicBool,
    sampled: Mutex<Option<DisplaySnapshot>>,
    /// The period each screen's display link last published, by screen number.
    link_periods: Mutex<HashMap<u64, RefreshPeriod>>,
}

impl IosDisplays {
    pub(super) fn new(marker: MainThreadMarker) -> Self {
        Self {
            source: DisplaySource::new(),
            version: os_version::read(),
            screens: NativeOwner::new(std::cell::RefCell::new(ScreenLedger::new()), marker),
            stale: AtomicBool::new(true),
            sampled: Mutex::new(None),
            link_periods: Mutex::new(HashMap::new()),
        }
    }

    /// Mark the snapshot stale; the next [`snapshot`](Self::snapshot) samples
    /// again. Any thread: this is one atomic store.
    pub(super) fn invalidate(&self) {
        self.stale.store(true, Ordering::Release);
    }

    /// How a display link on a screen that sustains `maximum_frames_per_second`
    /// is asked to run, on the version this platform runs on.
    pub(super) fn frame_rate_preference(
        &self,
        maximum_frames_per_second: i64,
    ) -> FrameRatePreference {
        frame_rate_preference(self.version, maximum_frames_per_second)
    }

    /// The displays now: the last sample while nothing has invalidated it,
    /// otherwise a fresh one.
    pub(super) fn snapshot(
        &self,
        marker: MainThreadMarker,
    ) -> Result<DisplaySnapshot, PlatformError> {
        let mut sampled = self.sampled.lock();
        if !self.stale.load(Ordering::Acquire)
            && let Some(snapshot) = sampled.as_ref()
        {
            return Ok(snapshot.clone());
        }
        // Cleared before reading, so an invalidation that lands while the
        // screens are read leaves the snapshot stale for the next call.
        self.stale.store(false, Ordering::Release);
        match self.sample(marker) {
            Ok(snapshot) => {
                *sampled = Some(snapshot.clone());
                Ok(snapshot)
            }
            Err(error) => {
                self.invalidate();
                Err(error)
            }
        }
    }

    /// The identity of `screen`; retains it. A screen the published snapshot
    /// does not hold yet marks the snapshot stale.
    pub(super) fn identify(&self, marker: MainThreadMarker, screen: &UIScreen) -> ScreenIdentity {
        let number = self
            .screens
            .get(marker)
            .borrow_mut()
            .number_of(&screen.retain());
        let id = number.and_then(|number| self.source.mint(&DisplayKey::IosScreen(number)));
        if let Some(id) = id
            && !self.stale.load(Ordering::Acquire)
            && self
                .sampled
                .lock()
                .as_ref()
                .is_none_or(|snapshot| snapshot.get(id).is_none())
        {
            self.invalidate();
        }
        ScreenIdentity { id, number }
    }

    /// Record the period the display link on screen `number` measured, and
    /// mark the snapshot stale.
    pub(super) fn report_link_period(&self, number: u64, period: RefreshPeriod) {
        self.link_periods.lock().insert(number, period);
        self.invalidate();
    }

    /// Forget the period of screen `number`: its link is gone.
    pub(super) fn forget_link_period(&self, number: u64) {
        if self.link_periods.lock().remove(&number).is_some() {
            self.invalidate();
        }
    }

    /// Read every screen UIKit lists and publish the result.
    fn sample(&self, marker: MainThreadMarker) -> Result<DisplaySnapshot, PlatformError> {
        let listed = UIScreen::screens(marker);
        let live: Vec<RetainedScreen> = listed.iter().collect();
        let mut ledger = self.screens.get(marker).borrow_mut();
        let mut displays = Vec::with_capacity(live.len());
        let mut live_numbers = Vec::with_capacity(live.len());
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
            display.refresh_period = self.link_periods.lock().get(&number).copied();
            displays.push(display);
        }
        ledger.release_unlisted(&live);
        drop(ledger);
        self.link_periods
            .lock()
            .retain(|number, _| live_numbers.contains(number));
        tracing::debug!("Sampled {} displays", displays.len());
        self.source.publish(displays)
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
pub(super) struct ScreenObserver {
    center: Retained<NSNotificationCenter>,
    tokens: Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>,
    active: Arc<AtomicBool>,
}

impl ScreenObserver {
    pub(super) fn new(displays: &Arc<IosDisplays>, signal: &Arc<OwnerSignal>) -> Self {
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
                crate::shared::panic_boundary::contain_owner_callback(|| {
                    if !active.load(Ordering::Acquire) {
                        return;
                    }
                    if let Some(displays) = displays.upgrade() {
                        displays.invalidate();
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
