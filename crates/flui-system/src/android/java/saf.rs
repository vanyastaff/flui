//! The Storage Access Framework: filling a picker's `Intent`, reading the
//! documents its answer names, and reading and writing each through the
//! application's content resolver, on a worker.

use std::fs::File;
use std::io::Write as _;
use std::os::fd::FromRawFd as _;
use std::sync::Arc;

use flui_platform::SystemFailure;
use flui_platform::capability::{IntentFrame, JniFrame};
use jni::objects::{Global, JObject, JObjectArray};
use jni::{Env, JValue, jni_sig, jni_str};

use super::super::documents::{
    self, Described, DocumentPicker, Interrupt, LetGo, MAX_NAME_UNITS, NotOpened, PickedDocuments,
    PickerIntent,
};
use super::super::grants::DocumentReach;
use super::{Java, bounded_string, in_frame, is_a, put_string, string_array, thrown, with_string};
use crate::dialog::DialogError;

/// [`DocumentPicker`] over the Storage Access Framework.
pub(crate) struct JavaPicker(pub(crate) Arc<Java>);

/// Fill `target`, the host's empty `Intent`, as `intent`.
fn fill_intent(
    env: &mut Env<'_>,
    target: &JObject<'_>,
    intent: &PickerIntent,
) -> jni::errors::Result<()> {
    let (action, mime_type) = match intent {
        PickerIntent::Open { mime_types, .. } => (
            "android.intent.action.OPEN_DOCUMENT",
            match mime_types.as_slice() {
                [only] => only.as_str(),
                _ => "*/*",
            },
        ),
        PickerIntent::Create { mime_type, .. } => {
            ("android.intent.action.CREATE_DOCUMENT", mime_type.as_str())
        }
    };
    with_string(env, target, jni_str!("setAction"), action)?;
    with_string(
        env,
        target,
        jni_str!("addCategory"),
        "android.intent.category.OPENABLE",
    )?;
    with_string(env, target, jni_str!("setType"), mime_type)?;
    match intent {
        PickerIntent::Open {
            mime_types,
            multiple,
        } => {
            if mime_types.len() > 1 {
                let extra = env.new_string("android.intent.extra.MIME_TYPES")?;
                let types = mime_types.iter().map(String::as_str).collect::<Vec<&str>>();
                let types = string_array(env, &types)?;
                env.call_method(
                    target,
                    jni_str!("putExtra"),
                    jni_sig!("(Ljava/lang/String;[Ljava/lang/String;)Landroid/content/Intent;"),
                    &[JValue::Object(&extra), JValue::Object(&types)],
                )?;
            }
            let extra = env.new_string("android.intent.extra.ALLOW_MULTIPLE")?;
            env.call_method(
                target,
                jni_str!("putExtra"),
                jni_sig!("(Ljava/lang/String;Z)Landroid/content/Intent;"),
                &[JValue::Object(&extra), JValue::Bool(*multiple)],
            )?;
        }
        PickerIntent::Create { name, .. } => {
            put_string(env, target, "android.intent.extra.TITLE", name.as_deref())?;
        }
    }
    Ok(())
}

/// The documents the answer `Intent` names, in the order it gives them: its
/// clip's items, or its data alone; `None` when it names more than `max`,
/// none of which is read.
fn picked_uris<'local>(
    env: &mut Env<'local>,
    answer: &JObject<'_>,
    max: usize,
) -> jni::errors::Result<Option<(JObjectArray<'local>, usize)>> {
    let uri_class = jni_str!("android/net/Uri");
    let clip = env
        .call_method(
            answer,
            jni_str!("getClipData"),
            jni_sig!("()Landroid/content/ClipData;"),
            &[],
        )?
        .l()?;
    let items = if clip.is_null() {
        0
    } else {
        env.call_method(&clip, jni_str!("getItemCount"), jni_sig!("()I"), &[])?
            .i()?
    };
    let Ok(items) = usize::try_from(items) else {
        return Ok(None);
    };
    if items == 0 {
        let uri = env
            .call_method(
                answer,
                jni_str!("getData"),
                jni_sig!("()Landroid/net/Uri;"),
                &[],
            )?
            .l()?;
        let count = usize::from(!uri.is_null());
        let uris = env.new_object_array(1, uri_class, JObject::null())?;
        uris.set_element(env, 0, &uri)?;
        return Ok(Some((uris, count)));
    }
    if items > max {
        return Ok(None);
    }
    let uris = env.new_object_array(
        i32::try_from(items).unwrap_or(i32::MAX),
        uri_class,
        JObject::null(),
    )?;
    let mut count = 0;
    for index in 0..items {
        let named = env.with_local_frame(4, |env| -> jni::errors::Result<bool> {
            let item = env
                .call_method(
                    &clip,
                    jni_str!("getItemAt"),
                    jni_sig!("(I)Landroid/content/ClipData$Item;"),
                    &[JValue::Int(i32::try_from(index).unwrap_or(i32::MAX))],
                )?
                .l()?;
            let uri = env
                .call_method(
                    &item,
                    jni_str!("getUri"),
                    jni_sig!("()Landroid/net/Uri;"),
                    &[],
                )?
                .l()?;
            if uri.is_null() {
                return Ok(false);
            }
            uris.set_element(env, count, &uri)?;
            Ok(true)
        })?;
        count += usize::from(named);
    }
    Ok(Some((uris, count)))
}

impl DocumentPicker for JavaPicker {
    type Documents = JavaDocuments;

    fn mime_type_of(&self, extension: &str) -> Option<String> {
        self.0.mime_type_of(extension)
    }

    fn fill(&self, frame: IntentFrame, intent: &PickerIntent) -> bool {
        in_frame(frame.activity(), |env, _activity| {
            // SAFETY: the host lends its empty `Intent` with the frame, a
            // reference valid on this thread until its call returns.
            let target = unsafe { JObject::from_raw(env, frame.intent().cast()) };
            fill_intent(env, &target, intent)
        })
        .is_ok()
    }

    fn picked(
        &self,
        frame: JniFrame,
        max: usize,
    ) -> Result<(JavaDocuments, Interrupt), DialogError> {
        let unreadable = DialogError::Failed(SystemFailure::MalformedAnswer);
        let read = in_frame(frame, |env, answer| {
            let Some((uris, count)) = picked_uris(env, answer, max)? else {
                return Ok(None);
            };
            let signal = env.new_object(
                jni_str!("android/os/CancellationSignal"),
                jni_sig!("()V"),
                &[],
            )?;
            Ok(Some((
                env.new_global_ref(&uris)?,
                count,
                env.new_global_ref(&signal)?,
            )))
        });
        let (uris, count, signal) = read.ok().flatten().ok_or(unreadable)?;
        let signal = Arc::new(signal);
        let documents = JavaDocuments {
            java: Arc::clone(&self.0),
            uris,
            count,
            signal: Arc::clone(&signal),
        };
        let java = Arc::clone(&self.0);
        let interrupt: Interrupt = Box::new(move || {
            // `CancellationSignal.cancel` tells the provider through a
            // one-way call, so the owner thread never waits for it.
            let _ = java.attached(|env| {
                env.call_method(&*signal, jni_str!("cancel"), jni_sig!("()V"), &[])
                    .map(drop)
            });
        });
        Ok((documents, interrupt))
    }
}

/// The documents a picker answered: their content URIs, read through the
/// application's resolver under one cancellation signal.
pub(crate) struct JavaDocuments {
    java: Arc<Java>,
    uris: Global<JObjectArray<'static>>,
    count: usize,
    signal: Arc<Global<JObject<'static>>>,
}

impl JavaDocuments {
    /// The value of `column` in the one row the provider answers for `uri`
    /// (`ContentResolver.query`), read by `read` from the cursor at column
    /// 0; `None` when the provider answers no row or a `null`. Every exit
    /// leaves no exception pending, so a provider that throws for one column
    /// does not fail the next call made for the document (another column,
    /// its type).
    fn column<R>(
        &self,
        env: &mut Env<'_>,
        uri: &JObject<'_>,
        column: &str,
        read: impl FnOnce(&mut Env<'_>, &JObject<'_>) -> jni::errors::Result<Option<R>>,
    ) -> jni::errors::Result<Option<R>> {
        let cursor = (|| {
            let projection = string_array(env, &[column])?;
            let nothing = JObject::null();
            env.call_method(
                &self.java.resolver,
                jni_str!("query"),
                jni_sig!(
                    "(Landroid/net/Uri;[Ljava/lang/String;Ljava/lang/String;[Ljava/lang/String;Ljava/lang/String;Landroid/os/CancellationSignal;)Landroid/database/Cursor;"
                ),
                &[
                    JValue::Object(uri),
                    JValue::Object(&projection),
                    JValue::Object(&nothing),
                    JValue::Object(&nothing),
                    JValue::Object(&nothing),
                    JValue::Object(&self.signal),
                ],
            )?
            .l()
        })();
        let cursor = match cursor {
            Ok(cursor) => cursor,
            Err(error) => {
                env.exception_clear();
                return Err(error);
            }
        };
        if cursor.is_null() {
            return Ok(None);
        }
        let value = (|| {
            let has_row = env
                .call_method(&cursor, jni_str!("moveToFirst"), jni_sig!("()Z"), &[])?
                .z()?;
            let is_null = !has_row
                || env
                    .call_method(
                        &cursor,
                        jni_str!("isNull"),
                        jni_sig!("(I)Z"),
                        &[JValue::Int(0)],
                    )?
                    .z()?;
            if is_null {
                Ok(None)
            } else {
                read(env, &cursor)
            }
        })();
        // The cursor is closed on every exit, with nothing pending.
        if value.is_err() {
            env.exception_clear();
        }
        let closed = env.call_method(&cursor, jni_str!("close"), jni_sig!("()V"), &[]);
        if closed.is_err() {
            env.exception_clear();
        }
        value
    }

    /// Document `index` opened in `mode`, as a file this side owns.
    fn open(&self, index: usize, mode: &str) -> Result<File, DialogError> {
        let opened = self.java.attached(|env| {
            env.with_local_frame(8, |env| -> jni::errors::Result<Result<i32, NotOpened>> {
                let uri = self.uris.get_element(env, index)?;
                let mode = env.new_string(mode)?;
                let descriptor = env.call_method(
                    &self.java.resolver,
                    jni_str!("openFileDescriptor"),
                    jni_sig!(
                        "(Landroid/net/Uri;Ljava/lang/String;Landroid/os/CancellationSignal;)Landroid/os/ParcelFileDescriptor;"
                    ),
                    &[
                        JValue::Object(&uri),
                        JValue::Object(&mode),
                        JValue::Object(&self.signal),
                    ],
                );
                let descriptor = match descriptor {
                    Ok(descriptor) => descriptor.l()?,
                    Err(jni::errors::Error::JavaException) => {
                        let Some(exception) = thrown(env) else {
                            return Ok(Err(NotOpened::Failed));
                        };
                        let reason = if is_a(env, &exception, jni_str!("java/lang/SecurityException"))
                        {
                            NotOpened::GrantEnded
                        } else if is_a(
                            env,
                            &exception,
                            jni_str!("android/os/OperationCanceledException"),
                        ) {
                            NotOpened::Cancelled
                        } else if is_a(env, &exception, jni_str!("java/io/FileNotFoundException"))
                        {
                            NotOpened::NotFound
                        } else {
                            NotOpened::Failed
                        };
                        return Ok(Err(reason));
                    }
                    Err(error) => return Err(error),
                };
                if descriptor.is_null() {
                    return Ok(Err(NotOpened::Failed));
                }
                env.call_method(&descriptor, jni_str!("detachFd"), jni_sig!("()I"), &[])?
                    .i()
                    .map(Ok)
            })
        });
        let descriptor = opened
            .unwrap_or(Err(NotOpened::Failed))
            .map_err(documents::open_refusal)?;
        // SAFETY: `ParcelFileDescriptor.detachFd()` hands the descriptor
        // over: no Java object owns or closes it any more, so this is its
        // only owner.
        Ok(unsafe { File::from_raw_fd(descriptor) })
    }
}

impl PickedDocuments for JavaDocuments {
    fn len(&self) -> usize {
        self.count
    }

    fn describe(&mut self, index: usize) -> Described {
        self.java
            .attached(|env| {
                env.with_local_frame(16, |env| -> jni::errors::Result<Described> {
                    let uri = self.uris.get_element(env, index)?;
                    let name = self.column(env, &uri, "_display_name", |env, cursor| {
                        let name = env
                            .call_method(
                                cursor,
                                jni_str!("getString"),
                                jni_sig!("(I)Ljava/lang/String;"),
                                &[JValue::Int(0)],
                            )?
                            .l()?;
                        bounded_string(env, name, MAX_NAME_UNITS)
                    });
                    let size = self.column(env, &uri, "_size", |env, cursor| {
                        env.call_method(
                            cursor,
                            jni_str!("getLong"),
                            jni_sig!("(I)J"),
                            &[JValue::Int(0)],
                        )?
                        .j()
                        .map(Some)
                    });
                    let mime_type = env
                        .call_method(
                            &self.java.resolver,
                            jni_str!("getType"),
                            jni_sig!("(Landroid/net/Uri;)Ljava/lang/String;"),
                            &[JValue::Object(&uri)],
                        )
                        .and_then(jni::JValueOwned::l)
                        .and_then(|mime_type| bounded_string(env, mime_type, MAX_NAME_UNITS));
                    if mime_type.is_err() {
                        env.exception_clear();
                    }
                    Ok(Described {
                        // A provider that fails a query describes nothing.
                        name: name.ok().flatten(),
                        mime_type: mime_type.ok().flatten(),
                        // A negative size is the provider stating none.
                        size: size
                            .ok()
                            .flatten()
                            .and_then(|size| u64::try_from(size).ok()),
                    })
                })
            })
            .unwrap_or_default()
    }

    fn read(&mut self, index: usize, at_most: u64, let_go: &LetGo) -> Result<Vec<u8>, DialogError> {
        let file = self.open(index, "r")?;
        documents::read_bounded(file, at_most, let_go)
    }

    fn write(&mut self, index: usize, contents: &[u8]) -> Result<(), DialogError> {
        // A provider that cannot truncate takes a plain write.
        let mut file = self.open(index, "wt").or_else(|_| self.open(index, "w"))?;
        file.write_all(contents)
            .map_err(|error| documents::io_failure(&error))?;
        match file.sync_all() {
            // A provider that hands over a pipe has nothing to sync.
            Err(error) if error.kind() != std::io::ErrorKind::InvalidInput => {
                Err(documents::io_failure(&error))
            }
            _ => Ok(()),
        }
    }

    fn discard(&mut self, index: usize) {
        // A document that stays is the provider's to show the user.
        let _ = self.java.attached(|env| {
            env.with_local_frame(4, |env| {
                let uri = self.uris.get_element(env, index)?;
                env.call_static_method(
                    jni_str!("android/provider/DocumentsContract"),
                    jni_str!("deleteDocument"),
                    jni_sig!("(Landroid/content/ContentResolver;Landroid/net/Uri;)Z"),
                    &[JValue::Object(&self.java.resolver), JValue::Object(&uri)],
                )
                .map(drop)
            })
        });
    }

    fn keep(&mut self, index: usize) -> Option<String> {
        let kept = self.java.attached(|env| {
            env.with_local_frame(8, |env| -> jni::errors::Result<Option<String>> {
                let uri = self.uris.get_element(env, index)?;
                // A provider that offers no persistable permission throws
                // `SecurityException`: the pick carries no grant.
                let taken = env.call_method(
                    &self.java.resolver,
                    jni_str!("takePersistableUriPermission"),
                    jni_sig!("(Landroid/net/Uri;I)V"),
                    &[
                        JValue::Object(&uri),
                        JValue::Int(FLAG_GRANT_READ_URI_PERMISSION),
                    ],
                );
                if taken.is_err() {
                    // Cleared within the frame: the next document's call
                    // must not find it pending.
                    env.exception_clear();
                    return Ok(None);
                }
                let text = env
                    .call_method(
                        &uri,
                        jni_str!("toString"),
                        jni_sig!("()Ljava/lang/String;"),
                        &[],
                    )?
                    .l()?;
                bounded_string(env, text, MAX_URI_UNITS)
            })
        });
        kept.ok().flatten()
    }

    fn is_kept(&mut self, index: usize) -> bool {
        let held = self.java.attached(|env| {
            env.with_local_frame(16, |env| -> jni::errors::Result<bool> {
                let uri = self.uris.get_element(env, index)?;
                let permissions = env
                    .call_method(
                        &self.java.resolver,
                        jni_str!("getPersistedUriPermissions"),
                        jni_sig!("()Ljava/util/List;"),
                        &[],
                    )?
                    .l()?;
                let count = env
                    .call_method(&permissions, jni_str!("size"), jni_sig!("()I"), &[])?
                    .i()?;
                for at in 0..count {
                    let found = env.with_local_frame(4, |env| -> jni::errors::Result<bool> {
                        let permission = env
                            .call_method(
                                &permissions,
                                jni_str!("get"),
                                jni_sig!("(I)Ljava/lang/Object;"),
                                &[JValue::Int(at)],
                            )?
                            .l()?;
                        let granted = env
                            .call_method(
                                &permission,
                                jni_str!("getUri"),
                                jni_sig!("()Landroid/net/Uri;"),
                                &[],
                            )?
                            .l()?;
                        let same = env
                            .call_method(
                                &granted,
                                jni_str!("equals"),
                                jni_sig!("(Ljava/lang/Object;)Z"),
                                &[JValue::Object(&uri)],
                            )?
                            .z()?;
                        Ok(same
                            && env
                                .call_method(
                                    &permission,
                                    jni_str!("isReadPermission"),
                                    jni_sig!("()Z"),
                                    &[],
                                )?
                                .z()?)
                    })?;
                    if found {
                        return Ok(true);
                    }
                }
                Ok(false)
            })
        });
        // A resolver that fails the question answers no permission held;
        // the exception it threw was cleared with the call.
        held.unwrap_or(false)
    }
}

/// `Intent.FLAG_GRANT_READ_URI_PERMISSION`.
const FLAG_GRANT_READ_URI_PERMISSION: i32 = 1;

/// The longest content URI a grant is made of, in UTF-16 units; a longer
/// one is not kept.
const MAX_URI_UNITS: usize = 8 * 1024;

/// [`DocumentReach`] through the application's content resolver: a URI
/// parsed (`Uri.parse`) into one document, read under a cancellation signal
/// of its own.
pub(crate) struct JavaReach(pub(crate) Arc<Java>);

impl DocumentReach for JavaReach {
    type Documents = JavaDocuments;

    fn reach(&self, uri: &str) -> Option<JavaDocuments> {
        let reached = self.0.attached(|env| {
            env.with_local_frame(8, |env| -> jni::errors::Result<_> {
                let text = env.new_string(uri)?;
                let parsed = env
                    .call_static_method(
                        jni_str!("android/net/Uri"),
                        jni_str!("parse"),
                        jni_sig!("(Ljava/lang/String;)Landroid/net/Uri;"),
                        &[JValue::Object(&text)],
                    )?
                    .l()?;
                let uris = env.new_object_array(1, jni_str!("android/net/Uri"), JObject::null())?;
                uris.set_element(env, 0, &parsed)?;
                let signal = env.new_object(
                    jni_str!("android/os/CancellationSignal"),
                    jni_sig!("()V"),
                    &[],
                )?;
                Ok((env.new_global_ref(&uris)?, env.new_global_ref(&signal)?))
            })
        });
        let (uris, signal) = reached.ok()?;
        Some(JavaDocuments {
            java: Arc::clone(&self.0),
            uris,
            count: 1,
            signal: Arc::new(signal),
        })
    }
}
