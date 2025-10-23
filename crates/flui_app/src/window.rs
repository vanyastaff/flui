//! Window management and application entry point
//!
//! This module provides window configuration and the main entry point
//! for running Flui applications.

use flui_core::DynWidget;
use crate::app::FluiApp;

/// Run a Flui application
///
/// This is the main entry point for running a Flui app. It creates a window
/// using eframe and runs the app's event loop.
///
/// # Parameters
///
/// - `root_widget`: The root widget of your application
///
/// # Example
///
/// ```rust,ignore
/// use flui_app::*;
///
/// #[derive(Debug, Clone)]
/// struct MyApp;
///
/// impl StatelessWidget for MyApp {
///     fn build(&self, _context: &BuildContext) -> Box<dyn Widget> {
///         Box::new(Text::new("Hello, World!"))
///     }
/// }
///
/// fn main() {
///     run_app(Box::new(MyApp)).unwrap();
/// }
/// ```
pub fn run_app(root_widget: Box<dyn DynWidget>) -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0])
            .with_title("Flui App"),
        ..Default::default()
    };

    eframe::run_native(
        "Flui App",
        options,
        Box::new(|_cc| {
            let app = FluiApp::new(root_widget);
            Ok(Box::new(app))
        }),
    )
}
