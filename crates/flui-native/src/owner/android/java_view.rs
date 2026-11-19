//! `FluiView`, the Java side of the Android backend (ADR-0348, ADR-0255).
//!
//! The view's classes are compiled from `crates/flui-native/android/java`
//! into one dex (`android/classes.dex`, checked against its sources by
//! `cargo xtask android-dex`), which `flui build` packs into the APK next to
//! `FluiActivity`, the Activity the generated manifest names. At platform
//! start the view class is loaded on the Java main thread through the
//! Activity's class loader, which reads the APK, after the platform checked
//! the classes' protocol ([`java_host`](super::java_host)). The view's native
//! methods are bound with `RegisterNatives` on the class that loader returned
//! (`FindClass` would search the system loader), and one full-window
//! `FluiView` is laid over the Activity's content.
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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};

use android_activity::AndroidApp;
use flui_foundation::geometry::DevicePixelRatio;
use flui_platform::DragDropEvent;
use jni::objects::{JClass, JObject, JObjectArray, JString};
use jni::refs::Global;
use jni::sys::{jboolean, jfloat, jint, jlong};
use jni::{Env, EnvUnowned, JValue, JavaVM, NativeMethod, jni_sig, jni_str};
use parking_lot::Mutex;

use super::EditorState;
use super::bridge_slot::BridgeSlot;
use super::main_thread;
use super::native_call::jni_native;
use super::view_install::{Installed, Released, ViewHost, ViewInstall};
use crate::owner::signal::OwnerSignal;
use crate::version::gate::android::AndroidApi;

use super::{ActivityWindow, ActivityWindowId};
use crate::owner::android::activity::with_activity;
use crate::window::pointer_capture::{CaptureCall, CaptureOwner, CaptureStep};

/// The binary name of the view class.
const VIEW_CLASS: &str = "dev.flui.platform.FluiView";

/// The host's drag-and-drop transport as the view's drag callbacks reach it:
/// `nativeDrag`, `nativeDropOffer` and `nativeDrop` run on the Java main thread and must be
/// answered at once, from what the owner published earlier (ADR-0211,
/// ADR-0348). The transport is the host's one [`DragTransport`].
///
/// [`DragTransport`]: flui_platform::data_transfer::DragTransport
pub(crate) trait ViewTransfer: flui_platform::data_transfer::DragTransport {
    /// The media types `nativeDrag` received, another application's, read
    /// within the bounds every host's drag holds a source to; a `null` array
    /// is empty.
    ///
    /// # Errors
    ///
    /// The first failed JNI call.
    fn mime_types(
        &self,
        env: &mut Env<'_>,
        array: &JObjectArray<'_>,
    ) -> jni::errors::Result<Vec<String>>;

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

    /// The key of the offer over the view, under which the view holds the
    /// clip of a drop before it asks [`Self::take_drop`]; 0 when none is.
    fn hover_key(&self) -> i64;

    /// The drop at `position` of the clip the view holds under `key`: whether
    /// it is taken. The view lets go of the clip at once when it is not, and
    /// when the target concludes it otherwise.
    fn take_drop(&self, view: &JavaView, key: i64, position: (f64, f64), local: bool) -> bool;

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

/// Why `FluiView` is not in the window after its installation finished. The
/// backend then runs without the view for the Activity's life: nothing
/// reinstalls it.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ViewMissing {
    /// Java refused the view.
    #[error("Java refused FluiView: {error} (handle kept: {handle_kept})")]
    Refused {
        /// What Java threw, as JNI reports it.
        error: String,
        /// The window could not say whether a view names the handle.
        handle_kept: bool,
    },
    /// The installation panicked or its work was dropped unrun.
    #[error("FluiView's installation stopped short: {failure} (handle kept: {handle_kept})")]
    StoppedShort {
        /// How the posted installation ended.
        failure: String,
        /// The window could not say whether a view names the handle.
        handle_kept: bool,
    },
}

/// What the view reported, for the owner to deliver.
pub(crate) enum ViewEvent {
    /// The view's installation finished: in the window, or why not.
    Installed(Result<(), ViewMissing>),
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
    /// Whether the system holds the pointer capture for `window` changed, as
    /// the capture owner attributes it ([`CaptureOwner`]).
    PointerCaptureChanged {
        /// The window whose capture changed.
        window: ActivityWindowId,
        /// Whether the system holds the capture for it now.
        held: bool,
    },
}

/// What a window tells the capture owner, which runs on the Java main thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CaptureCommand {
    /// The window asks for the mouse.
    Request(ActivityWindowId),
    /// The window lets go: its release command, or its retirement.
    Release(ActivityWindowId),
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
/// Each of `edited`, `editor_action` and `cursor_updates` comes from a native
/// method of the view, on the Java main thread under no lock of the view, and
/// answers what the view applies and queues.
pub(crate) trait InputMethod: Send + Sync {
    /// `nativeImeState`: what the view holds after an input method edit;
    /// `None` before the view holds a snapshot.
    fn edited(&self, state: EditorState) -> Option<InputMethodReply>;

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

    /// The accessibility manager reports whether any service is enabled, the
    /// only source of that fact: with none, the tree ends and the composition
    /// root is told, with nothing borrowed, that nobody listens, and no
    /// question starts the tree until a service is reported again. Java main
    /// thread; a repeated report of none tells nobody.
    fn services_changed(&self, enabled: bool);

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

/// A reference to the installed view.
type InstalledView = Arc<Global<JObject<'static>>>;

/// The `Box<Weak<JavaView>>` the view holds as its handle, as its address:
/// freed on the Java main thread once no view names it ([`ViewInstall`]).
type ViewHandle = usize;

/// The bridge between the owner thread and `FluiView` (see the module doc).
pub(crate) struct JavaView {
    app: AndroidApp,
    signal: Weak<OwnerSignal>,
    transfer: Arc<dyn ViewTransfer>,
    /// What answers the input method's calls through the view.
    input_method: Box<dyn InputMethod>,
    /// Tells the preference source of a changed setting, on the Java main
    /// thread; it takes only the source's own lock.
    settings_notice: Box<dyn Fn() + Send + Sync>,
    /// What answers TalkBack's calls through the view's node provider: the
    /// bridge of the window over the view, while it lives.
    accessibility: BridgeSlot<dyn ViewAccessibility>,
    installation: ViewInstall<InstalledView, ViewHandle>,
    /// What the view reported, oldest first.
    events: Mutex<VecDeque<ViewEvent>>,
    /// Whether the input method composes, as the view last reported.
    composing: AtomicBool,
    /// The window's scale factor, as the owner last read it, for the view's
    /// pixel coordinates. A lock held only to copy the value: its type, not a
    /// check at each read, keeps it a ratio.
    scale: Mutex<DevicePixelRatio>,
    /// The Activity's pointer capture, which every window's commands and
    /// every system answer go through on the Java main thread. A lock, though
    /// that one thread alone touches it: the bridge is an `Arc` the owner
    /// thread shares, so its fields are `Sync`, and no dependency binds a
    /// value to the Java main thread without an `unsafe impl Send`/`Sync`
    /// (the debt `docs/gaps.md` names under ADR-0327 §1). Never contended,
    /// and no JNI call runs under it.
    capture: Mutex<CaptureOwner<ActivityWindowId>>,
}

impl std::fmt::Debug for JavaView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JavaView").finish_non_exhaustive()
    }
}

impl JavaView {
    /// The bridge, with the view's installation posted to the Java main thread
    /// and the drag-and-drop transport `transfer` makes for it.
    /// `settings_notice` tells the preference source of each setting the
    /// view reports changed.
    pub(super) fn start(
        app: AndroidApp,
        signal: Weak<OwnerSignal>,
        input_method: Box<dyn InputMethod>,
        settings_notice: Box<dyn Fn() + Send + Sync>,
        transfer: TransferFor,
    ) -> Arc<Self> {
        let view = Arc::new_cyclic(|this: &Weak<Self>| Self {
            app,
            signal,
            transfer: transfer(this.clone()),
            input_method,
            settings_notice,
            accessibility: BridgeSlot::default(),
            installation: ViewInstall::new(),
            events: Mutex::new(VecDeque::new()),
            composing: AtomicBool::new(false),
            scale: Mutex::new(DevicePixelRatio::ONE),
            capture: Mutex::new(CaptureOwner::default()),
        });
        let (installing, unsettled) = (Arc::clone(&view), Arc::clone(&view));
        main_thread::post(
            &view.app,
            "FluiView.install",
            move || {
                installing.install();
                Ok(())
            },
            // An installation that stopped short takes out what Java put in
            // the window, and the owner hears that it finished.
            move |failure: main_thread::PostFailure| unsettled.installation_stopped(&failure),
        );
        view
    }

    /// Install the view, then tell the owner, which reports a failure once;
    /// the state is committed before anything runs that is not this crate's
    /// (a subscriber is user code). An installation the platform's end
    /// overtook tells nobody. Java main thread only.
    fn install(self: &Arc<Self>) {
        match self.installation.install(self) {
            Installed::Ready => {
                self.push_event(ViewEvent::Installed(Ok(())));
                tracing::info!(target: "flui_native::android", "FluiView is installed");
            }
            Installed::Refused { error, handle_kept } => {
                self.push_event(ViewEvent::Installed(Err(ViewMissing::Refused {
                    error,
                    handle_kept,
                })));
            }
            Installed::Detached => {}
        }
    }

    /// The installation stopped short (a panic, or its work dropped): what
    /// Java put in the window is taken out, and the owner hears why the view
    /// is missing. An installation that had finished, or never began, leaves
    /// nothing to report.
    fn installation_stopped(self: &Arc<Self>, failure: &main_thread::PostFailure) {
        let Some(released) = self.installation.stopped_short(self) else {
            return;
        };
        self.push_event(ViewEvent::Installed(Err(ViewMissing::StoppedShort {
            failure: format!("{failure:?}"),
            handle_kept: released == Released::Kept,
        })));
    }

    /// The platform's drag-and-drop transport.
    pub(crate) fn transfer(&self) -> &Arc<dyn ViewTransfer> {
        &self.transfer
    }

    /// Whether the view is in the window.
    pub(crate) fn is_ready(&self) -> bool {
        self.installation.view().is_some()
    }

    pub(super) fn composing(&self) -> bool {
        self.composing.load(Ordering::Acquire)
    }

    pub(crate) fn set_composing(&self, composing: bool) {
        self.composing.store(composing, Ordering::Release);
    }

    /// The window's scale, which the loop read by the hosts' one rule, for
    /// the Java threads' callbacks.
    pub(super) fn set_scale(&self, scale: DevicePixelRatio) {
        *self.scale.lock() = scale;
    }

    pub(crate) fn scale(&self) -> DevicePixelRatio {
        *self.scale.lock()
    }

    /// Everything the view reported since the last call, oldest first.
    pub(super) fn take_events(&self) -> VecDeque<ViewEvent> {
        std::mem::take(&mut *self.events.lock())
    }

    /// Queue `event` for the owner and wake it.
    pub(crate) fn push_event(&self, event: ViewEvent) {
        self.events.lock().push_back(event);
        if let Some(signal) = self.signal.upgrade() {
            let _ = signal.wake();
        }
    }

    /// A setting behind the preferences changed: the preference source is
    /// told before the owner wakes, so the turn the wake brings reads the
    /// system (ADR-0378).
    fn settings_changed(&self) {
        (self.settings_notice)();
        self.push_event(ViewEvent::SettingsChanged);
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

    /// Hand `command` to the capture owner on the Java main thread
    /// (API 26), which makes the one call it calls for and reports each
    /// window whose capture changed as [`ViewEvent::PointerCaptureChanged`].
    /// A command that never reaches the main thread changes nothing. Owner
    /// thread.
    pub(crate) fn capture_command(self: &Arc<Self>, command: CaptureCommand) {
        let this = Arc::clone(self);
        main_thread::run(&self.app, "a pointer capture command", move || {
            let step = {
                let mut owner = this.capture.lock();
                match command {
                    CaptureCommand::Request(window) => owner.request(window),
                    CaptureCommand::Release(window) => owner.release(window),
                }
            };
            this.carry_out_capture(step);
        });
    }

    /// The system answered (`onPointerCaptureChange`). Java main thread.
    fn capture_answered(self: &Arc<Self>, has_capture: bool) {
        let step = self.capture.lock().answered(has_capture);
        self.carry_out_capture(step);
    }

    /// The Activity window has the focus or not (`onWindowFocusChanged`, and
    /// once as `FluiView` enters the window), heard with the capture's
    /// answers. Java main thread.
    fn window_focus_changed(self: &Arc<Self>, focused: bool) {
        let step = self.capture.lock().focus_changed(focused);
        self.carry_out_capture(step);
    }

    /// Report each window whose capture changed, then make the call `step`
    /// calls for; a call that does not reach the system (no view, or it
    /// threw) is told to the owner, which changes nothing for it. Java main
    /// thread, with the owner's lock let go.
    fn carry_out_capture(self: &Arc<Self>, step: CaptureStep<ActivityWindowId>) {
        for (window, held) in step.changed.into_iter().flatten() {
            self.push_event(ViewEvent::PointerCaptureChanged { window, held });
        }
        let Some(call) = step.call else {
            return;
        };
        let method = match call {
            CaptureCall::Request(_) => jni_str!("requestPointerCapture"),
            CaptureCall::Release => jni_str!("releasePointerCapture"),
        };
        let made = self.view().is_some_and(|view| {
            self.attached(|env| {
                env.call_method(view.as_ref(), method, jni_sig!("()V"), &[])
                    .map(drop)
            })
            .is_ok()
        });
        if !made {
            self.capture.lock().call_failed(call);
        }
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
    /// runs when the view is not installed, the call fails or panics, it
    /// answers `false`, or the work is dropped unrun, so an obligation the
    /// call carried is still discharged ([`main_thread::post`]).
    pub(crate) fn post_or(
        self: &Arc<Self>,
        call: impl FnOnce(&mut Env<'_>, &JObject<'_>) -> jni::errors::Result<bool> + Send + 'static,
        failed: impl FnOnce(&Self) + Send + 'static,
    ) {
        let (this, owed) = (Arc::clone(self), Arc::clone(self));
        main_thread::post(
            &self.app,
            "a FluiView call",
            move || {
                let view = this
                    .view()
                    .ok_or_else(|| "the view is not installed".to_owned())?;
                match this.attached(|env| call(env, view.as_ref())) {
                    Ok(true) => Ok(()),
                    Ok(false) => Err("the call answered that it did not work".to_owned()),
                    Err(error) => Err(error.to_string()),
                }
            },
            move |failure: main_thread::PostFailure| {
                tracing::warn!(target: "flui_native::android", ?failure, "a FluiView call failed");
                failed(&owed);
            },
        );
    }

    /// The installed view, cloned out of the lock so no JNI call runs under it.
    pub(crate) fn view(&self) -> Option<InstalledView> {
        self.installation.view()
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

    /// The platform ended: the view leaves the window, stops calling native
    /// code, and its handle is freed after that, on the Java main thread; a
    /// view that may still name it keeps it.
    pub(super) fn detach(self: &Arc<Self>) {
        let this = Arc::clone(self);
        main_thread::run(&self.app, "FluiView.detach", move || {
            if this.installation.detach(&this) == Some(Released::Kept) {
                tracing::warn!(
                    target: "flui_native::android",
                    "FluiView did not detach; its handle is kept"
                );
            }
        });
    }
}

/// The installation's JNI calls (see [`ViewInstall`]), on the Java main
/// thread.
impl ViewHost for Arc<JavaView> {
    type View = InstalledView;
    type Handle = ViewHandle;
    type Error = String;

    fn mint(&self) -> ViewHandle {
        Box::into_raw(Box::new(Arc::downgrade(self))) as ViewHandle
    }

    #[expect(unsafe_code, reason = "the handle is a Box this module leaked")]
    fn free(&self, handle: ViewHandle) {
        // SAFETY: `handle` came from `mint`, and `ViewInstall` frees it once,
        // when no view names it: Java never got it, or the view that named it
        // detached (which clears the view's copy) on this same thread, the
        // only one that calls the view's native methods.
        drop(unsafe { Box::from_raw(handle as *mut Weak<JavaView>) });
    }

    fn install(&self, handle: ViewHandle) -> Result<InstalledView, String> {
        with_activity(&self.app, |env, activity| {
            let class = view_class(env, activity)?;
            register_natives(env, &class)?;
            let view = env
                .call_static_method(
                    &class,
                    jni_str!("install"),
                    jni_sig!("(Landroid/app/Activity;J)Ldev/flui/platform/FluiView;"),
                    &[JValue::Object(activity), JValue::Long(handle as jlong)],
                )?
                .l()?;
            env.new_global_ref(&view)
        })
        .map(Arc::new)
        .map_err(|error| error.to_string())
    }

    fn detach(&self, view: &InstalledView) -> Result<(), String> {
        self.attached(|env| {
            env.call_method(view.as_ref(), jni_str!("detach"), jni_sig!("()V"), &[])
                .map(drop)
        })
        .map_err(|error| error.to_string())
    }

    fn withdraw(&self, handle: ViewHandle) -> Result<bool, String> {
        with_activity(&self.app, |env, activity| {
            let class = view_class(env, activity)?;
            let window = env
                .call_method(
                    activity,
                    jni_str!("getWindow"),
                    jni_sig!("()Landroid/view/Window;"),
                    &[],
                )?
                .l()?;
            let decor = env
                .call_method(
                    &window,
                    jni_str!("getDecorView"),
                    jni_sig!("()Landroid/view/View;"),
                    &[],
                )?
                .l()?;
            let children = env
                .call_method(&decor, jni_str!("getChildCount"), jni_sig!("()I"), &[])?
                .i()?;
            for index in 0..children {
                let child = env
                    .call_method(
                        &decor,
                        jni_str!("getChildAt"),
                        jni_sig!("(I)Landroid/view/View;"),
                        &[JValue::Int(index)],
                    )?
                    .l()?;
                if !env.is_instance_of(&child, &class)? {
                    continue;
                }
                let named = env
                    .call_method(&child, jni_str!("handle"), jni_sig!("()J"), &[])?
                    .j()?;
                if named == handle as jlong {
                    env.call_method(&child, jni_str!("detach"), jni_sig!("()V"), &[])?;
                    return Ok(true);
                }
            }
            Ok(false)
        })
        .map_err(|error| error.to_string())
    }
}

/// `FluiView`, loaded through the Activity's class loader, which reads it
/// from the APK's dex (`FindClass` would search the system loader). The
/// platform checked the classes' protocol before it started
/// ([`java_host`](super::java_host)).
pub(crate) fn view_class<'local>(
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
    let name = env.new_string(VIEW_CLASS)?;
    let class = env
        .call_method(
            &loader,
            jni_str!("loadClass"),
            jni_sig!("(Ljava/lang/String;)Ljava/lang/Class;"),
            &[JValue::Object(&name)],
        )?
        .l()?;
    env.cast_local::<JClass<'_>>(class)
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
                jni_str!("nativeDropOffer"),
                jni_str!("(J)J"),
                native_drop_offer as *mut std::ffi::c_void,
            ),
            NativeMethod::from_raw_parts(
                jni_str!("nativeDrop"),
                jni_str!("(JJFFZ)Z"),
                native_drop as *mut std::ffi::c_void,
            ),
            NativeMethod::from_raw_parts(
                jni_str!("nativeImeState"),
                jni_str!("(JLjava/lang/String;IIII)V"),
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
                jni_str!("nativePointerCaptureChanged"),
                jni_str!("(JZ)V"),
                native_pointer_capture_changed as *mut std::ffi::c_void,
            ),
            NativeMethod::from_raw_parts(
                jni_str!("nativeWindowFocusChanged"),
                jni_str!("(JZ)V"),
                native_window_focus_changed as *mut std::ffi::c_void,
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
            NativeMethod::from_raw_parts(
                jni_str!("nativeAccessibilityServices"),
                jni_str!("(JZ)V"),
                native_accessibility_services as *mut std::ffi::c_void,
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
    // SAFETY: a non-zero handle is a `Box<Weak<JavaView>>` from the
    // installation's `mint`. The view passes it only from its own methods on
    // the Java main thread, and `ViewInstall` frees it on that thread only
    // once no view names it, so it is live here.
    let weak = unsafe { &*(handle as *const Weak<JavaView>) };
    weak.upgrade()
}

/// The bridge of the window over the view a handle names, while both live.
fn view_accessibility(handle: jlong) -> Option<Arc<dyn ViewAccessibility>> {
    bridge(handle).and_then(|view| view.accessibility())
}

/// The strings of `array`, in order; a `null` array is empty and a `null`
/// element is skipped. One element's local reference lives at a time. For an
/// array FLUI's own Java side bounds (an intent's record); another
/// application's media types are read by [`ViewTransfer::mime_types`].
pub(crate) fn strings(
    env: &mut Env<'_>,
    array: &JObjectArray<'_>,
) -> jni::errors::Result<Vec<String>> {
    if array.is_null() {
        return Ok(Vec::new());
    }
    let len = array.len(env)?;
    let mut out = Vec::with_capacity(len);
    for index in 0..len {
        let element = array.get_element(env, index)?;
        let text = env.cast_local::<JString<'_>>(element)?;
        if !text.is_null() {
            let copied = text.try_to_string(env);
            env.delete_local_ref(text);
            out.push(copied?);
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
    jni_native(
        "nativeDrag",
        &mut env,
        |()| false,
        |env| {
            let Some(view) = bridge(handle) else {
                return Ok(false);
            };
            let mime_types = view.transfer.mime_types(env, &mime_types)?;
            let scale = view.scale();
            Ok(view.transfer.drag(
                &view,
                action,
                (
                    scale.to_logical(f64::from(x)),
                    scale.to_logical(f64::from(y)),
                ),
                &mime_types,
                local,
                result,
            ))
        },
    )
}

extern "system" fn native_drop_offer<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
) -> jlong {
    jni_native(
        "nativeDropOffer",
        &mut env,
        |()| 0,
        |_| Ok(bridge(handle).map_or(0, |view| view.transfer.hover_key())),
    )
}

extern "system" fn native_drop<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
    offer: jlong,
    x: jfloat,
    y: jfloat,
    local: jboolean,
) -> jboolean {
    jni_native(
        "nativeDrop",
        &mut env,
        |()| false,
        |_| {
            let Some(view) = bridge(handle) else {
                return Ok(false);
            };
            let scale = view.scale();
            Ok(view.transfer.take_drop(
                &view,
                offer,
                (
                    scale.to_logical(f64::from(x)),
                    scale.to_logical(f64::from(y)),
                ),
                local,
            ))
        },
    )
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
) {
    jni_native(
        "nativeImeState",
        &mut env,
        |()| (),
        |env| {
            let Some(view) = bridge(handle) else {
                return Ok(());
            };
            let text = string(env, &text)?;
            let state = EditorState {
                text: text.unwrap_or_default(),
                selection_start,
                selection_end,
                composition_start,
                composition_end,
            };
            if let Some(reply) = view.input_method.edited(state) {
                view.reply(reply);
            }
            Ok(())
        },
    );
}

extern "system" fn native_ime_action<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
    action: jint,
    uptime_ms: jlong,
) {
    jni_native(
        "nativeImeAction",
        &mut env,
        |()| (),
        |_| {
            if let Some(view) = bridge(handle) {
                view.reply(view.input_method.editor_action(action, uptime_ms));
            }
            Ok(())
        },
    );
}

extern "system" fn native_ime_cursor_updates<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
    mode: jint,
) {
    jni_native(
        "nativeImeCursorUpdates",
        &mut env,
        |()| (),
        |_| {
            if let Some(view) = bridge(handle) {
                view.reply(view.input_method.cursor_updates(mode));
            }
            Ok(())
        },
    );
}

extern "system" fn native_settings_changed<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
) {
    jni_native(
        "nativeSettingsChanged",
        &mut env,
        |()| (),
        |_| {
            if let Some(view) = bridge(handle) {
                view.settings_changed();
            }
            Ok(())
        },
    );
}

extern "system" fn native_insets_changed<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
) {
    jni_native(
        "nativeInsetsChanged",
        &mut env,
        |()| (),
        |_| {
            if let Some(view) = bridge(handle) {
                view.push_event(ViewEvent::InsetsChanged);
            }
            Ok(())
        },
    );
}

extern "system" fn native_keyboard_motion<'local>(
    mut env: EnvUnowned<'local>,
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
    jni_native(
        "nativeKeyboardMotion",
        &mut env,
        |()| (),
        |_| {
            if let Some(view) = bridge(handle) {
                view.push_event(ViewEvent::KeyboardMotion(KeyboardReport {
                    from,
                    to,
                    start: rebased_start(start_nanos),
                    duration_millis,
                    values: [q1, q2, q3],
                }));
            }
            Ok(())
        },
    );
}

extern "system" fn native_keyboard_settled<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
) {
    jni_native(
        "nativeKeyboardSettled",
        &mut env,
        |()| (),
        |_| {
            if let Some(view) = bridge(handle) {
                view.push_event(ViewEvent::KeyboardSettled);
            }
            Ok(())
        },
    );
}

extern "system" fn native_pointer_capture_changed<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
    has_capture: jboolean,
) {
    jni_native(
        "nativePointerCaptureChanged",
        &mut env,
        |()| (),
        |_| {
            if let Some(view) = bridge(handle) {
                view.capture_answered(has_capture);
            }
            Ok(())
        },
    );
}

extern "system" fn native_window_focus_changed<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
    has_window_focus: jboolean,
) {
    jni_native(
        "nativeWindowFocusChanged",
        &mut env,
        |()| (),
        |_| {
            if let Some(view) = bridge(handle) {
                view.window_focus_changed(has_window_focus);
            }
            Ok(())
        },
    );
}

#[expect(unsafe_code, reason = "the bridge takes the call's raw JNI pointers")]
extern "system" fn native_node_info<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
    host: JObject<'local>,
    virtual_view_id: jint,
) -> jni::sys::jobject {
    jni_native(
        "nativeNodeInfo",
        &mut env,
        |()| std::ptr::null_mut(),
        |env| {
            Ok(
                view_accessibility(handle).map_or(std::ptr::null_mut(), |accessibility| {
                    // SAFETY: this native method's own environment and local
                    // references, on its thread, for a call that returns before
                    // the method does.
                    unsafe {
                        accessibility.node_info(env.get_raw(), host.as_raw(), virtual_view_id)
                    }
                }),
            )
        },
    )
}

#[expect(unsafe_code, reason = "the bridge takes the call's raw JNI pointers")]
extern "system" fn native_find_focus<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
    host: JObject<'local>,
    focus_type: jint,
) -> jni::sys::jobject {
    jni_native(
        "nativeFindFocus",
        &mut env,
        |()| std::ptr::null_mut(),
        |env| {
            Ok(
                view_accessibility(handle).map_or(std::ptr::null_mut(), |accessibility| {
                    // SAFETY: this native method's own environment and local
                    // references, on its thread, for a call that returns before
                    // the method does.
                    unsafe { accessibility.find_focus(env.get_raw(), host.as_raw(), focus_type) }
                }),
            )
        },
    )
}

#[expect(unsafe_code, reason = "the bridge takes the call's raw JNI pointers")]
extern "system" fn native_perform_action<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
    host: JObject<'local>,
    virtual_view_id: jint,
    action: jint,
    arguments: JObject<'local>,
) -> jboolean {
    jni_native(
        "nativePerformAction",
        &mut env,
        |()| false,
        |env| {
            Ok(view_accessibility(handle).is_some_and(|accessibility| {
                // SAFETY: this native method's own environment and local
                // references, on its thread, for a call that returns before the
                // method does.
                unsafe {
                    accessibility.perform_action(
                        env.get_raw(),
                        host.as_raw(),
                        virtual_view_id,
                        action,
                        arguments.as_raw(),
                    )
                }
            }))
        },
    )
}

/// Whether any accessibility service is enabled, as the manager answers now:
/// the window's bridge builds its tree only while one is.
extern "system" fn native_accessibility_services<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
    enabled: jboolean,
) {
    jni_native(
        "nativeAccessibilityServices",
        &mut env,
        |()| (),
        |_| {
            if let Some(accessibility) = view_accessibility(handle) {
                accessibility.services_changed(enabled);
            }
            Ok(())
        },
    );
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
    Some(crate::owner::frame_timing::rebased(
        platform_now,
        nanos(start_nanos)?,
    ))
}
