//! TalkBack for Android, via `accesskit_android`'s low-level [`Adapter`]
//! (gap 2 of `docs/gaps.md`).
//!
//! `FluiView` is FLUI's own view (ADR-0247), so the accessibility delegate it
//! installs (ADR-0255 decision 3) hands the framework a node provider that
//! forwards `createAccessibilityNodeInfo`, `findFocus` and `performAction` to
//! the adapter itself: AccessKit's injecting adapter and its dex are not
//! used. The view reaches the adapter through the accessibility slot of its
//! [`JavaView`], which this bridge fills as the window opens.
//!
//! # Threads
//!
//! The adapter lives on the Java main thread. The node provider's calls
//! arrive there and are answered at once from the tree the adapter holds. A
//! publish from the owner thread retains a self-contained update and posts
//! the update to the main thread, where it is applied and its events raised.
//! The posts run in order, so the adapter applies the updates in the order
//! the owner published them, and neither thread waits for the other.
//!
//! The adapter's lock is taken only on the main thread and never across an
//! event raise: TalkBack answers an event by asking for node info, which
//! re-enters the provider. A call that finds the lock held anyway answers as
//! a view with no tree.
//!
//! # TalkBack's cursor and explore by touch
//!
//! The adapter keeps where TalkBack's cursor is; FLUI reads it through the
//! accessor its vendored copy adds (`third_party/accesskit_android`) after
//! each action, hover and update it hands the adapter, and tells the
//! composition root when it moved, after the events are raised. Explore by
//! touch reaches the adapter from the native input queue, which NativeActivity
//! takes before any view: the loop offers a finger hovering over the
//! touchscreen to [`JavaView::explore_by_touch`], and the hover is applied on
//! the main thread like a publish (ADR-0255 decision 3).
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

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use accesskit::{NodeId, TreeId, TreeUpdate};
use accesskit_android::jni::JNIEnv;
use accesskit_android::jni::objects::JObject;
use accesskit_android::jni::sys as jni021;
use accesskit_android::{Adapter, PlatformAction, QueuedEvents};
use parking_lot::Mutex;

use crate::accessibility::bridge::{Action, Activation, BridgeShared};
use crate::owner::android::ActivityRef;
use crate::owner::android::java_view::{JavaView, ViewAccessibility};
use flui_semantics::platform::{
    AccessibilityActionListener, AccessibilityActivationListener, AssistiveFocusListener,
    PlatformAccessibility,
};

/// The adapter with the handlers it calls.
struct Provided {
    adapter: Adapter,
    activation: Activation,
    action: Action,
    /// Where TalkBack's cursor was when the composition root was last told.
    told: Option<(NodeId, TreeId)>,
}

/// The part of the bridge the Java main thread holds: the adapter, which the
/// view's node provider and the owner's posted updates reach.
struct NodeProvider {
    shared: Arc<BridgeShared>,
    provided: Mutex<Provided>,
}

impl NodeProvider {
    /// Run `f` with the adapter, or answer `None` when a call on this thread
    /// already holds it.
    fn with<R>(&self, f: impl FnOnce(&mut Provided) -> R) -> Option<R> {
        let Some(mut provided) = self.provided.try_lock() else {
            tracing::warn!(
                target: "flui_native::android",
                "TalkBack re-entered the node provider while it held the adapter"
            );
            return None;
        };
        Some(f(&mut provided))
    }

    /// Run `f`, which may move TalkBack's cursor, with the adapter; then,
    /// outside its lock, raise the events `f` made and tell the composition
    /// root where the cursor went, in that order. Java main thread.
    ///
    /// The cursor counts as told only once the listener returned: after a
    /// panic in it, the next call that finds the cursor where it is now tells
    /// the root again.
    fn moving<R>(
        &self,
        env: &mut JNIEnv<'_>,
        host: &JObject<'_>,
        f: impl FnOnce(&mut Provided) -> (Option<QueuedEvents>, R),
    ) -> Option<R> {
        let (events, answer, moved) = self.with(|provided| {
            let (events, answer) = f(provided);
            let now = provided.adapter.accessibility_focus();
            (events, answer, (now != provided.told).then_some(now))
        })?;
        raise(env, host, events);
        if let Some(node) = moved {
            self.shared.notify_assistive_focus(node);
            self.with(|provided| provided.told = node);
        }
        Some(answer)
    }

    /// Apply `update`: an update that removes the node under the cursor moves
    /// the cursor off the tree. Java main thread.
    fn apply(&self, env: &mut JNIEnv<'_>, host: &JObject<'_>, update: TreeUpdate) {
        self.moving(env, host, |provided| {
            (provided.adapter.update_if_active(move || update), ())
        });
    }

    /// Hand the adapter one touch-exploration hover. Java main thread.
    fn hover(&self, env: &mut JNIEnv<'_>, host: &JObject<'_>, action: i32, x: f32, y: f32) {
        self.moving(env, host, |provided| {
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

/// Clears a Java exception the adapter left pending, also when it panicked:
/// the adapter's JNI calls `unwrap`, and the provider's caller must see the
/// answer, not a throw.
struct ClearPending(*mut jni021::JNIEnv);

impl Drop for ClearPending {
    fn drop(&mut self) {
        // SAFETY: the pointer is the environment of the native call this
        // guard lives in, on its thread, as `with_env` below took it.
        let Ok(env) = (unsafe { JNIEnv::from_raw(self.0) }) else {
            return;
        };
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_describe();
            let _ = env.exception_clear();
        }
    }
}

/// Run `f` with the 0.21 environment and view behind the raw pointers of a
/// JNI call (see the module doc), clearing what it leaves pending.
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
    // jni 0.21 and 0.22 bind the same JNI types, each through its own jni-sys.
    let env = env.cast::<jni021::JNIEnv>();
    let _clear = ClearPending(env);
    // SAFETY: the caller's contract; the environment is borrowed, not
    // attached or detached.
    let mut env = unsafe { JNIEnv::from_raw(env) }.ok()?;
    // SAFETY: the caller's contract; a `JObject` deletes no reference when
    // dropped.
    let host = unsafe { JObject::from_raw(host.cast()) };
    Some(f(&mut env, &host))
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
                self.moving(env, host, |provided| {
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
            provided: Mutex::new(Provided {
                adapter: Adapter::default(),
                activation: Activation(Arc::clone(&shared)),
                action: Action(Arc::clone(&shared)),
                told: None,
            }),
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
}

impl PlatformAccessibility for AndroidAccessibility {
    fn publish(&self, update: TreeUpdate) {
        // Only self-contained updates may answer a late activation; see
        // `BridgeShared`.
        self.shared.retain_if_self_contained(&update);
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

    fn set_assistive_focus_listener(&self, listener: AssistiveFocusListener) {
        self.shared.set_assistive_focus_listener(listener);
    }
}

impl AndroidAccessibility {
    /// TalkBack's cursor comes through the vendored adapter's accessor, read
    /// after each action, hover and update; a run on a device has not checked
    /// it yet
    /// ([gap 36](https://github.com/vanyastaff/flui/blob/main/docs/gaps.md#36-accessibility-bridge-fidelity-properties-and-announcements)).
    pub(crate) const ASSISTIVE_FOCUS: flui_semantics::AssistiveFocusSupport =
        flui_semantics::AssistiveFocusSupport::Reported;
}

impl std::fmt::Debug for AndroidAccessibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AndroidAccessibility")
            .field("active", &self.shared.is_active())
            .finish_non_exhaustive()
    }
}
