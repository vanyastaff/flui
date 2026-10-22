//! Browser accessibility and language observations, owned by the host rather
//! than its canvas.
//!
//! Media queries report motion and contrast, and `navigator.languages` the
//! preferred locales. Each query's `change` and the window's `languagechange`
//! only wake the owner, which then samples every observation again.

use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, Ordering},
};

use flui_platform::SystemPreferences;
use wasm_bindgen::{JsCast, closure::Closure};
use web_sys::{Event, EventTarget, MediaQueryList, Window};

use super::platform::WebLoop;
use crate::{
    PlatformError, host,
    shared::{
        browser_preferences::{MediaFeature, MediaMatches},
        owner_signal::OwnerSignal,
        preferred_locales,
    },
};

const LANGUAGE_CHANGE: &str = "languagechange";
const CHANGE: &str = "change";

/// What a sample reads, cloned out of the host so no borrow of it is held
/// while the browser answers.
#[derive(Clone)]
pub(super) struct Sampler {
    window: Window,
    queries: Vec<(MediaFeature, MediaQueryList)>,
    active: Arc<AtomicBool>,
}

impl Sampler {
    /// Every observation the browser reports now.
    ///
    /// # Errors
    /// Refuses once the source has closed, including during the sample.
    pub(super) fn sample(&self) -> Result<SystemPreferences, PlatformError> {
        self.ensure_active("browser preference source is closed")?;
        let matches = MediaMatches::from_fn(|feature| {
            self.queries
                .iter()
                .find(|(observed, _)| *observed == feature)
                .is_some_and(|(_, query)| query.matches())
        });
        let navigator = self.window.navigator();
        let mut tags: Vec<String> = navigator
            .languages()
            .iter()
            .filter_map(|tag| tag.as_string())
            .collect();
        if tags.is_empty() {
            tags.extend(navigator.language());
        }
        self.ensure_active("browser preference source closed during sampling")?;
        let mut preferences = SystemPreferences::default();
        if let Some(motion) = matches.motion() {
            preferences = preferences.with_motion(motion);
        }
        if let Some(high_contrast) = matches.high_contrast() {
            preferences = preferences.with_high_contrast(high_contrast);
        }
        if let Some(locales) = preferred_locales::from_language_tags(tags) {
            preferences = preferences.with_locales(locales);
        }
        Ok(preferences)
    }

    fn ensure_active(&self, message: &'static str) -> Result<(), PlatformError> {
        if self.active.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err(PlatformError::Preferences {
                message: message.into(),
            })
        }
    }
}

pub struct WebPreferences {
    sampler: Sampler,
    callback: Option<Closure<dyn FnMut(Event)>>,
    /// Each target and event the callback is registered for, in order.
    registered: Vec<(EventTarget, &'static str)>,
}

impl WebPreferences {
    pub(super) fn new(signal: &Arc<OwnerSignal>) -> Result<Self, PlatformError> {
        let window = web_sys::window().ok_or_else(|| PlatformError::Preferences {
            message: "browser preference observation requires a Window".into(),
        })?;
        let mut queries = Vec::with_capacity(MediaFeature::ALL.len());
        for feature in MediaFeature::ALL {
            // A browser without a query answers none; that feature then stays
            // unobserved instead of failing the others.
            if let Some(query) =
                window
                    .match_media(feature.query())
                    .map_err(|error| PlatformError::Preferences {
                        message: format!("{} failed: {error:?}", feature.query()),
                    })?
            {
                queries.push((feature, query));
            }
        }
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
        // Own every registration as soon as it succeeds: if a later one fails,
        // Drop closes admission and removes the earlier ones.
        let mut preferences = Self {
            sampler: Sampler {
                window,
                queries,
                active,
            },
            callback: Some(callback),
            registered: Vec::with_capacity(MediaFeature::ALL.len() + 1),
        };
        let targets: Vec<(EventTarget, &'static str)> = preferences
            .sampler
            .queries
            .iter()
            .map(|(_, query)| (EventTarget::from(query.clone()), CHANGE))
            .chain([(
                EventTarget::from(preferences.sampler.window.clone()),
                LANGUAGE_CHANGE,
            )])
            .collect();
        for (target, event) in targets {
            let callback = preferences
                .callback
                .as_ref()
                .expect("BUG: the callback lives until Drop");
            target
                .add_event_listener_with_callback(event, callback.as_ref().unchecked_ref())
                .map_err(|error| PlatformError::Preferences {
                    message: format!("{event} observer registration failed: {error:?}"),
                })?;
            preferences.registered.push((target, event));
        }
        Ok(preferences)
    }

    pub(super) fn sampler(&self) -> Sampler {
        self.sampler.clone()
    }
}

impl std::fmt::Debug for WebPreferences {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebPreferences").finish_non_exhaustive()
    }
}

impl host::Preferences<WebLoop> for WebPreferences {
    fn sample(owner: &WebLoop) -> Result<SystemPreferences, PlatformError> {
        if !owner.signal.accepting() {
            return Err(PlatformError::Preferences {
                message: "browser preference owner has stopped".into(),
            });
        }
        // Sample outside the borrow: the browser's answers must not observe
        // the host's own state held.
        let Some(sampler) = owner
            .preferences
            .borrow()
            .as_ref()
            .map(WebPreferences::sampler)
        else {
            return Err(PlatformError::Preferences {
                message: "browser preference source is closed".into(),
            });
        };
        let preferences = sampler.sample()?;
        if !owner.signal.accepting() {
            return Err(PlatformError::Preferences {
                message: "browser preference owner stopped during sampling".into(),
            });
        }
        Ok(preferences)
    }
}

impl Drop for WebPreferences {
    fn drop(&mut self) {
        self.sampler.active.store(false, Ordering::Release);
        let Some(callback) = self.callback.take() else {
            return;
        };
        let mut failed = false;
        for (target, event) in self.registered.drain(..) {
            if let Err(error) =
                target.remove_event_listener_with_callback(event, callback.as_ref().unchecked_ref())
            {
                failed = true;
                crate::shared::panic_boundary::contain_owner_callback(|| {
                    tracing::warn!(?error, event, "browser preference observer removal failed");
                });
            }
        }
        if failed {
            // Keep the JS trampoline valid for a registration that survived.
            // Its capture is inert and cannot deliver to a replacement host.
            callback.forget();
        }
    }
}
