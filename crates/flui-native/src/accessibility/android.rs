//! TalkBack for Android, via `accesskit_android`'s low-level [`Adapter`]
//! (`docs/gaps.md#no-accessibility-on-mobile-and-the-web`).
//!
//! `FluiView` is FLUI's own view (ADR-0348), so the accessibility delegate it
//! installs (ADR-0255 decision 3) hands the framework a node provider that
//! forwards `createAccessibilityNodeInfo`, `findFocus` and `performAction` to
//! the adapter itself: AccessKit's injecting adapter and its dex are not
//! used. The view reaches the adapter through the accessibility slot of its
//! [`JavaView`], which this bridge fills as the window opens.
//!
//! # Threads
//!
//! The adapter lives on the Java main thread, in a
//! [`ProvidedTree`] made there by the first call that reaches it and held
//! in an [`OwnerSlot`] of that thread, which every other thread reaches as
//! nothing. The node provider's calls arrive there and are answered at once
//! from the tree the adapter holds. A publish from the owner thread retains
//! a self-contained update and posts the update to the main thread, where
//! it is applied and its events raised. The posts run in order, so the
//! adapter applies the updates in the order the owner published them, and
//! neither thread waits for the other.
//!
//! The adapter is borrowed only on the main thread and never across an
//! event raise: TalkBack answers an event by asking for node info, which
//! re-enters the provider. A call that finds it borrowed anyway is
//! [`Busy`] and answers as a view with no tree.
//!
//! # While a service listens
//!
//! `FluiView` reports whether any accessibility service is enabled
//! (`AccessibilityManager.isEnabled()`, asked as its accessibility-state
//! listener fires, as the view enters the window and as the bridge arrives),
//! which reaches [`ViewAccessibility::services_changed`]. While one is, a
//! service's first question through the node provider starts the tree; when
//! none is, the adapter drops its tree and the composition root, told on the
//! main thread with nothing borrowed, stops building it, so a publish posts
//! nothing. A question while none is enabled (Assist structure, the autofill
//! framework's compatibility mode, a hover posted before the report) is
//! answered from a placeholder and starts nothing ([`ProvidedTree`]). Touch
//! exploration ending is not a stop: another service (Switch Access, Select
//! to Speak) still reads the tree without it.
//!
//! # Explore by touch
//!
//! Explore by touch reaches the adapter from the native input queue, which
//! NativeActivity takes before any view: the loop offers a finger hovering
//! over the touchscreen to [`JavaView::explore_by_touch`], and the hover is
//! applied on the main thread like a publish (ADR-0255 decision 3). The
//! published adapter keeps where TalkBack's cursor is to itself, so this
//! bridge reports no assistive focus.
//!
//! # Two jni releases
//!
//! `accesskit_android` is built on jni 0.21 beside FLUI's 0.22, until it
//! moves to 0.22. The two meet only through raw pointers: the environment
//! and the view go in, the node info comes back as a local reference.

// This module calls the operating system directly, which has no safe wrapper:
// the workspace lint `unsafe_code = "warn"` is opted out here, at the module
// boundary, rather than for the whole crate (see `lib.rs`).
#![expect(unsafe_code)]

use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use accesskit::TreeUpdate;
use accesskit_android::jni::JNIEnv;
use accesskit_android::jni::objects::JObject;
use accesskit_android::jni::sys as jni021;
use accesskit_android::{Adapter, PlatformAction, QueuedEvents};

use crate::accessibility::bridge::BridgeShared;
use crate::accessibility::provided_tree::{Busy, Provided, ProvidedAdapter, ProvidedTree};
use crate::owner::android::ActivityRef;
use crate::owner::android::java_view::{JavaView, ViewAccessibility};
use crate::owner::android::native_call::{ClearsPending, EnvSlot};
use crate::owner::slot::OwnerSlot;
use flui_semantics::platform::{
    AccessibilityActionListener, AccessibilityActivationListener, PlatformAccessibility,
    RouteChange, RouteChangeSupport,
};

impl ProvidedAdapter for Adapter {
    type Events = QueuedEvents;

    fn update_if_active(&mut self, update: TreeUpdate) -> Option<QueuedEvents> {
        Adapter::update_if_active(self, move || update)
    }
}

/// The part of the bridge the Java main thread holds: the adapter, which the
/// view's node provider, the owner's posted updates and the accessibility
/// manager's notice reach.
struct NodeProvider {
    shared: Arc<BridgeShared>,
    /// Made by the first call that reaches it, on the Java main thread, and
    /// bound to that thread: every call into the adapter runs there.
    tree: OnceLock<OwnerSlot<Rc<ProvidedTree<Adapter>>>>,
}

impl NodeProvider {
    /// The tree, made on first use. Java main thread; `None` elsewhere.
    fn tree(&self) -> Option<Rc<ProvidedTree<Adapter>>> {
        self.tree
            .get_or_init(|| {
                let slot = OwnerSlot::new();
                if slot
                    .replace(Rc::new(ProvidedTree::new(Arc::clone(&self.shared))))
                    .is_err()
                {
                    tracing::error!(
                        target: "flui_native::android",
                        "TalkBack's tree holds nothing: owner slots are exhausted"
                    );
                }
                slot
            })
            .cloned()
    }

    /// Run `f` with the adapter, or answer `None` when a call on this thread
    /// is already inside it. Java main thread.
    fn with<R>(&self, f: impl FnOnce(&mut Provided<Adapter>) -> R) -> Option<R> {
        let answer = self.tree()?.with(f);
        if answer == Err(Busy) {
            tracing::warn!(
                target: "flui_native::android",
                "TalkBack re-entered the node provider while a call was inside the adapter"
            );
        }
        answer.ok()
    }

    /// Run `f` with the adapter; then, with the adapter free, raise the
    /// events `f` made. Java main thread.
    fn raising<R>(
        &self,
        env: &mut JNIEnv<'_>,
        host: &JObject<'_>,
        f: impl FnOnce(&mut Provided<Adapter>) -> (Option<QueuedEvents>, R),
    ) -> Option<R> {
        let (events, answer) = self.with(f)?;
        raise(env, host, events);
        Some(answer)
    }

    /// Apply `update`, which an adapter whose tree ended drops. Java main
    /// thread.
    fn apply(&self, env: &mut JNIEnv<'_>, host: &JObject<'_>, update: TreeUpdate) {
        let Some(tree) = self.tree() else {
            return;
        };
        let Ok(events) = tree.apply(update) else {
            tracing::warn!(
                target: "flui_native::android",
                "an update reached the adapter while a call was inside it and was dropped"
            );
            return;
        };
        raise(env, host, events);
    }

    /// Hand the adapter one touch-exploration hover. Java main thread.
    fn hover(&self, env: &mut JNIEnv<'_>, host: &JObject<'_>, action: i32, x: f32, y: f32) {
        self.raising(env, host, |provided| {
            let events = provided
                .adapter
                .on_hover_event(&mut provided.activation, action, x, y);
            (events, ())
        });
    }
}

/// Raise `events` against `host`, after the adapter's lock was released.
fn raise(env: &mut JNIEnv<'_>, host: &JObject<'_>, events: Option<QueuedEvents>) {
    if let Some(events) = events {
        events.raise(env, host);
    }
}

/// Run `f` with the 0.21 environment and view behind the raw pointers of a
/// JNI call (see the module doc), clearing what it leaves pending, also when
/// it panics: the adapter's JNI calls `unwrap`, and the provider's caller
/// must see the answer, not a throw ([`ClearsPending`]).
///
/// # Safety
///
/// `env` is the environment of a JNI call running on this thread, and `host`
/// a reference to the view valid for that call, which `f` does not outlive.
unsafe fn with_env<R>(
    env: *mut jni::sys::JNIEnv,
    host: jni::sys::jobject,
    f: impl FnOnce(&mut JNIEnv<'_>, &JObject<'_>) -> R,
) -> Option<R> {
    // SAFETY: the caller's contract: the environment of a JNI call on this
    // thread, which the guard, dropped at the end of this function, does not
    // outlive.
    let _clears = ClearsPending::new(unsafe { EnvSlot::new(env) });
    // jni 0.21 and 0.22 bind the same JNI types, each through its own jni-sys.
    let env = env.cast::<jni021::JNIEnv>();
    // SAFETY: the caller's contract; the environment is borrowed, not
    // attached or detached.
    let mut env = unsafe { JNIEnv::from_raw(env) }.ok()?;
    // SAFETY: the caller's contract; a `JObject` deletes no reference when
    // dropped.
    let host = unsafe { JObject::from_raw(host.cast()) };
    Some(f(&mut env, &host))
}

/// `AccessibilityEvent.TYPE_WINDOW_STATE_CHANGED`.
const TYPE_WINDOW_STATE_CHANGED: jni021::jint = 0x20;

/// Raise a window state change from `host`, carrying `name` as its text, as
/// a native screen does when it opens.
fn send_window_state_changed(
    env: &mut JNIEnv<'_>,
    host: &JObject<'_>,
    name: Option<&str>,
) -> accesskit_android::jni::errors::Result<()> {
    let parent = env
        .call_method(host, "getParent", "()Landroid/view/ViewParent;", &[])?
        .l()?;
    // A view detached since the change was posted has no window to tell.
    if parent.is_null() {
        return Ok(());
    }
    let event = env
        .call_static_method(
            "android/view/accessibility/AccessibilityEvent",
            "obtain",
            "(I)Landroid/view/accessibility/AccessibilityEvent;",
            &[TYPE_WINDOW_STATE_CHANGED.into()],
        )?
        .l()?;
    let context = env
        .call_method(host, "getContext", "()Landroid/content/Context;", &[])?
        .l()?;
    let package = env
        .call_method(&context, "getPackageName", "()Ljava/lang/String;", &[])?
        .l()?;
    env.call_method(
        &event,
        "setPackageName",
        "(Ljava/lang/CharSequence;)V",
        &[(&package).into()],
    )?;
    env.call_method(
        &event,
        "setSource",
        "(Landroid/view/View;)V",
        &[host.into()],
    )?;
    if let Some(name) = name {
        let text = env
            .call_method(&event, "getText", "()Ljava/util/List;", &[])?
            .l()?;
        let name = env.new_string(name)?;
        env.call_method(&text, "add", "(Ljava/lang/Object;)Z", &[(&name).into()])?;
    }
    env.call_method(
        &parent,
        "requestSendAccessibilityEvent",
        "(Landroid/view/View;Landroid/view/accessibility/AccessibilityEvent;)Z",
        &[host.into(), (&event).into()],
    )?;
    Ok(())
}

impl ViewAccessibility for NodeProvider {
    unsafe fn node_info(
        &self,
        env: *mut jni::sys::JNIEnv,
        host: jni::sys::jobject,
        virtual_view_id: jni::sys::jint,
    ) -> jni::sys::jobject {
        // SAFETY: the contract of `ViewAccessibility`, which this call keeps:
        // the closure ends with it.
        unsafe {
            with_env(env, host, |env, host| {
                self.with(|provided| {
                    provided.adapter.create_accessibility_node_info(
                        &mut provided.activation,
                        env,
                        host,
                        virtual_view_id,
                    )
                })
                .map_or(std::ptr::null_mut(), JObject::into_raw)
            })
            .map_or(std::ptr::null_mut(), <*mut _>::cast)
        }
    }

    unsafe fn find_focus(
        &self,
        env: *mut jni::sys::JNIEnv,
        host: jni::sys::jobject,
        focus_type: jni::sys::jint,
    ) -> jni::sys::jobject {
        // SAFETY: the contract of `ViewAccessibility`, which this call keeps:
        // the closure ends with it.
        unsafe {
            with_env(env, host, |env, host| {
                self.with(|provided| {
                    provided
                        .adapter
                        .find_focus(&mut provided.activation, env, host, focus_type)
                })
                .map_or(std::ptr::null_mut(), JObject::into_raw)
            })
            .map_or(std::ptr::null_mut(), <*mut _>::cast)
        }
    }

    unsafe fn perform_action(
        &self,
        env: *mut jni::sys::JNIEnv,
        host: jni::sys::jobject,
        virtual_view_id: jni::sys::jint,
        action: jni::sys::jint,
        arguments: jni::sys::jobject,
    ) -> bool {
        // SAFETY: the contract of `ViewAccessibility`, which this call keeps:
        // the closure ends with it.
        unsafe {
            with_env(env, host, |env, host| {
                // `arguments` is the call's local reference to the bundle, or
                // null, under the same contract as `host`.
                let arguments = JObject::from_raw(arguments.cast());
                // The bundle is read before the adapter is taken.
                let Some(action) = PlatformAction::from_java(env, action, &arguments) else {
                    return false;
                };
                // Taken means answered `true`, with the events raised.
                self.raising(env, host, |provided| {
                    let events = provided.adapter.perform_action(
                        &mut provided.action,
                        virtual_view_id,
                        &action,
                    );
                    let taken = events.is_some();
                    (events, taken)
                })
                .unwrap_or(false)
            })
            .unwrap_or(false)
        }
    }

    fn services_changed(&self, enabled: bool) {
        if let Some(tree) = self.tree() {
            tree.services_changed(enabled);
        }
    }

    /// TalkBack explores by touch only once it asked for the tree; the hover
    /// is applied on the Java main thread, after what was posted before it.
    fn explore(self: Arc<Self>, view: &Arc<JavaView>, action: i32, x: f32, y: f32) -> bool {
        if !self.shared.is_active() {
            return false;
        }
        view.post(move |env, view| {
            // SAFETY: the posted call runs attached on the Java main thread
            // with this environment, and `view` is the installed view's
            // reference, held for the call, which the closure ends with.
            unsafe {
                with_env(env.get_raw(), view.as_raw(), |env, host| {
                    self.hover(env, host, action, x, y);
                });
            }
            Ok(())
        });
        true
    }
}

/// TalkBack for the window over one `FluiView`.
pub struct AndroidAccessibility {
    shared: Arc<BridgeShared>,
    provider: Arc<NodeProvider>,
    view: Arc<JavaView>,
    /// Set by `detach`: a later publish only retains.
    detached: AtomicBool,
}

/// The Activity's window answers TalkBack through its view's node provider,
/// which this fills as the window opens.
impl crate::host::Accessibility<ActivityRef> for AndroidAccessibility {
    fn for_window(window: &ActivityRef) -> Arc<dyn crate::host::WindowAccessibility> {
        let shared = Arc::new(BridgeShared::new());
        let provider = Arc::new(NodeProvider {
            shared: Arc::clone(&shared),
            tree: OnceLock::new(),
        });
        window
            .view
            .provide_accessibility(Arc::clone(&provider) as Arc<dyn ViewAccessibility>);
        Arc::new(Self {
            shared,
            provider,
            view: Arc::clone(&window.view),
            detached: AtomicBool::new(false),
        })
    }
}

impl crate::host::WindowAccessibility for AndroidAccessibility {
    /// The view stops answering through this bridge; a window opened over it
    /// since keeps its own. Idempotent.
    fn detach(&self) {
        if !self.detached.swap(true, Ordering::AcqRel) {
            let provider: Arc<dyn ViewAccessibility> = Arc::clone(&self.provider) as _;
            self.view.withdraw_accessibility(&provider);
        }
    }

    /// Android follows the window's focus itself.
    fn focus_changed(&self, _focused: bool) {}

    /// Android places the nodes from the view's own position.
    fn bounds_changed(&self, _: crate::host::ScreenBounds) {}
}

impl PlatformAccessibility for AndroidAccessibility {
    fn publish(&self, update: TreeUpdate) {
        // Retained when self-contained; one published from inside the
        // activation is its answer and goes no further (`BridgeShared::publish`).
        let Some(update) = self.shared.publish(update) else {
            return;
        };
        // An inactive adapter drops what it is handed, so nothing is posted;
        // the activation reads the retained update instead.
        if self.detached.load(Ordering::Acquire) || !self.shared.is_active() {
            return;
        }
        let provider = Arc::clone(&self.provider);
        self.view.post(move |env, view| {
            // SAFETY: the posted call runs attached on the Java main thread
            // with this environment, and `view` is the installed view's
            // reference, held for the call, which the closure ends with.
            unsafe {
                with_env(env.get_raw(), view.as_raw(), |env, host| {
                    provider.apply(env, host, update);
                });
            }
            Ok(())
        });
    }

    fn is_active(&self) -> bool {
        self.shared.is_active()
    }

    fn set_activation_listener(&self, listener: AccessibilityActivationListener) {
        self.shared.set_activation_listener(listener);
    }

    fn set_action_listener(&self, listener: AccessibilityActionListener) {
        self.shared.set_action_listener(listener);
    }

    fn assistive_focus_support(&self) -> flui_semantics::AssistiveFocusSupport {
        Self::ASSISTIVE_FOCUS
    }

    fn route_change_support(&self) -> RouteChangeSupport {
        Self::ROUTE_CHANGE
    }

    /// Android's notice of a new window state, with the route's name as its
    /// text, which TalkBack reads as the screen it moved to.
    fn route_changed(&self, change: RouteChange) {
        if self.detached.load(Ordering::Acquire) || !self.shared.is_active() {
            return;
        }
        let name = change.name().map(str::to_owned);
        self.view.post(move |env, view| {
            // SAFETY: the posted call runs attached on the Java main thread
            // with this environment, and `view` is the installed view's
            // reference, held for the call, which the closure ends with.
            unsafe {
                with_env(env.get_raw(), view.as_raw(), |env, host| {
                    // A failed call leaves the screen unannounced; the
                    // pending exception is cleared as the environment is
                    // given back.
                    let _ = send_window_state_changed(env, host, name.as_deref());
                });
            }
            Ok(())
        });
    }
}

impl AndroidAccessibility {
    /// The published `accesskit_android` tracks TalkBack's cursor but tells
    /// the application nothing of it
    /// ([docs/gaps.md, "Accessibility bridge fidelity"](https://github.com/vanyastaff/flui/blob/main/docs/gaps.md#accessibility-bridge-fidelity-properties-and-announcements)).
    pub(crate) const ASSISTIVE_FOCUS: flui_semantics::AssistiveFocusSupport =
        flui_semantics::AssistiveFocusSupport::Unreported;

    /// A route change is a window state change TalkBack reads; a run on a
    /// device has not checked it yet.
    pub(crate) const ROUTE_CHANGE: RouteChangeSupport = RouteChangeSupport::Native;
}

impl std::fmt::Debug for AndroidAccessibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AndroidAccessibility")
            .field("active", &self.shared.is_active())
            .finish_non_exhaustive()
    }
}
