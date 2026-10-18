//! The Android display source (ADR-0236).
//!
//! One JNI sample reads every display (`DisplayManager.getDisplays`): its real
//! size and density, the rate of its current `Display.Mode`, the highest rate
//! among its supported modes, and the `Display.getDisplayId` that keys its
//! identity. The display the Activity is on is read the same way
//! (`Activity.getDisplay` from API 30, `WindowManager.getDefaultDisplay`
//! below).
//!
//! # When the sample is read again
//!
//! `display_snapshot()` runs on every owner wake, and a JNI round trip is not
//! free, so the last snapshot is kept and re-read only after something that can
//! change it announced itself:
//!
//! - `ConfigChanged` (rotation, density, size) and `Resume` (the Activity may
//!   have moved to another display while it was away);
//! - the choreographer's refresh-rate callback, registered on API 30 and later:
//!   an adaptive-rate switch (60 Hz to 120 Hz) raises no configuration change.
//!
//! Below API 30 the rate is read at those two events and nowhere else; there is
//! no polling. The callback runs in the application's own `poll_events` and only
//! marks the snapshot stale and wakes the owner, which does the read.

use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::NonNull;
use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, Ordering},
};
use std::thread::ThreadId;

use android_activity::AndroidApp;
use flui_platform_api::{DisplayId, DisplayKey, DisplaySnapshot, FrameRateHint, RefreshPeriod};
use jni::{
    Env, JValue, jni_sig, jni_str,
    objects::{JObject, JObjectArray, JString},
};
use parking_lot::Mutex;

use super::super::android_os_version::{
    AndroidApi, NdkFn, RefreshRateWatch, SetFrameRateFn, SymbolSource, set_frame_rate,
};
use crate::PreferenceQueryError;
use crate::error::PlatformError;
use crate::shared::android_display::{RawDisplay, max_refresh_hz, requested_frame_rate};
use crate::shared::display_source::DisplaySource;
use crate::shared::owner_signal::OwnerSignal;

/// `ANATIVEWINDOW_FRAME_RATE_COMPATIBILITY_DEFAULT`.
const FRAME_RATE_COMPATIBILITY_DEFAULT: i8 = 0;

/// Whether the snapshot is out of date, raised from the loop thread by the
/// events that can change a display.
struct Staleness {
    stale: AtomicBool,
    owner: Weak<OwnerSignal>,
}

impl Staleness {
    /// Mark the snapshot out of date and ask the owner for a turn to read it.
    fn raise(&self) {
        self.stale.store(true, Ordering::Release);
        if let Some(owner) = self.owner.upgrade() {
            // A failed wake leaves the owner's own retry debt.
            let _ = owner.wake();
        }
    }

    /// Whether the snapshot is out of date; reading clears it.
    fn take(&self) -> bool {
        self.stale.swap(false, Ordering::AcqRel)
    }

    /// Keep the snapshot out of date without asking for a turn: a read failed
    /// and the next owner wake tries again.
    fn keep(&self) {
        self.stale.store(true, Ordering::Release);
    }
}

/// What the last sample said about the display the Activity is on.
#[derive(Clone, Copy)]
struct ActivityDisplay {
    id: DisplayId,
    period: Option<RefreshPeriod>,
    max_hz: Option<f32>,
}

#[derive(Default)]
struct Current {
    snapshot: Option<DisplaySnapshot>,
    activity: Option<ActivityDisplay>,
}

/// The state of one registered refresh-rate callback.
///
/// Dropping it takes the callback down *before* the data it points at is freed,
/// so a late call cannot reach freed memory. It is dropped on the thread that
/// registered it, whose looper delivers the callback, by
/// [`AndroidDisplays::stop_watching`].
struct Registration {
    watch: RefreshRateWatch,
    choreographer: NonNull<c_void>,
    /// `Arc::into_raw` of the staleness the callback marks.
    data: *const Staleness,
    thread: ThreadId,
}

// SAFETY: the raw pointers are used only in `Drop`, which unregisters on the
// thread that registered (checked against `thread`) and otherwise leaks the
// data instead of freeing what the callback may still reach; `data` points at
// an `Arc<Staleness>` whose referent is `Send + Sync`.
#[expect(
    unsafe_code,
    reason = "a registration crosses threads inside the platform"
)]
unsafe impl Send for Registration {}

impl Drop for Registration {
    #[expect(
        unsafe_code,
        reason = "unregisters the choreographer callback, then frees its data"
    )]
    fn drop(&mut self) {
        if std::thread::current().id() != self.thread {
            // The callback may be running on the looper thread right now; only
            // that thread can take it down safely. Leaking the data keeps a
            // late call valid.
            tracing::warn!("refresh-rate callback outlived its thread; leaking its data");
            return;
        }
        // SAFETY: `choreographer` is this thread's instance, which lives as long
        // as the thread; the callback and data are the pair registered in
        // `watch_refresh_rate`, and `unregister` is the NDK function of that name.
        unsafe {
            (self.watch.unregister.get())(
                self.choreographer.as_ptr(),
                on_refresh_rate,
                self.data.cast_mut().cast(),
            );
        }
        // SAFETY: `data` came from `Arc::into_raw` and is released once, after
        // the callback was unregistered, so nothing can reach it any more.
        drop(unsafe { Arc::from_raw(self.data) });
    }
}

/// The choreographer's refresh-rate callback: the display's rate changed.
///
/// Runs on the thread that registered it, from its looper.
#[expect(
    unsafe_code,
    reason = "an extern \"C\" callback receiving the registered data"
)]
unsafe extern "C" fn on_refresh_rate(period_nanos: i64, data: *mut c_void) {
    // SAFETY: `data` is the `Arc<Staleness>` pointer registered with this
    // callback, which stays allocated until the callback is unregistered.
    let staleness = unsafe { &*data.cast::<Staleness>() };
    // A panic must not unwind into the NDK.
    let _ = catch_unwind(AssertUnwindSafe(|| {
        tracing::debug!(period_nanos, "Android: the display refresh rate changed");
        staleness.raise();
    }));
}

/// The displays of the device, sampled through JNI and kept between samples.
pub(super) struct AndroidDisplays {
    source: DisplaySource,
    api: AndroidApi,
    library: Arc<dyn SymbolSource>,
    set_frame_rate: Option<NdkFn<SetFrameRateFn>>,
    staleness: Arc<Staleness>,
    current: Mutex<Current>,
    watch: Mutex<Option<Registration>>,
}

impl AndroidDisplays {
    pub(super) fn new(
        api: AndroidApi,
        library: Arc<dyn SymbolSource>,
        owner: Weak<OwnerSignal>,
    ) -> Self {
        Self {
            source: DisplaySource::new(),
            api,
            set_frame_rate: set_frame_rate(&library, api),
            library,
            staleness: Arc::new(Staleness {
                stale: AtomicBool::new(true),
                owner,
            }),
            current: Mutex::new(Current::default()),
            watch: Mutex::new(None),
        }
    }

    /// The display state may have changed (configuration change, resume): read
    /// it again at the next owner turn, which this asks for.
    pub(super) fn invalidate(&self) {
        self.staleness.raise();
    }

    /// The snapshot: the kept one while nothing announced a change, a fresh
    /// sample otherwise.
    pub(super) fn snapshot(&self, app: &AndroidApp) -> Result<DisplaySnapshot, PlatformError> {
        if !self.staleness.take()
            && let Some(snapshot) = self.current.lock().snapshot.clone()
        {
            return Ok(snapshot);
        }
        let sample = match sample(app, self.api) {
            Ok(sample) => sample,
            Err(error) => {
                self.staleness.keep();
                return Err(PlatformError::Displays {
                    message: error.to_string(),
                });
            }
        };
        let mut infos = Vec::with_capacity(sample.displays.len());
        let mut activity = None;
        for raw in &sample.displays {
            let Some(id) = self.source.mint(&DisplayKey::AndroidId(raw.display_id)) else {
                continue;
            };
            let Some(info) = raw.info(id) else {
                continue;
            };
            if raw.display_id == sample.activity_display_id {
                activity = Some(ActivityDisplay {
                    id,
                    period: info.refresh_period,
                    max_hz: raw.max_refresh_hz,
                });
            }
            infos.push(info);
        }
        let snapshot = self
            .source
            .publish(infos)
            .inspect_err(|_| self.staleness.keep())?;
        let previous = std::mem::replace(
            &mut *self.current.lock(),
            Current {
                snapshot: Some(snapshot.clone()),
                activity,
            },
        );
        if previous.snapshot.as_ref().map(DisplaySnapshot::revision) != Some(snapshot.revision()) {
            log_snapshot(&snapshot, activity);
        }
        Ok(snapshot)
    }

    /// The display the Activity was on at the last sample.
    pub(super) fn activity_display(&self) -> Option<DisplayId> {
        self.current.lock().activity.map(|activity| activity.id)
    }

    /// The refresh period of that display at the last sample.
    pub(super) fn activity_period(&self) -> Option<RefreshPeriod> {
        self.current
            .lock()
            .activity
            .and_then(|activity| activity.period)
    }

    /// Ask the system for the refresh rate `hint` names on `window`, if this
    /// system has the request (API 30).
    #[expect(unsafe_code, reason = "calls ANativeWindow_setFrameRate")]
    pub(super) fn request_frame_rate(&self, window: NonNull<c_void>, hint: FrameRateHint) {
        let Some(set_frame_rate) = &self.set_frame_rate else {
            return;
        };
        let max_hz = self
            .current
            .lock()
            .activity
            .and_then(|activity| activity.max_hz);
        let Some(rate) = requested_frame_rate(hint, max_hz) else {
            tracing::debug!(
                ?hint,
                "Android: the display's highest rate is unknown; no request"
            );
            return;
        };
        // SAFETY: `window` is the `ANativeWindow` the caller holds a reference
        // to for the duration of this call, and the function takes the
        // `ANativeWindow*`, a rate and a compatibility value.
        let status = unsafe {
            (set_frame_rate.get())(window.as_ptr(), rate, FRAME_RATE_COMPATIBILITY_DEFAULT)
        };
        if status == 0 {
            tracing::info!(
                rate,
                high = matches!(hint, FrameRateHint::High),
                "Android: frame rate requested"
            );
        } else {
            tracing::warn!(
                status,
                rate,
                "Android: ANativeWindow_setFrameRate was refused"
            );
        }
    }

    /// Register for the choreographer's refresh-rate callback, on API 30 and
    /// later. Call on the thread whose looper runs the app (the callback is
    /// delivered there).
    #[expect(unsafe_code, reason = "registers a choreographer callback")]
    pub(super) fn watch_refresh_rate(&self) {
        let mut watch = self.watch.lock();
        if watch.is_some() {
            return;
        }
        let Some(functions) = RefreshRateWatch::resolve(&self.library, self.api) else {
            tracing::debug!(
                api = self.api.level(),
                "Android: no refresh-rate callback; the rate is read on configuration changes and resume"
            );
            return;
        };
        // SAFETY: `AChoreographer_getInstance` takes nothing and returns this
        // thread's instance, or null without a looper.
        let choreographer = unsafe { (functions.instance.get())() };
        let Some(choreographer) = NonNull::new(choreographer) else {
            tracing::warn!(
                "Android: no choreographer on the app thread; refresh rate changes are not watched"
            );
            return;
        };
        let data = Arc::into_raw(Arc::clone(&self.staleness));
        // SAFETY: the callback and data are the pair `Registration::drop`
        // unregisters before freeing `data`; the choreographer is this
        // thread's.
        unsafe {
            (functions.register.get())(
                choreographer.as_ptr(),
                on_refresh_rate,
                data.cast_mut().cast(),
            );
        }
        *watch = Some(Registration {
            watch: functions,
            choreographer,
            data,
            thread: std::thread::current().id(),
        });
    }

    /// Take the refresh-rate callback down. Call on the thread that registered
    /// it, before the platform goes away.
    pub(super) fn stop_watching(&self) {
        let registration = self.watch.lock().take();
        drop(registration);
    }
}

/// One `info` line per display each time the snapshot changes, which is rare:
/// the way to see on a device what the source read.
fn log_snapshot(snapshot: &DisplaySnapshot, activity: Option<ActivityDisplay>) {
    for info in snapshot.displays() {
        let refresh_period_us = info.refresh_period.map_or(0, |period| {
            u64::try_from(period.get().as_micros()).unwrap_or(u64::MAX)
        });
        let on_activity = activity.is_some_and(|activity| activity.id == info.id);
        tracing::info!(
            revision = snapshot.revision().get(),
            id = info.id.get(),
            name.public = %info.name,
            width = info.bounds.size.width,
            height = info.bounds.size.height,
            scale_factor = info.scale_factor,
            refresh_period_us,
            primary = info.is_primary,
            on_activity,
            "Android display"
        );
    }
}

/// What one JNI read found.
struct Sample {
    displays: Vec<RawDisplay>,
    /// The display the Activity is on.
    activity_display_id: i32,
}

fn sample(app: &AndroidApp, api: AndroidApi) -> Result<Sample, PreferenceQueryError> {
    super::preferences::with_activity(app, |env, activity| {
        env.with_local_frame(64, |env| -> jni::errors::Result<Sample> {
            let activity_display_id = activity_display_id(env, activity, api)?;
            let service = env
                .get_static_field(
                    jni_str!("android/content/Context"),
                    jni_str!("DISPLAY_SERVICE"),
                    jni_sig!("Ljava/lang/String;"),
                )?
                .l()?;
            let manager = env
                .call_method(
                    activity,
                    jni_str!("getSystemService"),
                    jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"),
                    &[JValue::Object(&service)],
                )?
                .l()?;
            let listed = env
                .call_method(
                    &manager,
                    jni_str!("getDisplays"),
                    jni_sig!("()[Landroid/view/Display;"),
                    &[],
                )?
                .l()?;
            let listed = JObjectArray::<JObject<'_>>::cast_local(env, listed)?;
            let count = listed.len(env)?;
            let mut displays = Vec::with_capacity(count);
            for index in 0..count {
                displays.push(env.with_local_frame(
                    32,
                    |env| -> jni::errors::Result<RawDisplay> {
                        let display = listed.get_element(env, index)?;
                        read_display(env, &display)
                    },
                )?);
            }
            Ok(Sample {
                displays,
                activity_display_id,
            })
        })
    })
}

/// `Display.getDisplayId` of the display the Activity is on.
fn activity_display_id(
    env: &mut Env<'_>,
    activity: &JObject<'_>,
    api: AndroidApi,
) -> jni::errors::Result<i32> {
    if api.level() >= 30 {
        match env.call_method(
            activity,
            jni_str!("getDisplay"),
            jni_sig!("()Landroid/view/Display;"),
            &[],
        ) {
            Ok(display) => {
                let display = display.l()?;
                if !display.is_null() {
                    return env
                        .call_method(&display, jni_str!("getDisplayId"), jni_sig!("()I"), &[])?
                        .i();
                }
            }
            // A context that is not tied to a display refuses; use the window manager.
            Err(_) => env.exception_clear(),
        }
    }
    let windows = env
        .call_method(
            activity,
            jni_str!("getWindowManager"),
            jni_sig!("()Landroid/view/WindowManager;"),
            &[],
        )?
        .l()?;
    let display = env
        .call_method(
            &windows,
            jni_str!("getDefaultDisplay"),
            jni_sig!("()Landroid/view/Display;"),
            &[],
        )?
        .l()?;
    env.call_method(&display, jni_str!("getDisplayId"), jni_sig!("()I"), &[])?
        .i()
}

fn read_display(env: &mut Env<'_>, display: &JObject<'_>) -> jni::errors::Result<RawDisplay> {
    let display_id = env
        .call_method(display, jni_str!("getDisplayId"), jni_sig!("()I"), &[])?
        .i()?;
    let name = env
        .call_method(
            display,
            jni_str!("getName"),
            jni_sig!("()Ljava/lang/String;"),
            &[],
        )?
        .l()?;
    let name = if name.is_null() {
        String::new()
    } else {
        env.cast_local::<JString<'_>>(name)?.try_to_string(env)?
    };

    let metrics = env.new_object(
        jni_str!("android/util/DisplayMetrics"),
        jni_sig!("()V"),
        &[],
    )?;
    env.call_method(
        display,
        jni_str!("getRealMetrics"),
        jni_sig!("(Landroid/util/DisplayMetrics;)V"),
        &[JValue::Object(&metrics)],
    )?;
    let width = env
        .get_field(&metrics, jni_str!("widthPixels"), jni_sig!("I"))?
        .i()?;
    let height = env
        .get_field(&metrics, jni_str!("heightPixels"), jni_sig!("I"))?
        .i()?;
    let density_dpi = env
        .get_field(&metrics, jni_str!("densityDpi"), jni_sig!("I"))?
        .i()?;

    // The rate of the mode the display runs now; `getRefreshRate` answers the
    // same on every system that has modes and stands in when the mode is absent.
    let mode = env
        .call_method(
            display,
            jni_str!("getMode"),
            jni_sig!("()Landroid/view/Display$Mode;"),
            &[],
        )?
        .l()?;
    let mut refresh_hz = if mode.is_null() {
        0.0
    } else {
        env.call_method(&mode, jni_str!("getRefreshRate"), jni_sig!("()F"), &[])?
            .f()?
    };
    if refresh_hz <= 0.0 {
        refresh_hz = env
            .call_method(display, jni_str!("getRefreshRate"), jni_sig!("()F"), &[])?
            .f()?;
    }

    let modes = env
        .call_method(
            display,
            jni_str!("getSupportedModes"),
            jni_sig!("()[Landroid/view/Display$Mode;"),
            &[],
        )?
        .l()?;
    let modes = JObjectArray::<JObject<'_>>::cast_local(env, modes)?;
    let mut rates = Vec::new();
    for index in 0..modes.len(env)? {
        let rate = env.with_local_frame(8, |env| -> jni::errors::Result<f32> {
            let mode = modes.get_element(env, index)?;
            env.call_method(&mode, jni_str!("getRefreshRate"), jni_sig!("()F"), &[])?
                .f()
        })?;
        rates.push(rate);
    }

    Ok(RawDisplay {
        display_id,
        name,
        width,
        height,
        density_dpi,
        refresh_hz,
        max_refresh_hz: max_refresh_hz(rates),
    })
}
