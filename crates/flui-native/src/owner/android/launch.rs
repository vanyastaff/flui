//! The launching intent's string extras, which `am start --es` hands the Activity: Android's
//! launch values (ADR-0288 §7).
//!
//! Read through system classes alone (`Activity.getIntent`, `Intent.getStringExtra`), on
//! whatever thread asks, attached to the VM for the call. An extra that is missing, empty or
//! not a string, and a read the bridge fails, read as absent: an application never fails to
//! start over a launch value.

use android_activity::AndroidApp;
use flui_platform::LaunchKey;
use jni::objects::{JObject, JString};
use jni::refs::Global;
use jni::{JValue, JavaVM, jni_sig, jni_str};

/// The launching intent's string extra named `key`.
#[expect(
    unsafe_code,
    reason = "AndroidApp lends live VM/Activity references; no owned Activity reference is constructed"
)]
pub(crate) fn string_extra(app: &AndroidApp, key: LaunchKey) -> Option<String> {
    // SAFETY: AndroidApp retains the live Android VM for as long as it lives.
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
    let read = vm.attach_current_thread(|env| -> jni::errors::Result<Option<String>> {
        let result = (|| {
            let raw = app.activity_as_ptr() as jni::sys::jobject;
            // SAFETY: borrow AndroidApp's global Activity; the temporary wrapper never deletes it.
            let activity = unsafe { env.as_cast_raw::<Global<JObject<'_>>>(&raw)? };
            let intent = env
                .call_method(
                    activity.as_ref(),
                    jni_str!("getIntent"),
                    jni_sig!("()Landroid/content/Intent;"),
                    &[],
                )?
                .l()?;
            if intent.is_null() {
                return Ok(None);
            }
            let name = env.new_string(key.as_str())?;
            let value = env
                .call_method(
                    &intent,
                    jni_str!("getStringExtra"),
                    jni_sig!("(Ljava/lang/String;)Ljava/lang/String;"),
                    &[JValue::Object(&name)],
                )?
                .l()?;
            if value.is_null() {
                return Ok(None);
            }
            env.cast_local::<JString<'_>>(value)?
                .try_to_string(env)
                .map(Some)
        })();
        if result.is_err() {
            env.exception_clear();
        }
        result
    });
    match read {
        Ok(value) => value.filter(|value| !value.is_empty()),
        Err(error) => {
            tracing::warn!(%key, %error, "the launching intent's extra could not be read");
            None
        }
    }
}
