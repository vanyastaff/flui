//! The Android display source (ADR-0236).
//!
//! One JNI sample reads every display (`DisplayManager.getDisplays`): its real
//! size and density, the rate of its current `Display.Mode`, the highest rate
//! among its supported modes, and the `Display.getDisplayId` that keys its
//! identity. The display the Activity is on is read the same way
//! (`Activity.getDisplay` from API 30, `WindowManager.getDefaultDisplay`
//! below). `getDisplays()` also lists virtual displays (a cast or a screen
//! recorder), which appear in the snapshot like any other.
//!
//! A display id is never reused within a boot, so a display that is unplugged
//! and plugged back in comes back under a new id and so a new identity.
//!
//! # When the sample is read again
//!
//! `display_snapshot()` runs on every owner wake, and a JNI round trip is not
//! free, so the shared [`DisplaySource`] keeps the last snapshot and the sample
//! is read again only after something that can change it announced itself:
//!
//! - `ConfigChanged` (rotation, density, size) and `Resume` (the Activity may
//!   have moved to another display while it was away);
//! - the choreographer's refresh-rate callback, registered on API 30 and later:
//!   an adaptive-rate switch (60 Hz to 120 Hz) raises no configuration change;
//! - the Activity's display missing from a sample (it is on a display the
//!   source has not seen yet): one more sample per snapshot revision.
//!
//! Below API 30 the rate is read at the first two events and nowhere else; there
//! is no polling. The callback runs in the application's own `poll_events` and
//! only invalidates the source and wakes the owner, which does the read.

use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::NonNull;
use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, Ordering},
};
use std::thread::ThreadId;

use android_activity::AndroidApp;
use android_activity::ndk::native_window::NativeWindow;
use flui_platform_api::{DisplayId, DisplayKey, DisplaySnapshot, FrameRateHint, RefreshPeriod};
use jni::{
    Env, JValue, jni_sig, jni_str,
    objects::{JObject, JObjectArray, JString},
};
use parking_lot::Mutex;

use crate::PreferenceQueryError;
use crate::error::PlatformError;
use crate::shared::android_display::{RawDisplay, max_refresh_hz, requested_frame_rate};
use crate::shared::android_ndk::{
    AChoreographer, NdkFn, RefreshRateWatch, SetFrameRate, SymbolSource,
};
use crate::shared::display_source::DisplaySource;
use crate::shared::os_version::android::{AndroidApi, ContextDisplay};
use crate::shared::owner_signal::OwnerSignal;

/// `ANATIVEWINDOW_FRAME_RATE_COMPATIBILITY_DEFAULT`.
const FRAME_RATE_COMPATIBILITY_DEFAULT: i8 = 0;

/// What the last sample said about the display the Activity is on.
#[derive(Clone, Copy)]
struct ActivityDisplay {
    id: DisplayId,
    period: Option<RefreshPeriod>,
    max_hz: Option<f32>,
}

/// Marks the source stale and asks the owner for a turn to read it. Shared with
/// the choreographer callback, which holds it through a raw `Arc` pointer.
struct Invalidator {
    source: Arc<DisplaySource>,
    owner: Weak<OwnerSignal>,
}

impl Invalidator {
    fn raise(&self) {
        self.source.invalidate();
        self.wake();
    }

    fn wake(&self) {
        if let Some(owner) = self.owner.upgrade() {
            // A failed wake leaves the owner's own retry debt.
            let _ = owner.wake();
        }
    }
}

/// The state of one registered refresh-rate callback.
///
/// Dropping it takes the callback down *before* the data it points at is freed,
/// so a late call cannot reach freed memory. It is dropped on the thread that
/// registered it, whose looper delivers the callback, by
/// [`AndroidDisplays::stop_watching`].
struct Registration {
    watch: RefreshRateWatch,
    choreographer: NonNull<AChoreographer>,
    /// `Arc::into_raw` of the invalidator the callback raises.
    data: *const Invalidator,
    thread: ThreadId,
}

#[expect(
    unsafe_code,
    reason = "a registration crosses threads inside the platform"
)]
// SAFETY: the raw pointers are used only in `Drop`, which unregisters on the
// thread that registered (checked against `thread`) and otherwise leaks the
// data instead of freeing what the callback may still reach; `data` points at
// an `Arc<Invalidator>` whose referent is `Send + Sync`.
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
    // SAFETY: `data` is the `Arc<Invalidator>` pointer registered with this
    // callback, which stays allocated until the callback is unregistered.
    let invalidator = unsafe { &*data.cast::<Invalidator>() };
    // A panic must not unwind into the NDK.
    let _ = catch_unwind(AssertUnwindSafe(|| {
        tracing::debug!(period_nanos, "Android: the display refresh rate changed");
        invalidator.raise();
    }));
}

/// The displays of the device, sampled through JNI and kept between samples.
pub(super) struct AndroidDisplays {
    source: Arc<DisplaySource>,
    api: AndroidApi,
    library: Arc<dyn SymbolSource>,
    set_frame_rate: Option<NdkFn<SetFrameRate>>,
    invalidator: Arc<Invalidator>,
    activity: Mutex<Option<ActivityDisplay>>,
    /// Set when a sample found another highest rate for the Activity's display,
    /// until the window has asked again with it.
    max_rate_changed: AtomicBool,
    watch: Mutex<Option<Registration>>,
}

impl AndroidDisplays {
    pub(super) fn new(
        api: AndroidApi,
        library: Arc<dyn SymbolSource>,
        owner: Weak<OwnerSignal>,
    ) -> Self {
        let source = Arc::new(DisplaySource::new());
        Self {
            invalidator: Arc::new(Invalidator {
                source: Arc::clone(&source),
                owner,
            }),
            source,
            api,
            set_frame_rate: NdkFn::resolve(&library, api),
            library,
            activity: Mutex::new(None),
            max_rate_changed: AtomicBool::new(false),
            watch: Mutex::new(None),
        }
    }

    /// The display state may have changed (configuration change, resume): read
    /// it again at the next owner turn, which this asks for.
    pub(super) fn invalidate(&self) {
        self.invalidator.raise();
    }

    /// The snapshot: the kept one while nothing announced a change, a fresh
    /// sample otherwise.
    pub(super) fn snapshot(&self, app: &AndroidApp) -> Result<DisplaySnapshot, PlatformError> {
        let before = self.source.published().revision();
        let mut missing_activity = None;
        let snapshot = self.source.snapshot_with(|| {
            let sample = sample(app, self.api).map_err(|error| PlatformError::Displays {
                message: error.to_string(),
            })?;
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
            if activity.is_none() {
                missing_activity = Some(sample.activity_display_id);
            }
            let previous = std::mem::replace(&mut *self.activity.lock(), activity);
            if previous.and_then(|display| display.max_hz)
                != activity.and_then(|display| display.max_hz)
            {
                self.max_rate_changed.store(true, Ordering::Release);
            }
            Ok(infos)
        })?;
        if let Some(display_id) = missing_activity {
            // The Activity is on a display this sample does not list: ask for
            // one more sample per snapshot revision, not one per owner wake.
            // An idle app has no other reason to wake, so the first miss asks
            // the owner for the turn that samples again.
            self.source
                .note_miss_with(u64::from(display_id.unsigned_abs()), || {
                    self.invalidator.wake();
                });
        }
        if snapshot.revision() != before {
            log_snapshot(&snapshot, *self.activity.lock(), before.get() == 0);
        }
        Ok(snapshot)
    }

    /// The display the Activity was on at the last sample.
    pub(super) fn activity_display(&self) -> Option<DisplayId> {
        self.activity.lock().map(|activity| activity.id)
    }

    /// The refresh period of that display at the last sample.
    pub(super) fn activity_period(&self) -> Option<RefreshPeriod> {
        self.activity.lock().and_then(|activity| activity.period)
    }

    /// Whether the highest rate of the Activity's display changed since this was
    /// last asked; reading clears it. A hint the window skipped for want of the
    /// rate is then asked again.
    pub(super) fn take_max_rate_change(&self) -> bool {
        self.max_rate_changed.swap(false, Ordering::AcqRel)
    }

    /// Ask the system for the refresh rate `hint` names on `window`, if this
    /// system has the request (API 30).
    #[expect(unsafe_code, reason = "calls ANativeWindow_setFrameRate")]
    pub(super) fn request_frame_rate(&self, window: &NativeWindow, hint: FrameRateHint) {
        let Some(set_frame_rate) = &self.set_frame_rate else {
            return;
        };
        let max_hz = self.activity.lock().and_then(|activity| activity.max_hz);
        let rate = requested_frame_rate(hint, max_hz);
        // SAFETY: `window` is a live reference to an `ANativeWindow` for the
        // duration of this call, and the function takes the `ANativeWindow*`, a
        // rate and a compatibility value.
        let status = unsafe {
            (set_frame_rate.get())(
                window.ptr().as_ptr(),
                rate,
                FRAME_RATE_COMPATIBILITY_DEFAULT,
            )
        };
        if status == 0 {
            tracing::debug!(
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
                api = %self.api,
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
        let data = Arc::into_raw(Arc::clone(&self.invalidator));
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

/// One line per display each time the snapshot changes, which is rare: the way
/// to see on a device what the source read. The first, at startup, is `info`;
/// later ones are `debug`.
fn log_snapshot(snapshot: &DisplaySnapshot, activity: Option<ActivityDisplay>, first: bool) {
    macro_rules! display_line {
        ($level:expr, $info:expr, $on_activity:expr, $refresh_period_us:expr) => {
            tracing::event!(
                $level,
                revision = snapshot.revision().get(),
                id = $info.id.get(),
                name.public = %$info.name,
                width = $info.bounds.size.width,
                height = $info.bounds.size.height,
                scale_factor = $info.scale_factor,
                refresh_period_us = $refresh_period_us,
                primary = $info.is_primary,
                on_activity = $on_activity,
                "Android display"
            )
        };
    }
    for info in snapshot.displays() {
        let refresh_period_us = info.refresh_period.map_or(0, |period| {
            u64::try_from(period.get().as_micros()).unwrap_or(u64::MAX)
        });
        let on_activity = activity.is_some_and(|activity| activity.id == info.id);
        if first {
            display_line!(tracing::Level::INFO, info, on_activity, refresh_period_us);
        } else {
            display_line!(tracing::Level::DEBUG, info, on_activity, refresh_period_us);
        }
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
    if api.since::<ContextDisplay>().is_some() {
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
