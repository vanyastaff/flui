//! [`TransformationController`] — the external handle for
//! [`InteractiveViewer`](super::InteractiveViewer)'s transform.

use std::fmt;
use std::sync::Arc;

use flui_foundation::{ChangeNotifier, Listenable, ListenerCallback, ListenerId};
use flui_geometry::Matrix4;
use flui_types::{Offset, geometry::Pixels};
use parking_lot::Mutex;

/// The heap-allocated state shared by every clone of a
/// [`TransformationController`].
struct Inner {
    value: Mutex<Matrix4>,
    notifier: ChangeNotifier,
}

impl Listenable for Inner {
    fn add_listener(&self, listener: ListenerCallback) -> ListenerId {
        self.notifier.add_listener(listener)
    }

    fn remove_listener(&self, id: ListenerId) {
        self.notifier.remove_listener(id);
    }

    fn remove_all_listeners(&self) {
        self.notifier.remove_all_listeners();
    }
}

/// A shared, cheaply-cloneable handle to an `InteractiveViewer`'s
/// transformation matrix.
///
/// Flutter parity: `widgets/interactive_viewer.dart` `TransformationController`
/// — "a thin wrapper on `ValueNotifier` whose value is a `Matrix4`". FLUI
/// shapes it the way sibling controllers in this crate are shaped (see
/// [`ScrollController`](crate::ScrollController) /
/// `flui_rendering::view::ScrollPosition`): `Arc`-backed shared state plus a
/// [`ChangeNotifier`], so every clone observes the same transform and the
/// same notifications, and [`as_listenable`](Self::as_listenable) hands out
/// a stable `Arc<dyn Listenable>` (`Arc::ptr_eq`-stable across calls) for
/// `AnimatedBuilder`'s subscribe/swap machinery.
///
/// The value defaults to [`Matrix4::identity`] — no transformation.
#[derive(Clone)]
pub struct TransformationController {
    inner: Arc<Inner>,
}

impl fmt::Debug for TransformationController {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TransformationController")
            .field("value", &*self.inner.value.lock())
            .finish_non_exhaustive()
    }
}

impl Default for TransformationController {
    fn default() -> Self {
        Self::new()
    }
}

impl TransformationController {
    /// A controller starting at the identity matrix (no transformation).
    #[must_use]
    pub fn new() -> Self {
        Self::with_value(Matrix4::identity())
    }

    /// A controller starting at `value`.
    #[must_use]
    pub fn with_value(value: Matrix4) -> Self {
        Self {
            inner: Arc::new(Inner {
                value: Mutex::new(value),
                notifier: ChangeNotifier::new(),
            }),
        }
    }

    /// The current transform.
    #[must_use]
    pub fn value(&self) -> Matrix4 {
        *self.inner.value.lock()
    }

    /// Replaces the transform and notifies listeners if it actually changed.
    ///
    /// Flutter parity: `ValueNotifier.value=` — no clamping is applied here.
    /// `InteractiveViewer`'s own gesture handling is what runs a candidate
    /// transform through `boundaryMargin`/`minScale`/`maxScale` before
    /// writing it through this setter; a direct `set_value` call (from a
    /// test, or an external animation) bypasses those clamps entirely,
    /// exactly as it does in Flutter.
    pub fn set_value(&self, value: Matrix4) {
        let mut guard = self.inner.value.lock();
        if guard.m == value.m {
            return;
        }
        *guard = value;
        drop(guard);
        self.inner.notifier.notify_listeners();
    }

    /// Returns the scene point at the given viewport point.
    ///
    /// A viewport point is relative to the parent while a scene point is
    /// relative to the child, regardless of transformation. The viewport
    /// transforms as the inverse of the child (moving the child left is
    /// equivalent to moving the viewport right).
    ///
    /// Flutter parity: `TransformationController.toScene`. Returns
    /// `viewport_point` unchanged if the current value is singular (should
    /// not happen for a matrix built solely from translation + uniform
    /// scale, but a non-invertible matrix has no meaningful scene point).
    #[must_use]
    pub fn to_scene(&self, viewport_point: Offset<Pixels>) -> Offset<Pixels> {
        let value = self.value();
        let Some(inverse) = value.try_inverse() else {
            return viewport_point;
        };
        let (x, y) = inverse.transform_point(viewport_point.dx, viewport_point.dy);
        Offset::new(x, y)
    }

    /// An `Arc<dyn Listenable>` sharing this controller's notifier.
    ///
    /// The same `Arc<Inner>` is coerced on every call (not a fresh
    /// allocation), so `Arc::ptr_eq` across two calls on the same controller
    /// — or on two clones of it — is stable. This is what lets
    /// `AnimatedBuilder`/`ValueListenableBuilder`-style subscribers detect
    /// "same controller, just a different clone" instead of resubscribing on
    /// every rebuild.
    #[must_use]
    pub fn as_listenable(&self) -> Arc<dyn Listenable> {
        Arc::clone(&self.inner) as Arc<dyn Listenable>
    }

    /// The number of listeners currently registered.
    ///
    /// Disposal-testing hook, mirroring `ChangeNotifier::len` — a caller
    /// that mounts a widget against this controller and unmounts it can
    /// assert this returns to `0`, proving the widget's subscription was
    /// actually removed rather than left dangling into a torn-down subtree.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.notifier.len()
    }

    /// Whether any listeners are currently registered. See [`len`](Self::len).
    #[must_use]
    pub fn has_listeners(&self) -> bool {
        self.inner.notifier.has_listeners()
    }

    /// Whether no listeners are currently registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.notifier.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[test]
    fn new_starts_at_identity() {
        let controller = TransformationController::new();
        assert_eq!(controller.value().m, Matrix4::identity().m);
    }

    #[test]
    fn set_value_notifies_on_real_change() {
        let controller = TransformationController::new();
        let notified = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&notified);
        controller.as_listenable().add_listener(Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }));

        controller.set_value(Matrix4::translation(10.0, 0.0, 0.0));
        assert_eq!(notified.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn set_value_same_matrix_does_not_renotify() {
        let controller = TransformationController::new();
        controller.set_value(Matrix4::translation(5.0, 5.0, 0.0));

        let notified = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&notified);
        controller.as_listenable().add_listener(Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }));

        controller.set_value(Matrix4::translation(5.0, 5.0, 0.0));
        assert_eq!(
            notified.load(Ordering::SeqCst),
            0,
            "writing the identical matrix must not notify"
        );

        controller.set_value(Matrix4::translation(6.0, 5.0, 0.0));
        assert_eq!(notified.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn clones_share_state() {
        let controller = TransformationController::new();
        let clone = controller.clone();
        controller.set_value(Matrix4::translation(1.0, 2.0, 0.0));
        assert_eq!(clone.value().m, Matrix4::translation(1.0, 2.0, 0.0).m);
    }

    #[test]
    fn as_listenable_is_ptr_stable_across_calls() {
        // The same controller must hand out `Arc::ptr_eq`-equal listenables
        // on repeated calls — this is what lets `AnimatedBuilder` detect
        // "same controller" across rebuilds instead of resubscribing.
        let controller = TransformationController::new();
        let a = controller.as_listenable();
        let b = controller.as_listenable();
        assert!(Arc::ptr_eq(&a, &b));

        let other = TransformationController::new();
        let c = other.as_listenable();
        assert!(!Arc::ptr_eq(&a, &c));
    }

    #[test]
    fn to_scene_accounts_for_translation_and_scale() {
        let controller = TransformationController::new();
        // Scene content scaled 2x then shifted by (10, 20) in viewport space.
        controller
            .set_value(Matrix4::translation(10.0, 20.0, 0.0) * Matrix4::scaling(2.0, 2.0, 1.0));

        let scene = controller.to_scene(Offset::new(
            flui_geometry::px(10.0),
            flui_geometry::px(20.0),
        ));
        // Viewport (10, 20) is exactly the translation, so it maps back to
        // the scene origin.
        assert!((scene.dx.get() - 0.0).abs() < 1e-5);
        assert!((scene.dy.get() - 0.0).abs() < 1e-5);
    }

    #[test]
    fn to_scene_identity_is_passthrough() {
        let controller = TransformationController::new();
        let point = Offset::new(flui_geometry::px(42.0), flui_geometry::px(7.0));
        assert_eq!(controller.to_scene(point), point);
    }
}
