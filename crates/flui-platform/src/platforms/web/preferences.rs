//! One browser motion observation, owned by the host rather than its canvas.

use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, Ordering},
};

use wasm_bindgen::{JsCast, closure::Closure};
use web_sys::{Event, MediaQueryList};

use crate::{PlatformError, shared::owner_signal::OwnerSignal};

pub(super) struct Preferences {
    pub(super) query: MediaQueryList,
    pub(super) no_preference: MediaQueryList,
    pub(super) active: Arc<AtomicBool>,
    callback: Option<Closure<dyn FnMut(Event)>>,
}

impl Preferences {
    pub(super) fn new(signal: &Arc<OwnerSignal>) -> Result<Option<Self>, PlatformError> {
        let window = web_sys::window().ok_or_else(|| PlatformError::Preferences {
            message: "browser preference observation requires a Window".into(),
        })?;
        let Some(query) = window
            .match_media("(prefers-reduced-motion: reduce)")
            .map_err(|error| PlatformError::Preferences {
                message: format!("motion query failed: {error:?}"),
            })?
        else {
            return Ok(None);
        };
        let Some(no_preference) = window
            .match_media("(prefers-reduced-motion: no-preference)")
            .map_err(|error| PlatformError::Preferences {
                message: format!("motion support query failed: {error:?}"),
            })?
        else {
            return Ok(None);
        };
        let active = Arc::new(AtomicBool::new(true));
        let callback_active = Arc::clone(&active);
        let signal: Weak<OwnerSignal> = Arc::downgrade(signal);
        let callback = Closure::new(move |_: Event| {
            if callback_active.load(Ordering::Acquire)
                && let Some(signal) = signal.upgrade()
            {
                let _ = signal.wake();
            }
        });
        query
            .add_event_listener_with_callback("change", callback.as_ref().unchecked_ref())
            .map_err(|error| PlatformError::Preferences {
                message: format!("motion observer registration failed: {error:?}"),
            })?;
        Ok(Some(Self {
            query,
            no_preference,
            active,
            callback: Some(callback),
        }))
    }
}

impl Drop for Preferences {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Release);
        let Some(callback) = self.callback.take() else {
            return;
        };
        if let Err(error) = self
            .query
            .remove_event_listener_with_callback("change", callback.as_ref().unchecked_ref())
        {
            // Keep the JS trampoline valid if native removal failed. Its
            // capture is inert and cannot deliver to a replacement host.
            callback.forget();
            crate::shared::panic_boundary::contain_owner_callback(|| {
                tracing::warn!(?error, "browser motion observer removal failed");
            });
        }
    }
}
