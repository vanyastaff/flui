//! Window management and application entry point
//!
//! This module provides window configuration and the main entry point
//! for running Flui applications.

use crate::app::FluiApp;
use flui_core::view::AnyView;

/// Run a Flui application
///
/// This is the main entry point for running a Flui app. It creates a window
/// using eframe and runs the app's event loop.
///
/// # Parameters
///
/// - `root_view`: The root view of your application (type-erased via `Box<dyn AnyView>`)
///
/// # Example
///
/// ```rust,ignore
/// use flui_app::*;
/// use flui_core::view::View;
/// use flui_core::element::ComponentElement;
///
/// #[derive(Debug, Clone)]
/// struct MyApp;
///
/// impl View for MyApp {
///     type State = ();
///     type Element = ComponentElement;
///
///     fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
///         // Build your UI here
///         todo!()
///     }
/// }
///
/// fn main() {
///     run_app(Box::new(MyApp)).unwrap();
/// }
/// ```
pub fn run_app(root_view: Box<dyn AnyView>) -> Result<(), eframe::Error> {
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
            let app = FluiApp::new(root_view);
            Ok(Box::new(app))
        }),
    )
}
