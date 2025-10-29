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
//! use flui_widgets::*;
//!
//! #[derive(Debug, Clone)]
//! struct MyApp;
//!
//! impl StatelessWidget for MyApp {
//!     fn build(&self, _context: &BuildContext) -> Box<dyn Widget> {
//!         Box::new(Text::new("Hello, World!"))
//!     }
//! }
//!
//! fn main() {
//!     run_app(Box::new(MyApp));
//! }
//! ```

pub mod app;
pub mod window;

// Re-exports
pub use app::FluiApp;
pub use window::run_app;

// Re-export commonly used types from flui_core
pub use flui_core::{
    BuildContext, Element, ElementTree, InheritedElement, InheritedWidget, ProxyWidget, State,
    StatefulElement, StatefulWidget, StatelessWidget, Widget,
};
