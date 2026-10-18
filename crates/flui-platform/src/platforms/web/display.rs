//! The Web display source.
//!
//! # What a page can know
//!
//! A browser tab sees one screen, the one `window.screen` describes, and learns
//! nothing about its refresh rate. Both are filled in here:
//!
//! - **The refresh period is an estimate**, taken from the timestamps of the
//!   page's own `requestAnimationFrame` callbacks (see
//!   [`crate::shared::refresh_estimate`] for the rule and its limits). It is
//!   unknown (`None`) until enough frames have run, follows the cadence the
//!   browser presents at rather than the panel's capability, and is estimated
//!   again after the tab becomes visible and after a pixel-ratio change.
//! - **Several screens** are listed only when the Window Management API is
//!   available and its permission is already granted: the platform never
//!   requests the permission, because a prompt is the application's explicit
//!   act. Without it the snapshot holds the one screen. Each listed screen is
//!   known by its JS object, compared by identity in a retained list
//!   ([`crate::shared::screen_identity`]); labels repeat and are not keys.
//!   The estimate belongs to the screen the window is on, the others stay
//!   unknown.
//!
//! # Sampling
//!
//! `Platform::display_snapshot` runs on every owner wake, so the snapshot is
//! cached and read from the browser again only after an event that can change
//! it: a pixel-ratio change, a new published period, a `change` of `window.screen`
//! (size, pixel ratio or position; Chromium raises it without any permission),
//! a change of the screen list, of a listed screen, or of the screen the window
//! is on. A browser whose `screen` has no `onchange` raises none of the screen
//! events, so there a move between two screens of equal pixel ratio but
//! different size is not seen until the next other event.
//!
//! The screen the window is on takes its scale from `window.devicePixelRatio`
//! in both modes, which includes page zoom; the other listed screens report
//! their own `ScreenDetailed.devicePixelRatio`.

use std::sync::Arc;

use flui_foundation::geometry::{Bounds, Point, Size};
use flui_platform_api::{DisplayId, DisplayInfo, DisplayKey, DisplaySnapshot, RefreshPeriod};
use parking_lot::Mutex;
use wasm_bindgen::{JsCast, JsValue};

use super::features::BrowserFeatures;
use crate::error::PlatformError;
use crate::shared::display_source::DisplaySource;
use crate::shared::refresh_estimate::RefreshEstimator;
use crate::shared::screen_identity::RetainedScreens;

/// The Web platform's display state: identities, the estimator and the cached
/// snapshot. Shared by the platform, its window and the browser listeners.
pub(super) struct WebDisplays {
    source: DisplaySource,
    pub(super) features: BrowserFeatures,
    state: Mutex<State>,
}

struct State {
    /// The last sample; `None` when an event invalidated it.
    snapshot: Option<DisplaySnapshot>,
    /// The display the window is on, as of the last sample.
    current: Option<DisplayId>,
    estimator: RefreshEstimator,
    /// The Window Management `ScreenDetails` object, while access is granted.
    details: Option<JsValue>,
    screens: RetainedScreens<JsValue>,
}

impl WebDisplays {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            source: DisplaySource::new(),
            features: BrowserFeatures::detect(),
            state: Mutex::new(State {
                snapshot: None,
                current: None,
                estimator: RefreshEstimator::new(),
                details: None,
                screens: RetainedScreens::new(),
            }),
        })
    }

    /// The cached snapshot, sampled from the browser when an event invalidated
    /// it.
    pub(super) fn snapshot(&self) -> Result<DisplaySnapshot, PlatformError> {
        let mut state = self.state.lock();
        if let Some(snapshot) = &state.snapshot {
            return Ok(snapshot.clone());
        }
        let snapshot = self.sample(&mut state)?;
        state.snapshot = Some(snapshot.clone());
        Ok(snapshot)
    }

    /// The display the window is on; `None` when the platform has no id for it.
    pub(super) fn current_display(&self) -> Option<DisplayId> {
        // The id is a product of sampling: it follows the cached snapshot.
        self.snapshot().ok()?;
        self.state.lock().current
    }

    /// Feed one animation-frame timestamp (milliseconds). `true` when the
    /// published period changed, so the snapshot must be taken again and the
    /// owner woken.
    pub(super) fn frame(&self, timestamp_ms: f64) -> bool {
        let mut state = self.state.lock();
        if state.estimator.frame(timestamp_ms).is_none() {
            return false;
        }
        state.snapshot = None;
        true
    }

    /// The cadence may have changed (the tab became visible, the page changed
    /// scale or screen): measure again.
    pub(super) fn restart_estimate(&self) {
        self.state.lock().estimator.restart();
    }

    /// The window is on another screen: the published period described the
    /// previous one, so none is reported until the new screen is measured.
    pub(super) fn forget_estimate(&self) {
        let mut state = self.state.lock();
        state.estimator.forget();
        state.snapshot = None;
    }

    /// Something the snapshot reports changed: sample the browser again on the
    /// next read.
    pub(super) fn invalidate(&self) {
        self.state.lock().snapshot = None;
    }

    /// Replace the `ScreenDetails` object, or drop it when access ended.
    /// Returns the previous one so the caller retires it outside the lock.
    pub(super) fn set_details(&self, details: Option<JsValue>) -> Option<JsValue> {
        let mut state = self.state.lock();
        if details.is_none() && state.details.is_none() {
            // Nothing was granted and nothing is: not an event.
            return None;
        }
        state.snapshot = None;
        state.estimator.restart();
        std::mem::replace(&mut state.details, details)
    }

    fn sample(&self, state: &mut State) -> Result<DisplaySnapshot, PlatformError> {
        let window = web_sys::window().ok_or_else(|| PlatformError::Displays {
            message: "no global window".into(),
        })?;
        let period = state.estimator.published();
        let listed = match state.details.clone() {
            Some(details) => self.listed_screens(state, &window, &details, period),
            None => None,
        };
        let (displays, current) = match listed {
            Some(listed) => listed,
            None => self.single_screen(&window, period)?,
        };
        state.current = current;
        self.source.publish(displays)
    }

    /// The one screen `window.screen` describes.
    fn single_screen(
        &self,
        window: &web_sys::Window,
        period: Option<RefreshPeriod>,
    ) -> Result<(Vec<DisplayInfo>, Option<DisplayId>), PlatformError> {
        let screen = window.screen().map_err(|_| PlatformError::Displays {
            message: "the window has no screen".into(),
        })?;
        let scale_factor = positive(window.device_pixel_ratio()).unwrap_or(1.0);
        let bounds = Bounds::new(
            Point::new(0, 0),
            Size::new(
                device_pixels(f64::from(screen.width().unwrap_or(1920)), scale_factor),
                device_pixels(f64::from(screen.height().unwrap_or(1080)), scale_factor),
            ),
        );
        let id = self.source.mint(&DisplayKey::Web("primary".into()));
        let displays = id
            .map(|id| {
                vec![with_period(
                    DisplayInfo::new(id, "Browser Screen", bounds, scale_factor).with_primary(true),
                    period,
                )]
            })
            .unwrap_or_default();
        Ok((displays, id))
    }

    /// The screens `ScreenDetails` lists, with the display the window is on;
    /// `None` when the object does not list any screen, which reads as the
    /// single-screen case.
    fn listed_screens(
        &self,
        state: &mut State,
        window: &web_sys::Window,
        details: &JsValue,
        period: Option<RefreshPeriod>,
    ) -> Option<(Vec<DisplayInfo>, Option<DisplayId>)> {
        let screens: Vec<JsValue> = js_sys::Array::from(&get(details, "screens")?).to_vec();
        if screens.is_empty() {
            return None;
        }
        let ids = state
            .screens
            .reconcile(screens.clone(), js_sys::Object::is, || {
                self.source.mint(&DisplayKey::Unkeyed)
            });
        let listed: Vec<(JsValue, DisplayId)> = screens
            .into_iter()
            .zip(ids)
            .filter_map(|(screen, id)| Some((screen, id?)))
            .collect();
        let current = get(details, "currentScreen")
            .and_then(|screen| state.screens.id_of(&screen, js_sys::Object::is))
            // A listing whose current screen is missing is still a listing: the
            // window is then on the primary screen.
            .or_else(|| {
                listed
                    .iter()
                    .find(|(screen, _)| is_primary(screen))
                    .map(|(_, id)| *id)
            });
        let window_scale = positive(window.device_pixel_ratio());
        let displays = listed
            .iter()
            .map(|(screen, id)| {
                if Some(*id) == current {
                    // The screen the window is on is measured like the
                    // single-screen case: by the window's own pixel ratio, which
                    // includes page zoom, so zoom behaves the same with and
                    // without screen access. Its estimate belongs to it alone.
                    with_period(screen_info(screen, *id, window_scale), period)
                } else {
                    screen_info(screen, *id, None)
                }
            })
            .collect();
        Some((displays, current))
    }
}

fn is_primary(screen: &JsValue) -> bool {
    get(screen, "isPrimary").is_some_and(|primary| primary.is_truthy())
}

/// One `ScreenDetailed` as a display. Its metrics are CSS pixels, scaled by
/// the screen's own pixel ratio (or `scale_override`), which is also how the
/// single-screen case reports a size. Origins are scaled the same way: the coordinate space of a
/// mixed-ratio layout is the browser's, and the origin is therefore the
/// browser's value in that screen's device pixels, not a global device layout.
fn screen_info(screen: &JsValue, id: DisplayId, scale_override: Option<f64>) -> DisplayInfo {
    let scale_factor = scale_override
        .or_else(|| get_number(screen, "devicePixelRatio").and_then(positive))
        .unwrap_or(1.0);
    let number = |name: &str, fallback: f64| get_number(screen, name).unwrap_or(fallback);
    let bounds = Bounds::new(
        Point::new(
            device_pixels(number("left", 0.0), scale_factor),
            device_pixels(number("top", 0.0), scale_factor),
        ),
        Size::new(
            device_pixels(number("width", 1920.0), scale_factor),
            device_pixels(number("height", 1080.0), scale_factor),
        ),
    );
    let usable = Bounds::new(
        Point::new(
            device_pixels(number("availLeft", number("left", 0.0)), scale_factor),
            device_pixels(number("availTop", number("top", 0.0)), scale_factor),
        ),
        Size::new(
            device_pixels(number("availWidth", number("width", 1920.0)), scale_factor),
            device_pixels(
                number("availHeight", number("height", 1080.0)),
                scale_factor,
            ),
        ),
    );
    let label = get(screen, "label")
        .and_then(|label| label.as_string())
        .filter(|label| !label.is_empty())
        .unwrap_or_else(|| "Browser Screen".to_string());
    let is_primary = is_primary(screen);
    DisplayInfo::new(id, label, bounds, scale_factor)
        .with_usable_bounds(usable)
        .with_primary(is_primary)
}

fn with_period(info: DisplayInfo, period: Option<RefreshPeriod>) -> DisplayInfo {
    match period {
        Some(period) => info.with_refresh_period(period.get()),
        None => info,
    }
}

/// A property that is present, neither `undefined` nor `null`.
pub(super) fn get(target: &JsValue, name: &str) -> Option<JsValue> {
    js_sys::Reflect::get(target, &JsValue::from_str(name))
        .ok()
        .filter(|value| !value.is_undefined() && !value.is_null())
}

fn get_number(target: &JsValue, name: &str) -> Option<f64> {
    get(target, name)?
        .as_f64()
        .filter(|value| value.is_finite())
}

fn positive(value: f64) -> Option<f64> {
    (value.is_finite() && value > 0.0).then_some(value)
}

/// CSS pixels as the device pixels a snapshot carries.
fn device_pixels(css: f64, scale_factor: f64) -> i32 {
    // `as` saturates at the `i32` range, which is the clamp wanted here.
    (css * scale_factor).round() as i32
}

/// Whether `value` is a JS object that can receive events.
pub(super) fn event_target(value: &JsValue) -> Option<web_sys::EventTarget> {
    value.dyn_ref::<web_sys::EventTarget>().cloned()
}
