//! Host-owned UIKit observations; notifications only wake the existing owner.

use std::cell::RefCell;
use std::ptr::NonNull;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use block2::RcBlock;
use flui_platform_api::{MotionPreference, SystemPreferences, TextWeightPreference};
use objc2::{MainThreadMarker, rc::Retained, runtime::ProtocolObject};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSObjectProtocol};
use objc2_ui_kit::{
    UIAccessibilityBoldTextStatusDidChangeNotification, UIAccessibilityIsBoldTextEnabled,
    UIAccessibilityIsReduceMotionEnabled, UIAccessibilityReduceMotionStatusDidChangeNotification,
};

use crate::{PlatformError, shared::owner_signal::OwnerSignal};

type ObserverToken = Retained<ProtocolObject<dyn NSObjectProtocol>>;

pub(super) struct Preferences {
    center: Retained<NSNotificationCenter>,
    observers: RefCell<Vec<ObserverToken>>,
    active: Arc<AtomicBool>,
}

impl Preferences {
    pub(super) fn new(signal: &Arc<OwnerSignal>, _marker: MainThreadMarker) -> Self {
        let center = NSNotificationCenter::defaultCenter();
        let active = Arc::new(AtomicBool::new(true));
        let callback_active = Arc::clone(&active);
        let signal = Arc::downgrade(signal);
        let callback = RcBlock::new(move |_: NonNull<NSNotification>| {
            // This notification may arrive on a foreign thread. Neither UIKit
            // reads nor UI callbacks enter it; OwnerSignal retains wake debt.
            // The temporary signal lease can be its final strong owner. Retire
            // any callback captures inside the existing owner panic boundary.
            crate::shared::panic_boundary::contain_owner_callback(|| {
                if callback_active.load(Ordering::Acquire)
                    && let Some(signal) = signal.upgrade()
                {
                    let _ = signal.wake();
                }
            });
        });
        // Own each token as soon as registration succeeds. If a later
        // registration unwinds, Drop closes admission and removes the earlier
        // token instead of leaving a partially installed observation set.
        let preferences = Self {
            center,
            observers: RefCell::new(Vec::with_capacity(2)),
            active,
        };
        // SAFETY: UIKit owns these immutable notification-name constants.
        let names = unsafe {
            [
                UIAccessibilityBoldTextStatusDidChangeNotification,
                UIAccessibilityReduceMotionStatusDidChangeNotification,
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

    pub(super) fn sample(
        &self,
        _marker: MainThreadMarker,
    ) -> Result<SystemPreferences, PlatformError> {
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
        if !self.active.load(Ordering::Acquire) {
            return Err(PlatformError::Preferences {
                message: "UIKit preference source closed during sampling".into(),
            });
        }
        Ok(SystemPreferences::default()
            .with_text_weight(if enabled {
                TextWeightPreference::Bold
            } else {
                TextWeightPreference::NoPreference
            })
            .with_motion(motion))
    }

    pub(super) fn close(&self) {
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

impl Drop for Preferences {
    fn drop(&mut self) {
        self.close();
    }
}
