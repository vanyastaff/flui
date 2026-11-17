//! Android clipboard (`ClipboardManager`), reached through JNI.
//!
//! The Activity's `getSystemService("clipboard")` is the system clipboard; the
//! manager posts to the Java main thread itself, so the owner thread may call
//! it. A read answers the text the primary clip's first item holds
//! (`ClipData.Item.getText`, [`item_text`]): an item that is a URI or an intent
//! holds none and reads as `Ok(None)`, as a clipboard on every other host
//! answers one with no text representation. No content provider is asked:
//! `coerceToText` would open another application's stream and read all of it
//! on the owner thread before any size is known. From Android 10 a read
//! answers `Ok(None)` unless the app has input focus, and from Android 13 the
//! system shows its own notice on a read: both are the system's policy, not a
//! failure. A failed call is logged; a read answers
//! [`ClipboardError::Unavailable`], and a write writes nothing and answers it
//! too. A write where the activity has no clipboard service answers
//! [`ClipboardError::Gone`]: no retry gives it one.

#[cfg(target_os = "android")]
use android_activity::AndroidApp;
use flui_platform::ClipboardRead;
#[cfg(target_os = "android")]
use flui_platform::{ClipboardError, ClipboardWrite, ClipboardWritten, Concealment};
#[cfg(target_os = "android")]
use jni::objects::{JObject, JString};
#[cfg(target_os = "android")]
use jni::{JValue, jni_sig, jni_str};

#[cfg(target_os = "android")]
use crate::host::SyncClipboard;
use crate::host::{admit_text, host_text};
#[cfg(target_os = "android")]
use crate::owner::android::activity::with_activity;
#[cfg(target_os = "android")]
use crate::version::gate::android::{AndroidApi, ClipExtras};

/// The Java side of the primary clip's first item, as a read asks it: the
/// calls [`item_text`] makes, which JNI answers on Android and a stand-in in
/// the tests.
pub(crate) trait ClipItem {
    /// A local reference to the item's text.
    type Text;
    /// A failed JNI call.
    type Error;

    /// `ClipData.Item.getText()`: `None` for an item that holds no text (a
    /// URI, an intent, HTML alone). Reads nothing from another process.
    fn text(&mut self) -> Result<Option<Self::Text>, Self::Error>;

    /// `CharSequence.length()`: the text's length in UTF-16 units, measured
    /// without copying it.
    fn units(&mut self, text: &Self::Text) -> Result<usize, Self::Error>;

    /// The text, copied into this process (`toString()`, then the copy into
    /// Rust).
    fn copy(&mut self, text: &Self::Text) -> Result<String, Self::Error>;
}

/// What a read answers for `item`, the primary clip's first: its text, refused
/// [`ClipboardError::TooLarge`](flui_platform::ClipboardError::TooLarge) on
/// its length before either copy is made when it is past
/// `host::MAX_TEXT` (the limit of every host's read), and
/// again on its UTF-8 bytes; `Ok(None)` for an item that holds no text.
///
/// Besides the Java string, at most two copies of the text live in this
/// process, and both only while the answer is made: the `String` JNI copies
/// out, and the shared text made from it before it is dropped.
///
/// # Errors
///
/// The first failed call.
pub(crate) fn item_text<I: ClipItem>(item: &mut I) -> Result<ClipboardRead, I::Error> {
    let Some(text) = item.text()? else {
        return Ok(Ok(None));
    };
    if let Err(refused) = admit_text(item.units(&text)?) {
        return Ok(Err(refused));
    }
    Ok(host_text(item.copy(&text)?))
}

/// The system clipboard of the Activity `app` runs.
#[cfg(target_os = "android")]
pub(crate) struct AndroidClipboard {
    app: AndroidApp,
    /// The running system's level: whether a clip carries extras.
    api: AndroidApi,
}

#[cfg(target_os = "android")]
impl std::fmt::Debug for AndroidClipboard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AndroidClipboard").finish_non_exhaustive()
    }
}

#[cfg(target_os = "android")]
impl AndroidClipboard {
    pub(crate) fn new(app: AndroidApp, api: AndroidApi) -> Self {
        Self { app, api }
    }
}

/// A clip's item, read through the JNI environment of a read.
#[cfg(target_os = "android")]
struct JavaClipItem<'a, 'local> {
    env: &'a mut jni::Env<'local>,
    item: JObject<'local>,
}

#[cfg(target_os = "android")]
impl<'local> ClipItem for JavaClipItem<'_, 'local> {
    type Text = JObject<'local>;
    type Error = jni::errors::Error;

    fn text(&mut self) -> jni::errors::Result<Option<JObject<'local>>> {
        let text = self
            .env
            .call_method(
                &self.item,
                jni_str!("getText"),
                jni_sig!("()Ljava/lang/CharSequence;"),
                &[],
            )?
            .l()?;
        Ok((!text.is_null()).then_some(text))
    }

    fn units(&mut self, text: &JObject<'local>) -> jni::errors::Result<usize> {
        let units = self
            .env
            .call_method(text, jni_str!("length"), jni_sig!("()I"), &[])?
            .i()?;
        Ok(usize::try_from(units).unwrap_or(0))
    }

    fn copy(&mut self, text: &JObject<'local>) -> jni::errors::Result<String> {
        let text = self
            .env
            .call_method(
                text,
                jni_str!("toString"),
                jni_sig!("()Ljava/lang/String;"),
                &[],
            )?
            .l()?;
        let text = self.env.cast_local::<JString<'_>>(text)?;
        text.try_to_string(self.env)
    }
}

/// `context.getSystemService("clipboard")`, the `ClipboardManager`.
#[cfg(target_os = "android")]
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

#[cfg(target_os = "android")]
impl SyncClipboard for AndroidClipboard {
    fn read_now(&self) -> ClipboardRead {
        with_activity(&self.app, |env, activity| {
            let manager = clipboard_manager(env, activity)?;
            if manager.is_null() {
                return Ok(Ok(None));
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
                return Ok(Ok(None));
            }
            let items = env
                .call_method(&clip, jni_str!("getItemCount"), jni_sig!("()I"), &[])?
                .i()?;
            if items < 1 {
                return Ok(Ok(None));
            }
            let item = env
                .call_method(
                    &clip,
                    jni_str!("getItemAt"),
                    jni_sig!("(I)Landroid/content/ClipData$Item;"),
                    &[JValue::Int(0)],
                )?
                .l()?;
            item_text(&mut JavaClipItem { env, item })
        })
        .unwrap_or_else(|error| {
            tracing::warn!(
                target: "flui_native::android",
                %error,
                "clipboard read failed"
            );
            Err(ClipboardError::Unavailable)
        })
    }

    fn write_now(&self, write: ClipboardWrite) -> ClipboardWritten {
        // Below the gate the write cannot carry the extra: it is an ordinary
        // one, as `concealment` says.
        let sensitive = write.is_concealed() && self.api.since::<ClipExtras>().is_some();
        let written = with_activity(&self.app, |env, activity| {
            let manager = clipboard_manager(env, activity)?;
            if manager.is_null() {
                // The system has no clipboard service for this activity, and
                // no retry will give it one.
                return Ok(Err(ClipboardError::Gone));
            }
            let label = env.new_string("")?;
            let text = env.new_string(write.as_text())?;
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
            if sensitive {
                mark_sensitive(env, &clip)?;
            }
            env.call_method(
                &manager,
                jni_str!("setPrimaryClip"),
                jni_sig!("(Landroid/content/ClipData;)V"),
                &[JValue::Object(&clip)],
            )?;
            Ok(Ok(()))
        });
        written.unwrap_or_else(|error| {
            tracing::warn!(
                target: "flui_native::android",
                %error,
                "clipboard write failed"
            );
            Err(ClipboardError::Unavailable)
        })
    }

    /// A concealed write carries `ClipDescription.EXTRA_IS_SENSITIVE`, which
    /// the system's copy preview and keyboards that read it honour; below
    /// API 24 a clip carries no extras.
    fn concealment(&self) -> Concealment {
        if self.api.since::<ClipExtras>().is_some() {
            Concealment::HintOnly
        } else {
            Concealment::Unavailable
        }
    }
}

/// Mark `clip` sensitive: `ClipDescription.EXTRA_IS_SENSITIVE` in its
/// description's extras (ADR-0152 §6). The key is the constant's value,
/// which a system before API 33 does not name but keyboards read all the
/// same. A clip that could not be marked is not written: the error ends the
/// write before `setPrimaryClip`.
#[cfg(target_os = "android")]
fn mark_sensitive(env: &mut jni::Env<'_>, clip: &JObject<'_>) -> jni::errors::Result<()> {
    let description = env
        .call_method(
            clip,
            jni_str!("getDescription"),
            jni_sig!("()Landroid/content/ClipDescription;"),
            &[],
        )?
        .l()?;
    let extras = env.new_object(
        jni_str!("android/os/PersistableBundle"),
        jni_sig!("()V"),
        &[],
    )?;
    let key = env.new_string(SENSITIVE_EXTRA)?;
    env.call_method(
        &extras,
        jni_str!("putBoolean"),
        jni_sig!("(Ljava/lang/String;Z)V"),
        &[JValue::Object(&key), JValue::Bool(true)],
    )?;
    env.call_method(
        &description,
        jni_str!("setExtras"),
        jni_sig!("(Landroid/os/PersistableBundle;)V"),
        &[JValue::Object(&extras)],
    )?;
    Ok(())
}

/// `ClipDescription.EXTRA_IS_SENSITIVE`.
#[cfg(target_os = "android")]
const SENSITIVE_EXTRA: &str = "android.content.extra.IS_SENSITIVE";

#[cfg(test)]
mod tests {
    use flui_platform::ClipboardError;

    use super::{ClipItem, item_text};
    use crate::host::MAX_TEXT;

    /// A clip's first item as JNI hands it over, counting what the read asks
    /// of it.
    struct Item {
        /// What `getText()` answers: `None` for a URI or an intent item.
        text: Option<String>,
        /// How many characters the read copied out.
        copied: usize,
    }

    impl ClipItem for Item {
        type Text = String;
        type Error = std::convert::Infallible;

        fn text(&mut self) -> Result<Option<String>, Self::Error> {
            Ok(self.text.clone())
        }

        fn units(&mut self, text: &String) -> Result<usize, Self::Error> {
            Ok(text.encode_utf16().count())
        }

        fn copy(&mut self, text: &String) -> Result<String, Self::Error> {
            self.copied += text.len();
            Ok(text.clone())
        }
    }

    fn read(text: Option<String>) -> (flui_platform::ClipboardRead, usize) {
        let mut item = Item { text, copied: 0 };
        let read = item_text(&mut item).expect("an infallible item");
        (read, item.copied)
    }

    /// A read answers the item's text and never another application's
    /// content: an item that holds none (a URI or an intent, whose text
    /// `coerceToText` would read from a content provider) answers `Ok(None)`
    /// with nothing copied, and text past the limit every host's read holds
    /// to is refused before it is copied.
    #[test]
    fn a_clipboard_read_takes_the_items_text_and_reads_no_provider() {
        assert_eq!(read(None), (Ok(None), 0));

        let (text, copied) = read(Some("notes".to_owned()));
        assert_eq!(
            text.map(|text| text.map(|text| text.to_string())),
            Ok(Some("notes".to_owned()))
        );
        assert_eq!(copied, 5);

        assert_eq!(
            read(Some("a".repeat(MAX_TEXT + 1))),
            (Err(ClipboardError::TooLarge), 0),
            "refused on its length, before the copy"
        );
    }

    /// A paste on the Java side takes the same text a read does, within the
    /// same limit: no class coerces an item to text, which reads a URI's
    /// content from its provider on the Java main thread, and the paste's
    /// limit is the read's.
    #[test]
    fn a_java_paste_reads_no_provider_and_holds_the_reads_limit() {
        use crate::owner::android::java_host::java_source;

        for class in [
            "dev.flui.platform.FluiView",
            "dev.flui.platform.FluiInputConnection",
            "dev.flui.platform.FluiContentReceiver",
            "dev.flui.platform.FluiHost",
            "dev.flui.platform.FluiActivity",
            "dev.flui.platform.FluiKeyboardMotion",
        ] {
            let source = java_source(class);
            assert!(!source.contains("coerceToText("), "{class} coerces an item");
        }
        let view = java_source("dev.flui.platform.FluiView");
        let stated = format!("static final int MAX_PASTED_BYTES = {MAX_TEXT};");
        assert!(view.contains(&stated), "{stated:?} in FluiView");
        for paste in [
            java_source("dev.flui.platform.FluiInputConnection"),
            java_source("dev.flui.platform.FluiContentReceiver"),
        ] {
            assert!(paste.contains("FluiView.clipText(clip)"), "{paste}");
        }
    }
}
