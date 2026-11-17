//! Public AppKit observations sampled on the application owner lane.
//!
//! Six notifications only wake the owner, which samples every value again:
//! the workspace's accessibility display options (reduce motion, increase
//! contrast), the current locale (preferred languages and regional formats),
//! the system colours and a switch between light and dark (the accent), the
//! preferred scroller style, and the application becoming active. AppKit posts nothing when the double-click
//! interval or "Click in the scroll bar to" changes. The user changes them in
//! System Settings, another application, so they are read again when this
//! one is active again; a change written while it stays active
//! (`defaults write`) waits for the next activation. A failed read schedules
//! one wake for its retry; nothing polls.
//!
//! The native AppKit loop keeps the observers and the source together
//! ([`AppKitPreferences`]).

// The notification-name constants are AppKit's and Foundation's statics.
#![expect(unsafe_code)]

use std::{
    sync::{Arc, Weak},
    time::Duration,
};

use flui_platform::{
    GesturePreferences, MotionPreference, ScrollBarPreferences, ScrollBarVisibility,
    ScrollTrackClick, SrgbColor, SystemPreferences,
};
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSApplication, NSApplicationDidBecomeActiveNotification, NSColor, NSColorSpace, NSEvent,
    NSPreferredScrollerStyleDidChangeNotification, NSScroller, NSScrollerStyle,
    NSSystemColorsDidChangeNotification, NSWorkspace,
    NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification,
};
use objc2_foundation::{
    NSCurrentLocaleDidChangeNotification, NSDistributedNotificationCenter, NSLocale,
    NSNotificationCenter, NSUserDefaults, ns_string,
};

use crate::owner::apple::macos::observation::Observation;
use crate::{
    PlatformError,
    owner::sources::SystemPreferenceSource,
    owner::{panic_boundary::contain_owner_callback, signal::OwnerSignal},
    preferences::{apple::regional, locales as preferred_locales, read::ReadAdmission},
};

/// What a loop keeps of AppKit's preferences: the observers, then the source,
/// so native observation ends before the source goes.
pub(crate) struct AppKitPreferences {
    _observers: PreferenceObservers,
    source: Arc<PreferenceSource>,
}

impl std::fmt::Debug for AppKitPreferences {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppKitPreferences").finish_non_exhaustive()
    }
}

impl AppKitPreferences {
    /// Observe AppKit's preferences, waking the owner behind `signal` when one
    /// changes, and read them once.
    pub(crate) fn new(signal: &Arc<OwnerSignal>) -> Self {
        let (source, observers) = PreferenceSource::new(signal);
        Self {
            _observers: observers,
            source,
        }
    }
}

impl SystemPreferenceSource for AppKitPreferences {
    fn sample(&self) -> Result<SystemPreferences, PlatformError> {
        self.source.sample()
    }
}

/// The macOS host's preferences: what AppKit reports, sampled on the owner lane.
struct PreferenceSource {
    signal: Weak<OwnerSignal>,
    admission: ReadAdmission,
}

impl std::fmt::Debug for PreferenceSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreferenceSource").finish_non_exhaustive()
    }
}

/// The notifications that invalidate a [`PreferenceSource`]. Owned on the main
/// thread beside the source and removed when dropped.
struct PreferenceObservers {
    _observations: [Observation; 6],
}

impl PreferenceSource {
    fn new(signal: &Arc<OwnerSignal>) -> (Arc<Self>, PreferenceObservers) {
        let source = Arc::new(Self {
            signal: Arc::downgrade(signal),
            admission: ReadAdmission::default(),
        });
        // Registration precedes the first getter: a change during the cold
        // read still wakes the owner, and a failed or contained read leaves
        // the observers in place for the next one.
        let observers = PreferenceObservers::new(signal);
        contain_owner_callback(|| {
            if let Err(error) = source.sample() {
                tracing::warn!(%error, "initial AppKit preference query failed");
            }
        });
        (source, observers)
    }
}

impl SystemPreferenceSource for PreferenceSource {
    fn sample(&self) -> Result<SystemPreferences, PlatformError> {
        if MainThreadMarker::new().is_none() {
            return Err(PlatformError::Preferences {
                message: "AppKit preferences require the application owner thread".into(),
            });
        }
        if !self
            .signal
            .upgrade()
            .is_some_and(|signal| signal.accepting())
        {
            return Err(PlatformError::Preferences {
                message: "the AppKit preference owner has stopped".into(),
            });
        }
        // A failed or unwound read owes a retry, and nothing else may wake the
        // owner for it: schedule that wake unless the read settled.
        let mut retry = RetryWake {
            source: self,
            owed: true,
        };
        let read = self
            .admission
            .read_observation(web_time::Instant::now(), sample);
        retry.owed = matches!(
            read,
            Err(ref error) if !matches!(error, PlatformError::PreferencesDeferred)
        );
        drop(retry);
        let (current, needs_delivery) = read?;
        if needs_delivery && let Some(signal) = self.signal.upgrade() {
            // Publication precedes admission. OwnerSignal retains delivery
            // debt when the hook has not been installed yet.
            let _ = signal.wake();
        }
        Ok(current)
    }
}

/// Wakes the owner when the read a failure postponed is due, unless the read
/// settled first; runs on unwind too.
struct RetryWake<'a> {
    source: &'a PreferenceSource,
    owed: bool,
}

impl Drop for RetryWake<'_> {
    fn drop(&mut self) {
        let Some(deadline) = self
            .owed
            .then(|| self.source.admission.retry_deadline())
            .flatten()
        else {
            return;
        };
        let signal = self.source.signal.clone();
        let delay = deadline.saturating_duration_since(web_time::Instant::now());
        crate::owner::apple::macos::owner_queue().exec_after(delay, move || {
            crate::owner::panic_boundary::os_entry(
                "the preference retry's wake",
                (),
                crate::owner::panic_boundary::AfterPanic::Answer(|()| ()),
                move || {
                    if let Some(signal) = signal.upgrade() {
                        let _ = signal.wake();
                    }
                },
            );
        });
    }
}

impl PreferenceObservers {
    fn new(signal: &Arc<OwnerSignal>) -> Self {
        let wake = || {
            let signal = Arc::downgrade(signal);
            move || {
                if let Some(signal) = signal.upgrade() {
                    let _ = signal.wake();
                }
            }
        };
        let workspace = NSWorkspace::sharedWorkspace().notificationCenter();
        let center = NSNotificationCenter::defaultCenter();
        // SAFETY: the names are AppKit's and Foundation's immutable
        // notification-name constants.
        let observations = unsafe {
            [
                Observation::new(
                    workspace,
                    NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification,
                    wake(),
                ),
                Observation::new(
                    center.clone(),
                    NSApplicationDidBecomeActiveNotification,
                    wake(),
                ),
                Observation::new(center.clone(), NSCurrentLocaleDidChangeNotification, wake()),
                Observation::new(center.clone(), NSSystemColorsDidChangeNotification, wake()),
                Observation::new(
                    center,
                    NSPreferredScrollerStyleDidChangeNotification,
                    wake(),
                ),
                // A switch between light and dark changes the accent the
                // appearance resolves; AppKit posts it to other applications
                // only through the distributed centre, and while this one is
                // inactive the activation notice reads it again.
                Observation::new(
                    NSDistributedNotificationCenter::defaultCenter().into_super(),
                    ns_string!("AppleInterfaceThemeChangedNotification"),
                    wake(),
                ),
            ]
        };
        Self {
            _observations: observations,
        }
    }
}

fn sample() -> Result<SystemPreferences, PlatformError> {
    let Some(main_thread) = MainThreadMarker::new() else {
        return Err(PlatformError::Preferences {
            message: "AppKit preferences require the application owner thread".into(),
        });
    };
    // Each value is read alone: an interval AppKit gives as no duration is
    // no observation, and the rest of the snapshot stands.
    let gestures = Duration::try_from_secs_f64(NSEvent::doubleClickInterval()).map_or_else(
        |_| GesturePreferences::default(),
        |interval| GesturePreferences::default().with_double_click_interval(interval),
    );
    let workspace = NSWorkspace::sharedWorkspace();
    let motion = if workspace.accessibilityDisplayShouldReduceMotion() {
        MotionPreference::Reduce
    } else {
        MotionPreference::NoPreference
    };
    let preferences = SystemPreferences::default()
        .with_gestures(gestures)
        .with_motion(motion)
        .with_high_contrast(workspace.accessibilityDisplayShouldIncreaseContrast())
        .with_scroll_bars(scroll_bars(main_thread))
        .with_accent_color(accent_color(main_thread));
    let languages = NSLocale::preferredLanguages();
    let locales = preferred_locales::from_language_tags(
        languages.iter().map(|language| language.to_string()),
    );
    let preferences = preferences.with_regional(regional::sample(
        locales.as_ref().and_then(|locales| locales.first()),
    ));
    Ok(match locales {
        Some(locales) => preferences.with_locales(locales),
        None => preferences,
    })
}

/// `controlAccentColor` as the application's appearance resolves it (dark
/// or light, increased contrast or not), in sRGB: a catalog colour, resolved
/// and converted before its components are read. The host publishes the
/// accent of the appearance it publishes beside it, and reads it again when
/// that appearance changes (ADR-0359 §1). A colour AppKit cannot convert, or
/// components an `SrgbColor` refuses, is no observation; the rest of the
/// snapshot stands.
fn accent_color(main_thread: MainThreadMarker) -> Option<SrgbColor> {
    let resolved = std::cell::Cell::new(None);
    // AppKit calls the block while the appearance is current, inside the
    // OS-entry boundary like every callback it makes.
    let read = block2::StackBlock::new(|| {
        crate::owner::panic_boundary::os_entry(
            "the accent read",
            (),
            crate::owner::panic_boundary::AfterPanic::Answer(|()| ()),
            || resolved.set(control_accent()),
        );
    });
    NSApplication::sharedApplication(main_thread)
        .effectiveAppearance()
        .performAsCurrentDrawingAppearance(&read);
    resolved.get()
}

/// `controlAccentColor` as the current drawing appearance resolves it, in
/// sRGB; `None` when AppKit cannot convert it or the result is refused.
fn control_accent() -> Option<SrgbColor> {
    let accent =
        NSColor::controlAccentColor().colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())?;
    SrgbColor::try_new(
        accent.redComponent(),
        accent.greenComponent(),
        accent.blueComponent(),
        accent.alphaComponent(),
    )
    .ok()
}

/// "Show scroll bars" as `preferredScrollerStyle` resolves it (an overlay
/// scroller while scrolling; a legacy scroller always, which "automatically"
/// also gives with a mouse), and "Click in the scroll bar to": AppKit's
/// `AppleScrollerPagingBehavior` default, absent until the user changes it
/// from "Jump to the next page".
fn scroll_bars(main_thread: MainThreadMarker) -> ScrollBarPreferences {
    let visibility = if NSScroller::preferredScrollerStyle(main_thread) == NSScrollerStyle::Overlay
    {
        ScrollBarVisibility::WhenScrolling
    } else {
        ScrollBarVisibility::Always
    };
    let track_click = if NSUserDefaults::standardUserDefaults()
        .boolForKey(ns_string!("AppleScrollerPagingBehavior"))
    {
        ScrollTrackClick::JumpToSpot
    } else {
        ScrollTrackClick::Page
    };
    ScrollBarPreferences::default()
        .with_visibility(visibility)
        .with_track_click(track_click)
}
