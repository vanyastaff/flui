//! The macOS display source (`NSScreen`).
//!
//! On `objc2` / `objc2-app-kit`, like the rest of the backend. Each screen
//! becomes a [`DisplayDescription`] whose refresh period is the one its
//! display paces at (`screen_cadence`): the rate of the display's current
//! mode (`CGDisplayCopyDisplayMode`), so a 120 Hz ProMotion panel reports
//! 120 Hz; a display link's nominal period where the mode reports 0 Hz; and
//! no shorter than 1/60 s for the built-in panel under Low Power Mode.
//!
//! A screen is keyed by the UUID of its display
//! (`CGDisplayCreateUUIDFromDisplayID`), which names the monitor and not the
//! `CGDirectDisplayID` of its current connection. A screen with no
//! `NSScreenNumber`, or no UUID, is left out of the snapshot.
//!
//! A screen's colour is the gamut it can show (`canRepresentDisplayGamut:`)
//! and, from macOS 10.15, the extended-dynamic-range headroom it can reach
//! (`maximumPotentialExtendedDynamicRangeColorComponentValue`), which a
//! `CAMetalLayer` with extended range is drawn up to.
//!
//! `display_snapshot` runs on every owner wake, so the last sample is kept and
//! read again only after an event marked it stale: the screen-parameters
//! notification, the power-state notification, or a window learning its
//! display or scale changed.

// CoreGraphics, CoreVideo and AppKit are reached through calls that have no
// safe wrapper; each block states the invariant it relies on.
#![expect(
    unsafe_code,
    reason = "CoreGraphics, CoreVideo and AppKit are reached through calls that have no safe wrapper"
)]

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::Arc;

use core_foundation::{
    base::TCFType,
    uuid::{CFUUID, CFUUIDGetUUIDBytes, CFUUIDRef},
};
use core_graphics::display::CGDisplay;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSApplicationDidChangeScreenParametersNotification, NSDisplayGamut, NSScreen};
use objc2_foundation::{
    NSArray, NSDictionary, NSNotificationCenter, NSNotificationName, NSNumber, NSProcessInfo,
    NSString,
};

use flui_platform::{DisplayDescription, DisplayId, DisplayKey, DisplaySnapshot};

#[cfg(not(target_arch = "aarch64"))]
use super::screen::MacosPotentialHeadroom;
use super::screen::screen_bounds;
use super::screen::{macos_display_key, macos_display_persistent_key, screen_color};
use super::screen_cadence::{self, CvTime, Panel, PowerMode, ProcessPowerState};
use crate::display::scale::ScaleStreaks;
use crate::display::source::DisplaySource;
use crate::error::PlatformError;
use crate::owner::apple::macos::bounds_of;
use crate::owner::apple::macos::observation::Observation;
use crate::owner::signal::OwnerSignal;
use crate::owner::watch::Watch;
use crate::version::gate::{Since, apple::MacOSVersion};

// `CGDisplayCreateUUIDFromDisplayID` is declared by CoreGraphics but exported
// by the ColorSync framework, which is why winit links the same one.
#[link(name = "ColorSync", kind = "framework")]
unsafe extern "C" {
    /// A retained `CFUUID` for the display, or null for an id that names none.
    fn CGDisplayCreateUUIDFromDisplayID(display: u32) -> CFUUIDRef;
}

// A display link is made only to be asked its nominal period and is released
// unstarted, so it runs no thread and calls nothing back. macOS 15 deprecates
// the type for `NSView.displayLink`, which is macOS 14's and reports a period
// only while it runs.
#[link(name = "CoreVideo", kind = "framework")]
unsafe extern "C" {
    /// A retained display link for `display` in `*link` and
    /// `kCVReturnSuccess` (0), or another status.
    fn CVDisplayLinkCreateWithCGDisplay(display: u32, link: *mut *mut c_void) -> i32;
    /// The refresh period the link's display is nominally driven at, which
    /// may be indefinite.
    fn CVDisplayLinkGetNominalOutputVideoRefreshPeriod(link: *mut c_void) -> CvTime;
    /// Give up one reference to the link.
    fn CVDisplayLinkRelease(link: *mut c_void);
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

/// The number a screen carries in its `deviceDescription`'s
/// `"NSScreenNumber"` entry, a `CGDirectDisplayID` at the width AppKit stores
/// it; `None` when the screen reports none.
fn screen_number_of(screen: &NSScreen) -> Option<u64> {
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
    macos_display_key(screen_number_of(screen), display_uuid)
}

/// The `CGDirectDisplayID` of the display `screen` is on; `None` for a screen
/// that carries no number, or one no display id holds.
fn display_id_of(screen: &NSScreen) -> Option<u32> {
    u32::try_from(screen_number_of(screen)?).ok()
}

/// The refresh rate, in hertz, of the *current* mode of the display
/// `display_id` (`CGDisplayModeGetRefreshRate` over
/// `CGDisplayCopyDisplayMode`); `None` for a display with no mode to read.
/// CoreGraphics reports 0 for a mode it cannot describe.
fn mode_hertz(display_id: u32) -> Option<f64> {
    Some(CGDisplay::new(display_id).display_mode()?.refresh_rate())
}

/// The refresh period a display link on the display `display_id` is nominally
/// driven at; `None` when CoreVideo makes no link for it (a display that is
/// asleep or gone).
fn link_nominal_period(display_id: u32) -> Option<CvTime> {
    let mut link: *mut c_void = std::ptr::null_mut();
    // SAFETY: `link` is a valid place for the one pointer the function
    // writes; any display id is accepted.
    let status = unsafe { CVDisplayLinkCreateWithCGDisplay(display_id, &raw mut link) };
    let link = NonNull::new(link)?;
    let nominal = if status == 0 {
        // SAFETY: `link` is the live display link the create call returned.
        Some(unsafe { CVDisplayLinkGetNominalOutputVideoRefreshPeriod(link.as_ptr()) })
    } else {
        None
    };
    // SAFETY: the create call handed this function the link's one reference,
    // which nothing else holds: the link was never started.
    unsafe { CVDisplayLinkRelease(link.as_ptr()) };
    nominal
}

/// Whether the display `display_id` is the machine's own panel.
fn panel_of(display_id: u32) -> Panel {
    if CGDisplay::new(display_id).is_builtin() {
        Panel::BuiltIn
    } else {
        Panel::External
    }
}

/// Whether the system is in Low Power Mode, which the proof says it can
/// report. Any thread.
fn power_mode(_: Since<ProcessPowerState>) -> PowerMode {
    if NSProcessInfo::processInfo().isLowPowerModeEnabled() {
        PowerMode::LowPower
    } else {
        PowerMode::Normal
    }
}

/// Why a change of Low Power Mode cannot be observed on a system that has the
/// mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
enum PowerStateNameMissing {
    /// No loaded image exports the constant.
    #[error("no loaded image exports `NSProcessInfoPowerStateDidChangeNotification`")]
    NotExported,
    /// The constant is there and holds no name.
    #[error("`NSProcessInfoPowerStateDidChangeNotification` holds no name")]
    Null,
}

/// The name Foundation posts a change of Low Power Mode under
/// (`NSProcessInfoPowerStateDidChangeNotification`).
///
/// Looked up by name and not linked: the constant is macOS 12's, and a binary
/// that linked it would not load on the releases below it this crate runs on.
///
/// # Errors
/// [`PowerStateNameMissing`] when the running Foundation exports no such
/// constant, or an empty one, although the proof says it should.
fn power_state_notification(
    _: Since<ProcessPowerState>,
) -> Result<&'static NSNotificationName, PowerStateNameMissing> {
    // SAFETY: `RTLD_DEFAULT` searches the images the process loaded, which
    // include Foundation; the name is NUL-terminated.
    let address = unsafe {
        libc::dlsym(
            libc::RTLD_DEFAULT,
            c"NSProcessInfoPowerStateDidChangeNotification".as_ptr(),
        )
    };
    let constant = NonNull::new(address.cast::<Option<&'static NSNotificationName>>())
        .ok_or(PowerStateNameMissing::NotExported)?;
    // SAFETY: Foundation exports the symbol as an initialised, immutable
    // `NSNotificationName` variable: a pointer, null or to a string that
    // lives for the process, which is what the pointee type holds.
    unsafe { constant.read() }.ok_or(PowerStateNameMissing::Null)
}

impl crate::owner::notices::StaleOnWake for MacosDisplays {
    fn mark_stale(&self) {
        self.source.invalidate();
    }
}

/// The displays of one macOS platform: their ids and the last snapshot, which
/// the [`DisplaySource`] keeps until an event invalidates it, and the OS
/// version, read once (ADR-0244).
pub struct MacosDisplays {
    source: DisplaySource,
    version: MacOSVersion,
    /// Whether each screen's last scale read was refused, so a run of refusals
    /// is reported once.
    scale_streak: crate::owner::apple::NativeOwner<std::cell::RefCell<ScaleStreaks<DisplayId>>>,
}

impl std::fmt::Debug for MacosDisplays {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MacosDisplays").finish_non_exhaustive()
    }
}

impl MacosDisplays {
    pub(crate) fn new(main: MainThreadMarker) -> Self {
        Self {
            source: DisplaySource::new(),
            version: MacOSVersion::read(),
            scale_streak: crate::owner::apple::NativeOwner::new(
                std::cell::RefCell::default(),
                main,
            ),
        }
    }

    /// Read every screen (`+[NSScreen screens]`).
    fn read(&self, mtm: MainThreadMarker) -> Vec<DisplayDescription> {
        let screens: Retained<NSArray<NSScreen>> = NSScreen::screens(mtm);
        let mut displays = Vec::with_capacity(screens.count());
        // Before macOS 12 the system has no Low Power Mode to be in.
        let power = self
            .version
            .since::<ProcessPowerState>()
            .map_or(PowerMode::Normal, power_mode);
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
            // The screen's scale by the hosts' one rule; a refusal is
            // reported with nothing borrowed.
            let backing_scale = screen.backingScaleFactor();
            let (ratio, refused) = self.scale_streak.get(mtm).borrow_mut().read(
                id,
                backing_scale,
                "NSScreen backingScaleFactor",
            );
            if let Some(refused) = refused {
                refused.report();
            }
            // NSScreen frames are in points with a bottom-left origin on the
            // primary screen; the snapshot carries the screen's place in that
            // desktop from the primary's top-left corner, and its bounds in
            // its own points, which are its logical pixels.
            let Some((origin, bounds)) = screen_bounds(
                bounds_of(screen.frame()),
                bounds_of(screen.visibleFrame()),
                primary_height,
            ) else {
                continue;
            };
            // The name System Settings shows ("Built-in Retina Display", a
            // monitor's model), macOS 10.15 and later.
            let mut display = DisplayDescription::new(
                id,
                screen.localizedName().to_string(),
                origin,
                bounds,
                ratio,
            )
            .with_primary(index == 0);
            let display_id = display_id_of(&screen);
            if let Some(uuid) = display_id.and_then(display_uuid) {
                display = display.with_persistent_key(macos_display_persistent_key(uuid));
            }
            // A screen that names no display has no mode and no link to ask.
            display.refresh_period = screen_cadence::refresh_period(
                display_id.and_then(mode_hertz),
                || display_id.and_then(link_nominal_period),
                display_id.map_or(Panel::External, panel_of),
                power,
            );
            // Every arm64 Mac runs macOS 11 or later, which has the report;
            // x86_64 reaches back to 10.13 and asks the gate.
            #[cfg(target_arch = "aarch64")]
            let potential = Some(screen.maximumPotentialExtendedDynamicRangeColorComponentValue());
            #[cfg(not(target_arch = "aarch64"))]
            let potential = self
                .version
                .since::<MacosPotentialHeadroom>()
                .map(|_| screen.maximumPotentialExtendedDynamicRangeColorComponentValue());
            display = display.with_color(Ok(screen_color(
                screen.canRepresentDisplayGamut(NSDisplayGamut::P3),
                potential,
            )));
            displays.push(display);
        }
        // A screen this sample did not read takes its streak with it.
        let read: Vec<DisplayId> = displays.iter().map(|display| display.id).collect();
        self.scale_streak.get(mtm).borrow_mut().keep_only(&read);
        displays
    }
}

/// The notifications after which the screens are read again, observed until
/// this is dropped: `NSApplicationDidChangeScreenParametersNotification`,
/// which AppKit posts when a display is attached or removed, or changes
/// resolution, refresh rate, arrangement or scale; and, from macOS 12,
/// Foundation's power-state notification, since Low Power Mode changes the
/// period of the built-in panel and none of its screen parameters.
///
/// Each handler only marks the snapshot stale and wakes the owner, on
/// whichever thread posts the notification (Foundation posts the power state
/// off the main thread); the owner samples on its own turn. A handler that
/// drops the last lease on the displays or the signal does so inside the
/// observation's panic boundary.
struct DisplayObservers {
    _screen_parameters: Observation,
    /// `None` before macOS 12, which posts no such notification, and where
    /// its name cannot be found, which is said in a warning: Low Power Mode
    /// is then read only when something else has the screens read again.
    _power_state: Option<Observation>,
}

impl DisplayObservers {
    fn new(displays: &Arc<MacosDisplays>, signal: &Arc<OwnerSignal>) -> Self {
        let mark_stale_and_wake = || {
            let displays = Arc::downgrade(displays);
            let signal = Arc::downgrade(signal);
            move || {
                if let Some(displays) = displays.upgrade() {
                    displays.source.invalidate();
                }
                if let Some(signal) = signal.upgrade() {
                    let _ = signal.wake();
                }
            }
        };
        let center = NSNotificationCenter::defaultCenter();
        // SAFETY: the name is AppKit's immutable notification-name constant.
        let screen_parameters = unsafe { NSApplicationDidChangeScreenParametersNotification };
        let power_state = displays
            .version
            .since::<ProcessPowerState>()
            .map(power_state_notification);
        let power_state = match power_state {
            Some(Ok(name)) => Some(Observation::new(
                center.clone(),
                name,
                mark_stale_and_wake(),
            )),
            Some(Err(error)) => {
                // Once per loop, with nothing borrowed: the observers are
                // made as the loop is.
                tracing::warn!(
                    %error,
                    "a change of Low Power Mode is not observed; the built-in display's \
                     refresh period follows it only when the displays are read again"
                );
                None
            }
            None => None,
        };
        Self {
            _power_state: power_state,
            _screen_parameters: Observation::new(center, screen_parameters, mark_stale_and_wake()),
        }
    }
}

impl crate::owner::apple::macos::HostDisplays for MacosDisplays {
    fn version(&self) -> MacOSVersion {
        self.version
    }

    /// The last sample while nothing has invalidated it, otherwise a fresh
    /// one.
    fn snapshot(&self, mtm: MainThreadMarker) -> Result<DisplaySnapshot, PlatformError> {
        self.source.snapshot_with(|| Ok(self.read(mtm)))
    }

    /// One atomic store, so any thread may call it.
    fn invalidate(&self) {
        self.source.invalidate();
    }

    fn published(&self) -> DisplaySnapshot {
        self.source.published()
    }

    /// A display the published snapshot does not hold yet (a monitor plugged
    /// in since the last sample) marks the snapshot stale, once per revision.
    fn id_of(&self, screen: &NSScreen) -> Option<DisplayId> {
        let id = display_key_of(screen).and_then(|key| self.source.mint(&key))?;
        if self.source.published().get(id).is_none() {
            self.source.note_miss(id.get());
        }
        Some(id)
    }

    fn observe(self: Arc<Self>, signal: &Arc<OwnerSignal>) -> Watch {
        Watch::new(DisplayObservers::new(&self, signal))
    }
}
