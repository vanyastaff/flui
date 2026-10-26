//! The Activity the loop runs in, reached through JNI.
//!
//! `AndroidApp` lends the running VM and a global reference to the Activity
//! for as long as it lives; a query attaches the calling thread and borrows
//! both. Every feature that asks the Activity something (its configuration,
//! its window, its services) asks through [`with_activity`].

use android_activity::AndroidApp;
use flui_platform::PreferenceQueryError;
use jni::{JavaVM, objects::JObject, refs::Global};

/// Run `query` with this thread attached to the VM and the Activity as a local
/// borrow. A query that fails leaves no pending Java exception behind.
///
/// # Errors
/// [`PreferenceQueryError::Native`] when the thread cannot attach or the query
/// fails.
#[expect(
    unsafe_code,
    reason = "AndroidApp lends live VM and Activity references to JNI"
)]
pub(crate) fn with_activity<T>(
    app: &AndroidApp,
    query: impl FnOnce(&mut jni::Env<'_>, &JObject<'_>) -> jni::errors::Result<T>,
) -> Result<T, PreferenceQueryError> {
    // SAFETY: app owns the running VM, and JavaVM is a borrowed handle.
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
    vm.attach_current_thread(|env| -> jni::errors::Result<_> {
        let result = (|| {
            let raw_activity = app.activity_as_ptr() as jni::sys::jobject;
            // SAFETY: AndroidApp owns this global Activity reference for the
            // entire borrow of app. as_cast_raw borrows it without deleting it.
            let activity = unsafe { env.as_cast_raw::<Global<JObject<'_>>>(&raw_activity)? };
            query(env, activity.as_ref())
        })();
        if result.is_err() {
            env.exception_clear();
        }
        result
    })
    .map_err(|error| PreferenceQueryError::Native {
        message: error.to_string(),
    })
}
