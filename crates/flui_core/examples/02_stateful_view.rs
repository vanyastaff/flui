//! Example 02: Stateful View with Hooks
//!
//! This example demonstrates using hooks for state management:
//! - use_signal for reactive state
//! - use_memo for derived/computed values
//! - use_effect for side effects
//!
//! Run with: `cargo run --example 02_stateful_view`

use flui_core::{BuildContext, Element};
use flui_core::view::{View, ChangeFlags};
use flui_core::hooks::{use_signal, use_memo, use_effect_simple};

mod mock_render;

use mock_render::{create_column_element, create_text_element, create_button_element};

// ============================================================================
// Counter - Simple Stateful View
// ============================================================================

/// A counter with increment/decrement buttons
///
/// Demonstrates:
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

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        println!("\nCounter::build() called with initial_value: {}", self.initial_value);

        // Create a signal for the counter value
        // Signals automatically trigger rebuilds when changed
        let count = use_signal(ctx, self.initial_value);

        println!("  Created signal with value: {}", count.get());

        // In a real implementation, we would:
        // 1. Create text showing count.get()
        // 2. Create increment button that calls count.update(|n| n + 1)
        // 3. Create decrement button that calls count.update(|n| n - 1)
        // 4. Compose them in a Column

        // For this example, we'll just show the structure
        println!("  Building UI:");
        println!("    - Text: 'Count: {}'", count.get());
        println!("    - Button: 'Increment' (would call count.update(|n| n + 1))");
        println!("    - Button: 'Decrement' (would call count.update(|n| n - 1))");

        let element = create_column_element(10.0, 3);
        (element, ())
    }
}

// ============================================================================
// ComputedView - View with Derived State
// ============================================================================

/// A view that shows a value and computed derivatives
///
/// Demonstrates:
/// - Using use_memo for expensive computations
/// - Memoization prevents recomputation
/// - Dependencies tracked automatically
#[derive(Debug, Clone)]
pub struct ComputedView {
    multiplier: i32,
}

impl ComputedView {
    pub fn new(multiplier: i32) -> Self {
        Self { multiplier }
    }
}

impl View for ComputedView {
    type Element = flui_core::element::ComponentElement;
    type State = ();

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        println!("\nComputedView::build() called with multiplier: {}", self.multiplier);

        // Base value
        let value = use_signal(ctx, 10);

        // Computed values - only recalculated when dependencies change
        let doubled = use_memo(ctx, |_hook_ctx| {
            let val = value.get();
            println!("  [MEMO] Computing doubled: {} * 2", val);
            val * 2
        });

        let multiplied = use_memo(ctx, |_hook_ctx| {
            let val = value.get();
            println!("  [MEMO] Computing multiplied: {} * {}", val, self.multiplier);
            val * self.multiplier
        });

        println!("  Current values:");
        println!("    value: {}", value.get());
        println!("    doubled: {}", doubled.get());
        println!("    multiplied: {}", multiplied.get());

        // Side effect - runs when value changes
        use_effect_simple(ctx, move || {
            println!("  [EFFECT] Value changed to: {}", value.get());
        });

        let element = create_column_element(10.0, 4);
        (element, ())
    }
}

// ============================================================================
// FormView - Complex State Management
// ============================================================================

/// A form with multiple fields and validation
///
/// Demonstrates:
/// - Multiple signals for different fields
/// - Computed validation state
/// - Effects for side effects
#[derive(Debug, Clone)]
pub struct FormView;

impl View for FormView {
    type Element = flui_core::element::ComponentElement;
    type State = ();

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        println!("\nFormView::build() called");

        // Multiple form fields
        let name = use_signal(ctx, String::from(""));
        let email = use_signal(ctx, String::from(""));
        let age = use_signal(ctx, 0);

        println!("  Created signals:");
        println!("    name: '{}'", name.get());
        println!("    email: '{}'", email.get());
        println!("    age: {}", age.get());

        // Computed validation
        let is_valid = use_memo(ctx, |_hook_ctx| {
            let name_val = name.get();
            let email_val = email.get();
            let age_val = age.get();

            let valid = !name_val.is_empty()
                && email_val.contains('@')
                && age_val >= 18;

            println!("  [MEMO] Computing is_valid:");
            println!("    name not empty: {}", !name_val.is_empty());
            println!("    email has @: {}", email_val.contains('@'));
            println!("    age >= 18: {}", age_val >= 18);
            println!("    => valid: {}", valid);

            valid
        });

        // Log when form becomes valid
        use_effect_simple(ctx, move || {
            if is_valid.get() {
                println!("  [EFFECT] Form is now VALID! ✓");
            } else {
                println!("  [EFFECT] Form is INVALID ✗");
            }
        });

        println!("  Building form UI...");

        let element = create_column_element(10.0, 4);
        (element, ())
    }
}

// ============================================================================
// Demo Application
// ============================================================================

fn main() {
    println!("=== Example 02: Stateful View with Hooks ===\n");

    println!("--- Demo 1: Simple Counter ---");
    let counter = Counter::new(0);
    println!("Created counter view: {:?}", counter);
    // In a real app: let (element, state) = counter.build(&mut ctx);

    println!("\n--- Demo 2: Computed Values ---");
    let computed = ComputedView::new(5);
    println!("Created computed view: {:?}", computed);
    // In a real app: let (element, state) = computed.build(&mut ctx);

    println!("\n--- Demo 3: Form with Validation ---");
    let form = FormView;
    println!("Created form view: {:?}", form);
    // In a real app: let (element, state) = form.build(&mut ctx);

    println!("\n=== Key Takeaways ===");
    println!("1. use_signal creates reactive state");
    println!("2. Signals automatically trigger rebuilds");
    println!("3. use_memo caches computed values");
    println!("4. Memos only recompute when dependencies change");
    println!("5. use_effect runs side effects");
    println!("6. Clone signals before moving into closures");
    println!("7. Signal clones are cheap (just Rc increment)");

    println!("\n=== Hook Rules (CRITICAL!) ===");
    println!("✅ DO:");
    println!("   - Call hooks at component top level");
    println!("   - Call hooks in same order every build");
    println!("   - Clone signals for closures");

    println!("\n❌ DON'T:");
    println!("   - Call hooks conditionally");
    println!("   - Call hooks in loops");
    println!("   - Call hooks in nested functions");

    println!("\n=== When to Use Hooks ===");
    println!("✅ Use hooks when:");
    println!("   - Need local component state");
    println!("   - Need reactive updates");
    println!("   - Need derived/computed values");
    println!("   - Need side effects");

    println!("\n❌ Don't use hooks when:");
    println!("   - View is purely presentational");
    println!("   - All data comes from props");
    println!("   - No user interaction needed");
}
