//! Starting an activity on Java's main thread: the `Intent` of a view, of a
//! document's view and of a share, and what the start answered.
//!
//! A file leaves as a content URI of `FluiDocuments`, never as a path: the
//! provider is handed the open descriptor, and the `Intent` carries the
//! read grant for the one activity Android resolves, or for the target the
//! user picks in the chooser. Every offer a start made is withdrawn when the
//! start did not happen (`StartAnswer::keeps_offers`), whether building its
//! `Intent` threw or Android refused it; an offer of a start that happened
//! ends at its grantee's first open, or at `FluiDocuments.OFFER_LIFETIME_MS`.

use std::fs::File;
use std::os::fd::AsRawFd as _;
use std::sync::Arc;

use flui_platform::capability::{Activity, Completion, CompletionSource, MainThreadRefused};
use jni::objects::{Global, JObject, JString};
use jni::{Env, JValue, jni_sig, jni_str};

use super::super::StartAnswer;
use super::super::share::common_type;
use super::{Java, in_frame, is_a, nullable, put_string, string_array, thrown, with_string};

/// `Intent.FLAG_GRANT_READ_URI_PERMISSION`.
const GRANT_READ: i32 = 1;

/// Why an `Intent` that hands over files was not built.
enum IntentError {
    /// `FluiDocuments.offer` refused one more document: it serves as many as
    /// it keeps.
    TooManyOffered,
    /// A JNI call failed; a Java exception it left was taken off the thread.
    Jni,
}

impl From<jni::errors::Error> for IntentError {
    fn from(_: jni::errors::Error) -> Self {
        Self::Jni
    }
}

/// The content URIs a start offered, each held until the start's answer
/// says whether they stay.
#[derive(Default)]
struct Offers(Vec<Global<JObject<'static>>>);

/// Start `intent` from `activity`, on Java's main thread, and say what the
/// start did.
fn start_activity(
    env: &mut Env<'_>,
    activity: &JObject<'_>,
    intent: Result<JObject<'_>, IntentError>,
) -> StartAnswer {
    let intent = match intent {
        Ok(intent) => intent,
        Err(IntentError::TooManyOffered) => return StartAnswer::TooManyOffered,
        Err(IntentError::Jni) => {
            env.exception_clear();
            return StartAnswer::Failed;
        }
    };
    let started = env
        .call_method(
            activity,
            jni_str!("startActivity"),
            jni_sig!("(Landroid/content/Intent;)V"),
            &[JValue::Object(&intent)],
        )
        .map(drop);
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
    /// under, named `name`, recorded in `offers`. The provider duplicates
    /// the descriptor, so `file` stays the caller's.
    fn offer<'local>(
        &self,
        env: &mut Env<'local>,
        context: &JObject<'_>,
        file: &File,
        name: &JString<'_>,
        offers: &mut Offers,
    ) -> Result<JObject<'local>, IntentError> {
        let offered = env.call_static_method(
            self.documents()?,
            jni_str!("offer"),
            jni_sig!("(Landroid/content/Context;ILjava/lang/String;)Landroid/net/Uri;"),
            &[
                JValue::Object(context),
                JValue::Int(file.as_raw_fd()),
                JValue::Object(name),
            ],
        );
        let uri = match offered {
            Ok(uri) => uri.l()?,
            // The one exception `offer` throws by contract is its refusal of
            // one more document; any other is a failure.
            Err(jni::errors::Error::JavaException) => {
                let full = thrown(env).is_some_and(|exception| {
                    is_a(env, &exception, jni_str!("java/lang/IllegalStateException"))
                });
                return Err(if full {
                    IntentError::TooManyOffered
                } else {
                    IntentError::Jni
                });
            }
            Err(error) => return Err(error.into()),
        };
        if uri.is_null() {
            return Err(jni::errors::Error::NullPtr("FluiDocuments.offer").into());
        }
        offers.0.push(env.new_global_ref(&uri)?);
        Ok(uri)
    }

    /// Withdraw every offer in `offers` (`FluiDocuments.withdraw`): their
    /// start did not happen, so no grant reaches them.
    fn withdraw(&self, env: &mut Env<'_>, offers: Offers) {
        let Ok(documents) = self.documents() else {
            return;
        };
        for uri in offers.0 {
            let withdrawn = env.call_static_method(
                documents,
                jni_str!("withdraw"),
                jni_sig!("(Landroid/net/Uri;)V"),
                &[JValue::Object(uri.as_obj())],
            );
            if withdrawn.is_err() {
                env.exception_clear();
            }
        }
    }

    /// The MIME type `FluiDocuments` serves a document named `name` as.
    fn type_of_name(&self, env: &mut Env<'_>, name: &JString<'_>) -> jni::errors::Result<String> {
        let mime_type = env
            .call_static_method(
                self.documents()?,
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
        offers: &mut Offers,
    ) -> Result<JObject<'local>, IntentError> {
        let name = env.new_string(name)?;
        let uri = self.offer(env, activity, file, &name, offers)?;
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

    /// Offer `file` as `name` for a share and add it to `uris` and to the
    /// share's clip, made from the first file; its local references go with
    /// the frame of this one file. Answers the file's MIME type.
    #[expect(
        clippy::too_many_arguments,
        reason = "one file's step of the share's build, over the build's state"
    )]
    fn share_file(
        &self,
        env: &mut Env<'_>,
        activity: &JObject<'_>,
        (file, name): &(File, String),
        send: &JObject<'_>,
        uris: &JObject<'_>,
        clip: &mut Option<Global<JObject<'static>>>,
        alone: bool,
        offers: &mut Offers,
    ) -> Result<String, IntentError> {
        env.with_local_frame(16, |env| -> Result<String, IntentError> {
            let name = env.new_string(name)?;
            let uri = self.offer(env, activity, file, &name, offers)?;
            let mime_type = self.type_of_name(env, &name)?;
            env.call_method(
                uris,
                jni_str!("add"),
                jni_sig!("(Ljava/lang/Object;)Z"),
                &[JValue::Object(&uri)],
            )?;
            let item = env.new_object(
                jni_str!("android/content/ClipData$Item"),
                jni_sig!("(Landroid/net/Uri;)V"),
                &[JValue::Object(&uri)],
            )?;
            match clip {
                Some(clip) => {
                    env.call_method(
                        clip.as_obj(),
                        jni_str!("addItem"),
                        jni_sig!("(Landroid/content/ClipData$Item;)V"),
                        &[JValue::Object(&item)],
                    )?;
                }
                None => {
                    let label = env.new_string("")?;
                    let clip_types = string_array(env, &[&mime_type])?;
                    let made = env.new_object(
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
                    *clip = Some(env.new_global_ref(&made)?);
                }
            }
            if alone {
                let stream = env.new_string("android.intent.extra.STREAM")?;
                env.call_method(
                    send,
                    jni_str!("putExtra"),
                    jni_sig!("(Ljava/lang/String;Landroid/os/Parcelable;)Landroid/content/Intent;"),
                    &[JValue::Object(&stream), JValue::Object(&uri)],
                )?;
            }
            Ok(mime_type)
        })
    }

    /// The chooser `Intent` of a share: `ACTION_SEND`, or
    /// `ACTION_SEND_MULTIPLE` for several files, whose clip carries the
    /// read grant to the target the user picks.
    fn share_intent<'local>(
        &self,
        env: &mut Env<'local>,
        activity: &JObject<'_>,
        (title, text): (Option<&str>, Option<&str>),
        files: &[(File, String)],
        offers: &mut Offers,
    ) -> Result<JObject<'local>, IntentError> {
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
            let uris = env.new_object(jni_str!("java/util/ArrayList"), jni_sig!("()V"), &[])?;
            let mut types = Vec::with_capacity(files.len());
            let mut clip = None;
            for file in files {
                let mime_type = self.share_file(
                    env,
                    activity,
                    file,
                    &send,
                    &uris,
                    &mut clip,
                    files.len() == 1,
                    offers,
                )?;
                types.push(mime_type);
            }
            if files.len() > 1 {
                let stream = env.new_string("android.intent.extra.STREAM")?;
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
            if let Some(clip) = &clip {
                env.call_method(
                    &send,
                    jni_str!("setClipData"),
                    jni_sig!("(Landroid/content/ClipData;)V"),
                    &[JValue::Object(clip.as_obj())],
                )?;
            }
            env.call_method(
                &send,
                jni_str!("addFlags"),
                jni_sig!("(I)Landroid/content/Intent;"),
                &[JValue::Int(GRANT_READ)],
            )?;
        }
        Ok(env
            .call_static_method(
                jni_str!("android/content/Intent"),
                jni_str!("createChooser"),
                jni_sig!(
                    "(Landroid/content/Intent;Ljava/lang/CharSequence;)Landroid/content/Intent;"
                ),
                &[JValue::Object(&send), JValue::Object(&title)],
            )?
            .l()?)
    }

    /// Start the `Intent` `build` makes, withdrawing every offer it made
    /// unless the start happened.
    fn start_with_offers<'local>(
        &self,
        env: &mut Env<'local>,
        activity: &JObject<'_>,
        build: impl FnOnce(&mut Env<'local>, &mut Offers) -> Result<JObject<'local>, IntentError>,
    ) -> StartAnswer {
        let mut offers = Offers::default();
        let intent = build(env, &mut offers);
        let answer = start_activity(env, activity, intent);
        if !answer.keeps_offers() {
            self.withdraw(env, offers);
        }
        answer
    }

    /// View the document open as `file` under `name`, on Java's main
    /// thread; the completion as for [`view`].
    pub(crate) fn view_document(
        self: &Arc<Self>,
        activity: &Activity,
        completions: &CompletionSource,
        file: File,
        name: String,
    ) -> Started {
        let java = Arc::clone(self);
        activity.on_main_thread(completions, move |frame| {
            in_frame(frame, |env, activity| {
                Ok(java.start_with_offers(env, activity, |env, offers| {
                    java.document_intent(env, activity, &file, &name, offers)
                }))
            })
            .unwrap_or(StartAnswer::Failed)
        })
    }

    /// Show the share sheet with `text` and `files`, each open and named,
    /// on Java's main thread; the completion as for [`view`].
    pub(crate) fn share(
        self: &Arc<Self>,
        activity: &Activity,
        completions: &CompletionSource,
        title: Option<String>,
        text: Option<String>,
        files: Vec<(File, String)>,
    ) -> Started {
        let java = Arc::clone(self);
        activity.on_main_thread(completions, move |frame| {
            in_frame(frame, |env, activity| {
                Ok(java.start_with_offers(env, activity, |env, offers| {
                    java.share_intent(
                        env,
                        activity,
                        (title.as_deref(), text.as_deref()),
                        &files,
                        offers,
                    )
                }))
            })
            .unwrap_or(StartAnswer::Failed)
        })
    }
}

/// What a start on Java's main thread answers: what it did, or why the host
/// did not run it there.
pub(crate) type Started = Completion<Result<StartAnswer, MainThreadRefused>>;

/// View `uri` in the activity Android resolves for it, on Java's main
/// thread: `ACTION_VIEW` under `CATEGORY_BROWSABLE`, with no grant, and none
/// of the crate's classes.
pub(crate) fn view(activity: &Activity, completions: &CompletionSource, uri: String) -> Started {
    activity.on_main_thread(completions, move |frame| {
        in_frame(frame, |env, activity| {
            let intent = view_intent(env, &uri).map_err(IntentError::from);
            Ok(start_activity(env, activity, intent))
        })
        .unwrap_or(StartAnswer::Failed)
    })
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
