//! Android clipboard (`ClipboardManager`), reached through JNI.
//!
//! The Activity's `getSystemService("clipboard")` is the system clipboard; the
//! manager posts to the Java main thread itself, so the owner thread may call
//! it. A read takes the primary clip's first item as text
//! (`ClipData.Item.coerceToText`), so a URI or an intent reads as its textual
//! form. From Android 10 a read answers `Ok(None)` unless the app has input
//! focus, and from Android 13 the system shows its own notice on a read: both
//! are the system's policy, not a failure. A failed call is logged; a read
//! answers [`ClipboardError::Unavailable`], a write writes nothing.

use android_activity::AndroidApp;
use flui_platform::{ClipboardError, ClipboardRead};
use jni::objects::JObject;
use jni::{JValue, jni_sig, jni_str};

use crate::host::{SyncClipboard, host_text};
use crate::owner::android::activity::with_activity;

/// The system clipboard of the Activity `app` runs.
pub(crate) struct AndroidClipboard {
    app: AndroidApp,
}

impl std::fmt::Debug for AndroidClipboard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AndroidClipboard").finish_non_exhaustive()
    }
}

impl AndroidClipboard {
    pub(crate) fn new(app: AndroidApp) -> Self {
        Self { app }
    }
}

/// `context.getSystemService("clipboard")`, the `ClipboardManager`.
fn clipboard_manager<'local>(
    env: &mut jni::Env<'local>,
    context: &JObject<'_>,
) -> jni::errors::Result<JObject<'local>> {
    let name = env.new_string("clipboard")?;
    env.call_method(
        context,
        jni_str!("getSystemService"),
        jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"),
        &[JValue::Object(&name)],
    )?
    .l()
}

impl SyncClipboard for AndroidClipboard {
    fn read_now(&self) -> ClipboardRead {
        let text = with_activity(&self.app, |env, activity| {
            let manager = clipboard_manager(env, activity)?;
            if manager.is_null() {
                return Ok(None);
            }
            let clip = env
                .call_method(
                    &manager,
                    jni_str!("getPrimaryClip"),
                    jni_sig!("()Landroid/content/ClipData;"),
                    &[],
                )?
                .l()?;
            if clip.is_null() {
                return Ok(None);
            }
            let items = env
                .call_method(&clip, jni_str!("getItemCount"), jni_sig!("()I"), &[])?
                .i()?;
            if items < 1 {
                return Ok(None);
            }
            let item = env
                .call_method(
                    &clip,
                    jni_str!("getItemAt"),
                    jni_sig!("(I)Landroid/content/ClipData$Item;"),
                    &[JValue::Int(0)],
                )?
                .l()?;
            let text = env
                .call_method(
                    &item,
                    jni_str!("coerceToText"),
                    jni_sig!("(Landroid/content/Context;)Ljava/lang/CharSequence;"),
                    &[JValue::Object(activity)],
                )?
                .l()?;
            if text.is_null() {
                return Ok(None);
            }
            let text = env
                .call_method(
                    &text,
                    jni_str!("toString"),
                    jni_sig!("()Ljava/lang/String;"),
                    &[],
                )?
                .l()?;
            let text = env.cast_local::<jni::objects::JString<'_>>(text)?;
            Ok(Some(text.try_to_string(env)?))
        })
        .map_err(|error| {
            tracing::warn!(
                target: "flui_native::android",
                %error,
                "clipboard read failed"
            );
            ClipboardError::Unavailable
        })?;
        text.map_or(Ok(None), host_text)
    }

    fn write_now(&self, text: String) {
        let written = with_activity(&self.app, |env, activity| {
            let manager = clipboard_manager(env, activity)?;
            if manager.is_null() {
                return Ok(());
            }
            let label = env.new_string("")?;
            let text = env.new_string(&text)?;
            let clip = env
                .call_static_method(
                    jni_str!("android/content/ClipData"),
                    jni_str!("newPlainText"),
                    jni_sig!(
                        "(Ljava/lang/CharSequence;Ljava/lang/CharSequence;)Landroid/content/ClipData;"
                    ),
                    &[JValue::Object(&label), JValue::Object(&text)],
                )?
                .l()?;
            env.call_method(
                &manager,
                jni_str!("setPrimaryClip"),
                jni_sig!("(Landroid/content/ClipData;)V"),
                &[JValue::Object(&clip)],
            )?;
            Ok(())
        });
        if let Err(error) = written {
            tracing::warn!(
                target: "flui_native::android",
                %error,
                "clipboard write failed"
            );
        }
    }
}
