//! The system's memory-trim level for this process, read on demand.
//!
//! `android-activity` does not deliver `onTrimMemory`: it needs a Java
//! `ComponentCallbacks2`, and the backend ships no Java. The same level is kept
//! in `ActivityManager.RunningAppProcessInfo.lastTrimLevel`, which
//! `ActivityManager.getMyMemoryState` fills in for the calling process (API 16).
//! The backend reads it when an Activity callback that can have moved it
//! arrives (see `shared::android_lifecycle`), never on a timer.

use android_activity::AndroidApp;
use jni::{jni_sig, jni_str, objects::JValue};

/// `lastTrimLevel` of this process, `None` when the system could not be asked.
pub(super) fn last_trim_level(app: &AndroidApp) -> Option<i32> {
    let level = super::preferences::with_activity(app, |env, _activity| {
        let info = env.new_object(
            jni_str!("android/app/ActivityManager$RunningAppProcessInfo"),
            jni_sig!("()V"),
            &[],
        )?;
        env.call_static_method(
            jni_str!("android/app/ActivityManager"),
            jni_str!("getMyMemoryState"),
            jni_sig!("(Landroid/app/ActivityManager$RunningAppProcessInfo;)V"),
            &[JValue::Object(&info)],
        )?;
        env.get_field(&info, jni_str!("lastTrimLevel"), jni_sig!("I"))?
            .i()
    });
    match level {
        Ok(level) => Some(level),
        Err(error) => {
            tracing::warn!(%error, "Android: the memory trim level could not be read");
            None
        }
    }
}
