//! The macOS display source (`NSScreen`).
//!
//! On `objc2` / `objc2-app-kit`, like the rest of the backend. Each screen
//! becomes a [`DisplayInfo`] whose refresh period is the rate of the display's
//! current mode (`CGDisplayCopyDisplayMode`), so a 120 Hz ProMotion panel
//! reports 120 Hz and a mode the display cannot describe reports nothing.
//!
//! A screen is keyed by the UUID of its display
//! (`CGDisplayCreateUUIDFromDisplayID`), which names the monitor and not the
//! `CGDirectDisplayID` of its current connection. A screen with no
//! `NSScreenNumber`, or no UUID, is left out of the snapshot.
//!
//! `display_snapshot` runs on every owner wake, so the last sample is kept and
//! read again only after an event marked it stale: the screen-parameters
//! notification, or a window learning its display or scale changed.

// CoreGraphics and AppKit are reached through calls that have no safe
// wrapper; each block states the invariant it relies on.
#![expect(unsafe_code)]

use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, Ordering},
};

use block2::RcBlock;
use core_foundation::{
    base::TCFType,
    uuid::{CFUUID, CFUUIDGetUUIDBytes, CFUUIDRef},
};
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{NSApplicationDidChangeScreenParametersNotification, NSScreen};
use objc2_foundation::{
    NSArray, NSDictionary, NSNotification, NSNotificationCenter, NSNumber, NSObjectProtocol,
    NSRect, NSString,
};

use flui_foundation::geometry::{Bounds, Point, Size};
use flui_platform::{DisplayId, DisplayInfo, DisplayKey, DisplaySnapshot, RefreshPeriod};

use super::period::core_graphics_refresh_period;
use super::{macos_display_key, macos_display_persistent_key};
use crate::display::source::DisplaySource;
use crate::error::PlatformError;
use crate::owner::signal::OwnerSignal;
use crate::shared::appkit_window_geometry::device_bounds_of_screen;

// `CGDisplayCreateUUIDFromDisplayID` is declared by CoreGraphics but exported
// by the ColorSync framework, which is why winit links the same one.
#[link(name = "ColorSync", kind = "framework")]
unsafe extern "C" {
    /// A retained `CFUUID` for the display, or null for an id that names none.
    fn CGDisplayCreateUUIDFromDisplayID(display: u32) -> CFUUIDRef;
}

/// The 16 UUID bytes of the display `display_id`; `None` for an id CoreGraphics
/// knows no display by.
fn display_uuid(display_id: u32) -> Option<[u8; 16]> {
    // SAFETY: the function takes a plain integer and returns an owned (+1)
    // `CFUUID` or null; any id value is accepted.
    let raw = unsafe { CGDisplayCreateUUIDFromDisplayID(display_id) };
    if raw.is_null() {
        return None;
    }
    // SAFETY: `raw` is non-null and came from a `Create` function, so this
    // reference is ours to release, which the wrapper does on drop.
    let uuid = unsafe { CFUUID::wrap_under_create_rule(raw) };
    // SAFETY: `uuid` keeps the `CFUUID` alive for the call.
    let b = unsafe { CFUUIDGetUUIDBytes(uuid.as_concrete_TypeRef()) };
    Some([
        b.byte0, b.byte1, b.byte2, b.byte3, b.byte4, b.byte5, b.byte6, b.byte7, b.byte8, b.byte9,
        b.byte10, b.byte11, b.byte12, b.byte13, b.byte14, b.byte15,
    ])
}

/// The `CGDirectDisplayID` a screen carries in its `deviceDescription`'s
/// `"NSScreenNumber"` entry; `None` when the screen reports none.
fn display_id_of(screen: &NSScreen) -> Option<u64> {
    // The `deviceDescription` value for "NSScreenNumber" is an `NSNumber`
    // holding the display id; `unsignedLongLongValue` reads it at full width.
    let key = NSString::from_str("NSScreenNumber");
    let description: Retained<NSDictionary<NSString, objc2::runtime::AnyObject>> =
        screen.deviceDescription();
    let value = description
        .objectForKey(&key)
        .and_then(|value| value.downcast::<NSNumber>().ok())?;
    Some(value.unsignedLongLongValue())
}

/// The key under which `screen` is issued a display id; `None` for a screen
/// that carries no display number or UUID, which is then left out rather than
/// keyed with a placeholder two such screens would share.
fn display_key_of(screen: &NSScreen) -> Option<DisplayKey> {
    macos_display_key(display_id_of(screen), display_uuid)
}

/// The refresh period of the display `screen` is on, or `None` when the
/// display cannot be reached or does not report a rate.
///
/// Reads the refresh rate of the display's *current* mode
/// (`CGDisplayModeGetRefreshRate` over `CGDisplayCopyDisplayMode`), the same
/// source the winit backend reads. A rate of 0 — CoreGraphics reports 0 for
/// modes it cannot describe, and some displays do — is unknown rather than an
/// infinite period.
///
/// Call on the owner lane: this messages the screen, which is AppKit traffic
/// this backend keeps there.
fn refresh_period_for_screen(screen: &NSScreen) -> Option<RefreshPeriod> {
    // `CGDirectDisplayID` is a `u32`; the `NSScreenNumber` number is a wider
    // CFNumber, so narrow it explicitly.
    let display_id = u32::try_from(display_id_of(screen)?).ok()?;
    let mode = core_graphics::display::CGDisplay::new(display_id).display_mode()?;
    core_graphics_refresh_period(mode.refresh_rate())
}

/// An AppKit rectangle as the geometry type the shared rules take.
pub(crate) fn bounds_of(rect: NSRect) -> Bounds<f64> {
    Bounds {
        origin: Point::new(rect.origin.x, rect.origin.y),
        size: Size::new(rect.size.width, rect.size.height),
    }
}

impl crate::shared::system_notice::StaleOnWake for MacosDisplays {
    fn mark_stale(&self) {
        self.invalidate();
    }
}

/// The displays of one macOS platform: their ids and the last snapshot, which
/// the [`DisplaySource`] keeps until an event invalidates it.
pub struct MacosDisplays {
    source: DisplaySource,
}

impl std::fmt::Debug for MacosDisplays {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MacosDisplays").finish_non_exhaustive()
    }
}

impl MacosDisplays {
    pub(crate) fn new() -> Self {
        Self {
            source: DisplaySource::new(),
        }
    }

    /// Mark the snapshot stale; the next [`snapshot`](Self::snapshot) samples
    /// again. Any thread: this is one atomic store.
    pub(crate) fn invalidate(&self) {
        self.source.invalidate();
    }

    /// The displays now: the last sample while nothing has invalidated it,
    /// otherwise a fresh one. Main thread.
    pub(crate) fn snapshot(&self, mtm: MainThreadMarker) -> Result<DisplaySnapshot, PlatformError> {
        self.source.snapshot_with(|| Ok(self.read(mtm)))
    }

    /// The id of the display `screen` is on; `None` for a screen the source
    /// leaves out. A display the published snapshot does not hold yet (a
    /// monitor plugged in since the last sample) marks the snapshot stale, once
    /// per revision.
    pub(crate) fn id_of(&self, screen: &NSScreen) -> Option<DisplayId> {
        let id = display_key_of(screen).and_then(|key| self.source.mint(&key))?;
        if self.source.published().get(id).is_none() {
            self.source.note_miss(id.get());
        }
        Some(id)
    }

    /// Read every screen (`+[NSScreen screens]`).
    fn read(&self, mtm: MainThreadMarker) -> Vec<DisplayInfo> {
        let screens: Retained<NSArray<NSScreen>> = NSScreen::screens(mtm);
        let mut displays = Vec::with_capacity(screens.count());
        // The first screen is always the primary (main) display, and its
        // height is where every other screen's top edge is measured from.
        let primary_height = screens
            .iter()
            .next()
            .map_or(0.0, |screen| screen.frame().size.height);
        for (index, screen) in screens.iter().enumerate() {
            let Some(id) = display_key_of(&screen).and_then(|key| self.source.mint(&key)) else {
                continue;
            };
            let scale = screen.backingScaleFactor();
            // NSScreen frames are in points with a bottom-left origin on the
            // primary screen; the snapshot carries device pixels of this
            // screen from the primary screen's top-left corner, like every
            // other platform's, so a window's top-left position and a
            // display's origin are measured the same way.
            let bounds = device_bounds_of_screen(bounds_of(screen.frame()), primary_height, scale);
            let usable_bounds =
                device_bounds_of_screen(bounds_of(screen.visibleFrame()), primary_height, scale);
            let mut display = DisplayInfo::new(
                id,
                format!("Display {}", display_id_of(&screen).unwrap_or_default()),
                bounds,
                scale,
            )
            .with_usable_bounds(usable_bounds)
            .with_primary(index == 0);
            if let Some(uuid) = display_id_of(&screen)
                .and_then(|number| u32::try_from(number).ok())
                .and_then(display_uuid)
            {
                display = display.with_persistent_key(macos_display_persistent_key(uuid));
            }
            display.refresh_period = refresh_period_for_screen(&screen);
            displays.push(display);
        }
        displays
    }
}

/// Watches `NSApplicationDidChangeScreenParametersNotification`, which AppKit
/// posts when a display is attached or removed, or changes resolution, refresh
/// rate, arrangement or scale. The handler only marks the snapshot stale and
/// wakes the owner; the owner samples on its own turn.
pub(crate) struct ScreenObserver {
    center: Retained<NSNotificationCenter>,
    token: Retained<ProtocolObject<dyn NSObjectProtocol>>,
    active: Arc<AtomicBool>,
}

impl ScreenObserver {
    pub(crate) fn new(displays: &Arc<MacosDisplays>, signal: &Arc<OwnerSignal>) -> Self {
        let active = Arc::new(AtomicBool::new(true));
        let callback = {
            let active = Arc::clone(&active);
            let displays: Weak<MacosDisplays> = Arc::downgrade(displays);
            let signal = Arc::downgrade(signal);
            RcBlock::new(move |_: std::ptr::NonNull<NSNotification>| {
                // The block is delivered on the thread that posts the
                // notification and never reads the notification. Dropping the
                // upgraded leases can release the last owner, so the whole
                // body runs inside the owner-callback panic boundary.
                crate::owner::panic_boundary::contain_owner_callback(|| {
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
        // SAFETY: the name is AppKit's immutable notification-name constant.
        let name = unsafe { NSApplicationDidChangeScreenParametersNotification };
        // SAFETY: no object filter and no queue, so the block runs on the
        // posting thread. It captures only atomics and weak handles to
        // thread-safe state, which makes it sendable; it ignores its argument.
        let token = unsafe {
            center.addObserverForName_object_queue_usingBlock(Some(name), None, None, &callback)
        };
        Self {
            center,
            token,
            active,
        }
    }
}

impl Drop for ScreenObserver {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Release);
        let token: &ProtocolObject<dyn NSObjectProtocol> = &self.token;
        // SAFETY: `token` is the exact observer this center returned at
        // registration; a callback already running holds only inert shared
        // state.
        unsafe {
            self.center.removeObserver(token.as_ref());
        }
    }
}
