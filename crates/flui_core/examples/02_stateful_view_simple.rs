//! Example 02: Stateful View with Hooks (Simplified)
//!
//! This example demonstrates the CONCEPTS of using hooks for state management.
//! The actual hook calls are shown but the examples focus on explaining patterns.
//!
//! Run with: `cargo run --example 02_stateful_view_simple`

use flui_core::{BuildContext, Element};
use flui_core::view::{View, ChangeFlags};

mod mock_render;

use mock_render::create_column_element;

// ============================================================================
// Counter - Simple Stateful View (Conceptual)
// ============================================================================

/// A counter with increment/decrement buttons
///
/// Demonstrates the CONCEPT of:
/// - Using use_signal for local state
/// - Cloning signals for closures
/// - Automatic rebuilds when state changes
#[derive(Debug, Clone)]
pub struct Counter {
    initial_value: i32,
}

impl Counter {
    pub fn new(initial_value: i32) -> Self {
        Self { initial_value }
    }
}

impl View for Counter {
    type Element = flui_core::element::ComponentElement;
    type State = ();

    fn build(self, _ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        println!("\n🔨 Counter::build() called");
        println!("   Initial value: {}", self.initial_value);

        // ============================================================
        // CONCEPTUAL HOOK USAGE (actual implementation differs):
        // ============================================================
        // let count = use_signal(ctx, self.initial_value);
        //
        // In real usage with proper framework:
        // 1. Signal tracks value and dependencies
        // 2. Calling count.set() triggers rebuild
        // 3. Widgets using count.get() re-render
        //
        // Example button would do:
        // Button::new("Increment", move || {
        //     count.update(|n| *n += 1);  // Triggers rebuild!
        // })
        // ============================================================

        println!("   ✓ Would create signal for reactive state");
        println!("   ✓ Would build UI with Text and Buttons");
        println!("   ✓ Buttons would update signal on click");

        let element = create_column_element(10.0, 3);
        (element, ())
    }
}

// ============================================================================
// Demo Application
// ============================================================================

fn main() {
    println!("=== Example 02: Stateful View with Hooks (Simplified) ===\n");

    println!("📚 This example explains HOOK CONCEPTS and PATTERNS");
    println!("   (Actual hook usage requires full framework context)\n");

    println!("--- Demo 1: Counter Pattern ---");
    let counter = Counter::new(0);
    println!("Created counter: {:?}", counter);

    // Simulate what would happen in real framework:
    println!("\n📖 How it works in real framework:");
    println!("   1. use_signal(ctx, 0) creates reactive state");
    println!("   2. count.get(hook_ctx) reads current value");
    println!("   3. count.set(5) updates value AND triggers rebuild");
    println!("   4. All widgets using count automatically re-render");

    println!("\n--- Hook Patterns Explained ---");

    println!("\n1️⃣  use_signal - Reactive State");
    println!("   Purpose: Create mutable state that triggers rebuilds");
    println!("   Usage:");
    println!("   ```rust");
    println!("   let count = use_signal(ctx, 0);");
    println!("   // In framework context:");
    println!("   // let val = count.get(hook_ctx);  // Read");
    println!("   // count.set(42);                   // Write + rebuild");
    println!("   ```");

    println!("\n2️⃣  use_memo - Computed Values");
    println!("   Purpose: Cache expensive computations");
    println!("   Usage:");
    println!("   ```rust");
    println!("   let doubled = use_memo(ctx, |hook_ctx| {{");
    println!("       count.get(hook_ctx) * 2");
    println!("   }});");
    println!("   // Only recomputes when count changes!");
    println!("   ```");

    println!("\n3️⃣  use_effect - Side Effects");
    println!("   Purpose: Run code when dependencies change");
    println!("   Usage:");
    println!("   ```rust");
    println!("   use_effect_simple(ctx, move || {{");
    println!("       println!(\"Count: {{}}\", count.get_untracked());");
    println!("   }});");
    println!("   ```");

    println!("\n--- The Three Rules of Hooks ---");
    println!("\n✅ Rule 1: Call hooks at TOP LEVEL");
    println!("   ✓ fn build(ctx) {{");
    println!("   ✓     let state1 = use_signal(ctx, 0);");
    println!("   ✓     let state2 = use_signal(ctx, \"\");");
    println!("   ✓ }}");

    println!("\n❌ Rule 1 Violation: Don't call conditionally");
    println!("   ✗ if condition {{");
    println!("   ✗     let state = use_signal(ctx, 0);  // WRONG!");
    println!("   ✗ }}");

    println!("\n✅ Rule 2: Call hooks in SAME ORDER");
    println!("   Hooks rely on call order to maintain state");
    println!("   Always call the same hooks every build");

    println!("\n✅ Rule 3: Make VALUES conditional, not HOOK CALLS");
    println!("   ✓ let is_visible = use_signal(ctx, true);");
    println!("   ✓ let content = if is_visible.get_untracked() {{");
    println!("   ✓     \"Visible\"");
    println!("   ✓ }} else {{");
    println!("   ✓     \"Hidden\"");
    println!("   ✓ }};");

    println!("\n--- Signal Cloning Pattern ---");
    println!("\n🔑 Signals are cheap to clone (just Rc increment)");
    println!("   ```rust");
    println!("   let count = use_signal(ctx, 0);");
    println!("   ");
    println!("   // Clone for closures");
    println!("   let count_inc = count.clone();");
    println!("   let count_dec = count.clone();");
    println!("   ");
    println!("   Button::new(\"Increment\", move || {{");
    println!("       count_inc.update(|n| *n += 1);");
    println!("   }})");
    println!("   ");
    println!("   // Original still available");
    println!("   Text::new(format!(\"Count: {{}}\", count.get(hook_ctx)))");
    println!("   ```");

    println!("\n--- Multiple Related Signals ---");
    println!("\n📝 Form with validation example:");
    println!("   ```rust");
    println!("   let email = use_signal(ctx, String::new());");
    println!("   let password = use_signal(ctx, String::new());");
    println!("   ");
    println!("   let is_valid = use_memo(ctx, |hook_ctx| {{");
    println!("       let e = email.get(hook_ctx);");
    println!("       let p = password.get(hook_ctx);");
    println!("       e.contains('@') && p.len() >= 8");
    println!("   }});");
    println!("   ```");

    println!("\n=== Key Takeaways ===");
    println!("\n✅ Hooks provide reactive state management");
    println!("✅ use_signal for mutable state");
    println!("✅ use_memo for derived/computed values");
    println!("✅ use_effect for side effects");
    println!("✅ Clone signals before moving into closures");
    println!("✅ Follow the 3 Rules of Hooks religiously!");

    println!("\n⚠️  Common Mistakes:");
    println!("   ❌ Calling hooks conditionally");
    println!("   ❌ Calling hooks in loops");
    println!("   ❌ Not cloning signals for closures");
    println!("   ❌ Forgetting that signal clones share state");

    println!("\n📖 For complete documentation, see:");
    println!("   crates/flui_core/docs/HOOKS_GUIDE.md");
    println!("   crates/flui_core/docs/VIEW_GUIDE.md");
}
