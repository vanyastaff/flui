//! Host-owned UIKit observations; notifications only wake the existing owner.
//!
//! Text size reaches text as exact `UIFontMetrics` answers per growth profile
//! (ADR-0183), so the snapshot carries no scalar text scale; the content-size
//! notification still wakes the owner, whose sizing capture asks again.
//!
//! The UIKit loop keeps the source on the main thread and closes it as it
//! quits.

// The notification names are UIKit's and Foundation's statics, and observer
// registration and removal are Objective-C messages.
#![expect(unsafe_code)]

use std::cell::RefCell;
use std::ptr::NonNull;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use block2::RcBlock;
use flui_platform::{MotionPreference, SystemPreferences, TextWeightPreference};
use objc2::{MainThreadMarker, rc::Retained, runtime::ProtocolObject};
use objc2_foundation::{
    NSCurrentLocaleDidChangeNotification, NSLocale, NSNotification, NSNotificationCenter,
    NSObjectProtocol,
};
use objc2_ui_kit::{
    UIAccessibilityBoldTextStatusDidChangeNotification, UIAccessibilityDarkerSystemColorsEnabled,
    UIAccessibilityDarkerSystemColorsStatusDidChangeNotification, UIAccessibilityIsBoldTextEnabled,
    UIAccessibilityIsReduceMotionEnabled, UIAccessibilityReduceMotionStatusDidChangeNotification,
    UIContentSizeCategoryDidChangeNotification,
};

use crate::{
    PlatformError, owner::apple::ios::UIKitPreferences, owner::signal::OwnerSignal,
    owner::sources::SystemPreferenceSource, preferences::locales as preferred_locales,
};

type ObserverToken = Retained<ProtocolObject<dyn NSObjectProtocol>>;

impl SystemPreferenceSource for IOSPreferences {
    fn sample(&self) -> Result<SystemPreferences, PlatformError> {
        if MainThreadMarker::new().is_none() {
            return Err(PlatformError::Preferences {
                message: "UIKit preference sampling must run on the main thread".into(),
            });
        }
        if !self.active.load(Ordering::Acquire) {
            return Err(PlatformError::Preferences {
                message: "UIKit preference source is closed".into(),
            });
        }
        let enabled = UIAccessibilityIsBoldTextEnabled();
        let motion = if UIAccessibilityIsReduceMotionEnabled() {
            MotionPreference::Reduce
        } else {
            MotionPreference::NoPreference
        };
        let high_contrast = UIAccessibilityDarkerSystemColorsEnabled();
        let locales = preferred_locales::from_language_tags(
            NSLocale::preferredLanguages()
                .iter()
                .map(|language| language.to_string()),
        );
        if !self.active.load(Ordering::Acquire) {
            return Err(PlatformError::Preferences {
                message: "UIKit preference source closed during sampling".into(),
            });
        }
        let preferences = SystemPreferences::default()
            .with_text_weight(if enabled {
                TextWeightPreference::Bold
            } else {
                TextWeightPreference::NoPreference
            })
            .with_motion(motion)
            .with_high_contrast(high_contrast);
        Ok(match locales {
            Some(locales) => preferences.with_locales(locales),
            None => preferences,
        })
    }
}

impl UIKitPreferences for IOSPreferences {
    fn close(&self) {
        self.active.store(false, Ordering::Release);
        let observers = std::mem::take(&mut *self.observers.borrow_mut());
        for observer in observers {
            let observer_ref: &ProtocolObject<dyn NSObjectProtocol> = &observer;
            // SAFETY: this is the exact retained token returned by this center's
            // registration. Admission is closed and no borrow survives removal;
            // a callback already in flight owns only inert ref-counted state.
            unsafe {
                self.center.removeObserver(observer_ref.as_ref());
            }
        }
    }
}

/// The iOS host's preferences: the notifications that wake the owner when UIKit
/// reports a change, and the sample read on the main thread.
pub(crate) struct IOSPreferences {
    center: Retained<NSNotificationCenter>,
    observers: RefCell<Vec<ObserverToken>>,
    active: Arc<AtomicBool>,
}

impl std::fmt::Debug for IOSPreferences {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IOSPreferences").finish_non_exhaustive()
    }
}

impl IOSPreferences {
    pub(crate) fn new(signal: &Arc<OwnerSignal>, _marker: MainThreadMarker) -> Self {
        let center = NSNotificationCenter::defaultCenter();
        let active = Arc::new(AtomicBool::new(true));
        let callback_active = Arc::clone(&active);
        let signal = Arc::downgrade(signal);
        let callback = RcBlock::new(move |_: NonNull<NSNotification>| {
            // This notification may arrive on a foreign thread. Neither UIKit
            // reads nor UI callbacks enter it; OwnerSignal retains wake debt.
            // The temporary signal lease can be its final strong owner. Retire
            // any callback captures inside the panic boundary.
            crate::owner::panic_boundary::os_entry(
                "the preference observer",
                (),
                crate::owner::panic_boundary::AfterPanic::Answer(|()| ()),
                || {
                    if callback_active.load(Ordering::Acquire)
                        && let Some(signal) = signal.upgrade()
                    {
                        let _ = signal.wake();
                    }
                },
            );
        });
        // Own each token as soon as registration succeeds. If a later
        // registration unwinds, Drop closes admission and removes the earlier
        // token instead of leaving a partially installed observation set.
        let preferences = Self {
            center,
            observers: RefCell::new(Vec::with_capacity(5)),
            active,
        };
        // SAFETY: UIKit and Foundation own these immutable notification-name
        // constants.
        let names = unsafe {
            [
                UIAccessibilityBoldTextStatusDidChangeNotification,
                UIAccessibilityReduceMotionStatusDidChangeNotification,
                UIAccessibilityDarkerSystemColorsStatusDidChangeNotification,
                UIContentSizeCategoryDidChangeNotification,
                NSCurrentLocaleDidChangeNotification,
            ]
        };
        for name in names {
            // SAFETY: each documented notification uses no object filter or
            // queue. The block captures only Send+Sync atomics and a weak
            // signal and never dereferences its argument. This owner retains
            // every successful token and removes it after closing admission.
            let observer = unsafe {
                preferences
                    .center
                    .addObserverForName_object_queue_usingBlock(Some(name), None, None, &callback)
            };
            preferences.observers.borrow_mut().push(observer);
        }
        preferences
    }
}

impl Drop for IOSPreferences {
    fn drop(&mut self) {
        UIKitPreferences::close(self);
    }
}
