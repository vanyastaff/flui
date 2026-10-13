//! [`TransformationController`] — the external handle for
//! [`InteractiveViewer`](super::InteractiveViewer)'s transform.

use std::{cell::Cell, fmt, rc::Rc};

use flui_foundation::geometry::Matrix4;
use flui_foundation::geometry::Offset;
use flui_foundation::{ChangeNotifier, Listenable, ListenerCallback, ListenerId};

/// The heap-allocated state shared by every clone of a
/// [`TransformationController`].
struct Inner {
    value: Cell<Matrix4>,
    notifier: ChangeNotifier,
}

impl Listenable for Inner {
    fn add_observer(&self, observer: flui_foundation::ListenerObserver) -> ListenerId {
        self.notifier.add_observer(observer)
    }

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
/// Every clone shares the transform and notification channel on the owning UI
/// thread through `Rc`. [`listenable`](Self::listenable) returns that same channel,
/// so subscribers can retain their subscription when given another clone.
///
/// The value defaults to [`Matrix4::identity`] — no transformation.
#[derive(Clone)]
pub struct TransformationController {
    inner: Rc<Inner>,
}

impl fmt::Debug for TransformationController {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = self.inner.value.get();
        f.debug_struct("TransformationController")
            .field("value", &value)
            .finish_non_exhaustive()
    }
}

impl Default for TransformationController {
    fn default() -> Self {
        Self::new()
    }
}

impl TransformationController {
    pub(crate) fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
    /// A controller starting at the identity matrix (no transformation).
    #[must_use]
    pub fn new() -> Self {
        Self::with_value(Matrix4::identity())
    }

    /// A controller starting at `value`.
    #[must_use]
    pub fn with_value(value: Matrix4) -> Self {
        Self {
            inner: Rc::new(Inner {
                value: Cell::new(value),
                notifier: ChangeNotifier::new(),
            }),
        }
    }

    /// The current transform.
    #[must_use]
    pub fn value(&self) -> Matrix4 {
        self.inner.value.get()
    }

    /// Replaces the transform and notifies listeners if it actually changed.
    ///
    /// No clamping is applied here.
    /// `InteractiveViewer`'s own gesture handling is what runs a candidate
    /// transform through `boundary_margin`/`min_scale`/`max_scale` before
    /// writing it through this setter; a direct `set_value` call (from a
    /// test, or an external animation) bypasses those clamps entirely.
    pub fn set_value(&self, value: Matrix4) {
        if self.inner.value.get().m == value.m {
            return;
        }
        self.inner.value.set(value);
        self.inner.notifier.notify_listeners();
    }

    /// Returns the scene point at the given viewport point.
    ///
    /// A viewport point is relative to the parent while a scene point is
    /// relative to the child, regardless of transformation. The viewport
    /// transforms as the inverse of the child (moving the child left is
    /// equivalent to moving the viewport right).
    ///
    /// Returns `viewport_point` unchanged if the current value is singular (should
    /// not happen for a matrix built solely from translation + uniform
    /// scale, but a non-invertible matrix has no meaningful scene point).
    #[must_use]
    pub fn to_scene(&self, viewport_point: Offset<f64>) -> Offset<f64> {
        let value = self.value();
        let Some(inverse) = value.try_inverse() else {
            return viewport_point;
        };
        let (x, y) = inverse.transform_point(viewport_point.dx, viewport_point.dy);
        Offset::new(x, y)
    }

    /// An `Rc<dyn Listenable>` sharing this controller's notification channel.
    ///
    /// Each call clones the existing `Rc`, retaining pointer identity without
    /// allocating a new channel. Replacing a handle with another clone therefore
    /// does not require subscribers to detach and register again.
    #[must_use]
    pub fn listenable(&self) -> Rc<dyn Listenable> {
        Rc::clone(&self.inner) as Rc<dyn Listenable>
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
