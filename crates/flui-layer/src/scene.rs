//! Scene - Composited layer tree ready for rendering
//!
//! A Scene represents a fully composed layer tree that can be rendered
//! to the screen. It's created by `SceneBuilder::build()` and consumed
//! by the rendering engine.
//!
//! # Architecture
//!
//! ```text
//! RenderObject.paint()
//!     │
//!     ▼
//! SceneBuilder (push/pop layers)
//!     │
//!     ▼
//! Scene (owns LayerTree, ready to render)
//!     │
//!     ▼
//! Engine.render(&scene)
//! ```
//!
//! # Example
//!
//! ```rust
//! use flui_layer::{CanvasLayer, Layer, LayerTree, Scene};
//! use flui_types::Size;
//!
//! // Create scene from a single layer
//! let scene = Scene::from_layer(
//!     Size::new(800.0, 600.0),
//!     Layer::from(CanvasLayer::new()),
//!     0,
//! );
//!
//! assert!(scene.has_content());
//! assert_eq!(scene.layer_count(), 1);
//! ```

use flui_foundation::LayerId;
use std::fmt;

use flui_types::{Pixels, Size};

use crate::{layer::Layer, link_registry::LinkRegistry, tree::LayerTree};

// ============================================================================
// COMPOSITION CALLBACK
// ============================================================================

/// A one-shot callback invoked when a [`Scene`] finalises compositing.
///
/// Stored on the owning [`Scene`] via [`Scene::add_composition_callback`] and
/// fired by [`Scene::fire_composition_callbacks`]. The callback is consumed on
/// fire; re-registration is the caller's responsibility.
///
/// The closure is `FnOnce() + Send + 'static` so it can carry owned state
/// across the build / fire boundary and survive a move to the render thread.
pub struct CompositionCallback(Box<dyn FnOnce() + Send + 'static>);

impl CompositionCallback {
    /// Wraps a closure in a callback. Prefer [`Scene::add_composition_callback`]
    /// for the canonical registration path.
    #[inline]
    pub fn new<F>(callback: F) -> Self
    where
        F: FnOnce() + Send + 'static,
    {
        Self(Box::new(callback))
    }

    /// Consumes the callback and invokes it.
    #[inline]
    pub fn fire(self) {
        (self.0)();
    }
}

impl fmt::Debug for CompositionCallback {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CompositionCallback")
            .finish_non_exhaustive()
    }
}

// ============================================================================
// SCENE
// ============================================================================

/// An opaque object representing a composited scene.
///
/// Scene owns the `LayerTree` and contains all the information
/// needed to render the composed layer tree to the screen.
///
/// # Lifecycle
///
/// 1. Create layers with `SceneBuilder`
/// 2. Call `build_scene()` to get a Scene (takes ownership of LayerTree)
/// 3. Pass Scene to the rendering engine
/// 4. Scene is consumed/disposed after rendering
///
/// # Thread Safety
///
/// Scene is `Send` - it can be transferred to the render thread.
#[derive(Debug)]
pub struct Scene {
    /// Viewport size
    size: Size<Pixels>,

    /// Layer tree containing all layers
    layer_tree: LayerTree,

    /// Root layer ID (if scene has content)
    root: Option<LayerId>,

    /// Leader-follower link registry for this scene
    link_registry: LinkRegistry,

    /// One-shot callbacks fired when this scene finalises compositing.
    composition_callbacks: Vec<CompositionCallback>,

    /// Frame number for debugging
    frame_number: u64,
}

impl Scene {
    /// Creates a new empty Scene.
    pub fn empty(size: Size<Pixels>) -> Self {
        Self {
            size,
            layer_tree: LayerTree::new(),
            root: None,
            link_registry: LinkRegistry::new(),
            composition_callbacks: Vec::new(),
            frame_number: 0,
        }
    }

    /// Creates a new Scene with LayerTree and root.
    pub fn new(
        size: Size<Pixels>,
        layer_tree: LayerTree,
        root: Option<LayerId>,
        frame_number: u64,
    ) -> Self {
        Self {
            size,
            layer_tree,
            root,
            link_registry: LinkRegistry::new(),
            composition_callbacks: Vec::new(),
            frame_number,
        }
    }

    /// Creates a Scene with full metadata.
    pub fn with_links(
        size: Size<Pixels>,
        layer_tree: LayerTree,
        root: Option<LayerId>,
        link_registry: LinkRegistry,
        frame_number: u64,
    ) -> Self {
        Self {
            size,
            layer_tree,
            root,
            link_registry,
            composition_callbacks: Vec::new(),
            frame_number,
        }
    }

    /// Creates a Scene from a single layer (convenience method).
    ///
    /// Creates a LayerTree with a single layer as root.
    pub fn from_layer(size: Size<Pixels>, layer: Layer, frame_number: u64) -> Self {
        let mut tree = LayerTree::new();
        let root_id = tree.insert(layer);
        Self {
            size,
            layer_tree: tree,
            root: Some(root_id),
            link_registry: LinkRegistry::new(),
            composition_callbacks: Vec::new(),
            frame_number,
        }
    }

    // ========================================================================
    // COMPOSITION CALLBACKS
    // ========================================================================

    /// Registers a one-shot callback to fire when this scene finalises
    /// compositing.
    ///
    /// Callbacks are consumed on fire (see [`fire_composition_callbacks`]).
    /// Re-registration is the caller's responsibility.
    ///
    /// [`fire_composition_callbacks`]: Self::fire_composition_callbacks
    pub fn add_composition_callback<F>(&mut self, callback: F)
    where
        F: FnOnce() + Send + 'static,
    {
        self.composition_callbacks
            .push(CompositionCallback::new(callback));
    }

    /// Returns the number of pending composition callbacks.
    #[inline]
    pub fn composition_callback_count(&self) -> usize {
        self.composition_callbacks.len()
    }

    /// Fires every registered composition callback exactly once, in
    /// registration order.
    ///
    /// The callback list is drained -- subsequent calls fire nothing until
    /// new callbacks are added.
    ///
    /// Each callback is wrapped in [`std::panic::catch_unwind`] to isolate
    /// programmer error in a single callback from the rest. A poisoned
    /// callback yields one [`LayerError::CallbackPoisoned`] entry in the
    /// returned vec; subsequent callbacks still fire. This mirrors the
    /// rendering crate's `Poisoned` shape introduced in Mythos Step 12 of
    /// the `flui-rendering` chain (commit `dc0fa1ad`).
    ///
    /// [`LayerError::CallbackPoisoned`]: crate::LayerError::CallbackPoisoned
    #[tracing::instrument(skip_all, name = "fire_composition_callbacks", fields(n = self.composition_callbacks.len()))]
    pub fn fire_composition_callbacks(&mut self) -> Vec<crate::LayerError> {
        use std::panic::{AssertUnwindSafe, catch_unwind};

        let mut errors = Vec::new();
        for callback in self.composition_callbacks.drain(..) {
            // `AssertUnwindSafe` here documents that callbacks are
            // self-contained `FnOnce() + Send` -- a panic in one callback
            // does not tear scene-level invariants (the callback list is
            // already drained by `drain(..)` before fire).
            if let Err(_payload) = catch_unwind(AssertUnwindSafe(|| callback.fire())) {
                errors.push(crate::LayerError::CallbackPoisoned {
                    panic_type: "composition_callback",
                });
            }
        }
        errors
    }

    /// Returns the viewport size.
    #[inline]
    pub fn size(&self) -> Size<Pixels> {
        self.size
    }

    /// Returns the layer tree.
    #[inline]
    pub fn layer_tree(&self) -> &LayerTree {
        &self.layer_tree
    }

    /// Returns the root layer ID of the scene.
    #[inline]
    pub fn root(&self) -> Option<LayerId> {
        self.root
    }

    /// Returns the root layer (if present).
    #[inline]
    pub fn root_layer(&self) -> Option<&Layer> {
        self.root.and_then(|id| self.layer_tree.get_layer(id))
    }

    /// Returns the link registry for leader-follower relationships.
    #[inline]
    pub fn link_registry(&self) -> &LinkRegistry {
        &self.link_registry
    }

    /// Returns the frame number.
    #[inline]
    pub fn frame_number(&self) -> u64 {
        self.frame_number
    }

    /// Returns true if the scene has content (has root layer).
    #[inline]
    pub fn has_content(&self) -> bool {
        self.root.is_some()
    }

    /// Returns true if the scene is empty (no root layer).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.root.is_none()
    }

    /// Returns the number of layers in the scene.
    #[inline]
    pub fn layer_count(&self) -> usize {
        self.layer_tree.len()
    }

    /// Returns a [`SceneBuilder`] that mutates this scene's layer tree.
    ///
    /// Convenience wrapper over `SceneBuilder::new(&mut scene.layer_tree)`.
    /// The builder borrows the scene's tree exclusively; root and metadata
    /// remain on the scene and are not touched by the builder.
    ///
    /// [`SceneBuilder`]: crate::compositor::SceneBuilder
    #[inline]
    pub fn builder(&mut self) -> crate::compositor::SceneBuilder<'_> {
        crate::compositor::SceneBuilder::new(&mut self.layer_tree)
    }
}

impl Default for Scene {
    fn default() -> Self {
        Self::empty(Size::ZERO)
    }
}

// `Scene: Send` is auto-derived from its fields (`LayerTree`, `LinkRegistry`,
// `Vec<CompositionCallback>` whose payload is `FnOnce() + Send + 'static`).
// No `unsafe impl` is needed -- Mythos Step 3 deletion.

// ============================================================================
// SCENE BUILDER INTEGRATION
// ============================================================================

impl crate::compositor::SceneBuilder<'_> {
    /// Finishes building and returns a Scene ready for rendering.
    ///
    /// **Note:** This method only captures the root LayerId. The LayerTree
    /// must be passed separately or use `build_scene_owned()`.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_layer::{CanvasLayer, LayerTree, Scene, SceneBuilder};
    /// use flui_types::Size;
    ///
    /// let mut tree = LayerTree::new();
    /// {
    ///     let mut builder = SceneBuilder::new(&mut tree);
    ///     let _ = builder.add_canvas(CanvasLayer::new());
    ///     // builder dropped here, tree still available
    /// }
    ///
    /// // Create scene with the tree
    /// let root_id = tree.root();
    /// let scene = Scene::new(Size::new(800.0, 600.0), tree, root_id, 0);
    /// assert!(scene.has_content());
    /// ```
    pub fn finish(self) -> Option<LayerId> {
        self.root()
    }

    /// Finishes building with frame number metadata.
    pub fn finish_with_frame(self, _frame_number: u64) -> Option<LayerId> {
        self.root()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use flui_types::{Offset, geometry::px};

    use super::*;
    use crate::CanvasLayer;

    #[test]
    fn test_scene_empty() {
        let scene = Scene::empty(Size::new(px(800.0), px(600.0)));
        assert!(scene.is_empty());
        assert!(!scene.has_content());
        assert!(scene.root().is_none());
        assert!(scene.root_layer().is_none());
        assert_eq!(scene.layer_count(), 0);
        assert_eq!(scene.size(), Size::new(px(800.0), px(600.0)));
    }

    #[test]
    fn test_scene_from_layer() {
        let scene = Scene::from_layer(
            Size::new(px(1920.0), px(1080.0)),
            Layer::from(CanvasLayer::new()),
            42,
        );

        assert!(!scene.is_empty());
        assert!(scene.has_content());
        assert!(scene.root().is_some());
        assert!(scene.root_layer().is_some());
        assert_eq!(scene.layer_count(), 1);
        assert_eq!(scene.frame_number(), 42);
        assert_eq!(scene.size(), Size::new(px(1920.0), px(1080.0)));
    }

    #[test]
    fn test_scene_new_with_tree() {
        let mut tree = LayerTree::new();
        let root_id = tree.insert(Layer::from(CanvasLayer::new()));

        let scene = Scene::new(Size::new(px(800.0), px(600.0)), tree, Some(root_id), 1);

        assert!(scene.has_content());
        assert_eq!(scene.root(), Some(root_id));
        assert_eq!(scene.layer_count(), 1);
    }

    #[test]
    fn test_scene_with_links() {
        let mut tree = LayerTree::new();
        let root_id = tree.insert(Layer::from(CanvasLayer::new()));

        let mut registry = LinkRegistry::new();
        let link = crate::LayerLink::new();
        registry.register_leader(link, root_id, Offset::ZERO, Size::new(px(100.0), px(50.0)));

        let scene = Scene::with_links(
            Size::new(px(800.0), px(600.0)),
            tree,
            Some(root_id),
            registry,
            123,
        );

        assert_eq!(scene.link_registry().leader_count(), 1);
        assert_eq!(scene.frame_number(), 123);
    }

    #[test]
    fn test_scene_default() {
        let scene = Scene::default();
        assert!(scene.is_empty());
        assert_eq!(scene.size(), Size::ZERO);
    }

    #[test]
    fn test_scene_send() {
        fn assert_send<T: Send>() {}
        assert_send::<Scene>();
    }

    #[test]
    fn test_scene_drop_consumes() {
        // Mythos Step 8: `Scene::dispose(self)` (which just called
        // `drop(self)`) was deleted; `drop(scene)` is the idiomatic
        // replacement.
        let scene = Scene::from_layer(
            Size::new(px(800.0), px(600.0)),
            Layer::from(CanvasLayer::new()),
            0,
        );
        drop(scene);
        // Scene is consumed by drop; no further use possible.
    }

    #[test]
    fn test_scene_builder_method() {
        let mut scene = Scene::empty(Size::new(px(100.0), px(100.0)));
        {
            let mut builder = scene.builder();
            let _ = builder.add_canvas(CanvasLayer::new());
        }
        // Builder dropped; scene retains the tree but does not auto-set root.
        assert_eq!(scene.layer_count(), 1);
    }

    #[test]
    fn test_scene_builder_finish() {
        let mut tree = LayerTree::new();
        let root_id = {
            let mut builder = crate::SceneBuilder::new(&mut tree);
            let _ = builder.add_canvas(CanvasLayer::new());
            builder.finish()
        };

        assert!(root_id.is_some());

        // Now create scene with the tree
        let scene = Scene::new(Size::new(px(800.0), px(600.0)), tree, root_id, 0);
        assert!(scene.has_content());
    }

    // ========================================================================
    // COMPOSITION CALLBACK TESTS (U2)
    // ========================================================================

    #[test]
    fn test_composition_callback_register_and_fire() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };

        let mut scene = Scene::empty(Size::new(px(100.0), px(100.0)));
        assert_eq!(scene.composition_callback_count(), 0);

        let counter = Arc::new(AtomicUsize::new(0));
        let c1 = Arc::clone(&counter);
        scene.add_composition_callback(move || {
            c1.fetch_add(1, Ordering::SeqCst);
        });
        let c2 = Arc::clone(&counter);
        scene.add_composition_callback(move || {
            c2.fetch_add(10, Ordering::SeqCst);
        });

        assert_eq!(scene.composition_callback_count(), 2);

        scene.fire_composition_callbacks();
        assert_eq!(counter.load(Ordering::SeqCst), 11);
        assert_eq!(scene.composition_callback_count(), 0);

        // Re-firing is a no-op (callbacks consumed).
        scene.fire_composition_callbacks();
        assert_eq!(counter.load(Ordering::SeqCst), 11);
    }

    #[test]
    fn test_composition_callback_poison_isolation() {
        // Mythos Step 9: a panicking callback must be caught and reported as
        // `LayerError::CallbackPoisoned` without preventing subsequent
        // callbacks from firing. This mirrors the rendering crate's
        // `Poisoned` shape (commit dc0fa1ad).
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };

        let mut scene = Scene::empty(Size::ZERO);
        let counter = Arc::new(AtomicUsize::new(0));

        let c1 = Arc::clone(&counter);
        scene.add_composition_callback(move || {
            c1.fetch_add(1, Ordering::SeqCst);
        });
        scene.add_composition_callback(|| {
            panic!("intentional poison in callback 2");
        });
        let c3 = Arc::clone(&counter);
        scene.add_composition_callback(move || {
            c3.fetch_add(100, Ordering::SeqCst);
        });

        let errors = scene.fire_composition_callbacks();
        // Two non-panicking callbacks each ran exactly once.
        assert_eq!(counter.load(Ordering::SeqCst), 101);
        // Exactly one poisoned callback was reported.
        assert_eq!(errors.len(), 1);
        assert!(matches!(
            errors[0],
            crate::LayerError::CallbackPoisoned { .. }
        ));
        // Callback list is drained even on poison.
        assert_eq!(scene.composition_callback_count(), 0);
    }

    #[test]
    fn test_scene_builder_pop_underflow() {
        // Mythos Step 9: SceneBuilder::pop returns Result instead of
        // panicking on empty stack. `try_pop` stays as the panic-free
        // probe form.
        let mut tree = LayerTree::new();
        let mut builder = crate::SceneBuilder::new(&mut tree);

        let err = builder.pop().unwrap_err();
        assert!(matches!(err, crate::LayerError::BuilderStackUnderflow));

        // try_pop is the panic-free probe; returns Option.
        assert!(builder.try_pop().is_none());
    }

    #[test]
    fn test_composition_callback_empty_fire_is_noop() {
        let mut scene = Scene::empty(Size::ZERO);
        scene.fire_composition_callbacks();
        assert_eq!(scene.composition_callback_count(), 0);
    }
}
