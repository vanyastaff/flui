//! The Web display source.
//!
//! # What a page can know
//!
//! A browser tab sees one screen, the one `window.screen` describes, and learns
//! nothing about its refresh rate. Both are filled in here:
//!
//! - **The refresh period is an estimate**, taken from the timestamps of the
//!   page's own `requestAnimationFrame` callbacks (see
//!   [`super::refresh_estimate`] for the rule and its limits). It is
//!   unknown (`None`) until enough frames have run, follows the cadence the
//!   browser presents at rather than the panel's capability, and is estimated
//!   again after the tab becomes visible and after a pixel-ratio change.
//! - **Several screens** are listed only when the Window Management API is
//!   available and its permission is already granted: the platform never
//!   requests the permission, because a prompt is the application's explicit
//!   act. Without it the snapshot holds the one screen. Each listed screen is
//!   known by its JS object, compared by identity in a retained list
//!   ([`super::screen_identity`]); labels repeat and are not keys.
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
//! # Scale, bounds and page zoom
//!
//! `scale_factor` of the screen the window is on is `window.devicePixelRatio`
//! in both modes, which includes page zoom; the other listed screens report
//! their own `ScreenDetailed.devicePixelRatio`. Bounds describe the physical
//! screen and must not move with zoom, so they use a ratio without it:
//!
//! - **With screen access** the screen's own `ScreenDetailed.devicePixelRatio`,
//!   which is the display's scale alone.
//! - **Without it** the page's pixel ratio when `window.screen` was first
//!   seen, taken as the display's own scale with the page at 100%. Chrome
//!   reports `screen.width` in device-independent pixels that do not change
//!   with zoom, and `devicePixelRatio` is the OS scale times the zoom, so the
//!   zoom cannot be read back out of the two later; a page that is zoomed when
//!   it starts reports bounds inflated by that zoom until the screen changes.
//!   A `change` of `window.screen` (another display) takes the ratio again.
//!   Firefox reports `screen.width` in zoomed CSS pixels, where this and the
//!   product with the current ratio agree only at 100%.
//!
//! # Identity across screen access
//!
//! When a listing first appears, its `currentScreen` takes the id of the
//! single-screen display the page had, so the window sees that display gain a
//! name rather than change; the other screens get fresh ids.

use std::sync::Arc;

use flui_foundation::geometry::{Bounds, Point, Size};
use flui_platform::{DisplayId, DisplayInfo, DisplayKey, DisplaySnapshot, RefreshPeriod};
use parking_lot::Mutex;
use wasm_bindgen::{JsCast, JsValue};

use super::browser::BrowserFeatures;
use super::refresh_estimate::RefreshEstimator;
use super::screen_identity::RetainedScreens;
use crate::display::source::DisplaySource;
use crate::error::PlatformError;

/// The Web platform's display state: identities, the estimator and the cached
/// snapshot. Shared by the platform, its window and the browser listeners.
pub struct WebDisplays {
    source: DisplaySource,
    pub(super) features: BrowserFeatures,
    state: Mutex<State>,
}

impl std::fmt::Debug for WebDisplays {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebDisplays").finish_non_exhaustive()
    }
}

struct State {
    /// The display the window is on, as of the last sample.
    current: Option<DisplayId>,
    estimator: RefreshEstimator,
    /// The Window Management `ScreenDetails` object, while access is granted.
    details: Option<JsValue>,
    screens: RetainedScreens<JsValue>,
    /// The ratio that turns `window.screen`'s size into device pixels while no
    /// screen list is available: the page's pixel ratio when the screen was
    /// first seen, taken as the display's own scale with the page at 100%.
    /// `None` until the first sample and after the screen changed.
    screen_ratio: Option<f64>,
}

impl crate::owner::web::HostDisplays for WebDisplays {
    /// The cached snapshot, sampled from the browser when an event invalidated
    /// it.
    fn snapshot(&self) -> Result<DisplaySnapshot, PlatformError> {
        self.source.snapshot_with(|| {
            let mut state = self.state.lock();
            self.sample(&mut state)
        })
    }

    /// The id is a product of sampling: it follows the cached snapshot.
    fn current_display(&self) -> Option<DisplayId> {
        crate::owner::web::HostDisplays::snapshot(self).ok()?;
        self.state.lock().current
    }

    /// A new published period marks the snapshot stale.
    fn frame(&self, timestamp_ms: f64) -> bool {
        let mut state = self.state.lock();
        if state.estimator.frame(timestamp_ms).is_none() {
            return false;
        }
        self.source.invalidate();
        true
    }

    fn observe(
        self: Arc<Self>,
        signal: &Arc<crate::owner::signal::OwnerSignal>,
    ) -> crate::owner::watch::Watch {
        crate::owner::watch::Watch::new(super::events::DisplayObservers::start(&self, signal))
    }
}

impl WebDisplays {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            source: DisplaySource::new(),
            features: BrowserFeatures::detect(),
            state: Mutex::new(State {
                current: None,
                estimator: RefreshEstimator::new(),
                details: None,
                screens: RetainedScreens::new(),
                screen_ratio: None,
            }),
        })
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
        self.source.invalidate();
    }

    /// `window.screen` changed (another size, another display): sample again,
    /// and take the page's pixel ratio then as the display's own scale.
    pub(super) fn screen_changed(&self) {
        let mut state = self.state.lock();
        state.screen_ratio = None;
        // Without screen access this event is the only sign of a move to
        // another monitor, and it cannot be told from a change of the same
        // screen's metrics: the old period may describe another display. With
        // a `ScreenDetails` the current-screen identity decides that instead.
        if state.details.is_none() {
            state.estimator.forget();
        }
        self.source.invalidate();
    }

    /// Something the snapshot reports changed: sample the browser again on the
    /// next read.
    pub(super) fn invalidate(&self) {
        self.source.invalidate();
    }

    /// Replace the `ScreenDetails` object, or drop it when access ended.
    /// Returns the previous one so the caller retires it outside the lock.
    pub(super) fn set_details(&self, details: Option<JsValue>) -> Option<JsValue> {
        let mut state = self.state.lock();
        if details.is_none() && state.details.is_none() {
            // Nothing was granted and nothing is: not an event.
            return None;
        }
        self.source.invalidate();
        state.estimator.restart();
        std::mem::replace(&mut state.details, details)
    }

    fn sample(&self, state: &mut State) -> Result<Vec<DisplayInfo>, PlatformError> {
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
            None => self.single_screen(state, &window, period)?,
        };
        state.current = current;
        Ok(displays)
    }

    /// The one screen `window.screen` describes.
    fn single_screen(
        &self,
        state: &mut State,
        window: &web_sys::Window,
        period: Option<RefreshPeriod>,
    ) -> Result<(Vec<DisplayInfo>, Option<DisplayId>), PlatformError> {
        let screen = window.screen().map_err(|_| PlatformError::Displays {
            message: "the window has no screen".into(),
        })?;
        let scale_factor = positive(window.device_pixel_ratio()).unwrap_or(1.0);
        let device_scale = *state.screen_ratio.get_or_insert(scale_factor);
        let bounds = Bounds::new(
            Point::new(0, 0),
            Size::new(
                device_pixels(f64::from(screen.width().unwrap_or(1920)), device_scale),
                device_pixels(f64::from(screen.height().unwrap_or(1080)), device_scale),
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
        // The screen the page saw as its only one is the listing's current
        // screen: the first time a listing appears it takes that display's id,
        // so the window sees the display gain a name, not change.
        if state.screens.is_empty()
            && let Some(current) = get(details, "currentScreen")
            && let Some(id) = self.source.mint(&DisplayKey::Web("primary".into()))
        {
            state.screens.adopt(current, id);
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
                    // The screen the window is on reports the window's own pixel
                    // ratio, which includes page zoom, so the scale behaves the
                    // same with and without screen access. Its estimate belongs
                    // to it alone.
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

/// One `ScreenDetailed` as a display. Its metrics are CSS pixels of the
/// screen, converted to device pixels with the screen's own
/// `devicePixelRatio`, which is the display's scale and does not include page
/// zoom: bounds describe the physical screen and stay put while the page is
/// zoomed. The reported scale is `scale_override` (the window's pixel ratio, zoom
/// included, for the screen the window is on) or else that same ratio. Origins
/// are converted the same way: the coordinate space of a mixed-ratio layout is
/// the browser's, and the origin is therefore the browser's value in that
/// screen's device pixels, not a global device layout.
fn screen_info(screen: &JsValue, id: DisplayId, scale_override: Option<f64>) -> DisplayInfo {
    let own_ratio = get_number(screen, "devicePixelRatio")
        .and_then(positive)
        .unwrap_or(1.0);
    let scale_factor = scale_override.unwrap_or(own_ratio);
    let number = |name: &str, fallback: f64| get_number(screen, name).unwrap_or(fallback);
    let device = |css: f64| device_pixels(css, own_ratio);
    let bounds = Bounds::new(
        Point::new(device(number("left", 0.0)), device(number("top", 0.0))),
        Size::new(
            device(number("width", 1920.0)),
            device(number("height", 1080.0)),
        ),
    );
    let usable = Bounds::new(
        Point::new(
            device(number("availLeft", number("left", 0.0))),
            device(number("availTop", number("top", 0.0))),
        ),
        Size::new(
            device(number("availWidth", number("width", 1920.0))),
            device(number("availHeight", number("height", 1080.0))),
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
