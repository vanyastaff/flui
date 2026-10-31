//! `FluiView`, the Java side of the Android backend (ADR-0247).
//!
//! The view's classes are compiled from `crates/flui-native/android/java`
//! into one dex that is part of this library (`android/classes.dex`, checked
//! against its sources by `cargo xtask android-dex`). At platform start the
//! dex is loaded on the Java main thread through a class loader parented to
//! the Activity's: `InMemoryDexClassLoader` from API 26, a `DexClassLoader`
//! over a read-only copy in the code cache below it. The view's native
//! methods are bound with `RegisterNatives` on the class that loader
//! returned (`FindClass` would search the system loader), and one full-window
//! `FluiView` is laid over the `NativeActivity`'s content. The manifest stays
//! `NativeActivity` with `hasCode="false"`.
//!
//! Threads: every call into the view is posted to the Java main thread, and
//! every native method the view calls only queues a [`ViewEvent`] for the
//! owner and wakes it. Neither thread waits for the other. What the system
//! needs answered at once (a drag's acceptance, a drop's result, the input
//! method's reads) comes from state the owner published earlier: the drop
//! feedback the target cached, and the mirror of the focused field.
//!
//! The view serves several features and names none of the text feature's
//! types: the input method's calls go to an [`InputMethod`], which the host's
//! table hands the view ([`AndroidHost`](super::AndroidHost)), and what it
//! makes of them reaches the window as work the owner runs
//! ([`ViewEvent::RunOnWindow`]).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Weak};

use android_activity::AndroidApp;
use flui_platform::DragDropEvent;
use jni::objects::{JClass, JObject, JObjectArray, JString};
use jni::refs::Global;
use jni::sys::{jboolean, jfloat, jint, jlong};
use jni::{Env, EnvUnowned, JValue, JavaVM, NativeMethod, jni_sig, jni_str};
use parking_lot::Mutex;

use super::EditorState;
use super::bridge_slot::BridgeSlot;
use crate::owner::panic_boundary::{AfterPanic, os_entry};
use crate::owner::signal::OwnerSignal;
use crate::version::gate::Since;
use crate::version::gate::android::{AndroidApi, InMemoryDex};

use super::{ActivityWindow, ActivityWindowId};
use crate::owner::android::activity::with_activity;

/// The view's classes, compiled from `android/java` (see the module doc).
const DEX: &[u8] = include_bytes!("../../../android/classes.dex");

/// The binary name of the view class.
const VIEW_CLASS: &str = "dev.flui.platform.FluiView";

/// The host's drag-and-drop transport as the view's drag callbacks reach it:
/// `nativeDrag` and `nativeDrop` run on the Java main thread and must be
/// answered at once, from what the owner published earlier (ADR-0211,
/// ADR-0247). The transport is the host's one [`DataTransferSource`].
///
/// [`DataTransferSource`]: flui_platform::data_transfer::DataTransferSource
pub(crate) trait ViewTransfer: flui_platform::data_transfer::DataTransferSource {
    /// One `DragEvent` other than the drop, of `action`, at `position` in
    /// logical pixels, offering `mime_types`; `local` is whether this activity
    /// started the drag and `result` the event's result. Returns what the view
    /// answers.
    fn drag(
        &self,
        view: &JavaView,
        action: i32,
        position: (f64, f64),
        mime_types: &[String],
        local: bool,
        result: bool,
    ) -> bool;

    /// The drop at `position`, whose clip carried `text`, `html` and `uris`:
    /// the key of the offer the drop holds, which the view hands back, or 0
    /// when the drop is refused.
    fn drop_clip(
        &self,
        view: &JavaView,
        position: (f64, f64),
        local: bool,
        text: Option<String>,
        html: Option<String>,
        uris: Vec<String>,
    ) -> i64;

    /// FLUI's own drag, which the view runs, advertises `allowed` to a target
    /// of this activity, or has ended (`None`).
    fn set_own(&self, allowed: Option<flui_platform::data_transfer::TransferActions>);

    /// The action FLUI's target took when FLUI's own drag dropped on it, once.
    fn take_local_drop(&self) -> Option<flui_platform::data_transfer::TransferActions>;

    /// The system drag source of a window over `view`, at `api`: the drags
    /// that window starts are the view's (`View.startDragAndDrop`).
    fn drag_source(&self, view: Arc<JavaView>, api: AndroidApi) -> Arc<dyn ActivityDragSource>;
}

/// A window's system drag source as the window that holds it reaches it:
/// what it hands out, and what the loop tells it through the window.
pub(crate) trait ActivityDragSource: flui_platform::drag::PlatformDragSource {
    /// The system drag ended: `result` is whether a target took it. Owner
    /// thread.
    fn finish(&self, result: bool);

    /// The window closed: a running drag ends cancelled. Owner thread.
    fn retire(&self);
}

/// Makes the host's [`ViewTransfer`] for the view it is given, which it reaches
/// back weakly.
pub(crate) type TransferFor = fn(Weak<JavaView>) -> Arc<dyn ViewTransfer>;

/// What the view reported, for the owner to deliver.
pub(crate) enum ViewEvent {
    /// The view's installation finished, whether or not it worked.
    Installed,
    /// What a feature made of a report on the Java main thread, run on the
    /// owner against the window, in order with the rest; dropped with no
    /// window.
    RunOnWindow(WindowWork),
    /// A drag over the view.
    Drag(DragDropEvent),
    /// The drag this view started ended; `result` is whether a target took it.
    DragEnded {
        /// `DragEvent.getResult()`.
        result: bool,
    },
    /// A setting behind the preferences changed that no configuration change
    /// reports: a content observer or, from API 34, the contrast listener.
    SettingsChanged,
    /// The window insets the view received changed.
    InsetsChanged,
    /// The keyboard began to move, or its motion drifted from the clock it
    /// was reported on (API 30).
    KeyboardMotion(KeyboardReport),
    /// The keyboard's motion ended or was cancelled (API 30).
    KeyboardSettled,
}

/// A keyboard motion as `FluiKeyboardMotion` reports it: bottom edges in device
/// pixels, the start on the event timeline (`None` for a stamp the monotonic
/// clock cannot have taken), the length, and the
/// interpolator's values a quarter, half and three quarters of the way.
#[derive(Debug, Clone, Copy)]
pub(crate) struct KeyboardReport {
    pub(crate) from: i32,
    pub(crate) to: i32,
    pub(crate) start: Option<flui_platform::EventTime>,
    pub(crate) duration_millis: i64,
    pub(crate) values: [f32; 3],
}

/// Work a feature hands the owner to run against the window.
pub(crate) type WindowWork = Box<dyn FnOnce(&dyn ActivityWindow) + Send>;

/// The input method's calls through `FluiView`, which the text feature answers.
///
/// The input method reads and edits the view's copy of the focused field on
/// the Java main thread and never waits for the owner; the owner publishes the
/// field to the view, and the view reports what the input method made of it.
/// Each call below but [`Self::hold`] and [`Self::pen_down`] comes from a
/// native method of the view, on the Java main thread under no lock of the
/// view, and answers what the view applies and queues.
pub(crate) trait InputMethod: Send + Sync {
    /// The view now holds `text`, which the owner published as `generation`:
    /// the reports that follow are made against it. On the owner thread.
    fn hold(&self, generation: u32, text: &str);

    /// `nativeImeState`: what the view holds after an input method edit made
    /// on the publication `generation` names; `None` when it changed nothing.
    fn edited(&self, generation: u32, state: EditorState) -> Option<InputMethodReply>;

    /// `nativeImeAction`: the input method performed its editor action
    /// (`EditorInfo.IME_ACTION_*`) at `uptime_ms` on the event clock.
    fn editor_action(&self, action: i32, uptime_ms: i64) -> InputMethodReply;

    /// `nativeImeCursorUpdates`: `requestCursorUpdates` with this mode.
    fn cursor_updates(&self, mode: i32) -> InputMethodReply;

    /// A pen tip touched `window` down at `at` (logical pixels), on the owner
    /// thread: inside the focused field, the input method's handwriting may
    /// take the stroke. Android's own initiation sees only the stylus events
    /// native code leaves unhandled.
    fn pen_down(&self, window: &dyn ActivityWindow, at: flui_foundation::geometry::Point<f64>);

    /// The window `window` opened over `view` at `api`: from now on the input
    /// method edits its field, which this keeps until the window closes. On
    /// the owner thread.
    fn window_opened(&self, window: ActivityWindowId, view: &Arc<JavaView>, api: AndroidApi);

    /// `window` is going away: its field is detached for good, and work the
    /// input method made for it finds none. On the owner thread.
    fn window_closed(&self, window: ActivityWindowId);

    /// The store host of `window`'s field, which serves the focused field to
    /// the input method; `None` for a window this does not hold. On the owner
    /// thread.
    fn store_host(
        &self,
        window: ActivityWindowId,
    ) -> Option<std::rc::Rc<dyn flui_platform::text_store::TextStoreHost>>;
}

/// TalkBack's calls through `FluiView`'s node provider, which the window's
/// accessibility bridge answers.
///
/// Each call comes from a native method of the view, on the Java main thread
/// under no lock of the view, and answers at once from the tree the bridge
/// applied on that thread. The environment and the view cross as raw
/// pointers, because the adapter behind them is built on another `jni`
/// release than this module.
///
/// # Safety
///
/// A caller of each method passes the environment of the native method it
/// runs in, on that method's thread, and the local references (`host`, the
/// bundle) that method received, none of which the call outlives.
#[expect(
    unsafe_code,
    reason = "the environment and the view cross as raw pointers"
)]
pub(crate) trait ViewAccessibility: Send + Sync {
    /// `createAccessibilityNodeInfo`: a local reference to the node info of
    /// `virtual_view_id` (`HOST_VIEW_ID` for the view itself), or null.
    unsafe fn node_info(
        &self,
        env: *mut jni::sys::JNIEnv,
        host: jni::sys::jobject,
        virtual_view_id: jint,
    ) -> jni::sys::jobject;

    /// `findFocus`: a local reference to the node info of the virtual view
    /// holding the focus of `focus_type`, or null.
    unsafe fn find_focus(
        &self,
        env: *mut jni::sys::JNIEnv,
        host: jni::sys::jobject,
        focus_type: jint,
    ) -> jni::sys::jobject;

    /// `performAction`: whether the tree took `action` on `virtual_view_id`,
    /// with its `arguments` bundle (which may be null).
    unsafe fn perform_action(
        &self,
        env: *mut jni::sys::JNIEnv,
        host: jni::sys::jobject,
        virtual_view_id: jint,
        action: jint,
        arguments: jni::sys::jobject,
    ) -> bool;

    /// One hover of TalkBack's explore by touch, with its `MotionEvent`
    /// action code and its position in the view's pixels, from the native
    /// input queue on the owner thread: whether the adapter takes it, and
    /// the hover is then not input. The adapter applies it on the Java main
    /// thread, through `view`.
    fn explore(self: Arc<Self>, view: &Arc<JavaView>, action: jint, x: f32, y: f32) -> bool;
}

/// What the text feature made of one of the input method's calls through the
/// view: the view applies it on the Java main thread and queues the work.
pub(crate) struct InputMethodReply {
    /// Whether the input method composes now; `None` when the call does not
    /// say.
    pub(crate) composing: Option<bool>,
    /// What the owner then runs against the window, in order with the view's
    /// other reports.
    pub(crate) work: WindowWork,
}

/// Where the view is in its life.
enum Install {
    /// Posted to the Java main thread, not run yet.
    Pending,
    /// The view is in the window.
    Ready(Arc<Global<JObject<'static>>>),
    /// It could not be installed; the backend runs without it.
    Failed,
    /// The platform ended and the view left the window.
    Detached,
}

struct State {
    install: Install,
    /// What the view reported, oldest first.
    events: VecDeque<ViewEvent>,
    /// The `Box<Weak<JavaView>>` the view holds as its handle, freed on the
    /// Java main thread once the view no longer names it.
    handle: Option<usize>,
}

/// The bridge between the owner thread and `FluiView` (see the module doc).
pub(crate) struct JavaView {
    app: AndroidApp,
    /// The device's API level, which picks the dex loader.
    api: AndroidApi,
    signal: Weak<OwnerSignal>,
    transfer: Arc<dyn ViewTransfer>,
    /// What answers the input method's calls through the view.
    input_method: Box<dyn InputMethod>,
    /// What answers TalkBack's calls through the view's node provider: the
    /// bridge of the window over the view, while it lives.
    accessibility: BridgeSlot<dyn ViewAccessibility>,
    state: Mutex<State>,
    /// Whether the input method composes, as the view last reported.
    composing: AtomicBool,
    /// The window's scale factor, as the owner last read it, for the view's
    /// pixel coordinates.
    scale: AtomicU64,
}

impl std::fmt::Debug for JavaView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JavaView").finish_non_exhaustive()
    }
}

impl JavaView {
    /// The bridge, with the view's installation posted to the Java main thread
    /// and the drag-and-drop transport `transfer` makes for it.
    pub(super) fn start(
        app: AndroidApp,
        signal: Weak<OwnerSignal>,
        api: AndroidApi,
        input_method: Box<dyn InputMethod>,
        transfer: TransferFor,
    ) -> Arc<Self> {
        let view = Arc::new_cyclic(|this: &Weak<Self>| Self {
            app,
            api,
            signal,
            transfer: transfer(this.clone()),
            input_method,
            accessibility: BridgeSlot::default(),
            state: Mutex::new(State {
                install: Install::Pending,
                events: VecDeque::new(),
                handle: None,
            }),
            composing: AtomicBool::new(false),
            scale: AtomicU64::new(1.0_f64.to_bits()),
        });
        let installing = Arc::clone(&view);
        view.app
            .run_on_java_main_thread(Box::new(move || installing.install()));
        view
    }

    /// The platform's drag-and-drop transport.
    pub(crate) fn transfer(&self) -> &Arc<dyn ViewTransfer> {
        &self.transfer
    }

    /// Whether the view is in the window.
    pub(crate) fn is_ready(&self) -> bool {
        matches!(self.state.lock().install, Install::Ready(_))
    }

    pub(super) fn composing(&self) -> bool {
        self.composing.load(Ordering::Acquire)
    }

    pub(crate) fn set_composing(&self, composing: bool) {
        self.composing.store(composing, Ordering::Release);
    }

    pub(super) fn set_scale(&self, scale: f64) {
        if scale.is_finite() && scale > 0.0 {
            self.scale.store(scale.to_bits(), Ordering::Relaxed);
        }
    }

    pub(crate) fn scale(&self) -> f64 {
        f64::from_bits(self.scale.load(Ordering::Relaxed))
    }

    /// Everything the view reported since the last call, oldest first.
    pub(super) fn take_events(&self) -> VecDeque<ViewEvent> {
        std::mem::take(&mut self.state.lock().events)
    }

    /// Queue `event` for the owner and wake it.
    pub(crate) fn push_event(&self, event: ViewEvent) {
        self.state.lock().events.push_back(event);
        if let Some(signal) = self.signal.upgrade() {
            let _ = signal.wake();
        }
    }

    pub(crate) fn push_drag(&self, event: DragDropEvent) {
        self.push_event(ViewEvent::Drag(event));
    }

    /// What answers the input method's calls through the view.
    pub(crate) fn input_method(&self) -> &dyn InputMethod {
        &*self.input_method
    }

    /// Hand TalkBack's calls through the node provider to `accessibility`,
    /// the bridge of the window that opened over the view, and turn the
    /// provider on. Owner thread; the view follows on the Java main thread.
    pub(crate) fn provide_accessibility(
        self: &Arc<Self>,
        accessibility: Arc<dyn ViewAccessibility>,
    ) {
        // The previous bridge is dropped here, outside the slot's lock.
        drop(self.accessibility.provide(accessibility));
        self.provide_nodes(true);
    }

    /// Stop handing the node provider's calls to `accessibility`, unless a
    /// window opened since put its own in its place, and turn the provider
    /// off. Owner thread.
    pub(crate) fn withdraw_accessibility(
        self: &Arc<Self>,
        accessibility: &Arc<dyn ViewAccessibility>,
    ) {
        if self.accessibility.withdraw(accessibility).is_some() {
            self.provide_nodes(false);
        }
    }

    /// One hover of TalkBack's explore by touch from the native input queue
    /// (ADR-0255 decision 3): whether the window's bridge takes it, in which
    /// case it is not input. Owner thread.
    pub(crate) fn explore_by_touch(self: &Arc<Self>, action: jint, x: f32, y: f32) -> bool {
        self.accessibility()
            .is_some_and(|accessibility| accessibility.explore(self, action, x, y))
    }

    /// Turn the view's node provider on or off. A call the view finds gone
    /// leaves nothing to turn off.
    fn provide_nodes(self: &Arc<Self>, provide: bool) {
        self.post(move |env, view| {
            env.call_method(
                view,
                jni_str!("provideNodes"),
                jni_sig!("(Z)V"),
                &[JValue::Bool(provide)],
            )
            .map(drop)
        });
    }

    /// The bridge answering the node provider, cloned out of the lock so no
    /// JNI call runs under it.
    fn accessibility(&self) -> Option<Arc<dyn ViewAccessibility>> {
        self.accessibility.get()
    }

    /// Apply what the input method's call made of it and queue its work.
    fn reply(&self, reply: InputMethodReply) {
        if let Some(composing) = reply.composing {
            self.set_composing(composing);
        }
        self.push_event(ViewEvent::RunOnWindow(reply.work));
    }

    /// Run `call` with the view on the Java main thread, if it is installed
    /// by then. Owner calls never wait for it.
    pub(crate) fn post(
        self: &Arc<Self>,
        call: impl FnOnce(&mut Env<'_>, &JObject<'_>) -> jni::errors::Result<()> + Send + 'static,
    ) {
        self.post_or(move |env, view| call(env, view).map(|()| true), |_| {});
    }

    /// [`Self::post`] for a call that answers whether it worked: `failed`
    /// runs on the Java main thread when the view is not installed, the call
    /// fails or panics, or it answers `false`, so an obligation the call
    /// carried is still discharged.
    pub(crate) fn post_or(
        self: &Arc<Self>,
        call: impl FnOnce(&mut Env<'_>, &JObject<'_>) -> jni::errors::Result<bool> + Send + 'static,
        failed: impl FnOnce(&Self) + Send + 'static,
    ) {
        let this = Arc::clone(self);
        self.app.run_on_java_main_thread(Box::new(move || {
            // Contained here, not by the main-thread loop around this closure,
            // which would swallow the panic before `failed` ran.
            let worked = jni_native("a FluiView call", || {
                this.view().is_some_and(|view| {
                    this.attached(|env| call(env, view.as_ref()))
                        .inspect_err(|error| {
                            tracing::warn!(target: "flui_native::android", %error, "a FluiView call failed");
                        })
                        .unwrap_or(false)
                })
            });
            if !worked {
                failed(&this);
            }
        }));
    }

    /// The installed view, cloned out of the lock so no JNI call runs under it.
    pub(crate) fn view(&self) -> Option<Arc<Global<JObject<'static>>>> {
        match &self.state.lock().install {
            Install::Ready(view) => Some(Arc::clone(view)),
            _ => None,
        }
    }

    /// Run `f` attached to the VM, clearing any exception it leaves.
    #[expect(unsafe_code, reason = "AndroidApp lends the live VM pointer to JNI")]
    pub(crate) fn attached<T>(
        &self,
        f: impl FnOnce(&mut Env<'_>) -> jni::errors::Result<T>,
    ) -> jni::errors::Result<T> {
        // SAFETY: `vm_as_ptr` is the process's running VM, which outlives
        // every `AndroidApp`; `JavaVM` borrows it and destroys nothing.
        let vm = unsafe { JavaVM::from_raw(self.app.vm_as_ptr().cast()) };
        vm.attach_current_thread(|env| {
            let result = f(env);
            if result.is_err() && env.exception_check() {
                env.exception_describe();
                env.exception_clear();
            }
            result
        })
    }

    /// Load the dex, bind the native methods and lay the view over the
    /// Activity's content. Java main thread only.
    fn install(self: Arc<Self>) {
        let in_memory = self.api.since::<InMemoryDex>();
        let handle = Box::into_raw(Box::new(Arc::downgrade(&self)));
        let installed = with_activity(&self.app, |env, activity| {
            install_view(env, activity, handle as jlong, in_memory).map_err(|error| {
                tracing::warn!(target: "flui_native::android", %error, "FluiView was not installed");
                jni::errors::Error::JniCall(jni::errors::JniError::Unknown)
            })
        });
        let ready = if let Ok(view) = installed {
            let mut state = self.state.lock();
            if matches!(state.install, Install::Detached) {
                drop(state);
                // The platform ended first: take the view straight out.
                let _ = self.attached(|env| {
                    env.call_method(&view, jni_str!("detach"), jni_sig!("()V"), &[])
                        .map(drop)
                });
                free_handle(handle);
                false
            } else {
                state.install = Install::Ready(Arc::new(view));
                state.handle = Some(handle as usize);
                true
            }
        } else {
            free_handle(handle);
            let mut state = self.state.lock();
            if !matches!(state.install, Install::Detached) {
                state.install = Install::Failed;
            }
            false
        };
        tracing::info!(
            target: "flui_native::android",
            ready,
            api = %self.api,
            loader = if in_memory.is_some() { "in-memory" } else { "code-cache file" },
            "FluiView installation finished"
        );
        self.push_event(ViewEvent::Installed);
    }

    /// The platform ended: the view leaves the window, stops calling native
    /// code, and its handle is freed after that, on the Java main thread.
    pub(super) fn detach(self: &Arc<Self>) {
        let this = Arc::clone(self);
        self.app.run_on_java_main_thread(Box::new(move || {
            let (view, handle) = {
                let mut state = this.state.lock();
                let previous = std::mem::replace(&mut state.install, Install::Detached);
                let view = match previous {
                    Install::Ready(view) => Some(view),
                    _ => None,
                };
                (view, state.handle.take())
            };
            if let Some(view) = view {
                let _ = this.attached(|env| {
                    env.call_method(view.as_ref(), jni_str!("detach"), jni_sig!("()V"), &[])
                        .map(drop)
                });
            }
            if let Some(handle) = handle {
                free_handle(handle as *mut Weak<JavaView>);
            }
        }));
    }
}

/// Free a handle the view no longer names.
#[expect(unsafe_code, reason = "the handle is a Box this module leaked")]
fn free_handle(handle: *mut Weak<JavaView>) {
    // SAFETY: `handle` came from `Box::into_raw` in `JavaView::install`, and
    // is freed exactly once: either there, when the view never received it,
    // or by `detach` after `FluiView.detach()` cleared the view's copy on this
    // same (Java main) thread, which is the only thread that calls the view's
    // native methods.
    drop(unsafe { Box::from_raw(handle) });
}

#[derive(Debug, thiserror::Error)]
enum InstallError {
    #[error("JNI: {0}")]
    Jni(#[from] jni::errors::Error),
    #[error("writing the dex to the code cache: {0}")]
    Io(#[from] std::io::Error),
    #[error("the dex in the code cache does not match the library's")]
    Tampered,
}

/// Load the view class through a loader parented to the Activity's, bind its
/// native methods and install the view. With `in_memory` the dex is loaded
/// from this library's bytes, else from its copy in the code cache.
fn install_view(
    env: &mut Env<'_>,
    activity: &JObject<'_>,
    handle: jlong,
    in_memory: Option<Since<InMemoryDex>>,
) -> Result<Global<JObject<'static>>, InstallError> {
    let parent = env
        .call_method(
            activity,
            jni_str!("getClassLoader"),
            jni_sig!("()Ljava/lang/ClassLoader;"),
            &[],
        )?
        .l()?;
    let loader = if in_memory.is_some() {
        let bytes = env.byte_array_from_slice(DEX)?;
        let buffer = env
            .call_static_method(
                jni_str!("java/nio/ByteBuffer"),
                jni_str!("wrap"),
                jni_sig!("([B)Ljava/nio/ByteBuffer;"),
                &[JValue::Object(&bytes)],
            )?
            .l()?;
        env.new_object(
            jni_str!("dalvik/system/InMemoryDexClassLoader"),
            jni_sig!("(Ljava/nio/ByteBuffer;Ljava/lang/ClassLoader;)V"),
            &[JValue::Object(&buffer), JValue::Object(&parent)],
        )?
    } else {
        let cache = env
            .call_method(
                activity,
                jni_str!("getCodeCacheDir"),
                jni_sig!("()Ljava/io/File;"),
                &[],
            )?
            .l()?;
        let cache = env
            .call_method(
                &cache,
                jni_str!("getAbsolutePath"),
                jni_sig!("()Ljava/lang/String;"),
                &[],
            )?
            .l()?;
        let cache = env.cast_local::<JString<'_>>(cache)?.try_to_string(env)?;
        let path = code_cache_dex(std::path::Path::new(&cache))?;
        let path = env.new_string(path.to_string_lossy())?;
        let optimized = env.new_string(&cache)?;
        env.new_object(
            jni_str!("dalvik/system/DexClassLoader"),
            jni_sig!(
                "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/ClassLoader;)V"
            ),
            &[
                JValue::Object(&path),
                JValue::Object(&optimized),
                JValue::Object(&JObject::null()),
                JValue::Object(&parent),
            ],
        )?
    };
    let name = env.new_string(VIEW_CLASS)?;
    let class = env
        .call_method(
            &loader,
            jni_str!("loadClass"),
            jni_sig!("(Ljava/lang/String;)Ljava/lang/Class;"),
            &[JValue::Object(&name)],
        )?
        .l()?;
    let class = env.cast_local::<JClass<'_>>(class)?;
    register_natives(env, &class)?;
    let view = env
        .call_static_method(
            &class,
            jni_str!("install"),
            jni_sig!("(Landroid/app/Activity;J)Ldev/flui/platform/FluiView;"),
            &[JValue::Object(activity), JValue::Long(handle)],
        )?
        .l()?;
    match env.new_global_ref(&view) {
        Ok(view) => Ok(view),
        Err(error) => {
            // The view is in the window and names `handle`, which the caller
            // frees on this error: take it out first, so it calls nothing.
            env.exception_clear();
            let _ = env.call_method(&view, jni_str!("detach"), jni_sig!("()V"), &[]);
            env.exception_clear();
            Err(error.into())
        }
    }
}

/// The dex as a read-only file in the code cache, rewritten when what is
/// there is not byte for byte the library's. The only dynamically loaded
/// file the backend writes, below API 26.
fn code_cache_dex(cache: &std::path::Path) -> Result<std::path::PathBuf, InstallError> {
    let path = cache.join(format!("flui-view-{:016x}.dex", fingerprint(DEX)));
    if std::fs::read(&path).ok().as_deref() != Some(DEX) {
        if path.exists() {
            let mut writable = std::fs::metadata(&path)?.permissions();
            #[expect(
                clippy::permissions_set_readonly_false,
                reason = "the stale copy is replaced, then made read-only again"
            )]
            writable.set_readonly(false);
            std::fs::set_permissions(&path, writable)?;
        }
        std::fs::write(&path, DEX)?;
        let mut readonly = std::fs::metadata(&path)?.permissions();
        readonly.set_readonly(true);
        std::fs::set_permissions(&path, readonly)?;
    }
    if std::fs::read(&path)? != DEX {
        return Err(InstallError::Tampered);
    }
    Ok(path)
}

/// FNV-1a over `bytes`: names the copy, so a library with another dex never
/// reads this one's file.
fn fingerprint(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[expect(unsafe_code, reason = "RegisterNatives takes function pointers")]
fn register_natives(env: &mut Env<'_>, class: &JClass<'_>) -> jni::errors::Result<()> {
    // SAFETY: each pointer is an `extern "system"` function below whose
    // parameters are, in order, the JNI environment, the class (every method
    // is static), and the arguments the signature next to it names, with the
    // return type it names.
    let methods = unsafe {
        [
            NativeMethod::from_raw_parts(
                jni_str!("nativeDrag"),
                jni_str!("(JIFF[Ljava/lang/String;ZZ)Z"),
                native_drag as *mut std::ffi::c_void,
            ),
            NativeMethod::from_raw_parts(
                jni_str!("nativeDrop"),
                jni_str!("(JFFZLjava/lang/String;Ljava/lang/String;[Ljava/lang/String;)J"),
                native_drop as *mut std::ffi::c_void,
            ),
            NativeMethod::from_raw_parts(
                jni_str!("nativeImeState"),
                jni_str!("(JLjava/lang/String;IIIII)V"),
                native_ime_state as *mut std::ffi::c_void,
            ),
            NativeMethod::from_raw_parts(
                jni_str!("nativeImeAction"),
                jni_str!("(JIJ)V"),
                native_ime_action as *mut std::ffi::c_void,
            ),
            NativeMethod::from_raw_parts(
                jni_str!("nativeImeCursorUpdates"),
                jni_str!("(JI)V"),
                native_ime_cursor_updates as *mut std::ffi::c_void,
            ),
            NativeMethod::from_raw_parts(
                jni_str!("nativeSettingsChanged"),
                jni_str!("(J)V"),
                native_settings_changed as *mut std::ffi::c_void,
            ),
            NativeMethod::from_raw_parts(
                jni_str!("nativeInsetsChanged"),
                jni_str!("(J)V"),
                native_insets_changed as *mut std::ffi::c_void,
            ),
            NativeMethod::from_raw_parts(
                jni_str!("nativeKeyboardMotion"),
                jni_str!("(JIIJJFFF)V"),
                native_keyboard_motion as *mut std::ffi::c_void,
            ),
            NativeMethod::from_raw_parts(
                jni_str!("nativeKeyboardSettled"),
                jni_str!("(J)V"),
                native_keyboard_settled as *mut std::ffi::c_void,
            ),
            NativeMethod::from_raw_parts(
                jni_str!("nativeNodeInfo"),
                jni_str!(
                    "(JLandroid/view/View;I)Landroid/view/accessibility/AccessibilityNodeInfo;"
                ),
                native_node_info as *mut std::ffi::c_void,
            ),
            NativeMethod::from_raw_parts(
                jni_str!("nativeFindFocus"),
                jni_str!(
                    "(JLandroid/view/View;I)Landroid/view/accessibility/AccessibilityNodeInfo;"
                ),
                native_find_focus as *mut std::ffi::c_void,
            ),
            NativeMethod::from_raw_parts(
                jni_str!("nativePerformAction"),
                jni_str!("(JLandroid/view/View;IILandroid/os/Bundle;)Z"),
                native_perform_action as *mut std::ffi::c_void,
            ),
        ]
    };
    // SAFETY: as above; every method is static, and so receives the class.
    unsafe { env.register_native_methods(class, &methods) }
}

/// The bridge a handle names, while it lives.
#[expect(unsafe_code, reason = "the handle is a Box this module leaked")]
fn bridge(handle: jlong) -> Option<Arc<JavaView>> {
    if handle == 0 {
        return None;
    }
    // SAFETY: a non-zero handle is a `Box<Weak<JavaView>>` from
    // `JavaView::install`. The view passes it only from its own methods on the
    // Java main thread, and `free_handle` frees it on that thread only after
    // `FluiView.detach()` set the view's copy to zero, so it is live here.
    let weak = unsafe { &*(handle as *const Weak<JavaView>) };
    weak.upgrade()
}

/// Run a native method's body, containing a panic before it reaches the JVM:
/// the method then answers its type's default.
fn jni_native<T: Default>(method: &'static str, body: impl FnOnce() -> T) -> T {
    os_entry(method, (), AfterPanic::Answer(|()| T::default()), body)
}

fn strings(env: &mut Env<'_>, array: &JObjectArray<'_>) -> jni::errors::Result<Vec<String>> {
    if array.is_null() {
        return Ok(Vec::new());
    }
    let len = array.len(env)?;
    let mut out = Vec::with_capacity(len);
    for index in 0..len {
        let element = array.get_element(env, index)?;
        let text = env.cast_local::<JString<'_>>(element)?;
        if !text.is_null() {
            out.push(text.try_to_string(env)?);
        }
    }
    Ok(out)
}

fn string(env: &Env<'_>, text: &JString<'_>) -> jni::errors::Result<Option<String>> {
    if text.is_null() {
        Ok(None)
    } else {
        text.try_to_string(env).map(Some)
    }
}

extern "system" fn native_drag<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
    action: jint,
    x: jfloat,
    y: jfloat,
    mime_types: JObjectArray<'local>,
    local: jboolean,
    result: jboolean,
) -> jboolean {
    jni_native("nativeDrag", || {
        let Some(view) = bridge(handle) else {
            return false;
        };
        let mime_types = env.with_env(|env| strings(env, &mime_types)).into_outcome();
        let mime_types = match mime_types {
            jni::Outcome::Ok(types) => types,
            _ => Vec::new(),
        };
        let scale = view.scale();
        view.transfer.drag(
            &view,
            action,
            (f64::from(x) / scale, f64::from(y) / scale),
            &mime_types,
            local,
            result,
        )
    })
}

extern "system" fn native_drop<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
    x: jfloat,
    y: jfloat,
    local: jboolean,
    text: JString<'local>,
    html: JString<'local>,
    uris: JObjectArray<'local>,
) -> jlong {
    jni_native("nativeDrop", || {
        let Some(view) = bridge(handle) else {
            return 0;
        };
        let read = env
            .with_env(|env| -> jni::errors::Result<_> {
                Ok((
                    string(env, &text)?,
                    string(env, &html)?,
                    strings(env, &uris)?,
                ))
            })
            .into_outcome();
        let jni::Outcome::Ok((text, html, uris)) = read else {
            return 0;
        };
        let scale = view.scale();
        view.transfer.drop_clip(
            &view,
            (f64::from(x) / scale, f64::from(y) / scale),
            local,
            text,
            html,
            uris,
        )
    })
}

extern "system" fn native_ime_state<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
    text: JString<'local>,
    selection_start: jint,
    selection_end: jint,
    composition_start: jint,
    composition_end: jint,
    generation: jint,
) {
    jni_native("nativeImeState", || {
        let Some(view) = bridge(handle) else {
            return;
        };
        let text = env.with_env(|env| string(env, &text)).into_outcome();
        let jni::Outcome::Ok(text) = text else {
            return;
        };
        let state = EditorState {
            text: text.unwrap_or_default(),
            selection_start,
            selection_end,
            composition_start,
            composition_end,
        };
        if let Some(reply) = view.input_method.edited(generation.cast_unsigned(), state) {
            view.reply(reply);
        }
    });
}

extern "system" fn native_ime_action<'local>(
    _env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
    action: jint,
    uptime_ms: jlong,
) {
    jni_native("nativeImeAction", || {
        if let Some(view) = bridge(handle) {
            view.reply(view.input_method.editor_action(action, uptime_ms));
        }
    });
}

extern "system" fn native_ime_cursor_updates<'local>(
    _env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
    mode: jint,
) {
    jni_native("nativeImeCursorUpdates", || {
        if let Some(view) = bridge(handle) {
            view.reply(view.input_method.cursor_updates(mode));
        }
    });
}

extern "system" fn native_settings_changed<'local>(
    _env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
) {
    jni_native("nativeSettingsChanged", || {
        if let Some(view) = bridge(handle) {
            view.push_event(ViewEvent::SettingsChanged);
        }
    });
}

extern "system" fn native_insets_changed<'local>(
    _env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
) {
    jni_native("nativeInsetsChanged", || {
        if let Some(view) = bridge(handle) {
            view.push_event(ViewEvent::InsetsChanged);
        }
    });
}

extern "system" fn native_keyboard_motion<'local>(
    _env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
    from: jint,
    to: jint,
    start_nanos: jlong,
    duration_millis: jlong,
    q1: jfloat,
    q2: jfloat,
    q3: jfloat,
) {
    jni_native("nativeKeyboardMotion", || {
        if let Some(view) = bridge(handle) {
            view.push_event(ViewEvent::KeyboardMotion(KeyboardReport {
                from,
                to,
                start: rebased_start(start_nanos),
                duration_millis,
                values: [q1, q2, q3],
            }));
        }
    });
}

extern "system" fn native_keyboard_settled<'local>(
    _env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
) {
    jni_native("nativeKeyboardSettled", || {
        if let Some(view) = bridge(handle) {
            view.push_event(ViewEvent::KeyboardSettled);
        }
    });
}

/// Run one of the node provider's calls with the window's bridge, containing a
/// panic before it reaches the JVM: the call then answers what `after_panic`
/// makes, as it does with no bridge.
fn node_provider<R>(
    method: &'static str,
    handle: jlong,
    after_panic: fn(()) -> R,
    body: impl FnOnce(&dyn ViewAccessibility) -> R,
) -> R {
    os_entry(
        method,
        (),
        AfterPanic::Answer(after_panic),
        || match bridge(handle).and_then(|view| view.accessibility()) {
            Some(accessibility) => body(&*accessibility),
            None => after_panic(()),
        },
    )
}

#[expect(unsafe_code, reason = "the bridge takes the call's raw JNI pointers")]
extern "system" fn native_node_info<'local>(
    env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
    host: JObject<'local>,
    virtual_view_id: jint,
) -> jni::sys::jobject {
    node_provider(
        "nativeNodeInfo",
        handle,
        |()| std::ptr::null_mut(),
        |accessibility| {
            // SAFETY: this native method's own environment and local
            // references, on its thread, for a call that returns before the
            // method does.
            unsafe { accessibility.node_info(env.as_raw(), host.as_raw(), virtual_view_id) }
        },
    )
}

#[expect(unsafe_code, reason = "the bridge takes the call's raw JNI pointers")]
extern "system" fn native_find_focus<'local>(
    env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
    host: JObject<'local>,
    focus_type: jint,
) -> jni::sys::jobject {
    node_provider(
        "nativeFindFocus",
        handle,
        |()| std::ptr::null_mut(),
        |accessibility| {
            // SAFETY: this native method's own environment and local
            // references, on its thread, for a call that returns before the
            // method does.
            unsafe { accessibility.find_focus(env.as_raw(), host.as_raw(), focus_type) }
        },
    )
}

#[expect(unsafe_code, reason = "the bridge takes the call's raw JNI pointers")]
extern "system" fn native_perform_action<'local>(
    env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
    host: JObject<'local>,
    virtual_view_id: jint,
    action: jint,
    arguments: JObject<'local>,
) -> jboolean {
    node_provider(
        "nativePerformAction",
        handle,
        |()| false,
        |accessibility| {
            // SAFETY: this native method's own environment and local
            // references, on its thread, for a call that returns before the
            // method does.
            unsafe {
                accessibility.perform_action(
                    env.as_raw(),
                    host.as_raw(),
                    virtual_view_id,
                    action,
                    arguments.as_raw(),
                )
            }
        },
    )
}

/// `start_nanos`, a `System.nanoTime()` reading (`CLOCK_MONOTONIC`), on the
/// event timeline: as far from this reading of the event clock as it is from
/// this reading of the monotonic one.
fn rebased_start(start_nanos: jlong) -> Option<flui_platform::EventTime> {
    let nanos = |value: i64| {
        u64::try_from(value)
            .ok()
            .map(std::time::Duration::from_nanos)
    };
    let platform_now = nanos(super::frame_clock::monotonic_nanos())?;
    Some(crate::owner::handlers::rebased(
        platform_now,
        nanos(start_nanos)?,
    ))
}
