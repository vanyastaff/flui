//! Base binding trait
//!
//! BindingBase provides the foundation for all FLUI bindings.
//! Bindings are the bridge between platform events and framework components.

/// Base trait for all bindings
///
/// All bindings (Gesture, Scheduler, Renderer, Widgets) implement this trait
/// to provide a unified initialization interface.
pub trait BindingBase: Send + Sync {
    /// Initialize the binding
    ///
    /// Called once during WidgetsFlutterBinding::ensure_initialized()
    /// to set up the binding's internal state and connections.
    fn init(&mut self);
}
