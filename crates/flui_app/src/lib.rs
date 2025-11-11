//! Flui Application Framework
//!
//! This crate provides the application framework for Flui, including:
//! - `FluiApp`: Main application structure
//! - `run_app()`: Entry point to run a Flui application
//! - eframe integration for window management and rendering
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_app::*;
//! use flui_core::view::View;
//! use flui_core::element::ComponentElement;
//!
//! #[derive(Debug, Clone)]
//! struct MyApp;
//!
//! impl View for MyApp {
//!     type State = ();
//!     type Element = ComponentElement;
//!
//!     fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
//!         // Build your UI here
//!         todo!()
//!     }
//! }
//!
//! fn main() {
//!     run_app(Box::new(MyApp)).unwrap();
//! }
//! ```

pub mod app;
pub mod event_callbacks;
#[cfg(target_arch = "wasm32")]
pub mod wasm;
pub mod window;
pub mod window_state;



// Re-exports
pub use app::FluiApp;
pub use event_callbacks::WindowEventCallbacks;
pub use window::run_app;
pub use window_state::WindowStateTracker;

// Re-export commonly used types from flui_core
pub use flui_core::{
    // Element system
    element::{ComponentElement, Element, ProviderElement, RenderElement},

    // Foundation types
    foundation::{ElementId, Key, Slot},

    // View system (new API)
    view::{AnyView, BuildContext, View, ViewElement},
};

