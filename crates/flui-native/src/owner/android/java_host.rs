//! The contract between this library and FLUI's Java classes in the APK
//! (ADR-0255 decisions 1, 2 and 5): the handshake before the platform starts,
//! and the queue that carries what `FluiActivity` receives and
//! `NativeActivity` drops.
//!
//! The classes (`FluiActivity`, `FluiHost`, `FluiView` and their helpers) are
//! compiled from `crates/flui-native/android/java` into the checked-in dex,
//! which `flui build` packs into the APK from the `flui-native` the
//! application's lockfile resolves; the generated manifest names
//! `FluiActivity`. A packager that bypasses either can put another FLUI
//! version's classes next to this library, or none. So before the platform
//! starts, and so before any native method is bound, the owner reads through
//! the Activity's class loader whether the APK has `FluiHost`, the protocol it
//! states and whether the Activity is a `FluiActivity`, and [`check`] turns
//! anything else into a [`PlatformError`] naming what it found.
//!
//! `FluiActivity` forwards its callbacks into a queue in `FluiHost`, which
//! buffers until a platform binds its wake handle (an entry can arrive before
//! `android_main` runs). Once bound, each entry wakes the owner (ADR-0249),
//! which drains the queue on its own thread and reads each entry as a
//! [`HostEntry`]; an intent the Activity receives waits in a second queue, as
//! the record [`ReceivedIntent`](super::intent::ReceivedIntent) reads, and
//! the same drain takes it as [`HostEntry::Intent`]. The Java side queues and
//! wakes and never waits for the owner, and the owner never waits for the
//! Java main thread.

use super::intent::ReceivedIntent;
use crate::PlatformError;

/// The version of the contract between this library and the Java classes:
/// the native methods and their signatures, the methods the library calls,
/// the entries of the queue, and what each means. `FluiHost.PROTOCOL` states
/// it on the Java side; the two are raised together.
pub(crate) const PROTOCOL: i32 = 6;

/// The binary name of the Activity class the manifest names.
pub(crate) const ACTIVITY_CLASS: &str = "dev.flui.platform.FluiActivity";

/// The binary name of the class that states the protocol.
pub(crate) const HOST_CLASS: &str = "dev.flui.platform.FluiHost";

/// What the Activity's class loader found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Found {
    /// The class of the running Activity.
    pub(crate) activity: String,
    /// Whether that Activity is a `FluiActivity`, or a subclass of it.
    pub(crate) is_flui_activity: bool,
    /// `FluiHost.PROTOCOL`, or `None` when the loader has no `FluiHost`.
    pub(crate) protocol: Option<i32>,
}

/// Whether the platform may start over the classes `found` describes.
///
/// # Errors
///
/// [`PlatformError::HostProtocol`] when the classes state another protocol,
/// and [`PlatformError::Init`] when the APK has none of FLUI's classes or its
/// Activity is not a `FluiActivity`.
pub(crate) fn check(found: &Found) -> Result<(), PlatformError> {
    let Some(packaged) = found.protocol else {
        return Err(PlatformError::Init {
            message: format!(
                "the APK has no {HOST_CLASS}, so none of FLUI's Java classes: package the app \
                 with `flui build`, which puts flui-native's classes.dex into the APK and names \
                 {ACTIVITY_CLASS} in the manifest (ADR-0255)"
            ),
        });
    };
    if packaged != PROTOCOL {
        return Err(PlatformError::HostProtocol {
            library: PROTOCOL,
            packaged,
        });
    }
    if !found.is_flui_activity {
        return Err(PlatformError::Init {
            message: format!(
                "the manifest names {} as the Activity, which is not a {ACTIVITY_CLASS}; FLUI \
                 receives the Activity's callbacks through it, and `flui build` generates a \
                 manifest that names it (ADR-0255)",
                found.activity
            ),
        });
    }
    Ok(())
}

/// Read [`Found`] through the Activity's class loader and [`check`] it. On
/// whatever thread starts the platform, attached to the VM for the call.
///
/// # Errors
///
/// As [`check`], and [`PlatformError::Init`] when the classes cannot be read.
#[cfg(target_os = "android")]
pub(crate) fn verify(app: &android_activity::AndroidApp) -> Result<(), PlatformError> {
    let found = crate::owner::android::activity::with_activity(app, |env, activity| {
        env.with_local_frame(16, |env| read(env, activity))
    })
    .map_err(|error| PlatformError::Init {
        message: format!(
            "reading FLUI's Java classes through the Activity's class loader: {error}"
        ),
    })?;
    check(&found)
}

#[cfg(target_os = "android")]
fn read(
    env: &mut jni::Env<'_>,
    activity: &jni::objects::JObject<'_>,
) -> jni::errors::Result<Found> {
    use jni::{jni_sig, jni_str};

    let class = env.get_object_class(activity)?;
    let name = env
        .call_method(
            &class,
            jni_str!("getName"),
            jni_sig!("()Ljava/lang/String;"),
            &[],
        )?
        .l()?;
    let activity_name = env
        .cast_local::<jni::objects::JString<'_>>(name)?
        .try_to_string(env)?;
    let loader = env
        .call_method(
            activity,
            jni_str!("getClassLoader"),
            jni_sig!("()Ljava/lang/ClassLoader;"),
            &[],
        )?
        .l()?;
    let Some(host) = load_class(env, &loader, HOST_CLASS)? else {
        return Ok(Found {
            activity: activity_name,
            is_flui_activity: false,
            protocol: None,
        });
    };
    let protocol = env
        .get_static_field(&host, jni_str!("PROTOCOL"), jni_sig!("I"))?
        .i()?;
    let is_flui_activity = match load_class(env, &loader, ACTIVITY_CLASS)? {
        Some(flui_activity) => env.is_instance_of(activity, &flui_activity)?,
        None => false,
    };
    Ok(Found {
        activity: activity_name,
        is_flui_activity,
        protocol: Some(protocol),
    })
}

/// The class `loader` finds by the binary name `name`, or `None` when it
/// throws (`ClassNotFoundException`); the exception is cleared.
#[cfg(target_os = "android")]
pub(crate) fn load_class<'local>(
    env: &mut jni::Env<'local>,
    loader: &jni::objects::JObject<'_>,
    name: &str,
) -> jni::errors::Result<Option<jni::objects::JClass<'local>>> {
    use jni::{JValue, jni_sig, jni_str};

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
            Ok(Some(env.cast_local::<jni::objects::JClass<'_>>(class)?))
        }
        Err(jni::errors::Error::JavaException) => {
            env.exception_clear();
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

/// `FluiHost.TRIM_MEMORY`: the kind of an `onTrimMemory` entry.
const TRIM_MEMORY: i32 = 1;

/// `FluiHost.OPEN_FAILED`: the provider refused or failed.
#[cfg(all(test, not(target_arch = "wasm32")))]
const OPEN_FAILED: i32 = -1;
/// `FluiHost.OPEN_GRANT_ENDED`: the read grant the intent carried has ended.
#[cfg(any(target_os = "android", all(test, not(target_arch = "wasm32"))))]
const OPEN_GRANT_ENDED: i32 = -2;
/// `FluiHost.OPEN_CANCELLED`: the open was cancelled.
#[cfg(any(target_os = "android", all(test, not(target_arch = "wasm32"))))]
const OPEN_CANCELLED: i32 = -3;

/// What `FluiHost.openDocument` answered: the descriptor it detached, or why
/// it has none. A cancelled open passed its deadline, the only reason the
/// library cancels one before it retires, and an answer it does not know is
/// a failure.
#[cfg(any(target_os = "android", all(test, not(target_arch = "wasm32"))))]
pub(crate) fn opened_descriptor(answer: i32) -> Result<i32, super::documents::OpenRefusal> {
    use super::documents::OpenRefusal;
    match answer {
        descriptor if descriptor >= 0 => Ok(descriptor),
        OPEN_GRANT_ENDED => Err(OpenRefusal::GrantEnded),
        OPEN_CANCELLED => Err(OpenRefusal::TimedOut),
        // `OPEN_FAILED`, and any answer this library does not know.
        _ => Err(OpenRefusal::Failed),
    }
}

/// One entry of `FluiHost`'s queue, as the owner drains it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum HostEntry {
    /// `onTrimMemory(level)`, with the `ComponentCallbacks2` level.
    TrimMemory(i32),
    /// An intent `onCreate` or `onNewIntent` received.
    #[cfg_attr(
        all(not(target_os = "android"), not(test)),
        expect(dead_code, reason = "only the Android queue drains intents")
    )]
    Intent(ReceivedIntent),
}

impl HostEntry {
    /// The entries `FluiHost.drain` returns, laid out as kind and value
    /// pairs, oldest first. A kind this library does not know is skipped: the
    /// handshake rules it out, so it is a defect of one side, not input.
    pub(crate) fn from_pairs(pairs: &[i32]) -> Vec<Self> {
        pairs
            .as_chunks::<2>()
            .0
            .iter()
            .filter_map(|&pair| match pair {
                [TRIM_MEMORY, level] => Some(Self::TrimMemory(level)),
                [kind, value] => {
                    tracing::warn!(
                        target: "flui_native::android",
                        kind,
                        value,
                        "FluiHost queued an entry of a kind this library does not know"
                    );
                    None
                }
            })
            .collect()
    }

    /// The intents of the records `FluiHost.peekOpened` returned, oldest
    /// first, each read on its own: a record that could not be read
    /// (`None`) or does not read as an intent is skipped and logged, and the
    /// others are delivered.
    #[cfg_attr(
        all(not(target_os = "android"), not(test)),
        expect(dead_code, reason = "only the Android queue drains intents")
    )]
    pub(crate) fn from_records(records: Vec<Option<Vec<String>>>) -> Vec<Self> {
        records
            .into_iter()
            .filter_map(|record| {
                if record.is_none() {
                    tracing::warn!(
                        target: "flui_native::android",
                        "an intent FluiHost queued could not be read; it is dropped"
                    );
                }
                record
            })
            .filter_map(ReceivedIntent::from_record)
            .map(Self::Intent)
            .collect()
    }
}

#[cfg(target_os = "android")]
pub(crate) use queue::{HostQueue, JavaSignal, host_class, spawn_documents};

#[cfg(target_os = "android")]
mod queue {
    use std::os::fd::{FromRawFd as _, OwnedFd};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Weak};

    use android_activity::AndroidApp;
    use flui_platform::OpenedFile;
    use jni::objects::{JClass, JIntArray, JObject, JObjectArray, JString};
    use jni::refs::Global;
    use jni::sys::jlong;
    use jni::{Env, EnvUnowned, JValue, JavaVM, NativeMethod, jni_sig, jni_str};
    use parking_lot::Mutex;

    use super::{HOST_CLASS, HostEntry, load_class};
    use crate::owner::android::activity::with_activity;
    use crate::owner::android::activity_results::Answer;
    use crate::owner::android::documents::{
        CancelSignal, DocumentProvider, OpenRefusal, Opener, Outcome, work,
    };
    use crate::owner::android::java_view::strings;
    use crate::owner::android::main_thread;
    use crate::owner::android::native_call::jni_native;
    use crate::owner::android::result_bridge::{HeldIntent, drain_answers};
    use crate::owner::signal::OwnerSignal;

    /// What the native wake reaches: the flag the owner takes before it
    /// drains, and the owner's signal.
    struct Inbox {
        pending: AtomicBool,
        signal: Weak<OwnerSignal>,
    }

    impl Inbox {
        /// An entry is queued: mark it, then wake the owner. The flag is set
        /// first, so the turn the wake runs sees it.
        fn wake(&self) {
            self.pending.store(true, Ordering::Release);
            if let Some(signal) = self.signal.upgrade() {
                let _ = signal.wake();
            }
        }
    }

    /// Where the wake handle is. Changed on the Java main thread only.
    enum Binding {
        /// Posted to the Java main thread, not bound yet.
        Pending,
        /// `FluiHost` names `handle`, a `Box<Arc<Inbox>>`; `class` is
        /// `FluiHost`, held to unbind it without the Activity.
        Bound {
            handle: usize,
            class: Global<JClass<'static>>,
        },
        /// The platform ended; nothing binds any more.
        Detached,
    }

    /// The owner's end of `FluiHost`'s queue.
    pub(crate) struct HostQueue {
        app: AndroidApp,
        inbox: Arc<Inbox>,
        binding: Mutex<Binding>,
    }

    impl std::fmt::Debug for HostQueue {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("HostQueue").finish_non_exhaustive()
        }
    }

    impl HostQueue {
        /// The queue, with its wake handle bound on the Java main thread;
        /// entries queued before then wake `signal` once it is.
        pub(crate) fn start(app: AndroidApp, signal: Weak<OwnerSignal>) -> Arc<Self> {
            let queue = Arc::new(Self {
                app,
                inbox: Arc::new(Inbox {
                    pending: AtomicBool::new(false),
                    signal,
                }),
                binding: Mutex::new(Binding::Pending),
            });
            let binding = Arc::clone(&queue);
            main_thread::run(&queue.app, "FluiHost.bind", move || binding.bind());
            queue
        }

        /// Bind the natives and the wake handle. Java main thread.
        fn bind(&self) {
            if !matches!(*self.binding.lock(), Binding::Pending) {
                return;
            }
            let handle = Box::into_raw(Box::new(Arc::clone(&self.inbox)));
            let bound = with_activity(&self.app, |env, activity| {
                bind_host(env, activity, handle as jlong)
            });
            match bound {
                Ok(class) => {
                    *self.binding.lock() = Binding::Bound {
                        handle: handle as usize,
                        class,
                    };
                }
                Err(error) => {
                    tracing::warn!(
                        target: "flui_native::android",
                        %error,
                        "FluiHost's queue was not bound; the Activity's callbacks are not delivered"
                    );
                    free_handle(handle);
                }
            }
        }

        /// The platform ended: `FluiHost` forgets the handle, unless a later
        /// platform of the process bound its own since, and the handle is then
        /// freed, both on the Java main thread. Entries stay queued for the
        /// next platform of the process.
        pub(crate) fn detach(self: &Arc<Self>) {
            let this = Arc::clone(self);
            main_thread::run(&self.app, "FluiHost.unbind", move || {
                let previous = std::mem::replace(&mut *this.binding.lock(), Binding::Detached);
                let Binding::Bound { handle, class } = previous else {
                    return;
                };
                match attached(&this.app, |env| {
                    env.call_static_method(
                        &class,
                        jni_str!("unbind"),
                        jni_sig!("(J)V"),
                        &[JValue::Long(handle as jlong)],
                    )
                    .map(drop)
                }) {
                    Ok(()) => free_handle(handle as *mut Arc<Inbox>),
                    // `FluiHost` may still name the handle: it stays allocated.
                    Err(error) => tracing::warn!(
                        target: "flui_native::android",
                        %error,
                        "FluiHost's queue was not unbound; its wake handle is kept"
                    ),
                }
            });
        }

        /// Every entry queued since the last call, oldest first, then every
        /// intent, and every answer to a request for a result; empty, with
        /// no JNI call, unless a wake came in between. Owner thread.
        pub(crate) fn take(&self) -> (Vec<HostEntry>, Vec<Answer<HeldIntent>>) {
            if !self.inbox.pending.swap(false, Ordering::AcqRel) {
                return (Vec::new(), Vec::new());
            }
            // Each queue is drained on its own, and `FluiHost` lets go of
            // what was read only once it is read whole: a failure leaves that
            // queue's entries queued for the next turn and loses none.
            let pairs = self.drained("entries", drain);
            let records = self.drained("intents", drain_opened);
            let answers = self
                .drained("answers", drain_answers)
                .map(|(answers, remaining)| {
                    // More than one drain reads: the next turn reads the rest.
                    if remaining > 0 {
                        self.inbox.wake();
                    }
                    answers
                });
            let mut entries = HostEntry::from_pairs(&pairs.unwrap_or_default());
            entries.extend(HostEntry::from_records(records.unwrap_or_default()));
            (entries, answers.unwrap_or_default())
        }

        /// One drain of `FluiHost`, or `None` when it failed: what it queued
        /// stays queued, and the next turn tries again.
        fn drained<T>(
            &self,
            queue: &'static str,
            drain: impl FnOnce(&mut Env<'_>, &JObject<'_>) -> jni::errors::Result<T>,
        ) -> Option<T> {
            let drained = with_activity(&self.app, |env, activity| {
                env.with_local_frame(8, |env| drain(env, activity))
            });
            match drained {
                Ok(drained) => Some(drained),
                Err(error) => {
                    self.inbox.pending.store(true, Ordering::Release);
                    tracing::warn!(
                        target: "flui_native::android",
                        queue,
                        %error,
                        "FluiHost's queue could not be drained"
                    );
                    None
                }
            }
        }
    }

    /// `f` on this thread, attached to the VM, without the Activity.
    #[expect(unsafe_code, reason = "AndroidApp lends the live VM pointer to JNI")]
    fn attached<T>(
        app: &AndroidApp,
        f: impl FnOnce(&mut Env<'_>) -> jni::errors::Result<T>,
    ) -> jni::errors::Result<T> {
        // SAFETY: `vm_as_ptr` is the process's running VM, which outlives
        // every `AndroidApp`; `JavaVM` borrows it and destroys nothing.
        let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
        vm.attach_current_thread(|env| {
            let result = f(env);
            if result.is_err() {
                env.exception_clear();
            }
            result
        })
    }

    /// `FluiHost`, through the Activity's class loader.
    pub(crate) fn host_class<'local>(
        env: &mut Env<'local>,
        activity: &JObject<'_>,
    ) -> jni::errors::Result<JClass<'local>> {
        let loader = env
            .call_method(
                activity,
                jni_str!("getClassLoader"),
                jni_sig!("()Ljava/lang/ClassLoader;"),
                &[],
            )?
            .l()?;
        load_class(env, &loader, HOST_CLASS)?.ok_or(jni::errors::Error::JavaException)
    }

    /// Register `nativeWake` on `FluiHost` and hand it `handle`; the class,
    /// held globally, on success.
    fn bind_host(
        env: &mut Env<'_>,
        activity: &JObject<'_>,
        handle: jlong,
    ) -> jni::errors::Result<Global<JClass<'static>>> {
        let class = host_class(env, activity)?;
        let global = env.new_global_ref(&class)?;
        register_natives(env, &class)?;
        env.call_static_method(
            &class,
            jni_str!("bind"),
            jni_sig!("(J)V"),
            &[JValue::Long(handle)],
        )?;
        Ok(global)
    }

    /// Every entry `FluiHost` queued, read whole before `FluiHost` lets go
    /// of exactly those, so a failure leaves them queued.
    fn drain(env: &mut Env<'_>, activity: &JObject<'_>) -> jni::errors::Result<Vec<i32>> {
        let class = host_class(env, activity)?;
        let pairs = env
            .call_static_method(&class, jni_str!("peekEntries"), jni_sig!("()[I"), &[])?
            .l()?;
        let pairs = JIntArray::cast_local(env, pairs)?;
        let mut out = vec![0; pairs.len(env)?];
        pairs.get_region(env, 0, &mut out)?;
        let read = i32::try_from(out.len() / 2).map_err(|_| jni::errors::Error::JavaException)?;
        env.call_static_method(
            &class,
            jni_str!("commitEntries"),
            jni_sig!("(I)V"),
            &[JValue::Int(read)],
        )?;
        Ok(out)
    }

    /// The Java side of opening the documents an intent names, on the
    /// documents' worker: `FluiHost.openDocument` and `documentName` through
    /// the Activity's content resolver, each under the open's
    /// `CancellationSignal`.
    pub(crate) struct JavaDocuments {
        app: AndroidApp,
    }

    /// An open's `android.os.CancellationSignal`. Its `cancel` tells the
    /// provider through a one-way binder call, so the owner thread that sends
    /// it never waits for the provider.
    pub(crate) struct JavaSignal {
        app: AndroidApp,
        signal: Global<JObject<'static>>,
    }

    impl CancelSignal for JavaSignal {
        fn cancel(&self) {
            let cancelled = with_activity(&self.app, |env, _| {
                env.call_method(
                    self.signal.as_ref(),
                    jni_str!("cancel"),
                    jni_sig!("()V"),
                    &[],
                )
                .map(drop)
            });
            if let Err(error) = cancelled {
                tracing::warn!(
                    target: "flui_native::android",
                    %error,
                    "a document's open could not be cancelled"
                );
            }
        }
    }

    impl DocumentProvider for JavaDocuments {
        type Signal = JavaSignal;

        fn signal(&mut self) -> Result<JavaSignal, OpenRefusal> {
            with_activity(&self.app, |env, _| {
                let signal = env.new_object(
                    jni_str!("android/os/CancellationSignal"),
                    jni_sig!("()V"),
                    &[],
                )?;
                env.new_global_ref(&signal)
            })
            .map(|signal| JavaSignal {
                app: self.app.clone(),
                signal,
            })
            .map_err(|error| {
                tracing::warn!(
                    target: "flui_native::android",
                    %error,
                    "no cancellation signal for a document's open"
                );
                OpenRefusal::Failed
            })
        }

        fn open(&mut self, uri: &str, signal: &JavaSignal) -> Outcome {
            let opened = with_activity(&self.app, |env, activity| {
                env.with_local_frame(8, |env| {
                    open_document(env, activity, uri, signal.signal.as_ref())
                })
            });
            let (descriptor, name) = match opened {
                Ok(opened) => opened?,
                Err(error) => {
                    tracing::warn!(
                        target: "flui_native::android",
                        %error,
                        "a document an intent opened could not be read"
                    );
                    return Err(OpenRefusal::Failed);
                }
            };
            OpenedFile::try_from_descriptor(descriptor, name).map_err(|error| {
                tracing::warn!(
                    target: "flui_native::android",
                    %error,
                    "a document an intent opened could not be held"
                );
                OpenRefusal::Failed
            })
        }
    }

    /// Start the worker that opens the documents `opener` queues, waking the
    /// owner through `signal` as each is opened.
    pub(crate) fn spawn_documents(
        app: AndroidApp,
        opener: Arc<Opener<JavaSignal>>,
        signal: Weak<OwnerSignal>,
    ) -> std::io::Result<()> {
        std::thread::Builder::new()
            .name("flui-documents".to_owned())
            .spawn(move || {
                let mut provider = JavaDocuments { app };
                work(&opener, &mut provider, || {
                    if let Some(signal) = signal.upgrade() {
                        let _ = signal.wake();
                    }
                });
            })
            .map(drop)
    }

    /// `FluiHost.openDocument` and `FluiHost.documentName` for `uri` under
    /// `signal`: the descriptor this library then owns and the name, or why
    /// the provider gave none. A failure after the descriptor is taken
    /// closes it.
    #[expect(unsafe_code, reason = "a detached descriptor becomes an OwnedFd")]
    fn open_document(
        env: &mut Env<'_>,
        activity: &JObject<'_>,
        uri: &str,
        signal: &JObject<'_>,
    ) -> jni::errors::Result<Result<(OwnedFd, Option<String>), OpenRefusal>> {
        let class = host_class(env, activity)?;
        let text = env.new_string(uri)?;
        let answer = env
            .call_static_method(
                &class,
                jni_str!("openDocument"),
                jni_sig!(
                    "(Landroid/content/Context;Ljava/lang/String;Landroid/os/CancellationSignal;)I"
                ),
                &[
                    JValue::Object(activity),
                    JValue::Object(&text),
                    JValue::Object(signal),
                ],
            )?
            .i()?;
        let raw = match super::opened_descriptor(answer) {
            Ok(raw) => raw,
            Err(refusal) => return Ok(Err(refusal)),
        };
        // SAFETY: `openDocument` answers `ParcelFileDescriptor.detachFd()`,
        // which hands the descriptor over: no Java object owns or closes it
        // any more, so this is its only owner.
        let descriptor = unsafe { OwnedFd::from_raw_fd(raw) };
        let name = env
            .call_static_method(
                &class,
                jni_str!("documentName"),
                jni_sig!(
                    "(Landroid/content/Context;Ljava/lang/String;Landroid/os/CancellationSignal;)Ljava/lang/String;"
                ),
                &[
                    JValue::Object(activity),
                    JValue::Object(&text),
                    JValue::Object(signal),
                ],
            )?
            .l()?;
        let name = env.cast_local::<JString<'_>>(name)?;
        let name = if name.is_null() {
            None
        } else {
            Some(name.try_to_string(env)?)
        };
        Ok(Ok((descriptor, name)))
    }

    /// Every intent's record `FluiHost` queued, oldest first, each read in a
    /// frame of its own so that a full queue holds no more local references
    /// than one record; `FluiHost` lets go of exactly those once all are
    /// read, so a failure leaves them queued.
    fn drain_opened(
        env: &mut Env<'_>,
        activity: &JObject<'_>,
    ) -> jni::errors::Result<Vec<Option<Vec<String>>>> {
        let class = host_class(env, activity)?;
        let records = env
            .call_static_method(
                &class,
                jni_str!("peekOpened"),
                jni_sig!("()[[Ljava/lang/String;"),
                &[],
            )?
            .l()?;
        let records = env.cast_local::<JObjectArray<'_>>(records)?;
        let len = records.len(env)?;
        let mut out = Vec::with_capacity(len);
        for index in 0..len {
            // A record that cannot be read is `None`, taken off the queue
            // with the rest: it would fail the same way on every turn.
            let record = env.with_local_frame(4, |env| -> jni::errors::Result<_> {
                let record = records.get_element(env, index)?;
                let record = env.cast_local::<JObjectArray<'_>>(record)?;
                strings(env, &record)
            });
            if record.is_err() {
                env.exception_clear();
            }
            out.push(record.ok());
        }
        let read = i32::try_from(len).map_err(|_| jni::errors::Error::JavaException)?;
        env.call_static_method(
            &class,
            jni_str!("commitOpened"),
            jni_sig!("(I)V"),
            &[JValue::Int(read)],
        )?;
        Ok(out)
    }

    #[expect(unsafe_code, reason = "RegisterNatives takes function pointers")]
    fn register_natives(env: &mut Env<'_>, class: &JClass<'_>) -> jni::errors::Result<()> {
        // SAFETY: `native_wake` is an `extern "system"` function whose
        // parameters are the JNI environment, the class (the method is
        // static) and the `long` the signature names, returning `void`.
        let methods = unsafe {
            [NativeMethod::from_raw_parts(
                jni_str!("nativeWake"),
                jni_str!("(J)V"),
                native_wake as *mut std::ffi::c_void,
            )]
        };
        // SAFETY: as above.
        unsafe { env.register_native_methods(class, &methods) }
    }

    /// Free a handle `FluiHost` no longer names.
    #[expect(unsafe_code, reason = "the handle is a Box this module leaked")]
    fn free_handle(handle: *mut Arc<Inbox>) {
        // SAFETY: `handle` came from `Box::into_raw` in `HostQueue::bind` and
        // is freed once: there, when `FluiHost` never received it, or by
        // `detach` after `FluiHost.unbind()` returned on this same
        // (Java main) thread, when `FluiHost` no longer names it: it cleared
        // it, or a later platform replaced it. That thread is the only one
        // that passes a handle to `nativeWake`.
        drop(unsafe { Box::from_raw(handle) });
    }

    #[expect(unsafe_code, reason = "the handle is a Box this module leaked")]
    extern "system" fn native_wake<'local>(
        mut env: EnvUnowned<'local>,
        _class: JClass<'local>,
        handle: jlong,
    ) {
        jni_native(
            "FluiHost.nativeWake",
            &mut env,
            |()| (),
            |_| {
                if handle != 0 {
                    // SAFETY: a non-zero handle is a `Box<Arc<Inbox>>` from
                    // `HostQueue::bind`. `FluiHost` passes it only on the Java
                    // main thread, and `free_handle` frees it on that thread only
                    // once `FluiHost` no longer names it, so it is live here.
                    let inbox = unsafe { &*(handle as *const Arc<Inbox>) };
                    inbox.wake();
                }
                Ok(())
            },
        );
    }
}

/// The Java source of the class named `binary_name`, from the sources the
/// checked-in dex is compiled from (`cargo xtask android-dex` holds the dex
/// to them).
#[cfg(test)]
pub(crate) fn java_source(binary_name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("android/java")
        .join(format!("{}.java", binary_name.replace('.', "/")));
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::{
        ACTIVITY_CLASS, Found, HOST_CLASS, HostEntry, PROTOCOL, TRIM_MEMORY, check, java_source,
    };
    use crate::PlatformError;

    fn found(protocol: Option<i32>, is_flui_activity: bool) -> Found {
        Found {
            activity: "com.example.MainActivity".to_owned(),
            is_flui_activity,
            protocol,
        }
    }

    /// The classes this library is built with state its protocol, and the
    /// Activity the manifest names is one of them, extending `NativeActivity`.
    #[test]
    fn the_java_classes_state_the_protocol_of_the_library() {
        let host = java_source(HOST_CLASS);
        let stated = format!("public static final int PROTOCOL = {PROTOCOL};");
        assert!(host.contains(&stated), "{stated:?} in\n{host}");
        let activity = java_source(ACTIVITY_CLASS);
        assert!(
            activity.contains("public class FluiActivity extends NativeActivity"),
            "{activity}"
        );
    }

    /// Each way the APK's classes can differ from the library is refused,
    /// naming what was found; only matching classes under a `FluiActivity`
    /// start.
    #[test]
    fn the_platform_starts_only_over_its_own_java_classes() {
        assert!(check(&found(Some(PROTOCOL), true)).is_ok());

        let other = PROTOCOL + 1;
        match check(&found(Some(other), true)) {
            Err(PlatformError::HostProtocol { library, packaged }) => {
                assert_eq!((library, packaged), (PROTOCOL, other));
            }
            outcome => panic!("another protocol: {outcome:?}"),
        }
        let message = check(&found(Some(other), true))
            .expect_err("another protocol")
            .to_string();
        assert!(
            message.contains(&format!("protocol {other}"))
                && message.contains(&format!("library {PROTOCOL}")),
            "{message}"
        );

        let missing = check(&found(None, false))
            .expect_err("no classes")
            .to_string();
        assert!(
            missing.contains(HOST_CLASS) && missing.contains("flui build"),
            "{missing}"
        );

        let activity = check(&found(Some(PROTOCOL), false))
            .expect_err("another Activity")
            .to_string();
        assert!(
            activity.contains("com.example.MainActivity") && activity.contains(ACTIVITY_CLASS),
            "{activity}"
        );
    }

    /// `FluiActivity` forwards `onTrimMemory`, and the owner reads the queue
    /// as `FluiHost` lays it out, with the kind the Java side states, oldest
    /// first; a kind it does not know and half a pair are skipped.
    #[test]
    fn the_host_queue_is_read_as_the_java_classes_lay_it_out() {
        let activity = java_source(ACTIVITY_CLASS);
        assert!(
            activity.contains("FluiHost.trimMemory(level);"),
            "{activity}"
        );
        let host = java_source(HOST_CLASS);
        let stated = format!("static final int TRIM_MEMORY = {TRIM_MEMORY};");
        assert!(host.contains(&stated), "{stated:?} in\n{host}");

        assert_eq!(
            HostEntry::from_pairs(&[TRIM_MEMORY, 15, TRIM_MEMORY + 98, 1, TRIM_MEMORY, 80]),
            [HostEntry::TrimMemory(15), HostEntry::TrimMemory(80)]
        );
        assert_eq!(HostEntry::from_pairs(&[]), []);

        // An intent's record names its launch as the library reads it, from
        // `onCreate` (cold) and `onNewIntent` (running), and the Activity
        // reads the latest intent from then on.
        for stated in [
            format!(
                "static final String COLD = \"{}\";",
                super::super::intent::COLD
            ),
            format!(
                "static final String RUNNING = \"{}\";",
                super::super::intent::RUNNING
            ),
            format!(
                "static final int MAX_OPENED = {};",
                flui_platform::MAX_OPENED_ITEMS
            ),
            format!(
                "static final int MAX_URI_LENGTH = {};",
                flui_platform::MAX_OPENED_URL_LEN
            ),
        ] {
            assert!(host.contains(&stated), "{stated:?} in\n{host}");
        }
        for forwarded in [
            "FluiHost.opened(intent, true);",
            "setIntent(intent);\n        FluiHost.opened(intent, false);",
            // Activity results, the state their keys survive in, and the
            // starts held while the Activity is stopped.
            "FluiHost.activityResult(requestCode, resultCode, data);",
            "FluiHost.save(outState);",
            "FluiHost.restore(savedInstanceState);",
            "FluiHost.started(this);",
            "FluiHost.stopped(this);",
            "FluiHost.destroyed(this);",
        ] {
            assert!(activity.contains(forwarded), "{forwarded:?} in\n{activity}");
        }
        assert_eq!(
            HostEntry::from_pairs(&[TRIM_MEMORY, 5, TRIM_MEMORY]),
            [HostEntry::TrimMemory(5)]
        );
    }

    /// `FluiHost.openDocument` answers as the library reads it: a descriptor,
    /// or the refusal its code names (the grant ended, the open cancelled at
    /// its deadline, the provider failed, and any code this library does not
    /// know as a failure); it opens and names the document under the
    /// signal the library cancels.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_document_open_answers_as_the_java_classes_state_it() {
        use super::super::documents::OpenRefusal;
        use super::{OPEN_CANCELLED, OPEN_FAILED, OPEN_GRANT_ENDED, opened_descriptor};

        let host = java_source(HOST_CLASS);
        for stated in [
            format!("static final int OPEN_FAILED = {OPEN_FAILED};"),
            format!("static final int OPEN_GRANT_ENDED = {OPEN_GRANT_ENDED};"),
            format!("static final int OPEN_CANCELLED = {OPEN_CANCELLED};"),
            "static int openDocument(Context context, String uri, CancellationSignal signal)"
                .to_owned(),
            ".openFileDescriptor(Uri.parse(uri), \"r\", signal)".to_owned(),
            "static String documentName(Context context, String uri, CancellationSignal signal)"
                .to_owned(),
        ] {
            assert!(host.contains(&stated), "{stated:?} in FluiHost");
        }
        assert_eq!(opened_descriptor(0), Ok(0));
        assert_eq!(opened_descriptor(41), Ok(41));
        assert_eq!(
            opened_descriptor(OPEN_GRANT_ENDED),
            Err(OpenRefusal::GrantEnded)
        );
        assert_eq!(
            opened_descriptor(OPEN_CANCELLED),
            Err(OpenRefusal::TimedOut)
        );
        assert_eq!(opened_descriptor(OPEN_FAILED), Err(OpenRefusal::Failed));
        assert_eq!(opened_descriptor(-99), Err(OpenRefusal::Failed));
    }

    /// One entry or intent record that cannot be read does not hold the rest
    /// back: at the head, in the middle and at the end of a drain, an entry
    /// of a kind this library does not know and an intent record that could
    /// not be read, or does not read as an intent, are skipped, and every
    /// other one is delivered in order.
    #[test]
    fn a_record_that_cannot_be_read_is_skipped_and_the_rest_delivered() {
        let cold = || {
            Some(vec![
                super::super::intent::COLD.to_owned(),
                "android.intent.action.VIEW".to_owned(),
                "https://example.com/a".to_owned(),
            ])
        };
        for at in [0, 1, 2] {
            let mut pairs = vec![TRIM_MEMORY, 10, TRIM_MEMORY, 20];
            pairs.splice(at * 2..at * 2, [TRIM_MEMORY + 98, 0]);
            assert_eq!(
                HostEntry::from_pairs(&pairs),
                [HostEntry::TrimMemory(10), HostEntry::TrimMemory(20)],
                "an unknown entry at {at}"
            );

            for poison in [None, Some(vec!["neither".to_owned()])] {
                let mut records = vec![cold(), cold()];
                records.insert(at, poison.clone());
                let intents = HostEntry::from_records(records);
                assert_eq!(intents.len(), 2, "{poison:?} at {at}");
                assert!(
                    intents
                        .iter()
                        .all(|entry| matches!(entry, HostEntry::Intent(_))),
                    "{intents:?}"
                );
            }
        }
    }
}
