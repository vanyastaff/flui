//! The Windows host's system preferences: WinRT's `UISettings` and
//! `AccessibilitySettings` and the Win32 system metrics and parameters,
//! sampled on the owner's STA independently of any window.
//!
//! The owner keeps the source (`OwnerPreferences`): its broadcast receiver
//! tells it of setting changes, and a failed read is retried on a
//! paced owner wake.

// WinRT activation and initialization, and the Win32 metric queries. Each
// `unsafe` block states its invariant.
#![expect(unsafe_code)]

use std::cell::{Cell, RefCell};
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use flui_foundation::geometry::{DevicePixelRatio, Size};
use flui_platform::{
    GesturePreferences, MotionPreference, NativeMouseGeometry, PreferenceQueryError,
    ScrollBarPreferences, ScrollBarVisibility, SrgbColor, SystemPreferences, WheelPreferences,
    WheelStep,
};
use windows::Foundation::TypedEventHandler;
use windows::UI::ViewManagement::{AccessibilitySettings, UIColorType, UISettings};
use windows::Win32::Foundation::{
    E_NOINTERFACE, GetLastError, HWND, LPARAM, LRESULT, SetLastError, WIN32_ERROR, WPARAM,
};
use windows::Win32::System::WinRT::{
    RO_INIT_SINGLETHREADED, RoActivateInstance, RoInitialize, RoUninitialize,
};
use windows::Win32::UI::HiDpi::GetSystemMetricsForDpi;
use windows::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime;
use windows::Win32::UI::WindowsAndMessaging::{
    SM_CXDOUBLECLK, SM_CXDRAG, SM_CYDOUBLECLK, SM_CYDRAG, SPI_GETWHEELSCROLLCHARS,
    SPI_GETWHEELSCROLLLINES, SYSTEM_METRICS_INDEX, SYSTEM_PARAMETERS_INFO_ACTION,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW, WM_DWMCOLORIZATIONCOLORCHANGED,
    WM_SETTINGCHANGE,
};
use windows::core::{Interface, RuntimeName};

use crate::PlatformError;
use crate::owner::panic_boundary::{AfterPanic, contain_owner_callback, os_entry};
use crate::owner::signal::OwnerSignal;
use crate::owner::sources::SystemPreferenceSource;
use crate::owner::windows::broadcast::Subscriber;
use crate::owner::windows::control::OwnerPreferences;
use crate::preferences::retries::Retries;

mod languages;
mod regional;
#[cfg(test)]
mod retry_tests;
#[cfg(test)]
mod tests;

const RETRY_INTERVAL: Duration = Duration::from_millis(100);

/// The retries of a read that failed or left a value unread: 0.1 to 3.2 s
/// apart, then none until the system next reports a setting change.
const READ_RETRIES: Retries = Retries::new(RETRY_INTERVAL, 6);

struct Invalidation {
    accepting: AtomicBool,
    pending: AtomicBool,
    signal: Weak<OwnerSignal>,
}

impl Invalidation {
    fn notify(&self) {
        if self.accepting.load(Ordering::Acquire)
            && let Some(signal) = self.signal.upgrade()
            && signal.accepting()
        {
            self.pending.store(true, Ordering::Release);
            // Pending refresh survives a failed post or an absent owner hook.
            let _ = signal.wake();
        }
    }
}

/// The preference source's part of the system's broadcasts: setting changes,
/// which mark the preferences stale and wake the owner. It only observes, so
/// every other subscriber hears the same message. It shares the source's
/// admission, so once the source is gone a receiver that outlives it marks
/// nothing pending and wakes nobody.
pub(crate) struct PreferenceBroadcasts(Arc<Invalidation>);

impl Subscriber for PreferenceBroadcasts {
    fn message(
        &self,
        _hwnd: HWND,
        message: u32,
        _wparam: WPARAM,
        _lparam: LPARAM,
    ) -> Option<LRESULT> {
        // A setting change, or a change of the colour DWM tints frames with,
        // which follows the accent; `UISettings.ColorValuesChanged` reports
        // the same change, and whichever arrives first marks the snapshot
        // stale.
        if message == WM_SETTINGCHANGE || message == WM_DWMCOLORIZATIONCOLORCHANGED {
            self.0.notify();
        }
        None
    }
}

/// Native subscriptions and interfaces belong to the owner context, not its
/// Send + Sync platform wrapper. Interfaces retire before WinRT. The system's
/// setting broadcasts reach it through the owner's broadcast receiver
/// (`OwnerPreferences::broadcasts`).
pub(crate) struct PreferenceSource {
    invalidation: Arc<Invalidation>,
    ui: UISettings,
    accessibility: AccessibilitySettings,
    /// Each `UISettings` change event the source registered, with its token.
    observers: Vec<(UiSettingsEvent, i64)>,
    cache: SampleCache,
    _entry: WinRtEntry,
}

impl std::fmt::Debug for PreferenceSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreferenceSource").finish_non_exhaustive()
    }
}

impl SystemPreferenceSource for PreferenceSource {
    fn sample(&self) -> Result<SystemPreferences, PlatformError> {
        let previous = self.cache.value.borrow().clone();
        self.sample_with(|| Ok(sample(&self.ui, &self.accessibility, previous.as_ref())))
    }
}

/// What the owner asks of the source it keeps.
impl OwnerPreferences for PreferenceSource {
    fn pending(&self) -> bool {
        self.invalidation.pending.load(Ordering::Acquire)
    }

    fn retry_deadline(&self) -> Option<web_time::Instant> {
        (self.invalidation.accepting.load(Ordering::Acquire)
            && self
                .invalidation
                .signal
                .upgrade()
                .is_some_and(|signal| signal.accepting()))
        .then(|| self.cache.retry.get().map(|retry| retry.wake_at))
        .flatten()
    }

    fn retry_if_due(&self) {
        let now = web_time::Instant::now();
        if self
            .retry_deadline()
            .is_some_and(|deadline| deadline <= now)
        {
            // Rearm before posting: failed admission and a missing owner hook
            // retain a paced obligation, without relying on a user-window paint.
            if let Some(mut retry) = self.cache.retry.get() {
                retry.wake_at = now + RETRY_INTERVAL;
                self.cache.retry.set(Some(retry));
            }
            if let Some(signal) = self.invalidation.signal.upgrade() {
                let _ = signal.wake();
            }
        }
    }

    /// Setting changes, which the owner's broadcast receiver tells this
    /// source of.
    fn broadcasts(&self) -> Rc<dyn Subscriber> {
        Rc::new(PreferenceBroadcasts(Arc::clone(&self.invalidation)))
    }

    fn mouse_gestures(&self, dpi: u32) -> Result<GesturePreferences, PreferenceQueryError> {
        mouse_gestures(dpi)
    }
}

/// The double-click interval and the mouse geometry at `dpi`, read together:
/// what a window's clicks and gestures are measured with, and, at 96 DPI,
/// the snapshot's window-independent reading.
fn mouse_gestures(dpi: u32) -> Result<GesturePreferences, PreferenceQueryError> {
    // SAFETY: this getter takes no pointers and only reads the system setting.
    let double_click = unsafe { GetDoubleClickTime() };
    Ok(GesturePreferences::default()
        .with_double_click_interval(Duration::from_millis(u64::from(double_click)))
        .with_native_mouse_geometry(mouse_geometry(dpi)?))
}

/// The source an owner keeps, as the type this file built: the tests reach
/// its failure injection through it.
#[cfg(test)]
pub(crate) fn kept(source: Rc<dyn OwnerPreferences>) -> Rc<PreferenceSource> {
    let source: Rc<dyn std::any::Any> = source;
    source
        .downcast()
        .expect("BUG: the Windows tables keep this file's source")
}

impl PreferenceSource {
    /// A source that wakes the owner behind `signal` when a preference
    /// changes.
    pub(crate) fn try_new(signal: &Arc<OwnerSignal>) -> Result<Self, PlatformError> {
        let entry = WinRtEntry::enter().map_err(native_error)?;
        // SAFETY: this owner holds its WinRT entry; class strings are owned.
        // Direct activation avoids the generated process-wide factory cache,
        // which can retain a factory beyond the previous host's apartment.
        let ui = unsafe { RoActivateInstance(&UISettings::NAME.into()) }
            .and_then(|instance| instance.cast())
            .map_err(native_error)?;
        // SAFETY: the same live owner entry guards this activation and its result.
        let accessibility = unsafe { RoActivateInstance(&AccessibilitySettings::NAME.into()) }
            .and_then(|instance| instance.cast())
            .map_err(native_error)?;
        let invalidation = Arc::new(Invalidation {
            accepting: AtomicBool::new(true),
            pending: AtomicBool::new(true),
            signal: Arc::downgrade(signal),
        });
        let mut source = Self {
            invalidation,
            ui,
            accessibility,
            observers: Vec::with_capacity(UiSettingsEvent::ALL.len()),
            cache: SampleCache::default(),
            _entry: entry,
        };
        // Each registration is owned as soon as it succeeds: a later failure
        // drops the source, which revokes the earlier ones.
        for event in UiSettingsEvent::ALL {
            if let Some(token) = event.observe(&source.ui, &source.invalidation)? {
                source.observers.push((event, token));
            }
        }
        Ok(source)
    }

    #[cfg(test)]
    fn invalidate_for_retry_test(&self) {
        self.invalidation.pending.store(true, Ordering::Release);
    }

    /// One read through the source's cache, with `read` standing for the
    /// system's answer.
    pub(crate) fn sample_with<R: Into<Sample>>(
        &self,
        read: impl FnOnce() -> Result<R, PlatformError>,
    ) -> Result<SystemPreferences, PlatformError> {
        self.cache.read(&self.invalidation.pending, read)
    }
}

/// One native read: the values Windows answered and, for each getter that
/// failed, the value accepted before it (none on a cold source).
pub(crate) struct Sample {
    values: SystemPreferences,
    /// Every getter answered; otherwise the read owes a retry.
    complete: bool,
}

impl From<SystemPreferences> for Sample {
    fn from(values: SystemPreferences) -> Self {
        Self {
            values,
            complete: true,
        }
    }
}

#[derive(Default)]
struct SampleCache {
    reading: Cell<bool>,
    value: RefCell<Option<SystemPreferences>>,
    retry: Cell<Option<Retry>>,
}

#[derive(Clone, Copy)]
struct Retry {
    // Reposting a wake must not push the permitted read into the future.
    read_after: web_time::Instant,
    wake_at: web_time::Instant,
    /// The retries of `READ_RETRIES` spent on reads that failed in a row;
    /// `None` for a read again that a change during the last read owes.
    spent: Option<u32>,
}

struct SampleAttempt<'a> {
    reading: &'a Cell<bool>,
    pending: &'a AtomicBool,
    committed: bool,
    /// The committed read left a getter unanswered.
    incomplete: bool,
    retry: &'a Cell<Option<Retry>>,
    /// The failure retries spent before this attempt.
    spent: Option<u32>,
    started: web_time::Instant,
}

impl Drop for SampleAttempt<'_> {
    fn drop(&mut self) {
        let failed = !self.committed || self.incomplete;
        if failed {
            let spent = self.spent.map_or(0, |spent| spent.saturating_add(1));
            match READ_RETRIES.pause(spent) {
                Some(pause) => {
                    self.pending.store(true, Ordering::Release);
                    let next = self.started + pause;
                    self.retry.set(Some(Retry {
                        read_after: next,
                        wake_at: next,
                        spent: Some(spent),
                    }));
                }
                // Every retry spent: nothing is read again until the system
                // reports a change, which marks the source pending.
                None => self.retry.set(None),
            }
        } else if self.pending.load(Ordering::Acquire) {
            // A change arrived during the read: read again at once.
            self.retry.set(Some(Retry {
                read_after: self.started,
                wake_at: self.started + RETRY_INTERVAL,
                spent: None,
            }));
        } else {
            self.retry.set(None);
        }
        self.reading.set(false);
    }
}

impl SampleCache {
    fn read<R: Into<Sample>>(
        &self,
        pending: &AtomicBool,
        read: impl FnOnce() -> Result<R, PlatformError>,
    ) -> Result<SystemPreferences, PlatformError> {
        self.read_at(pending, web_time::Instant::now(), read)
    }

    fn read_at<R: Into<Sample>>(
        &self,
        pending: &AtomicBool,
        now: web_time::Instant,
        read: impl FnOnce() -> Result<R, PlatformError>,
    ) -> Result<SystemPreferences, PlatformError> {
        // Native getters can pump messages. Refuse even a cached nested read:
        // publishing it could overtake the outer observation being assembled.
        if self.reading.get() || self.retry.get().is_some_and(|retry| now < retry.read_after) {
            return Err(PlatformError::PreferencesDeferred);
        }
        if !pending.load(Ordering::Acquire)
            && let Some(value) = self.value.borrow().as_ref()
        {
            return Ok(value.clone());
        }
        self.reading.set(true);
        let mut reading = SampleAttempt {
            reading: &self.reading,
            pending,
            committed: false,
            incomplete: false,
            retry: &self.retry,
            spent: self.retry.get().and_then(|retry| retry.spent),
            started: now,
        };
        // Consume only the old obligation. A notification during the getter
        // must remain pending after this observation has been committed.
        pending.store(false, Ordering::Release);
        let Sample { values, complete } = read()?.into();
        self.value.replace(Some(values.clone()));
        reading.committed = true;
        reading.incomplete = !complete;
        Ok(values)
    }
}

impl Drop for PreferenceSource {
    fn drop(&mut self) {
        self.invalidation.accepting.store(false, Ordering::Release);
        // A failed revocation can retain the delegate. Its only capture is
        // ref-counted invalidation state, now inert; it never owns UI state.
        for (event, token) in self.observers.drain(..) {
            if let Err(error) = event.revoke(&self.ui, token) {
                contain_owner_callback(
                    || tracing::warn!(%error, ?event, "preference observer revocation failed"),
                );
            }
        }
    }
}

/// A `UISettings` change event the source observes: each only marks the
/// snapshot stale and wakes the owner, which reads every value again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UiSettingsEvent {
    TextScale,
    Animations,
    /// The accent and the other UI colours.
    Colors,
    /// "Always show scrollbars" (Windows 10 1809).
    AutoHideScrollBars,
}

impl UiSettingsEvent {
    const ALL: [Self; 4] = [
        Self::TextScale,
        Self::Animations,
        Self::Colors,
        Self::AutoHideScrollBars,
    ];

    /// Register this event's delegate, which notifies `invalidation`; its
    /// token, or `None` where this Windows has no such event.
    fn observe(
        self,
        ui: &UISettings,
        invalidation: &Arc<Invalidation>,
    ) -> Result<Option<i64>, PlatformError> {
        let invalidation = Arc::clone(invalidation);
        let registered = match self {
            Self::TextScale => ui.TextScaleFactorChanged(&TypedEventHandler::new(move |_, _| {
                notify_from_os("the text-scale delegate", &invalidation)
            })),
            Self::Animations => {
                ui.AnimationsEnabledChanged(&TypedEventHandler::new(move |_, _| {
                    notify_from_os("the animations delegate", &invalidation)
                }))
            }
            Self::Colors => ui.ColorValuesChanged(&TypedEventHandler::new(move |_, _| {
                notify_from_os("the colour-values delegate", &invalidation)
            })),
            Self::AutoHideScrollBars => {
                ui.AutoHideScrollBarsChanged(&TypedEventHandler::new(move |_, _| {
                    notify_from_os("the scroll-bar delegate", &invalidation)
                }))
            }
        };
        unless_missing(registered)
    }

    fn revoke(self, ui: &UISettings, token: i64) -> windows::core::Result<()> {
        match self {
            Self::TextScale => ui.RemoveTextScaleFactorChanged(token),
            Self::Animations => ui.RemoveAnimationsEnabledChanged(token),
            Self::Colors => ui.RemoveColorValuesChanged(token),
            Self::AutoHideScrollBars => ui.RemoveAutoHideScrollBarsChanged(token),
        }
    }
}

/// One successful WinRT entry, independent of the platform's outer COM entry.
struct WinRtEntry(PhantomData<Rc<()>>);

impl WinRtEntry {
    fn enter() -> windows::core::Result<Self> {
        // SAFETY: the platform gate admitted its STA owner. Only a successful
        // call (including S_FALSE) creates the guard that balances this entry.
        unsafe { RoInitialize(RO_INIT_SINGLETHREADED)? };
        Ok(Self(PhantomData))
    }
}

impl Drop for WinRtEntry {
    fn drop(&mut self) {
        // SAFETY: this !Send guard balances exactly one entry on this thread;
        // PreferenceSource declares it after the interfaces so they retire first.
        unsafe { RoUninitialize() };
    }
}

fn native_error(error: windows::core::Error) -> PlatformError {
    PlatformError::Preferences {
        message: error.to_string(),
    }
}

fn wheel_count(action: SYSTEM_PARAMETERS_INFO_ACTION) -> Result<u32, PlatformError> {
    let mut count = 0_u32;
    // SAFETY: both callers use SPI getters that write exactly one UINT into
    // the live output local. No update flags or input buffer are supplied.
    unsafe {
        SystemParametersInfoW(
            action,
            0,
            Some((&raw mut count).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    }
    .map_err(native_error)?;
    Ok(count)
}

fn vertical_wheel(count: u32) -> WheelStep {
    // WinUser.h defines WHEEL_PAGESCROLL as UINT_MAX for vertical lines only;
    // SPI_GETWHEELSCROLLCHARS returns a character count. windows-rs 0.62.2
    // does not emit that macro as a binding.
    if count == u32::MAX {
        WheelStep::Page
    } else {
        WheelStep::Lines(count)
    }
}

/// The wheel preferences of one native read: vertical line count and horizontal
/// character count, kept as the separate units Windows reports them in.
fn wheel_preferences(lines: u32, characters: u32) -> WheelPreferences {
    WheelPreferences::default()
        .with_vertical(vertical_wheel(lines))
        .with_horizontal_characters(characters)
}

/// A `SM_CXDRAG`/`SM_CYDRAG` metric as a drag half-extent.
///
/// These already denote displacement on each side, unlike the full
/// double-click rectangle. Their documented negative form has the same
/// magnitude; widen before absolute value and refuse an unrepresentable size.
fn drag_extent(metric: i32) -> Result<i32, PreferenceQueryError> {
    i32::try_from(i64::from(metric).abs())
        .map_err(|_| flui_platform::InvalidPreference::GestureArea.into())
}

/// Query mouse metrics at the consuming coordinate context, without subscribing.
fn mouse_geometry(dpi: u32) -> Result<NativeMouseGeometry, PreferenceQueryError> {
    let ratio = DevicePixelRatio::try_new(f64::from(dpi) / 96.0).ok_or(
        PreferenceQueryError::Unsupported(flui_platform::Unsupported::new(
            flui_platform::UnsupportedReason::Unavailable,
        )),
    )?;
    let metric = |index: SYSTEM_METRICS_INDEX| -> Result<i32, PreferenceQueryError> {
        // SAFETY: these scalar getters take no pointers. Clear the thread's error
        // slot so a legitimate zero metric cannot inherit an older API failure.
        let (value, error) = unsafe {
            SetLastError(WIN32_ERROR(0));
            let value = GetSystemMetricsForDpi(index, dpi);
            (value, GetLastError())
        };
        if value == 0 && error.0 != 0 {
            return Err(PreferenceQueryError::failed(
                windows::core::Error::from(error).to_string(),
            ));
        }
        Ok(value)
    };
    let double_click = Size::new(metric(SM_CXDOUBLECLK)?, metric(SM_CYDOUBLECLK)?);
    let drag = Size::new(
        drag_extent(metric(SM_CXDRAG)?)?,
        drag_extent(metric(SM_CYDRAG)?)?,
    );
    Ok(NativeMouseGeometry::try_new(double_click, drag, ratio)?)
}

/// One read of every value, each getter on its own: a getter that fails
/// leaves its value as `previous` accepted it (none on a cold source) and
/// marks the read incomplete, so the cache retries it while the others
/// stand.
fn sample(
    ui: &UISettings,
    accessibility: &AccessibilitySettings,
    previous: Option<&SystemPreferences>,
) -> Sample {
    let mut sample = Sample {
        values: previous.cloned().unwrap_or_default(),
        complete: true,
    };
    sample.take(
        ui.TextScaleFactor().map_err(native_error),
        |values, scale| values.with_text_scale(scale).map_err(refused),
    );
    sample.take(
        ui.AnimationsEnabled().map_err(native_error),
        |values, enabled| {
            Ok(values.with_motion(if enabled {
                MotionPreference::NoPreference
            } else {
                MotionPreference::Reduce
            }))
        },
    );
    sample.take(
        accessibility.HighContrast().map_err(native_error),
        |values, high| Ok(values.with_high_contrast(high)),
    );
    sample.take(languages::sample(), |values, locales| {
        Ok(values
            .with_regional(regional::sample(locales.first()))
            .with_locales(locales))
    });
    // This window-independent reading detects changes. Exact presentation
    // metrics are queried separately at its actual DPI.
    sample.take(
        mouse_gestures(96).map_err(|error| PlatformError::Preferences {
            message: error.to_string(),
        }),
        |values, gestures| Ok(values.with_gestures(gestures)),
    );
    sample.take(
        wheel_count(SPI_GETWHEELSCROLLLINES)
            .and_then(|lines| Ok((lines, wheel_count(SPI_GETWHEELSCROLLCHARS)?))),
        |values, (lines, characters)| Ok(values.with_wheel(wheel_preferences(lines, characters))),
    );
    sample.take(
        unless_missing(ui.GetColorValue(UIColorType::Accent)),
        |values, accent| Ok(values.with_accent_color(accent.map(accent_color))),
    );
    sample.take(
        unless_missing(ui.AutoHideScrollBars()),
        |values, auto_hide| {
            Ok(values.with_scroll_bars(
                auto_hide.map_or_else(ScrollBarPreferences::default, scroll_bars),
            ))
        },
    );
    sample
}

impl Sample {
    /// Record one getter's answer through `apply`; a getter that failed, or
    /// an answer the snapshot refuses, keeps the value already held and makes
    /// the read incomplete.
    fn take<T>(
        &mut self,
        read: Result<T, PlatformError>,
        apply: impl FnOnce(SystemPreferences, T) -> Result<SystemPreferences, PlatformError>,
    ) {
        match read.and_then(|answer| apply(self.values.clone(), answer)) {
            Ok(values) => self.values = values,
            Err(_) => self.complete = false,
        }
    }
}

fn refused(error: flui_platform::InvalidPreference) -> PlatformError {
    PlatformError::Preferences {
        message: error.to_string(),
    }
}

/// The OS's call of a `UISettings` delegate named `name`: one notification of
/// `invalidation` inside the OS-entry boundary.
fn notify_from_os(name: &'static str, invalidation: &Invalidation) -> windows::core::Result<()> {
    os_entry(name, (), AfterPanic::Answer(|()| Ok(())), || {
        invalidation.notify();
        Ok(())
    })
}

/// `UISettings`' accent, an 8-bit sRGB colour with its alpha.
fn accent_color(color: windows::UI::Color) -> SrgbColor {
    SrgbColor::from_rgba8(color.R, color.G, color.B, color.A)
}

/// "Always show scrollbars" off lets scroll bars collapse while the content
/// rests; on keeps them shown. Windows has no setting for a click in the
/// track, which pages (Shift-click jumps), so that stays unobserved.
fn scroll_bars(auto_hide: bool) -> ScrollBarPreferences {
    ScrollBarPreferences::default().with_visibility(if auto_hide {
        ScrollBarVisibility::WhenScrolling
    } else {
        ScrollBarVisibility::Always
    })
}

/// A getter's value, or `None` where this Windows lacks the interface that
/// carries it (an older `IUISettings` revision): no observation, not a
/// failed read.
fn unless_missing<T>(read: windows::core::Result<T>) -> Result<Option<T>, PlatformError> {
    match read {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.code() == E_NOINTERFACE => Ok(None),
        Err(error) => Err(native_error(error)),
    }
}
