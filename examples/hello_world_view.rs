//! Hello World - Working Example with NEW View Architecture
//!
//! This demonstrates a minimal working app with the new View trait.
//! Since flui_widgets is not yet migrated, we create a simple custom view.
//!
//! Run with: cargo run --example hello_world_view --features flui_app

#[cfg(feature = "flui_app")]
use flui_app::run_app;

#[cfg(feature = "flui_app")]
use flui_core::view::View;
#[cfg(feature = "flui_app")]
use flui_core::element::ComponentElement;
#[cfg(feature = "flui_app")]
use flui_core::BuildContext;

#[cfg(feature = "flui_app")]
/// Simple Hello World app using NEW View trait
#[derive(Debug, Clone)]
struct HelloWorldApp;

#[cfg(feature = "flui_app")]
impl View for HelloWorldApp {
    type Element = ComponentElement;
    type State = ();

    fn build(self, _ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        // For now, create a minimal ComponentElement
        // In a real app, this would build a UI tree
        println!("HelloWorldApp::build() called");

        // TODO: Create actual UI once widgets are migrated
        // For now, just create an empty ComponentElement
        todo!("Implement UI building once flui_widgets is migrated to View API")
    }
}

fn main() -> Result<(), eframe::Error> {
    println!("=== Flui Hello World - NEW View Architecture ===");
    println!();

    #[cfg(not(feature = "flui_app"))]
    {
        println!("❌ ERROR: This example requires the 'flui_app' feature!");
        println!();
        println!("Run with:");
        println!("  cargo run --example hello_world_view --features flui_app");
        println!();
        return Ok(());
    }

    #[cfg(feature = "flui_app")]
    {
        println!("⚠️  IMPORTANT: This example requires flui_widgets to be migrated.");
        println!("   Current status:");
        println!("   ✅ flui_app - Migrated to View API");
        println!("   ❌ flui_widgets - Still using old Widget API (110+ errors)");
        println!();
        println!("Next steps:");
        println!("1. Migrate flui_widgets to View trait");
        println!("2. Update this example to use migrated widgets");
        println!();

        // This will fail at runtime because build() calls todo!()
        // Uncomment when widgets are ready:
        // run_app(Box::new(HelloWorldApp))
    }

    Ok(())
}
