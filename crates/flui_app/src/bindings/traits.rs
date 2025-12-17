//! Binding traits - base interfaces for the binding system.
//!
//! This module defines the trait hierarchy for FLUI's binding system,
//! following Flutter's mixin-based architecture adapted to Rust traits.
//!
//! # Flutter Equivalence
//!
//! - `BindingBase` → [`Binding`] trait
//! - Mixin pattern → Trait composition
//!
//! # Architecture
//!
//! ```text
//! Binding (base trait)
//!   ├── initInstances() equivalent
//!   └── Basic lifecycle
//!
//! RendererBindingBehavior
//!   ├── Pipeline owner management
//!   ├── RenderView management
//!   └── drawFrame()
//!
//! WidgetsBindingBehavior (from flui-view)
//!   ├── BuildOwner management
//!   └── Element tree operations
//!
//! GestureBindingBehavior (from flui-interaction)
//!   └── Hit testing dispatch
//! ```

use std::any::TypeId;

/// Base trait for all bindings.
///
/// This is the Rust equivalent of Flutter's `BindingBase` class.
/// All binding mixins in Flutter (RendererBinding, WidgetsBinding, etc.)
/// require the base class, so all our binding traits extend this.
///
/// # Responsibilities
///
/// - Lifecycle management (initialization)
/// - Singleton pattern support
/// - Service extension registration (debug features)
///
/// # Flutter Equivalence
///
/// From `flutter/lib/src/foundation/binding.dart`:
///
/// ```dart
/// abstract class BindingBase {
///   BindingBase() {
///     initInstances();
///     initServiceExtensions();
///   }
///
///   void initInstances();
///   void initServiceExtensions();
/// }
/// ```
pub trait Binding: Send + Sync {
    /// Initialize the binding instances.
    ///
    /// Called during binding construction. Subclasses should:
    /// 1. Call `super.init_instances()` (in Rust: manually call parent)
    /// 2. Set their singleton instance
    /// 3. Initialize internal state
    fn init_instances(&mut self);

    /// Initialize service extensions (debug mode features).
    ///
    /// Service extensions provide debugging tools via the VM service.
    /// In release mode, this typically does nothing.
    fn init_service_extensions(&mut self) {
        // Default: no service extensions
    }

    /// Returns the binding's type ID for debugging.
    fn binding_type(&self) -> TypeId
    where
        Self: 'static,
    {
        TypeId::of::<Self>()
    }

    /// Whether the binding has been fully initialized.
    fn is_initialized(&self) -> bool;

    /// Perform reassembly (hot reload support).
    ///
    /// This is called when code changes during development.
    /// Bindings should invalidate caches and re-register callbacks.
    fn perform_reassemble(&mut self) {
        // Default: no-op
    }
}

/// Trait for bindings that manage rendering.
///
/// This corresponds to Flutter's `RendererBinding` mixin behavior.
/// It defines the interface for managing the render tree and pipeline.
///
/// # Flutter Equivalence
///
/// From `flutter/lib/src/rendering/binding.dart`:
///
/// ```dart
/// mixin RendererBinding on BindingBase, SchedulerBinding, GestureBinding {
///   PipelineOwner get rootPipelineOwner;
///   Iterable<RenderView> get renderViews;
///   void drawFrame();
///   void addRenderView(RenderView view);
///   void removeRenderView(RenderView view);
/// }
/// ```
pub trait RendererBindingBehavior: Binding {
    /// Type of the pipeline owner used.
    type PipelineOwner;

    /// Type of render views managed.
    type RenderView;

    /// Returns the root pipeline owner.
    ///
    /// The root pipeline owner is the top of the pipeline owner tree.
    /// Child pipeline owners can be added to manage separate render trees.
    fn root_pipeline_owner(&self) -> &Self::PipelineOwner;

    /// Returns mutable access to the root pipeline owner.
    fn root_pipeline_owner_mut(&mut self) -> &mut Self::PipelineOwner;

    /// Returns an iterator over all render views.
    fn render_views(&self) -> impl Iterator<Item = &Self::RenderView>;

    /// Adds a render view to the binding.
    ///
    /// The binding will:
    /// - Set and update the view's configuration
    /// - Call `composite_frame()` when producing frames
    /// - Forward pointer events for hit testing
    fn add_render_view(&mut self, view: Self::RenderView);

    /// Removes a render view from the binding.
    fn remove_render_view(&mut self, view_id: u64);

    /// Pump the rendering pipeline to generate a frame.
    ///
    /// This executes the phases in order:
    /// 1. Layout - compute sizes and positions
    /// 2. Compositing bits - determine layer requirements
    /// 3. Paint - generate display lists
    /// 4. Semantics - update accessibility tree
    ///
    /// Call this after the build phase (from WidgetsBinding).
    fn draw_frame(&mut self);

    /// Handle metrics change (window resize, etc.).
    fn handle_metrics_changed(&mut self) {}

    /// Handle text scale factor change.
    fn handle_text_scale_factor_changed(&mut self) {}

    /// Handle platform brightness change.
    fn handle_platform_brightness_changed(&mut self) {}
}

/// Trait for bindings that manage widgets/elements.
///
/// This corresponds to Flutter's `WidgetsBinding` mixin behavior.
/// FLUI already has `WidgetsBinding` in flui-view, so this trait
/// defines the interface for integration.
///
/// # Flutter Equivalence
///
/// From `flutter/lib/src/widgets/binding.dart`:
///
/// ```dart
/// mixin WidgetsBinding on BindingBase, SchedulerBinding, GestureBinding, RendererBinding {
///   BuildOwner get buildOwner;
///   void attachRootWidget(Widget rootWidget);
///   void drawFrame(); // Calls buildScope() then super.drawFrame()
/// }
/// ```
pub trait WidgetsBindingBehavior: RendererBindingBehavior {
    /// Type of the build owner used.
    type BuildOwner;

    /// Type of elements in the tree.
    type Element;

    /// Returns the build owner.
    fn build_owner(&self) -> &Self::BuildOwner;

    /// Returns mutable access to the build owner.
    fn build_owner_mut(&mut self) -> &mut Self::BuildOwner;

    /// Attach a root widget.
    fn attach_root_widget<V>(&mut self, view: &V)
    where
        V: crate::View;

    /// Execute the build phase.
    ///
    /// This runs `build_owner.build_scope()` to rebuild dirty elements.
    fn build_scope(&mut self);

    /// Whether there are pending builds.
    fn has_pending_builds(&self) -> bool;
}

/// Trait for bindings that manage gestures.
///
/// This corresponds to Flutter's `GestureBinding` mixin behavior.
/// FLUI already has `GestureBinding` in flui-interaction, so this trait
/// defines the interface for integration.
///
/// # Flutter Equivalence
///
/// From `flutter/lib/src/gestures/binding.dart`:
///
/// ```dart
/// mixin GestureBinding on BindingBase implements HitTestable {
///   void dispatchEvent(PointerEvent event, HitTestResult? result);
///   void hitTest(HitTestResult result, Offset position);
/// }
/// ```
pub trait GestureBindingBehavior: Binding {
    /// Type of hit test results.
    type HitTestResult;

    /// Type of pointer events.
    type PointerEvent;

    /// Dispatch a pointer event after hit testing.
    fn dispatch_event(&self, event: &Self::PointerEvent, result: Option<&Self::HitTestResult>);

    /// Perform hit testing at a position.
    fn hit_test(&self, result: &mut Self::HitTestResult, position: (f32, f32));
}

/// Trait for bindings that manage scheduling.
///
/// This corresponds to Flutter's `SchedulerBinding` mixin behavior.
/// FLUI has a `Scheduler` in flui-scheduler.
///
/// # Flutter Equivalence
///
/// From `flutter/lib/src/scheduler/binding.dart`:
///
/// ```dart
/// mixin SchedulerBinding on BindingBase {
///   void scheduleFrame();
///   void handleDrawFrame();
///   void addPersistentFrameCallback(callback);
/// }
/// ```
pub trait SchedulerBindingBehavior: Binding {
    /// Schedule a new frame.
    fn schedule_frame(&mut self);

    /// Called when it's time to draw a frame.
    fn handle_draw_frame(&mut self);

    /// Whether a frame is currently scheduled.
    fn frame_scheduled(&self) -> bool;
}
