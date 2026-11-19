//! Android as the providers call it through JNI, and the registration.
//!
//! The library keeps no `JavaVM` of its own: a UI runtime's first provider
//! reads the VM, the Activity's class loader and the application's content
//! resolver in a frame the host's Activity lends ([`Java::load`]) and keeps
//! them with the runtime (`ProviderContext::runtime_local`). A call on the
//! owner thread that needs the Activity runs in a frame it lends; a worker
//! attaches itself to the VM for each call; work for Java's main thread goes
//! through the host (`Activity::on_main_thread`), which runs it inside its
//! panic boundary and refuses it past its deadline.
//!
//! The crate's own classes are reached only by the providers that need them
//! ([`Java::classes`]): the launcher and the share sheet hand files through
//! `FluiDocuments`, and the message box's listeners are a
//! `java.lang.reflect.Proxy` whose handler is `FluiCallback`. They are read
//! once, `FluiCallback.PROTOCOL` is checked before anything is bound, and the
//! user's folders, the system information and the file dialogs work without
//! them. Each `FluiCallback` holds a handle, an [`Arc`] of a [`CallbackSlot`]
//! the library leaked for it: `FluiCallback.called` runs the slot's closure,
//! and `FluiCallback.released`, once the object is collected, gives the
//! reference back.
//!
//! No compiler checks the class names, method names and signatures spelled
//! here: each was read against the platform's `android.jar`.

mod alert;
mod saf;
mod starts;

use std::cell::RefCell;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use flui_platform::capability::{
    Activity, Capability, CapabilityRegistrar, JniFrame, ProviderContext, Resolver,
};
use flui_platform::launch::{Launcher, LauncherCapability};
use flui_platform::{Unsupported, UnsupportedReason};
use jni::objects::{Global, JClass, JObject, JObjectArray, JString, JThrowable};
use jni::strings::JNIStr;
use jni::sys::{jint, jlong};
use jni::{Env, EnvUnowned, JValue, JavaVM, NativeMethod, jni_sig, jni_str};

pub(crate) use alert::BoxOnScreen;
use saf::{JavaPicker, JavaReach};
pub(crate) use starts::{Started, view};

use super::documents::{self, MAX_NAME_UNITS};
use super::workers::Workers;
use super::{CALLBACK_CLASS, DOCUMENTS_CLASS, PROTOCOL};
use crate::autostart::AutostartCapability;
use crate::dialog::{FileDialogCapability, MessageBoxCapability, MessageBoxes};
use crate::grant::{GrantCapability, Grants};
use crate::jump_list::JumpListCapability;
use crate::os::{Os, OsCapability};
use crate::owner::android::native_call::jni_native;
use crate::share::{Share, ShareCapability};
use crate::shortcuts::GlobalShortcutCapability;
use crate::taskbar::TaskbarCapability;
use crate::user_dirs::{UserDirCapability, UserDirs};

/// What a call Android makes through a `FluiCallback` runs: it is given the
/// calling thread's environment, the interface method's name, and the `int`
/// the method carries (`OnClickListener.onClick`'s button), else 0.
type Called = Box<dyn FnMut(&mut Env<'_>, &str, i32) + Send>;

/// The closure one `FluiCallback` runs, for as long as Java holds the
/// object.
struct CallbackSlot(Mutex<Option<Called>>);

impl CallbackSlot {
    fn new(called: impl FnMut(&mut Env<'_>, &str, i32) + Send + 'static) -> Arc<Self> {
        Arc::new(Self(Mutex::new(Some(Box::new(called)))))
    }

    /// Run the closure for one call. It runs outside the lock, so a call
    /// Android makes from inside it finds nothing to run and returns.
    fn call(&self, env: &mut Env<'_>, method: &str, argument: i32) {
        let taken = self.0.lock().unwrap_or_else(PoisonError::into_inner).take();
        let Some(mut called) = taken else {
            return;
        };
        called(env, method, argument);
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(called);
    }
}

extern "system" fn called<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
    method: JString<'local>,
    argument: jint,
) {
    jni_native(
        "FluiCallback.called",
        &mut env,
        |()| (),
        |env| {
            if handle != 0 {
                let method = method.try_to_string(env)?;
                // SAFETY: a non-zero handle is an `Arc<CallbackSlot>` leaked
                // by `Java::proxy` for the `FluiCallback` that passes it,
                // which is reachable for the length of this call, so it has
                // not been collected and `released`, the one place the
                // reference is given back, has not run for it.
                let slot = unsafe { &*(handle as *const CallbackSlot) };
                slot.call(env, &method, argument);
            }
            Ok(())
        },
    );
}

extern "system" fn released<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
) {
    jni_native(
        "FluiCallback.released",
        &mut env,
        |()| (),
        |_| {
            if handle != 0 {
                // SAFETY: a non-zero handle is an `Arc<CallbackSlot>` leaked
                // by `Java::proxy`; `FluiCallback` passes it here once, from
                // the phantom reference enqueued after its object was
                // collected, so nothing calls the object again and this takes
                // back the one reference Java held.
                drop(unsafe { Arc::from_raw(handle as *const CallbackSlot) });
            }
            Ok(())
        },
    );
}

/// Run `call` with the environment and the object a host's call lends in
/// `frame`.
fn in_frame<R>(
    frame: JniFrame,
    call: impl FnOnce(&mut Env<'_>, &JObject<'_>) -> jni::errors::Result<R>,
) -> jni::errors::Result<R> {
    // SAFETY: the host lends the `JNIEnv*` of this thread, valid until its
    // call returns; the guard stays on this stack frame, inside that call.
    let mut guard = unsafe { jni::AttachGuard::from_unowned(frame.env().cast()) };
    let env = guard.borrow_env_mut();
    // SAFETY: the host lends a reference valid on this thread until its call
    // returns, and keeps its own wrapper of it out of this call's reach.
    let lent = unsafe { JObject::from_raw(env, frame.object().cast()) };
    call(env, &lent)
}

/// The exception a failed call left pending, taken off the thread.
fn thrown<'local>(env: &mut Env<'local>) -> Option<JThrowable<'local>> {
    let exception = env.exception_occurred();
    env.exception_clear();
    exception
}

/// Whether `exception` is an instance of the class named `class`.
fn is_a(env: &mut Env<'_>, exception: &JThrowable<'_>, class: &JNIStr) -> bool {
    let is = env.is_instance_of(exception, class).unwrap_or(false);
    // A class this Android does not have leaves its own exception.
    env.exception_clear();
    is
}

/// `value` as a Java string, `null` for none.
fn nullable<'local>(
    env: &mut Env<'local>,
    value: Option<&str>,
) -> jni::errors::Result<JString<'local>> {
    match value {
        Some(value) => env.new_string(value),
        None => Ok(JString::null()),
    }
}

/// `values` as a `String[]`.
fn string_array<'local>(
    env: &mut Env<'local>,
    values: &[&str],
) -> jni::errors::Result<JObjectArray<'local>> {
    let array = env.new_object_array(
        i32::try_from(values.len()).unwrap_or(i32::MAX),
        jni_str!("java/lang/String"),
        JObject::null(),
    )?;
    for (index, value) in values.iter().enumerate() {
        let value = env.new_string(value)?;
        array.set_element(env, index, &value)?;
    }
    Ok(array)
}

/// The string `value` holds when it has at most `max_units` UTF-16 units;
/// `None` for `null` and for a longer one, which is not copied.
fn bounded_string(
    env: &mut Env<'_>,
    value: JObject<'_>,
    max_units: usize,
) -> jni::errors::Result<Option<String>> {
    if value.is_null() {
        return Ok(None);
    }
    let units = env
        .call_method(&value, jni_str!("length"), jni_sig!("()I"), &[])?
        .i()?;
    if usize::try_from(units).is_ok_and(|units| units <= max_units) {
        let value = env.cast_local::<JString<'_>>(value)?;
        value.try_to_string(env).map(Some)
    } else {
        Ok(None)
    }
}

/// `Intent.putExtra(String, String)` on `intent`, for a value there is.
fn put_string(
    env: &mut Env<'_>,
    intent: &JObject<'_>,
    extra: &str,
    value: Option<&str>,
) -> jni::errors::Result<()> {
    let Some(value) = value else {
        return Ok(());
    };
    let extra = env.new_string(extra)?;
    let value = env.new_string(value)?;
    env.call_method(
        intent,
        jni_str!("putExtra"),
        jni_sig!("(Ljava/lang/String;Ljava/lang/String;)Landroid/content/Intent;"),
        &[JValue::Object(&extra), JValue::Object(&value)],
    )
    .map(drop)
}

/// A call on `intent` of the method `name`, which takes a string and
/// answers the `Intent`: `setAction`, `setType`, `addCategory`.
fn with_string(
    env: &mut Env<'_>,
    intent: &JObject<'_>,
    name: &JNIStr,
    value: &str,
) -> jni::errors::Result<()> {
    let value = env.new_string(value)?;
    env.call_method(
        intent,
        name,
        jni_sig!("(Ljava/lang/String;)Landroid/content/Intent;"),
        &[JValue::Object(&value)],
    )
    .map(drop)
}

/// An answer that two threads race to give: a message box's listener on
/// Java's main thread, and its hold or the refusal of its work on the owner
/// thread. Whichever takes it first answers.
pub(crate) struct Waiting<T>(Arc<Mutex<Option<Resolver<T>>>>);

impl<T> Clone for Waiting<T> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl<T: Send + 'static> Waiting<T> {
    /// `answer`, waiting for its value.
    pub(crate) fn new(answer: Resolver<T>) -> Self {
        Self(Arc::new(Mutex::new(Some(answer))))
    }

    /// The answer, for whoever takes it first.
    pub(crate) fn take(&self) -> Option<Resolver<T>> {
        // The lock is held for the take alone.
        self.0.lock().unwrap_or_else(PoisonError::into_inner).take()
    }

    /// Answer `value`, unless the answer was taken already.
    pub(crate) fn answer(&self, value: T) {
        if let Some(answer) = self.take() {
            // The owner retiring first leaves nothing waiting.
            let _ = answer.resolve(value);
        }
    }
}

/// Why the crate's Java classes were not reached.
#[derive(Clone, Copy, Debug, thiserror::Error)]
enum NotLoaded {
    /// The Activity's class loader lacks them: the APK was packed without
    /// the crate's dex.
    #[error("the APK holds no dev.flui.system classes")]
    NotPackaged,
    /// The classes in the APK state another protocol than the library's.
    #[error("the APK's classes state protocol {0}, the library {PROTOCOL}")]
    OtherProtocol(i32),
    /// Android never attached `FluiDocuments`: the manifest declares no
    /// `<provider>` of the class.
    #[error("the manifest declares no provider of dev.flui.system.FluiDocuments")]
    NoProvider,
    /// A JNI call failed; the failure was logged.
    #[error("a JNI call failed")]
    Jni,
}

/// The class `loader` finds by the binary name `name`, or `None` when it
/// throws (`ClassNotFoundException`); the exception is cleared.
fn load_class<'local>(
    env: &mut Env<'local>,
    loader: &JObject<'_>,
    name: &str,
) -> jni::errors::Result<Option<JClass<'local>>> {
    let binary_name = env.new_string(name)?;
    let loaded = env.call_method(
        loader,
        jni_str!("loadClass"),
        jni_sig!("(Ljava/lang/String;)Ljava/lang/Class;"),
        &[JValue::Object(&binary_name)],
    );
    match loaded {
        Ok(class) => {
            let class = class.l()?;
            Ok(Some(env.cast_local::<JClass<'_>>(class)?))
        }
        Err(jni::errors::Error::JavaException) => {
            env.exception_clear();
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

/// The crate's own classes, read and bound once.
struct Classes {
    callback: Global<JClass<'static>>,
    documents: Global<JClass<'static>>,
}

/// What the providers of one UI runtime call Android through.
pub(crate) struct Java {
    vm: JavaVM,
    /// The Activity's class loader, which holds the crate's classes and
    /// defines the proxies.
    loader: Global<JObject<'static>>,
    /// The application's `ContentResolver`, which outlives an Activity.
    resolver: Global<JObject<'static>>,
    /// The crate's classes, once a provider that needs them asked.
    classes: OnceLock<Result<Classes, NotLoaded>>,
}

impl Java {
    /// Read the class loader and the content resolver through the Activity
    /// lent in `frame`.
    fn load(frame: JniFrame) -> jni::errors::Result<Self> {
        in_frame(frame, |env, activity| {
            let loader = env
                .call_method(
                    activity,
                    jni_str!("getClassLoader"),
                    jni_sig!("()Ljava/lang/ClassLoader;"),
                    &[],
                )?
                .l()?;
            let context = env
                .call_method(
                    activity,
                    jni_str!("getApplicationContext"),
                    jni_sig!("()Landroid/content/Context;"),
                    &[],
                )?
                .l()?;
            let resolver = env
                .call_method(
                    &context,
                    jni_str!("getContentResolver"),
                    jni_sig!("()Landroid/content/ContentResolver;"),
                    &[],
                )?
                .l()?;
            // SAFETY: the host lends the process's `JavaVM*`, which lives as
            // long as the process.
            let vm = unsafe { JavaVM::from_raw(frame.vm().cast()) };
            Ok(Self {
                vm,
                loader: env.new_global_ref(&loader)?,
                resolver: env.new_global_ref(&resolver)?,
                classes: OnceLock::new(),
            })
        })
    }

    /// The crate's classes: read through the Activity's class loader the
    /// first time, their protocol checked before `FluiCallback`'s natives
    /// are bound, and `FluiDocuments` attached by Android under the
    /// authority the manifest gives it.
    fn classes(&self) -> Result<&Classes, NotLoaded> {
        self.classes
            .get_or_init(|| {
                self.attached(|env| self.read_classes(env))
                    .unwrap_or_else(|error| {
                        tracing::warn!(
                            target: "flui_system::android",
                            %error,
                            "reading flui-system's Java classes failed"
                        );
                        Err(NotLoaded::Jni)
                    })
            })
            .as_ref()
            .map_err(|not_loaded| *not_loaded)
    }

    fn read_classes(&self, env: &mut Env<'_>) -> jni::errors::Result<Result<Classes, NotLoaded>> {
        let (Some(callback), Some(documents)) = (
            load_class(env, &self.loader, CALLBACK_CLASS)?,
            load_class(env, &self.loader, DOCUMENTS_CLASS)?,
        ) else {
            return Ok(Err(NotLoaded::NotPackaged));
        };
        let protocol = env
            .get_static_field(&callback, jni_str!("PROTOCOL"), jni_sig!("I"))?
            .i()?;
        if protocol != PROTOCOL {
            return Ok(Err(NotLoaded::OtherProtocol(protocol)));
        }
        let authority = env
            .call_static_method(
                &documents,
                jni_str!("authority"),
                jni_sig!("()Ljava/lang/String;"),
                &[],
            )?
            .l()?;
        if authority.is_null() {
            return Ok(Err(NotLoaded::NoProvider));
        }
        // SAFETY: `called` and `released` are `extern "system"` functions
        // whose parameters are the JNI environment, the class (the methods
        // are static), and what each signature names: a `long`, a `String`
        // and an `int`, and a `long`; each returns `void`.
        let natives = unsafe {
            [
                NativeMethod::from_raw_parts(
                    jni_str!("called"),
                    jni_str!("(JLjava/lang/String;I)V"),
                    called as *mut std::ffi::c_void,
                ),
                NativeMethod::from_raw_parts(
                    jni_str!("released"),
                    jni_str!("(J)V"),
                    released as *mut std::ffi::c_void,
                ),
            ]
        };
        // SAFETY: as above.
        unsafe { env.register_native_methods(&callback, &natives) }?;
        Ok(Ok(Classes {
            callback: env.new_global_ref(&callback)?,
            documents: env.new_global_ref(&documents)?,
        }))
    }

    /// `FluiDocuments`, once [`classes`](Self::classes) answered.
    fn documents(&self) -> jni::errors::Result<&Global<JClass<'static>>> {
        self.classes()
            .map(|classes| &classes.documents)
            .map_err(|_| jni::errors::Error::NullPtr("FluiDocuments"))
    }

    /// Run `call` with this thread's environment, attaching the thread for
    /// the call when it is not attached; a Java exception `call` leaves is
    /// its error, and is cleared.
    fn attached<R>(
        &self,
        call: impl FnOnce(&mut Env<'_>) -> jni::errors::Result<R>,
    ) -> jni::errors::Result<R> {
        self.vm.attach_current_thread(call)
    }

    /// An object that implements `interfaces`, each call of which runs
    /// `slot`'s closure on the thread Android calls it on.
    fn proxy<'local>(
        &self,
        env: &mut Env<'local>,
        interfaces: &[&JNIStr],
        slot: &Arc<CallbackSlot>,
    ) -> jni::errors::Result<JObject<'local>> {
        let callback = &self
            .classes()
            .map_err(|_| jni::errors::Error::NullPtr("FluiCallback"))?
            .callback;
        let classes = env.new_object_array(
            i32::try_from(interfaces.len()).unwrap_or(i32::MAX),
            jni_str!("java/lang/Class"),
            JObject::null(),
        )?;
        for (index, interface) in interfaces.iter().enumerate() {
            let class = env.find_class(interface)?;
            classes.set_element(env, index, &class)?;
        }
        let leaked = Arc::into_raw(Arc::clone(slot));
        let handler = env.new_object(callback, jni_sig!("(J)V"), &[JValue::Long(leaked as jlong)]);
        let handler = match handler {
            Ok(handler) => handler,
            Err(error) => {
                // SAFETY: `leaked` came from `Arc::into_raw` above, and no
                // `FluiCallback` was made to hold it, so this is the one
                // place that gives the reference back.
                drop(unsafe { Arc::from_raw(leaked) });
                return Err(error);
            }
        };
        env.call_static_method(
            jni_str!("java/lang/reflect/Proxy"),
            jni_str!("newProxyInstance"),
            jni_sig!(
                "(Ljava/lang/ClassLoader;[Ljava/lang/Class;Ljava/lang/reflect/InvocationHandler;)Ljava/lang/Object;"
            ),
            &[
                JValue::Object(&self.loader),
                JValue::Object(&classes),
                JValue::Object(&handler),
            ],
        )?
        .l()
    }

    /// The MIME type Android maps `extension` to (`MimeTypeMap`), if any.
    pub(crate) fn mime_type_of(&self, extension: &str) -> Option<String> {
        self.attached(|env| {
            let map = env
                .call_static_method(
                    jni_str!("android/webkit/MimeTypeMap"),
                    jni_str!("getSingleton"),
                    jni_sig!("()Landroid/webkit/MimeTypeMap;"),
                    &[],
                )?
                .l()?;
            let extension = env.new_string(extension)?;
            let mime_type = env
                .call_method(
                    &map,
                    jni_str!("getMimeTypeFromExtension"),
                    jni_sig!("(Ljava/lang/String;)Ljava/lang/String;"),
                    &[JValue::Object(&extension)],
                )?
                .l()?;
            bounded_string(env, mime_type, MAX_NAME_UNITS)
        })
        .ok()
        .flatten()
    }
}

/// The folder `Environment` names for `kind`
/// (`getExternalStoragePublicDirectory`), as a path, read in the frame the
/// Activity lends. It reads no storage and needs none of the crate's
/// classes.
///
/// # Errors
///
/// The JNI call failed.
pub(crate) fn public_folder(frame: JniFrame, kind: &str) -> jni::errors::Result<Option<String>> {
    in_frame(frame, |env, _activity| {
        let kind = env.new_string(kind)?;
        let folder = env
            .call_static_method(
                jni_str!("android/os/Environment"),
                jni_str!("getExternalStoragePublicDirectory"),
                jni_sig!("(Ljava/lang/String;)Ljava/io/File;"),
                &[JValue::Object(&kind)],
            )?
            .l()?;
        if folder.is_null() {
            return Ok(None);
        }
        let path = env
            .call_method(
                &folder,
                jni_str!("getAbsolutePath"),
                jni_sig!("()Ljava/lang/String;"),
                &[],
            )?
            .l()?;
        bounded_string(env, path, MAX_NAME_UNITS)
    })
}

/// One UI runtime's way to Android, read with its first provider.
#[derive(Default)]
struct Loaded(RefCell<Option<Arc<Java>>>);

/// Whether a provider calls the crate's own Java classes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Reach {
    /// It calls `FluiDocuments` or `FluiCallback`: an APK without them, or
    /// with another version of them, refuses it.
    OwnClasses,
    /// It calls Android's classes alone.
    FrameworkOnly,
}

/// The Activity and the JNI state a provider of `C` works through.
///
/// A runtime whose host offers no Activity (a headless one) has no path to
/// Android and is refused with `NotYetOnThisHost`. A provider that needs the
/// crate's classes is refused with `NotDeclared` by an APK packed without
/// them, with another version of them, or whose manifest declares no
/// `FluiDocuments`.
fn host<C: Capability>(
    ctx: &ProviderContext<'_>,
    reach: Reach,
) -> Result<(Activity, Arc<Java>), Unsupported> {
    let refused = |reason| Unsupported::of::<C>(reason);
    let activity = ctx
        .activity()
        .ok_or_else(|| refused(UnsupportedReason::NotYetOnThisHost))?;
    let loaded = ctx.runtime_local::<Loaded>();
    let cached = loaded.0.borrow().clone();
    let java = match cached {
        Some(java) => java,
        // The log line follows the host's call, with nothing of its lent.
        None => match activity.with_java(Java::load) {
            Some(Ok(java)) => {
                let java = Arc::new(java);
                loaded.0.replace(Some(Arc::clone(&java)));
                java
            }
            Some(Err(error)) => {
                tracing::warn!(
                    target: "flui_system::android",
                    %error,
                    "the Activity's class loader and content resolver were not read"
                );
                return Err(refused(UnsupportedReason::Unavailable));
            }
            None => return Err(refused(UnsupportedReason::Unavailable)),
        },
    };
    if reach == Reach::OwnClasses
        && let Err(not_loaded) = java.classes()
    {
        tracing::warn!(
            target: "flui_system::android",
            reason = %not_loaded,
            "flui-system's Java classes were not reached"
        );
        return Err(refused(match not_loaded {
            NotLoaded::NotPackaged | NotLoaded::OtherProtocol(_) | NotLoaded::NoProvider => {
                UnsupportedReason::NotDeclared {
                    what: "flui-system's Java classes and content provider, which `flui build` packs and declares from the crate's declarations",
                }
            }
            NotLoaded::Jni => UnsupportedReason::Unavailable,
        }));
    }
    Ok((activity, java))
}

/// Provide `C` as a refusal: Android has no such thing.
fn not_on_android<C: Capability>(registrar: &mut CapabilityRegistrar) {
    registrar.provide::<C>(|_: &ProviderContext<'_>| {
        Err::<C::Handle, _>(Unsupported::of::<C>(UnsupportedReason::NotOnThisPlatform))
    });
}

/// Register the Android providers: the launcher, the file dialogs, the
/// message box, the share sheet, the user's folders and the system
/// information; autostart, global shortcuts, the launcher entry and the jump
/// list are refused, since an Android application has none of them.
pub(crate) fn register(registrar: &mut CapabilityRegistrar) {
    registrar.provide::<LauncherCapability>(|ctx: &ProviderContext<'_>| {
        let (activity, java) = host::<LauncherCapability>(ctx, Reach::OwnClasses)?;
        Ok::<_, Unsupported>(Launcher::new(
            super::launcher::AndroidLauncher::new(
                activity,
                java,
                ctx.runtime_local::<Workers>(),
                ctx.completions().clone(),
            ),
            ctx,
        ))
    });
    registrar.provide::<FileDialogCapability>(|ctx: &ProviderContext<'_>| {
        let (activity, java) = host::<FileDialogCapability>(ctx, Reach::FrameworkOnly)?;
        documents::provide(ctx, activity, std::rc::Rc::new(JavaPicker(java)))
    });
    registrar.provide::<MessageBoxCapability>(|ctx: &ProviderContext<'_>| {
        let (activity, java) = host::<MessageBoxCapability>(ctx, Reach::OwnClasses)?;
        MessageBoxes::try_new(
            super::message_box::AndroidMessageBoxes::new(activity, java, ctx.completions().clone()),
            ctx,
        )
    });
    registrar.provide::<ShareCapability>(|ctx: &ProviderContext<'_>| {
        let (activity, java) = host::<ShareCapability>(ctx, Reach::OwnClasses)?;
        Share::try_new(
            super::share::AndroidShare::new(
                activity,
                java,
                ctx.runtime_local::<Workers>(),
                ctx.completions().clone(),
            ),
            ctx,
        )
    });
    registrar.provide::<UserDirCapability>(|ctx: &ProviderContext<'_>| {
        let activity = ctx.activity().ok_or_else(|| {
            Unsupported::of::<UserDirCapability>(UnsupportedReason::NotYetOnThisHost)
        })?;
        Ok::<_, Unsupported>(UserDirs::new(
            super::user_dirs::AndroidUserDirs::new(activity),
            ctx,
        ))
    });
    registrar.provide::<OsCapability>(|ctx: &ProviderContext<'_>| {
        Ok::<_, Unsupported>(Os::new(super::os::AndroidOs::read(), ctx))
    });
    registrar.provide::<GrantCapability>(|ctx: &ProviderContext<'_>| {
        let (_activity, java) = host::<GrantCapability>(ctx, Reach::FrameworkOnly)?;
        Ok::<_, Unsupported>(Grants::new(
            super::grants::AndroidGrants::new(
                std::rc::Rc::new(JavaReach(java)),
                ctx.runtime_local::<Workers>(),
            ),
            ctx,
        ))
    });
    not_on_android::<AutostartCapability>(registrar);
    not_on_android::<GlobalShortcutCapability>(registrar);
    not_on_android::<TaskbarCapability>(registrar);
    not_on_android::<JumpListCapability>(registrar);
}
