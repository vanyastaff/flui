//! The Windows host's system preferences: WinRT's `UISettings` and
//! `AccessibilitySettings` and the Win32 system metrics and parameters,
//! sampled on the owner's STA independently of any window.
//!
//! The owner keeps the source (`OwnerPreferences`): its broadcast receiver
//! tells it of setting changes, and a failed read is retried on a
//! paced owner wake. The winit host on Windows keeps the same source.

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
    SystemPreferences, WheelPreferences, WheelStep,
};
use windows::Foundation::TypedEventHandler;
use windows::UI::ViewManagement::{AccessibilitySettings, UISettings};
use windows::Win32::Foundation::{
    GetLastError, HWND, LPARAM, LRESULT, SetLastError, WIN32_ERROR, WPARAM,
};
use windows::Win32::System::WinRT::{
    RO_INIT_SINGLETHREADED, RoActivateInstance, RoInitialize, RoUninitialize,
};
use windows::Win32::UI::HiDpi::GetSystemMetricsForDpi;
use windows::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime;
use windows::Win32::UI::WindowsAndMessaging::{
    SM_CXDOUBLECLK, SM_CXDRAG, SM_CYDOUBLECLK, SM_CYDRAG, SPI_GETWHEELSCROLLCHARS,
    SPI_GETWHEELSCROLLLINES, SYSTEM_METRICS_INDEX, SYSTEM_PARAMETERS_INFO_ACTION,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW, WM_SETTINGCHANGE,
};
use windows::core::{Interface, RuntimeName};

use crate::PlatformError;
use crate::owner::sources::SystemPreferenceSource;
use crate::owner::windows::broadcast::Subscriber;
use crate::owner::windows::control::OwnerPreferences;
use crate::owner::{panic_boundary::contain_owner_callback, signal::OwnerSignal};

mod languages;
#[cfg(test)]
mod retry_tests;
#[cfg(test)]
mod tests;

const RETRY_INTERVAL: Duration = Duration::from_millis(100);

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
        if message == WM_SETTINGCHANGE {
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
    text_token: Option<i64>,
    motion_token: Option<i64>,
    cache: SampleCache,
    _entry: WinRtEntry,
}

impl std::fmt::Debug for PreferenceSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreferenceSource").finish_non_exhaustive()
    }
}

/// The winit host on Windows starts the same source as it resumes and samples
/// it on its owner turns.
#[cfg(feature = "winit-backend")]
mod winit {
    use std::rc::Rc;
    use std::sync::Arc;

    use super::PreferenceSource;
    use crate::PlatformError;
    use crate::owner::signal::OwnerSignal;
    use crate::owner::windows::control::OwnerPreferences;
    use crate::owner::winit::PreferenceStart;

    /// The system's settings as the winit loop starts them on its owner
    /// thread.
    #[derive(Debug)]
    pub(crate) struct WinitSettings;

    impl PreferenceStart for WinitSettings {
        fn start(
            &self,
            signal: &Arc<OwnerSignal>,
        ) -> Result<Rc<dyn OwnerPreferences>, PlatformError> {
            Ok(Rc::new(PreferenceSource::new(signal)?))
        }
    }
}

#[cfg(feature = "winit-backend")]
pub(crate) use winit::WinitSettings;

impl SystemPreferenceSource for PreferenceSource {
    fn sample(&self) -> Result<SystemPreferences, PlatformError> {
        self.sample_with(|| sample(&self.ui, &self.accessibility))
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
    pub(crate) fn new(signal: &Arc<OwnerSignal>) -> Result<Self, PlatformError> {
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
            text_token: None,
            motion_token: None,
            cache: SampleCache::default(),
            _entry: entry,
        };
        let text = Arc::clone(&source.invalidation);
        source.text_token = Some(
            source
                .ui
                .TextScaleFactorChanged(&TypedEventHandler::new(move |_, _| {
                    contain_owner_callback(|| text.notify());
                    Ok(())
                }))
                .map_err(native_error)?,
        );
        let motion = Arc::clone(&source.invalidation);
        source.motion_token = Some(
            source
                .ui
                .AnimationsEnabledChanged(&TypedEventHandler::new(move |_, _| {
                    contain_owner_callback(|| motion.notify());
                    Ok(())
                }))
                .map_err(native_error)?,
        );
        Ok(source)
    }

    #[cfg(test)]
    fn invalidate_for_retry_test(&self) {
        self.invalidation.pending.store(true, Ordering::Release);
    }

    /// One read through the source's cache, with `read` standing for the
    /// system's answer.
    pub(crate) fn sample_with(
        &self,
        read: impl FnOnce() -> Result<SystemPreferences, PlatformError>,
    ) -> Result<SystemPreferences, PlatformError> {
        self.cache.read(&self.invalidation.pending, read)
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
}

struct SampleAttempt<'a> {
    reading: &'a Cell<bool>,
    pending: &'a AtomicBool,
    committed: bool,
    retry: &'a Cell<Option<Retry>>,
    started: web_time::Instant,
}

impl Drop for SampleAttempt<'_> {
    fn drop(&mut self) {
        if !self.committed {
            self.pending.store(true, Ordering::Release);
        }
        if self.pending.load(Ordering::Acquire) {
            let next = self.started + RETRY_INTERVAL;
            self.retry.set(Some(Retry {
                read_after: if self.committed { self.started } else { next },
                wake_at: next,
            }));
        } else {
            self.retry.set(None);
        }
        self.reading.set(false);
    }
}

impl SampleCache {
    fn read(
        &self,
        pending: &AtomicBool,
        read: impl FnOnce() -> Result<SystemPreferences, PlatformError>,
    ) -> Result<SystemPreferences, PlatformError> {
        self.read_at(pending, web_time::Instant::now(), read)
    }

    fn read_at(
        &self,
        pending: &AtomicBool,
        now: web_time::Instant,
        read: impl FnOnce() -> Result<SystemPreferences, PlatformError>,
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
            retry: &self.retry,
            started: now,
        };
        // Consume only the old obligation. A notification during the getter
        // must remain pending after this observation has been committed.
        pending.store(false, Ordering::Release);
        let value = read()?;
        self.value.replace(Some(value.clone()));
        reading.committed = true;
        Ok(value)
    }
}

impl Drop for PreferenceSource {
    fn drop(&mut self) {
        self.invalidation.accepting.store(false, Ordering::Release);
        // A failed revocation can retain the delegate. Its only capture is
        // ref-counted invalidation state, now inert; it never owns UI state.
        for result in [
            self.text_token
                .take()
                .map(|token| self.ui.RemoveTextScaleFactorChanged(token)),
            self.motion_token
                .take()
                .map(|token| self.ui.RemoveAnimationsEnabledChanged(token)),
        ]
        .into_iter()
        .flatten()
        {
            if let Err(error) = result {
                contain_owner_callback(
                    || tracing::warn!(%error, "preference observer revocation failed"),
                );
            }
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
pub(crate) fn mouse_geometry(dpi: u32) -> Result<NativeMouseGeometry, PreferenceQueryError> {
    let ratio =
        DevicePixelRatio::new(f64::from(dpi) / 96.0).ok_or(PreferenceQueryError::Unavailable)?;
    let metric = |index: SYSTEM_METRICS_INDEX| -> Result<i32, PreferenceQueryError> {
        // SAFETY: these scalar getters take no pointers. Clear the thread's error
        // slot so a legitimate zero metric cannot inherit an older API failure.
        let (value, error) = unsafe {
            SetLastError(WIN32_ERROR(0));
            let value = GetSystemMetricsForDpi(index, dpi);
            (value, GetLastError())
        };
        if value == 0 && error.0 != 0 {
            return Err(PreferenceQueryError::Native {
                message: windows::core::Error::from(error).to_string(),
            });
        }
        Ok(value)
    };
    let double_click = Size::new(metric(SM_CXDOUBLECLK)?, metric(SM_CYDOUBLECLK)?);
    let drag = Size::new(
        drag_extent(metric(SM_CXDRAG)?)?,
        drag_extent(metric(SM_CYDRAG)?)?,
    );
    Ok(NativeMouseGeometry::new(double_click, drag, ratio)?)
}

fn sample(
    ui: &UISettings,
    accessibility: &AccessibilitySettings,
) -> Result<SystemPreferences, PlatformError> {
    let text_scale = ui.TextScaleFactor().map_err(native_error)?;
    let motion = if ui.AnimationsEnabled().map_err(native_error)? {
        MotionPreference::NoPreference
    } else {
        MotionPreference::Reduce
    };
    let high_contrast = accessibility.HighContrast().map_err(native_error)?;
    // SAFETY: this getter takes no pointers and only reads the system setting.
    let double_click = unsafe { GetDoubleClickTime() };
    let snapshot = SystemPreferences::default()
        .with_text_scale(text_scale)
        .map_err(|error| PlatformError::Preferences {
            message: error.to_string(),
        })?
        .with_motion(motion)
        .with_high_contrast(high_contrast)
        .with_locales(languages::sample()?)
        .with_gestures(
            GesturePreferences::default()
                .with_double_click_interval(Duration::from_millis(u64::from(double_click)))
                // This window-independent reading detects changes. Exact
                // presentation metrics are queried separately at its actual DPI.
                .with_native_mouse_geometry(mouse_geometry(96).map_err(|error| {
                    PlatformError::Preferences {
                        message: error.to_string(),
                    }
                })?),
        )
        .with_wheel(wheel_preferences(
            wheel_count(SPI_GETWHEELSCROLLLINES)?,
            wheel_count(SPI_GETWHEELSCROLLCHARS)?,
        ));
    Ok(snapshot)
}
