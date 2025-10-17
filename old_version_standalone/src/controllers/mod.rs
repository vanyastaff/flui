//! UI Controllers for managing widget state and behavior
//!
//! Controllers are mutable state managers that persist across frames.
//! They are fundamentally different from Widgets:
//!
//! # Widget vs Controller
//!
//! | Aspect | Widget | Controller |
//! |--------|--------|------------|
//! | Ownership | `self` (move) | `&mut self` (borrow) |
//! | Lifetime | One frame | Multiple frames |
//! | Pattern | Declarative UI | Imperative state |
//! | bon? | ✅ Yes | ❌ No |
//! | Trait method | `fn ui(self, ...)` | `fn update(&mut self, ...)` |
//! | Example | Container, Row | AnimationController |
//!
//! # Example
//! ```ignore
//! // Controller lives in App state
//! struct MyApp {
//!     animation: AnimationController,
//! }
//!
//! impl eframe::App for MyApp {
//!     fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
//!         // Update controller
//!         self.animation.update(ctx);
//!
//!         // Use controller value in widgets
//!         let opacity = self.animation.value();
//!         Container::builder()
//!             .color(Color::from_rgba(255, 0, 0, (opacity * 255.0) as u8))
//!             .ui(ui);
//!     }
//! }
//! ```

pub mod animation;
pub mod change_tracker;
pub mod focus;
pub mod input;
pub mod theme_controller;
pub mod validation;
pub mod visibility;

pub use animation::{AnimationController, AnimationCurve, AnimationState};
pub use change_tracker::ChangeTracker;
pub use focus::FocusController;
pub use input::InputController;
pub use theme_controller::{ThemeController, ThemeMode, ThemeTransition, ThemeBuilder};
pub use validation::ValidationController;
pub use visibility::VisibilityController;

/// Controller trait - for mutable state that persists across frames.
///
/// Controllers manage state and are updated each frame but NOT consumed.
/// This is fundamentally different from Widgets which are consumed during rendering.
///
/// # Comparison with Widget
///
/// ```text
/// Widget:      fn ui(self, ui: &mut Ui) -> Response    // move semantics
/// Controller:  fn update(&mut self, ctx: &Context)      // borrow semantics
/// ```
///
/// # Example
/// ```ignore
/// pub struct MyController {
///     value: f32,
/// }
///
/// impl Controller for MyController {
///     fn update(&mut self, ctx: &egui::Context) {
///         self.value += 0.01;
///         if self.value > 1.0 {
///             self.value = 0.0;
///         }
///         if self.is_active() {
///             ctx.request_repaint();
///         }
///     }
///
///     fn reset(&mut self) {
///         self.value = 0.0;
///     }
///
///     fn is_active(&self) -> bool {
///         self.value < 1.0  // Active until reaches 1.0
///     }
/// }
/// ```
pub trait Controller {
    /// Update controller state.
    ///
    /// Called every frame to update internal state.
    /// Use `ctx.request_repaint()` if you need continuous updates.
    ///
    /// # Example
    /// ```ignore
    /// fn update(&mut self, ctx: &egui::Context) {
    ///     if self.is_animating() {
    ///         self.tick();
    ///         ctx.request_repaint();  // Request next frame
    ///     }
    /// }
    /// ```
    fn update(&mut self, ctx: &egui::Context);

    /// Reset controller to initial state.
    ///
    /// # Example
    /// ```ignore
    /// fn reset(&mut self) {
    ///     self.value = 0.0;
    ///     self.state = State::Idle;
    /// }
    /// ```
    fn reset(&mut self);

    /// Get controller's debug name for diagnostics.
    ///
    /// Returns the type name by default.
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Check if controller is active/animating.
    ///
    /// Used to determine if continuous updates are needed.
    /// Default implementation returns false (no continuous updates).
    ///
    /// # Example
    /// ```ignore
    /// fn is_active(&self) -> bool {
    ///     self.is_animating()
    /// }
    /// ```
    fn is_active(&self) -> bool {
        false
    }
}