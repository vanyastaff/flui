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
use flui_platform::{
    ClipboardError, ClipboardWrite, ClipboardWritten, Concealment, ExpiryClearing,
};
#[cfg(target_os = "android")]
use jni::objects::{JByteArray, JClass, JObject};
#[cfg(target_os = "android")]
use jni::{JValue, jni_sig, jni_str};

#[cfg(target_os = "android")]
use crate::host::{ClearDecision, ExpiredClear, ExpiredEntry, ExpiringRecord, SyncClipboard};
use crate::host::{admit_text, host_text};
#[cfg(target_os = "android")]
use crate::owner::android::activity::with_activity;
#[cfg(target_os = "android")]
use crate::owner::slot::OwnerCell;
#[cfg(target_os = "android")]
use crate::version::gate::android::{AndroidApi, ClearPrimaryClip, ClipExtras};

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

    /// `FluiView.utf8Length`: the text's length in UTF-8 bytes, the unit the
    /// limit is stated in, counted in Java without copying it.
    fn utf8_len(&mut self, text: &Self::Text) -> Result<usize, Self::Error>;

    /// `FluiView.utf8`: the text encoded as UTF-8 in one Java array, then
    /// copied into this process; the array is let go of once copied.
    fn utf8(&mut self, text: &Self::Text) -> Result<Vec<u8>, Self::Error>;
}

/// What a read answers for `item`, the primary clip's first: its text, refused
/// [`ClipboardError::TooLarge`](flui_platform::ClipboardError::TooLarge) on
/// its UTF-8 length before any copy is made when it is past
/// `host::MAX_TEXT` (the limit of every host's read); `Ok(None)` for an item
/// that holds no text.
///
/// The copies of the text made for the answer, besides the clip's own: the
/// UTF-8 array Java encodes, the bytes JNI copies out of it, and the shared
/// text made from those. At most two of them live at once, each within the
/// limit: the array and the bytes while JNI copies, the bytes and the shared
/// text while the answer is made.
///
/// # Errors
///
/// The first failed call.
pub(crate) fn item_text<I: ClipItem>(item: &mut I) -> Result<ClipboardRead, I::Error> {
    let Some(text) = item.text()? else {
        return Ok(Ok(None));
    };
    if let Err(refused) = admit_text(item.utf8_len(&text)?) {
        return Ok(Err(refused));
    }
    Ok(match String::from_utf8(item.utf8(&text)?) {
        Ok(text) => host_text(text),
        // `FluiView.utf8` encodes UTF-8: anything else is a defect of one
        // side, which no retry changes.
        Err(_) => Err(flui_platform::ClipboardError::Unavailable),
    })
}

/// The system clipboard of the Activity `app` runs.
///
/// An expiring write's clip carries, beside `EXTRA_IS_SENSITIVE`, this
/// clipboard's writer mark and the entry's number in its description's
/// extras (ADR-0363). The clear reads the primary clip's description and
/// clears only while both still match: two binder calls, so a copy another
/// application makes between them is cleared with the entry; every other
/// later copy carries other extras. From Android 10 the system hands an
/// application without input focus no description, and the clear answers
/// [`ExpiredClear::Unseen`] rather than guess.
#[cfg(target_os = "android")]
pub(crate) struct AndroidClipboard {
    app: AndroidApp,
    /// The running system's level: whether a clip carries extras, and
    /// whether the manager clears its primary clip.
    api: AndroidApi,
    /// A number this clipboard alone marks its expiring clips with, so an
    /// entry number another process's FLUI application wrote is not ours.
    writer: i64,
    /// The last expiring entry this clipboard landed, with the marks its clip
    /// carries: the owner thread's alone. The platform keeps the clipboard in
    /// its `Send + Sync` state, so the record is an owner cell, which every
    /// other thread reaches as nothing.
    expiring: OwnerCell<ExpiringRecord<ClipMarks>>,
}

/// The marks an expiring clip carries in its description's extras: the
/// writer and the entry. A clip without them reads as zeros.
#[cfg(target_os = "android")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ClipMarks {
    writer: i64,
    entry: i64,
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
        Self {
            app,
            api,
            writer: writer_mark(),
            expiring: OwnerCell::new(),
        }
    }
}

/// A writer mark no other process is likely to draw: the process's random
/// hash seed, never zero (a clip without the mark reads zero).
#[cfg(target_os = "android")]
fn writer_mark() -> i64 {
    use std::hash::{BuildHasher as _, Hasher as _};
    let mut hasher = std::hash::RandomState::new().build_hasher();
    hasher.write_u32(std::process::id());
    i64::from_ne_bytes(hasher.finish().to_ne_bytes()) | 1
}

/// A clip's item, read through the JNI environment of a read; `view` is
/// `FluiView`'s class, whose static methods measure and encode the text.
#[cfg(target_os = "android")]
struct JavaClipItem<'a, 'local> {
    env: &'a mut jni::Env<'local>,
    item: JObject<'local>,
    view: JClass<'local>,
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

    fn utf8_len(&mut self, text: &JObject<'local>) -> jni::errors::Result<usize> {
        let len = self
            .env
            .call_static_method(
                &self.view,
                jni_str!("utf8Length"),
                jni_sig!("(Ljava/lang/CharSequence;)J"),
                &[JValue::Object(text)],
            )?
            .j()?;
        Ok(usize::try_from(len).unwrap_or(usize::MAX))
    }

    fn utf8(&mut self, text: &JObject<'local>) -> jni::errors::Result<Vec<u8>> {
        let bytes = self
            .env
            .call_static_method(
                &self.view,
                jni_str!("utf8"),
                jni_sig!("(Ljava/lang/CharSequence;)[B"),
                &[JValue::Object(text)],
            )?
            .l()?;
        let bytes = self.env.cast_local::<JByteArray<'_>>(bytes)?;
        let copied = self.env.convert_byte_array(&bytes);
        self.env.delete_local_ref(bytes);
        copied
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
            let view = crate::owner::android::java_view::view_class(env, activity)?;
            item_text(&mut JavaClipItem { env, item, view })
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
        // one, as `concealment` says, and an expiring one is not cleared, as
        // `expiry_clearing` says.
        let extras = self.api.since::<ClipExtras>().is_some();
        let sensitive = write.is_concealed() && extras;
        let entry = write.entry().filter(|_| extras);
        let marks = entry.map(|entry| ClipMarks {
            writer: self.writer,
            entry: i64::from_ne_bytes(entry.get().to_ne_bytes()),
        });
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
                mark_sensitive(env, &clip, marks)?;
            }
            env.call_method(
                &manager,
                jni_str!("setPrimaryClip"),
                jni_sig!("(Landroid/content/ClipData;)V"),
                &[JValue::Object(&clip)],
            )?;
            Ok(Ok(()))
        });
        let written = written.unwrap_or_else(|error| {
            tracing::warn!(
                target: "flui_native::android",
                %error,
                "clipboard write failed"
            );
            Err(ClipboardError::Unavailable)
        });
        if written.is_ok() {
            self.expiring.with(|expiring| {
                expiring.landed(entry, marks.unwrap_or(ClipMarks::NONE));
            });
        }
        written
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

    /// From API 24 a clip carries the marks the clear compares. From Android
    /// 10 only the application with input focus sees the clipboard, so a
    /// time that comes in the background clears when the application is next
    /// in front; Android gives an application no notice of the screen
    /// locking.
    fn expiry_clearing(&self) -> ExpiryClearing {
        if self.api.since::<ClipExtras>().is_some() {
            ExpiryClearing::AtTimeOrInFront
        } else {
            ExpiryClearing::Unavailable
        }
    }

    /// The description is read and the clip cleared in two binder calls (see
    /// the type's docs).
    fn clear_expired_now(&self, entry: ExpiredEntry) -> ExpiredClear {
        if self.api.since::<ClipExtras>().is_none() {
            return ExpiredClear::Unsupported;
        }
        let clears = self.api.since::<ClearPrimaryClip>().is_some();
        let expiring = &self.expiring;
        let cleared = with_activity(&self.app, |env, activity| {
            let manager = clipboard_manager(env, activity)?;
            if manager.is_null() {
                return Ok(ExpiredClear::Failed(ClipboardError::Gone));
            }
            let Some(marks) = primary_marks(env, &manager)? else {
                return Ok(ExpiredClear::Unseen);
            };
            let decision = expiring.with(|expiring| expiring.decide(entry, &marks));
            match decision {
                // Off the owner thread the record is out of reach.
                None => return Ok(ExpiredClear::Failed(ClipboardError::Gone)),
                Some(ClearDecision::NotHeld) => return Ok(ExpiredClear::NotHeld),
                Some(ClearDecision::Clear) => {}
            }
            if clears {
                env.call_method(&manager, jni_str!("clearPrimaryClip"), jni_sig!("()V"), &[])?;
            } else {
                // Before API 28 the manager has no clear: an empty clip
                // takes the entry's place.
                let empty = env.new_string("")?;
                let clip = env
                    .call_static_method(
                        jni_str!("android/content/ClipData"),
                        jni_str!("newPlainText"),
                        jni_sig!(
                            "(Ljava/lang/CharSequence;Ljava/lang/CharSequence;)Landroid/content/ClipData;"
                        ),
                        &[JValue::Object(&empty), JValue::Object(&empty)],
                    )?
                    .l()?;
                env.call_method(
                    &manager,
                    jni_str!("setPrimaryClip"),
                    jni_sig!("(Landroid/content/ClipData;)V"),
                    &[JValue::Object(&clip)],
                )?;
            }
            // Forgotten once the clear succeeded, so a failed one keeps the
            // entry for its retry. The system tells clip listeners through
            // the main looper, never inside the call, so no write lands here.
            expiring.with(|expiring| expiring.landed(None, ClipMarks::NONE));
            Ok(ExpiredClear::Cleared)
        });
        cleared.unwrap_or_else(|error| {
            tracing::warn!(
                target: "flui_native::android",
                %error,
                "an expired clipboard entry could not be cleared"
            );
            ExpiredClear::Failed(ClipboardError::Unavailable)
        })
    }
}

#[cfg(target_os = "android")]
impl ClipMarks {
    /// What a clip without the marks reads as.
    const NONE: Self = Self {
        writer: 0,
        entry: 0,
    };
}

/// The marks the primary clip's description carries, zeros for a clip
/// without them or without extras; `None` when the system hands this
/// application no description (none on the clipboard, or no input focus
/// from Android 10), so whether the entry is still there cannot be told.
#[cfg(target_os = "android")]
fn primary_marks(
    env: &mut jni::Env<'_>,
    manager: &JObject<'_>,
) -> jni::errors::Result<Option<ClipMarks>> {
    let description = env
        .call_method(
            manager,
            jni_str!("getPrimaryClipDescription"),
            jni_sig!("()Landroid/content/ClipDescription;"),
            &[],
        )?
        .l()?;
    if description.is_null() {
        return Ok(None);
    }
    let extras = env
        .call_method(
            &description,
            jni_str!("getExtras"),
            jni_sig!("()Landroid/os/PersistableBundle;"),
            &[],
        )?
        .l()?;
    if extras.is_null() {
        return Ok(Some(ClipMarks::NONE));
    }
    let mut mark = |key: &str| -> jni::errors::Result<i64> {
        let key = env.new_string(key)?;
        env.call_method(
            &extras,
            jni_str!("getLong"),
            jni_sig!("(Ljava/lang/String;J)J"),
            &[JValue::Object(&key), JValue::Long(0)],
        )?
        .j()
    };
    Ok(Some(ClipMarks {
        writer: mark(WRITER_EXTRA)?,
        entry: mark(ENTRY_EXTRA)?,
    }))
}

/// Mark `clip` sensitive: `ClipDescription.EXTRA_IS_SENSITIVE` in its
/// description's extras (ADR-0152 §6), and, for an expiring write, `marks`
/// beside it (ADR-0363). The sensitive key is the constant's value, which a
/// system before API 33 does not name but keyboards read all the same. A
/// clip that could not be marked is not written: the error ends the write
/// before `setPrimaryClip`.
#[cfg(target_os = "android")]
fn mark_sensitive(
    env: &mut jni::Env<'_>,
    clip: &JObject<'_>,
    marks: Option<ClipMarks>,
) -> jni::errors::Result<()> {
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
    if let Some(marks) = marks {
        for (name, value) in [(WRITER_EXTRA, marks.writer), (ENTRY_EXTRA, marks.entry)] {
            let name = env.new_string(name)?;
            env.call_method(
                &extras,
                jni_str!("putLong"),
                jni_sig!("(Ljava/lang/String;J)V"),
                &[JValue::Object(&name), JValue::Long(value)],
            )?;
        }
    }
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

/// The extra naming the clipboard that wrote an expiring clip.
#[cfg(target_os = "android")]
const WRITER_EXTRA: &str = "dev.flui.clipboard.writer";

/// The extra naming an expiring clip's entry.
#[cfg(target_os = "android")]
const ENTRY_EXTRA: &str = "dev.flui.clipboard.entry";

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
        /// How many bytes the read copied out.
        copied: usize,
    }

    impl ClipItem for Item {
        type Text = String;
        type Error = std::convert::Infallible;

        fn text(&mut self) -> Result<Option<String>, Self::Error> {
            Ok(self.text.clone())
        }

        fn utf8_len(&mut self, text: &String) -> Result<usize, Self::Error> {
            Ok(text.len())
        }

        fn utf8(&mut self, text: &String) -> Result<Vec<u8>, Self::Error> {
            self.copied += text.len();
            Ok(text.clone().into_bytes())
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
    /// to is refused before it is copied, measured in the limit's own unit:
    /// text whose UTF-16 units fit but whose UTF-8 bytes do not is refused
    /// too.
    #[test]
    fn a_clipboard_read_takes_the_items_text_and_reads_no_provider() {
        assert_eq!(read(None), (Ok(None), 0));

        let (text, copied) = read(Some("notes €".to_owned()));
        assert_eq!(
            text.map(|text| text.map(|text| text.to_string())),
            Ok(Some("notes €".to_owned()))
        );
        assert_eq!(copied, 9);

        assert_eq!(
            read(Some("a".repeat(MAX_TEXT + 1))),
            (Err(ClipboardError::TooLarge), 0),
            "refused on its length, before the copy"
        );
        let wide = "€".repeat(MAX_TEXT / 3 + 1);
        assert!(wide.encode_utf16().count() <= MAX_TEXT, "its units fit");
        assert_eq!(
            read(Some(wide)),
            (Err(ClipboardError::TooLarge), 0),
            "refused on its bytes, before the copy"
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
        for stated in [
            format!("static final int MAX_PASTED_BYTES = {MAX_TEXT};"),
            // What the read measures and copies through.
            "static long utf8Length(CharSequence text)".to_owned(),
            "static byte[] utf8(CharSequence text)".to_owned(),
        ] {
            assert!(view.contains(&stated), "{stated:?} in FluiView");
        }
        for paste in [
            java_source("dev.flui.platform.FluiInputConnection"),
            java_source("dev.flui.platform.FluiContentReceiver"),
        ] {
            assert!(paste.contains("FluiView.clipText(clip)"), "{paste}");
        }
    }
}
