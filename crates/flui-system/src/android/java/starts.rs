//! Starting an activity on Java's main thread: the `Intent` of a view, of a
//! document's view and of a share, and what the start answered.
//!
//! A file leaves as a content URI of `FluiDocuments`, never as a path: the
//! provider is handed the open descriptor, and the `Intent` carries the
//! read grant for the one activity Android resolves, or for the target the
//! user picks in the chooser.

use std::fs::File;
use std::os::fd::AsRawFd as _;
use std::sync::Arc;

use flui_platform::capability::Activity;
use jni::objects::{JObject, JString};
use jni::{Env, JValue, jni_sig, jni_str};

use super::super::StartAnswer;
use super::super::share::common_type;
use super::{Java, is_a, nullable, put_string, string_array, thrown, with_string};

/// `Intent.FLAG_GRANT_READ_URI_PERMISSION`.
const GRANT_READ: i32 = 1;

/// Start `intent` from `activity`, on Java's main thread, and say what the
/// start did.
fn start_activity(
    env: &mut Env<'_>,
    activity: &JObject<'_>,
    intent: jni::errors::Result<JObject<'_>>,
) -> StartAnswer {
    let started = intent.and_then(|intent| {
        env.call_method(
            activity,
            jni_str!("startActivity"),
            jni_sig!("(Landroid/content/Intent;)V"),
            &[JValue::Object(&intent)],
        )
        .map(drop)
    });
    let Err(error) = started else {
        return StartAnswer::Started;
    };
    let exception = matches!(error, jni::errors::Error::JavaException)
        .then(|| thrown(env))
        .flatten();
    env.exception_clear();
    match exception {
        Some(exception)
            if is_a(
                env,
                &exception,
                jni_str!("android/content/ActivityNotFoundException"),
            ) =>
        {
            StartAnswer::NoHandler
        }
        Some(exception) if is_a(env, &exception, jni_str!("java/lang/SecurityException")) => {
            StartAnswer::Denied
        }
        _ => StartAnswer::Failed,
    }
}

impl Java {
    /// The content URI `FluiDocuments` serves the document open as `file`
    /// under, named `name`. The provider duplicates the descriptor, so
    /// `file` stays the caller's.
    fn offer<'local>(
        &self,
        env: &mut Env<'local>,
        context: &JObject<'_>,
        file: &File,
        name: &JString<'_>,
    ) -> jni::errors::Result<JObject<'local>> {
        let uri = env
            .call_static_method(
                &self.documents,
                jni_str!("offer"),
                jni_sig!("(Landroid/content/Context;ILjava/lang/String;)Landroid/net/Uri;"),
                &[
                    JValue::Object(context),
                    JValue::Int(file.as_raw_fd()),
                    JValue::Object(name),
                ],
            )?
            .l()?;
        if uri.is_null() {
            Err(jni::errors::Error::NullPtr("FluiDocuments.offer"))
        } else {
            Ok(uri)
        }
    }

    /// The MIME type `FluiDocuments` serves a document named `name` as.
    fn type_of_name(&self, env: &mut Env<'_>, name: &JString<'_>) -> jni::errors::Result<String> {
        let mime_type = env
            .call_static_method(
                &self.documents,
                jni_str!("typeOfName"),
                jni_sig!("(Ljava/lang/String;)Ljava/lang/String;"),
                &[JValue::Object(name)],
            )?
            .l()?;
        let mime_type = env.cast_local::<JString<'_>>(mime_type)?;
        mime_type.try_to_string(env)
    }

    /// The `Intent` that views the document open as `file` under `name`:
    /// `ACTION_VIEW` of a content URI of `FluiDocuments`, typed, with the
    /// read grant for the activity Android resolves.
    fn document_intent<'local>(
        &self,
        env: &mut Env<'local>,
        activity: &JObject<'_>,
        file: &File,
        name: &str,
    ) -> jni::errors::Result<JObject<'local>> {
        let name = env.new_string(name)?;
        let uri = self.offer(env, activity, file, &name)?;
        let mime_type = self.type_of_name(env, &name)?;
        let mime_type = env.new_string(mime_type)?;
        let action = env.new_string("android.intent.action.VIEW")?;
        let intent = env.new_object(
            jni_str!("android/content/Intent"),
            jni_sig!("(Ljava/lang/String;)V"),
            &[JValue::Object(&action)],
        )?;
        env.call_method(
            &intent,
            jni_str!("setDataAndType"),
            jni_sig!("(Landroid/net/Uri;Ljava/lang/String;)Landroid/content/Intent;"),
            &[JValue::Object(&uri), JValue::Object(&mime_type)],
        )?;
        env.call_method(
            &intent,
            jni_str!("addFlags"),
            jni_sig!("(I)Landroid/content/Intent;"),
            &[JValue::Int(GRANT_READ)],
        )?;
        Ok(intent)
    }

    /// The chooser `Intent` of a share: `ACTION_SEND`, or
    /// `ACTION_SEND_MULTIPLE` for several files, whose clip carries the
    /// read grant to the target the user picks.
    fn share_intent<'local>(
        &self,
        env: &mut Env<'local>,
        activity: &JObject<'_>,
        title: Option<&str>,
        text: Option<&str>,
        files: &[(File, String)],
    ) -> jni::errors::Result<JObject<'local>> {
        let action = env.new_string(if files.len() > 1 {
            "android.intent.action.SEND_MULTIPLE"
        } else {
            "android.intent.action.SEND"
        })?;
        let send = env.new_object(
            jni_str!("android/content/Intent"),
            jni_sig!("(Ljava/lang/String;)V"),
            &[JValue::Object(&action)],
        )?;
        put_string(env, &send, "android.intent.extra.TEXT", text)?;
        put_string(env, &send, "android.intent.extra.SUBJECT", title)?;
        put_string(env, &send, "android.intent.extra.TITLE", title)?;
        let title = nullable(env, title)?;
        if files.is_empty() {
            with_string(env, &send, jni_str!("setType"), "text/plain")?;
        } else {
            let stream = env.new_string("android.intent.extra.STREAM")?;
            let uris = env.new_object(jni_str!("java/util/ArrayList"), jni_sig!("()V"), &[])?;
            let mut types = Vec::with_capacity(files.len());
            let mut clip = JObject::null();
            for (file, name) in files {
                let name = env.new_string(name)?;
                let uri = self.offer(env, activity, file, &name)?;
                let mime_type = self.type_of_name(env, &name)?;
                env.call_method(
                    &uris,
                    jni_str!("add"),
                    jni_sig!("(Ljava/lang/Object;)Z"),
                    &[JValue::Object(&uri)],
                )?;
                let item = env.new_object(
                    jni_str!("android/content/ClipData$Item"),
                    jni_sig!("(Landroid/net/Uri;)V"),
                    &[JValue::Object(&uri)],
                )?;
                if clip.is_null() {
                    let label = env.new_string("")?;
                    let clip_types = string_array(env, &[&mime_type])?;
                    clip = env.new_object(
                        jni_str!("android/content/ClipData"),
                        jni_sig!(
                            "(Ljava/lang/CharSequence;[Ljava/lang/String;Landroid/content/ClipData$Item;)V"
                        ),
                        &[
                            JValue::Object(&label),
                            JValue::Object(&clip_types),
                            JValue::Object(&item),
                        ],
                    )?;
                } else {
                    env.call_method(
                        &clip,
                        jni_str!("addItem"),
                        jni_sig!("(Landroid/content/ClipData$Item;)V"),
                        &[JValue::Object(&item)],
                    )?;
                }
                if files.len() == 1 {
                    env.call_method(
                        &send,
                        jni_str!("putExtra"),
                        jni_sig!(
                            "(Ljava/lang/String;Landroid/os/Parcelable;)Landroid/content/Intent;"
                        ),
                        &[JValue::Object(&stream), JValue::Object(&uri)],
                    )?;
                }
                types.push(mime_type);
            }
            if files.len() > 1 {
                env.call_method(
                    &send,
                    jni_str!("putParcelableArrayListExtra"),
                    jni_sig!("(Ljava/lang/String;Ljava/util/ArrayList;)Landroid/content/Intent;"),
                    &[JValue::Object(&stream), JValue::Object(&uris)],
                )?;
            }
            let common =
                common_type(types.iter().map(String::as_str)).unwrap_or_else(|| "*/*".to_owned());
            with_string(env, &send, jni_str!("setType"), &common)?;
            env.call_method(
                &send,
                jni_str!("setClipData"),
                jni_sig!("(Landroid/content/ClipData;)V"),
                &[JValue::Object(&clip)],
            )?;
            env.call_method(
                &send,
                jni_str!("addFlags"),
                jni_sig!("(I)Landroid/content/Intent;"),
                &[JValue::Int(GRANT_READ)],
            )?;
        }
        env.call_static_method(
            jni_str!("android/content/Intent"),
            jni_str!("createChooser"),
            jni_sig!("(Landroid/content/Intent;Ljava/lang/CharSequence;)Landroid/content/Intent;"),
            &[JValue::Object(&send), JValue::Object(&title)],
        )?
        .l()
    }

    /// View `uri` in the activity Android resolves for it, on Java's main
    /// thread: `ACTION_VIEW` under `CATEGORY_BROWSABLE`, with no grant.
    /// `heard` hears what the start did; `false`, and `heard` is dropped
    /// unrun, when the job was not posted.
    pub(crate) fn view(
        &self,
        activity: &Activity,
        uri: String,
        heard: impl FnOnce(StartAnswer) + Send + 'static,
    ) -> bool {
        self.on_main_thread(activity, move |env, activity| {
            let intent = view_intent(env, &uri);
            heard(start_activity(env, activity, intent));
        })
    }

    /// View the document open as `file` under `name`, on Java's main
    /// thread; `heard` and the answer as for [`view`](Self::view).
    pub(crate) fn view_document(
        self: &Arc<Self>,
        activity: &Activity,
        file: File,
        name: String,
        heard: impl FnOnce(StartAnswer) + Send + 'static,
    ) -> bool {
        let java = Arc::clone(self);
        self.on_main_thread(activity, move |env, activity| {
            let intent = java.document_intent(env, activity, &file, &name);
            heard(start_activity(env, activity, intent));
        })
    }

    /// Show the share sheet with `text` and `files`, each open and named,
    /// on Java's main thread; `heard` and the answer as for
    /// [`view`](Self::view).
    pub(crate) fn share(
        self: &Arc<Self>,
        activity: &Activity,
        title: Option<String>,
        text: Option<String>,
        files: Vec<(File, String)>,
        heard: impl FnOnce(StartAnswer) + Send + 'static,
    ) -> bool {
        let java = Arc::clone(self);
        self.on_main_thread(activity, move |env, activity| {
            let intent =
                java.share_intent(env, activity, title.as_deref(), text.as_deref(), &files);
            heard(start_activity(env, activity, intent));
        })
    }
}

/// The `Intent` that views `uri`: `ACTION_VIEW` under `CATEGORY_BROWSABLE`.
fn view_intent<'local>(env: &mut Env<'local>, uri: &str) -> jni::errors::Result<JObject<'local>> {
    let action = env.new_string("android.intent.action.VIEW")?;
    let uri = env.new_string(uri)?;
    let parsed = env
        .call_static_method(
            jni_str!("android/net/Uri"),
            jni_str!("parse"),
            jni_sig!("(Ljava/lang/String;)Landroid/net/Uri;"),
            &[JValue::Object(&uri)],
        )?
        .l()?;
    let intent = env.new_object(
        jni_str!("android/content/Intent"),
        jni_sig!("(Ljava/lang/String;Landroid/net/Uri;)V"),
        &[JValue::Object(&action), JValue::Object(&parsed)],
    )?;
    with_string(
        env,
        &intent,
        jni_str!("addCategory"),
        "android.intent.category.BROWSABLE",
    )?;
    Ok(intent)
}
