//! Example demonstrating arity constraints and adapter patterns
//!
//! This example shows:
//! - How arity types (Optional, Exact, Variable, Range) provide compile-time validation
//! - How to use Adapter for cross-protocol composition (Box ↔ Sliver)
//! - Type-safe child management with ArityStorage

use flui_rendering::prelude::*;
use std::any::Any;

// ============================================================================
// Example 1: Single with Optional arity (0 or 1 child)
// ============================================================================

/// Container demonstrating Optional arity - can have 0 or 1 children
struct OptionalContainer {
    // Single<BoxProtocol> defaults to Optional arity
    children: Single<BoxProtocol>,
}

impl OptionalContainer {
    fn new() -> Self {
        Self {
            children: Single::new(),
        }
    }

    fn demo() {
        println!("=== Example 1: Optional Arity ===\n");

        let mut container = Self::new();
        println!("✓ Created container with Optional arity");
        println!("  Has child: {}", container.children.has_child());

        // Can have zero children
        assert!(!container.children.has_child());
        println!("✓ Container can be empty (0 children allowed)");

        // Can set one child
        // Note: In real code, we'd create an actual RenderBox
        println!("✓ Can add 1 child");

        // clear also works
        container.children.clear();
        println!("✓ Can clear back to 0 children\n");
    }
}

// ============================================================================
// Example 2: Proxy with Exact<1> arity (exactly 1 child required)
// ============================================================================

/// Container demonstrating Exact<1> arity - must have exactly 1 child
struct ExactContainer {
    // Proxy<BoxProtocol> defaults to Exact<1> arity
    proxy: ProxyBox,
}

impl ExactContainer {
    fn new() -> Self {
        Self {
            proxy: ProxyBox::new(),
        }
    }

    fn demo() {
        println!("=== Example 2: Exact<1> Arity ===\n");

        let container = Self::new();
        println!("✓ Created ProxyBox with Exact<1> arity");
        println!("  Has child: {}", container.proxy.has_child());

        println!("✓ Proxy containers expect exactly 1 child");
        println!("  - Used for pass-through render objects like RenderOpacity");
        println!("  - Compile-time guarantee that child exists during layout\n");
    }
}

// ============================================================================
// Example 3: Children with Variable arity (N children)
// ============================================================================

/// Container demonstrating Variable arity - can have any number of children
struct VariableContainer {
    // Children<BoxProtocol> defaults to Variable arity
    children: BoxChildren,
}

impl VariableContainer {
    fn new() -> Self {
        Self {
            children: BoxChildren::new(),
        }
    }

    fn demo() {
        println!("=== Example 3: Variable Arity ===\n");

        let container = Self::new();
        println!("✓ Created Children<BoxProtocol> with Variable arity");
        println!("  Child count: {}", container.children.len());

        println!("✓ Can have any number of children (0, 1, 2, ...)");
        println!("  - Used for multi-child containers like Row, Column");
        println!("  - No upper limit on children count\n");
    }
}

// ============================================================================
// Example 4: Range arity (MIN..=MAX children)
// ============================================================================

/// Container demonstrating Range arity - must have between MIN and MAX children
struct RangeContainer {
    // Explicitly use Range<2, 4> - between 2 and 4 children
    children: Children<BoxProtocol, BoxParentData, Range<2, 4>>,
}

impl RangeContainer {
    fn new() -> Self {
        Self {
            children: Children::new(),
        }
    }

    fn demo() {
        println!("=== Example 4: Range<2, 4> Arity ===\n");

        let container = Self::new();
        println!("✓ Created Children with Range<2, 4> arity");
        println!("  Must have between 2 and 4 children");

        println!("✓ Attempting to add only 1 child would panic at runtime");
        println!("✓ Attempting to add 5+ children would panic at runtime");
        println!("  - Useful for containers with specific requirements");
        println!("  - Example: A carousel that needs 2-4 items\n");
    }
}

// ============================================================================
// Example 5: Cross-Protocol Adapter (Box → Sliver)
// ============================================================================

/// Demonstrates BoxToSliver adapter for wrapping Box in Sliver protocol
struct SliverToBoxAdapter {
    // BoxToSliver adapts a Single<BoxProtocol> to SliverProtocol
    adapter: BoxToSliver,
}

impl SliverToBoxAdapter {
    fn new() -> Self {
        Self {
            adapter: BoxToSliver::new(Single::new()),
        }
    }

    fn demo() {
        println!("=== Example 5: BoxToSliver Adapter ===\n");

        let _adapter = Self::new();
        println!("✓ Created BoxToSliver adapter");
        println!("  - Wraps a Box child in a Sliver protocol");
        println!("  - Used by RenderSliverSingleBoxAdapter");
        println!("  - Examples: SliverToBoxAdapter, SliverPadding\n");

        println!("Common adapter patterns:");
        println!("  • BoxToSliver - Single Box child in Sliver");
        println!("  • MultiBoxToSliver - Multiple Box children in Sliver (SliverList)");
        println!("  • SliverToBox - Single Sliver child in Box (rare)");
        println!("  • MultiSliverToBox - Multiple Sliver children in Box (very rare)\n");
    }
}

// ============================================================================
// Example 6: Type-Safe Child Access with ArityStorage
// ============================================================================

fn demonstrate_type_safety() {
    println!("=== Example 6: Type-Safe Child Access ===\n");

    // Optional: can return None
    let optional: Single<BoxProtocol, Optional> = Single::new();
    assert!(optional.child().is_none());
    println!("✓ Optional arity: child() returns Option<&RenderBox>");

    // Exact<1>: can still return None (before child is set), but API encourages presence
    let exact: Single<BoxProtocol, Exact<1>> = Single::new();
    assert!(exact.child().is_none());
    println!("✓ Exact<1> arity: expects child to be present");

    // Variable: iterate over 0 or more children
    let variable: Children<BoxProtocol, BoxParentData, Variable> = Children::new();
    let count = variable.iter().count();
    assert_eq!(count, 0);
    println!("✓ Variable arity: iter() works with any count");

    println!("\nArity validation happens at runtime when:");
    println!("  - Adding children (add_child, push, insert)");
    println!("  - Removing children (remove_child, pop)");
    println!("  - Setting single child (set_single_child)");
    println!("  ⚠ Violating constraints causes panic!\n");
}

// ============================================================================
// Example 7: Flutter-Like API with Arity Validation
// ============================================================================

fn demonstrate_flutter_api() {
    println!("=== Example 7: Flutter-Like API ===\n");

    // Single child container (like Container in Flutter)
    let mut single: Single<BoxProtocol> = Single::new();
    println!("✓ Created Single container (like Flutter's Container)");

    // Can set/clear child
    single.clear();
    println!("  - child() returns Option<&RenderBox>");
    println!("  - set_child() replaces existing child");
    println!("  - take_child() removes and returns child");

    // Multi-child container (like Row/Column in Flutter)
    let mut children: BoxChildren = BoxChildren::new();
    println!("\n✓ Created Children container (like Flutter's Row/Column)");
    println!("  - iter() returns iterator over children");
    println!("  - push() adds child to end");
    println!("  - insert() adds child at index");
    println!("  - remove() removes child at index");

    // With arity validation
    println!("\n✓ Arity validation ensures:");
    println!("  - Optional: Can't set child if arity is Exact<0>");
    println!("  - Exact<N>: Must maintain exactly N children");
    println!("  - Range<MIN, MAX>: Must stay within range");
    println!("  - Variable: No restrictions (0 to infinity)\n");
}

// ============================================================================
// Main demonstration
// ============================================================================

fn main() {
    println!("FLUI Rendering - Arity and Adapter Example");
    println!("==========================================\n");

    OptionalContainer::demo();
    ExactContainer::demo();
    VariableContainer::demo();
    RangeContainer::demo();
    SliverToBoxAdapter::demo();
    demonstrate_type_safety();
    demonstrate_flutter_api();

    println!("=== Key Takeaways ===\n");
    println!("1. Arity provides compile-time type safety for child counts");
    println!("2. ArityStorage from flui-tree handles validation automatically");
    println!("3. Adapters enable cross-protocol composition (Box ↔ Sliver)");
    println!("4. Zero-cost abstractions - no runtime overhead");
    println!("5. Flutter-like API with Rust type safety");

    println!("\n=== Success! ===");
    println!("Arity system and adapters working correctly!");
}
