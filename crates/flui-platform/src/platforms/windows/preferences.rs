//! Window-independent native preference sampling on the platform's STA.

use std::cell::{Cell, RefCell};
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use flui_foundation::geometry::{DevicePixelRatio, Size};
use flui_platform_api::{
    GesturePreferences, MotionPreference, NativeMouseGeometry, PreferenceQueryError,
    SystemPreferences, WheelPreferences, WheelStep,
};
use windows::Foundation::TypedEventHandler;
use windows::UI::ViewManagement::{AccessibilitySettings, UISettings};
use windows::Win32::Foundation::{GetLastError, SetLastError, WIN32_ERROR};
use windows::Win32::System::WinRT::{
    RO_INIT_SINGLETHREADED, RoActivateInstance, RoInitialize, RoUninitialize,
};
use windows::Win32::UI::HiDpi::GetSystemMetricsForDpi;
use windows::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime;
use windows::Win32::UI::WindowsAndMessaging::{
    SM_CXDOUBLECLK, SM_CXDRAG, SM_CYDOUBLECLK, SM_CYDRAG, SPI_GETWHEELSCROLLCHARS,
    SPI_GETWHEELSCROLLLINES, SYSTEM_METRICS_INDEX, SYSTEM_PARAMETERS_INFO_ACTION,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
};
use windows::core::{Interface, RuntimeName};

use crate::PlatformError;
use crate::shared::{owner_signal::OwnerSignal, panic_boundary::contain_owner_callback};

mod languages;
mod receiver;
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

/// Native subscriptions and interfaces belong to the owner context, not its
/// Send + Sync platform wrapper. Interfaces and receiver retire before WinRT.
pub(crate) struct PreferenceSource {
    invalidation: Arc<Invalidation>,
    _receiver: receiver::Receiver,
    ui: UISettings,
    accessibility: AccessibilitySettings,
    text_token: Option<i64>,
    motion_token: Option<i64>,
    cache: SampleCache,
    _entry: WinRtEntry,
}

impl PreferenceSource {
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
        let receiver = receiver::Receiver::new(Arc::clone(&invalidation)).map_err(native_error)?;
        let mut source = Self {
            invalidation,
            _receiver: receiver,
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

    pub(crate) fn pending(&self) -> bool {
        self.invalidation.pending.load(Ordering::Acquire)
    }

    #[cfg(test)]
    pub(super) fn invalidate_for_retry_test(&self) {
        self.invalidation.pending.store(true, Ordering::Release);
    }

    #[cfg(test)]
    #[expect(
        clippy::used_underscore_binding,
        reason = "native ingress test addresses the receiver retained only for its lifetime in production"
    )]
    pub(crate) fn send_setting_change_for_test(&self) {
        self._receiver.send_setting_change();
    }

    pub(crate) fn sample(&self) -> Result<SystemPreferences, PlatformError> {
        self.sample_with(|| sample(&self.ui, &self.accessibility))
    }

    pub(crate) fn sample_with(
        &self,
        read: impl FnOnce() -> Result<SystemPreferences, PlatformError>,
    ) -> Result<SystemPreferences, PlatformError> {
        self.cache.read(&self.invalidation.pending, read)
    }

    pub(crate) fn retry_deadline(&self) -> Option<web_time::Instant> {
        (self.invalidation.accepting.load(Ordering::Acquire)
            && self
                .invalidation
                .signal
                .upgrade()
                .is_some_and(|signal| signal.accepting()))
        .then(|| self.cache.retry.get().map(|retry| retry.wake_at))
        .flatten()
    }

    pub(crate) fn retry_if_due(&self) {
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

/// Query mouse metrics at the consuming coordinate context, without subscribing.
pub(super) fn mouse_geometry(dpi: u32) -> Result<NativeMouseGeometry, PreferenceQueryError> {
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
    // SM_CXDRAG/CYDRAG already denote displacement on each side, unlike the
    // full double-click rectangle. Their documented negative form has the same
    // magnitude; widen before absolute value and refuse an unrepresentable size.
    let drag_extent = |index| -> Result<i32, PreferenceQueryError> {
        i32::try_from(i64::from(metric(index)?).abs())
            .map_err(|_| flui_platform_api::InvalidPreference::GestureArea.into())
    };
    let drag = Size::new(drag_extent(SM_CXDRAG)?, drag_extent(SM_CYDRAG)?);
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
                    PlatformError::Preferences { message: error.to_string() }
                })?),
        )
        .with_wheel(
            WheelPreferences::default()
                .with_vertical(vertical_wheel(wheel_count(SPI_GETWHEELSCROLLLINES)?))
                .with_horizontal_characters(wheel_count(SPI_GETWHEELSCROLLCHARS)?),
        );
    Ok(snapshot)
}
