//! Android clipboard (`ClipboardManager`), reached through JNI.
//!
//! The Activity's `getSystemService("clipboard")` is the system clipboard; the
//! manager posts to the Java main thread itself, so the owner thread may call
//! it. A read takes the primary clip's first item as text
//! (`ClipData.Item.coerceToText`), so a URI or an intent reads as its textual
//! form. From Android 10 a read answers `Ok(None)` unless the app has input
//! focus, and from Android 13 the system shows its own notice on a read: both
//! are the system's policy, not a failure. A failed call is logged; a read
//! answers [`ClipboardError::Unavailable`], and a write writes nothing and
//! answers it too. A write where the activity has no clipboard service
//! answers [`ClipboardError::Gone`]: no retry gives it one.

use android_activity::AndroidApp;
use flui_platform::{
    ClipboardError, ClipboardRead, ClipboardWrite, ClipboardWritten, Concealment, ExpiryClearing,
};
use jni::objects::JObject;
use jni::{JValue, jni_sig, jni_str};

use crate::host::{
    ClearDecision, ExpiredClear, ExpiredEntry, ExpiringRecord, SyncClipboard, admit_text, host_text,
};
use crate::owner::android::activity::with_activity;
use crate::version::gate::android::{AndroidApi, ClearPrimaryClip, ClipExtras};

/// The system clipboard of the Activity `app` runs.
///
/// An expiring write's clip carries, beside `EXTRA_IS_SENSITIVE`, this
/// clipboard's writer mark and the entry's number in its description's
/// extras (ADR-0362). The clear reads the primary clip's description and
/// clears only while both still match: two binder calls, so a copy another
/// application makes between them is cleared with the entry; every other
/// later copy carries other extras. From Android 10 the system hands an
/// application without input focus no description, and the clear answers
/// [`ExpiredClear::Unseen`] rather than guess.
pub(crate) struct AndroidClipboard {
    app: AndroidApp,
    /// The running system's level: whether a clip carries extras, and
    /// whether the manager clears its primary clip.
    api: AndroidApi,
    /// A number this clipboard alone marks its expiring clips with, so an
    /// entry number another process's FLUI application wrote is not ours.
    writer: i64,
    /// The last expiring entry this clipboard landed, with the marks its clip
    /// carries. The platform shares the clipboard across threads, but only
    /// the owner thread writes and clears it; the lock never waits.
    expiring: parking_lot::Mutex<ExpiringRecord<ClipMarks>>,
}

/// The marks an expiring clip carries in its description's extras: the
/// writer and the entry. A clip without them reads as zeros.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ClipMarks {
    writer: i64,
    entry: i64,
}

impl std::fmt::Debug for AndroidClipboard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AndroidClipboard").finish_non_exhaustive()
    }
}

impl AndroidClipboard {
    pub(crate) fn new(app: AndroidApp, api: AndroidApi) -> Self {
        Self {
            app,
            api,
            writer: writer_mark(),
            expiring: parking_lot::Mutex::default(),
        }
    }
}

/// A writer mark no other process is likely to draw: the process's random
/// hash seed, never zero (a clip without the mark reads zero).
fn writer_mark() -> i64 {
    use std::hash::{BuildHasher as _, Hasher as _};
    let mut hasher = std::hash::RandomState::new().build_hasher();
    hasher.write_u32(std::process::id());
    i64::from_ne_bytes(hasher.finish().to_ne_bytes()) | 1
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
            let text = env
                .call_method(
                    &item,
                    jni_str!("coerceToText"),
                    jni_sig!("(Landroid/content/Context;)Ljava/lang/CharSequence;"),
                    &[JValue::Object(activity)],
                )?
                .l()?;
            if text.is_null() {
                return Ok(Ok(None));
            }
            // Measured in UTF-16 units before either copy: Java's `toString`
            // and the one into Rust.
            let units = env
                .call_method(&text, jni_str!("length"), jni_sig!("()I"), &[])?
                .i()?;
            if let Err(refused) = admit_text(usize::try_from(units).unwrap_or(0)) {
                return Ok(Err(refused));
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
            Ok(host_text(text.try_to_string(env)?))
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
            self.expiring
                .lock()
                .landed(entry, marks.unwrap_or(ClipMarks::NONE));
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

    /// From API 24 a clip carries the marks the clear compares; Android
    /// gives an application no notice of the screen locking.
    fn expiry_clearing(&self) -> ExpiryClearing {
        if self.api.since::<ClipExtras>().is_some() {
            ExpiryClearing::AtTime
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
            match expiring.lock().decide(entry, &marks) {
                ClearDecision::NotHeld => return Ok(ExpiredClear::NotHeld),
                ClearDecision::Clear => {}
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
            expiring.lock().landed(None, ClipMarks::NONE);
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
/// beside it (ADR-0362). The sensitive key is the constant's value, which a
/// system before API 33 does not name but keyboards read all the same. A
/// clip that could not be marked is not written: the error ends the write
/// before `setPrimaryClip`.
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
const SENSITIVE_EXTRA: &str = "android.content.extra.IS_SENSITIVE";

/// The extra naming the clipboard that wrote an expiring clip.
const WRITER_EXTRA: &str = "dev.flui.clipboard.writer";

/// The extra naming an expiring clip's entry.
const ENTRY_EXTRA: &str = "dev.flui.clipboard.entry";
