//! ElementId Demo
//!
//! This example demonstrates the new ElementId type with NonZeroUsize,
//! showing niche optimization and type safety benefits.
//!
//! Run with: cargo run --example element_id_demo

use flui_core::ElementId;
use std::mem::size_of;

fn main() {
    println!("=== ElementId Demo ===\n");

    // Example 1: Size comparison (niche optimization)
    println!("1. Niche Optimization:");
    println!("   ElementId size:          {} bytes", size_of::<ElementId>());
    println!("   Option<ElementId> size:  {} bytes", size_of::<Option<ElementId>>());
    println!("   usize size:              {} bytes", size_of::<usize>());
    println!("   Option<usize> size:      {} bytes", size_of::<Option<usize>>());
    println!();
    println!("   ✓ Option<ElementId> has NO overhead!");
    println!("   ✗ Option<usize> adds 8 bytes overhead\n");

    // Example 2: Creating ElementIds
    println!("2. Creating ElementIds:");
    let id1 = ElementId::new(1);
    let id2 = ElementId::new(42);
    let id3 = ElementId::new(1000);
    println!("   id1 = {:?} (value: {})", id1, id1.get());
    println!("   id2 = {:?} (value: {})", id2, id2.get());
    println!("   id3 = {:?} (value: {})", id3, id3.get());
    println!();

    // Example 3: Type safety - cannot create zero
    println!("3. Type Safety:");
    println!("   ✓ Can create: ElementId::new(1)");
    println!("   ✗ Cannot create: ElementId::new(0) -> panics!");
    println!("   Use new_checked() for safe creation:");
    let maybe_zero = ElementId::new_checked(0);
    let maybe_one = ElementId::new_checked(1);
    println!("      new_checked(0) = {:?}", maybe_zero);
    println!("      new_checked(1) = {:?}", maybe_one);
    println!();

    // Example 4: Using Option<ElementId> (no sentinel needed!)
    println!("4. Option<ElementId> Pattern:");

    struct TreeNode {
        id: ElementId,
        parent: Option<ElementId>,
        left_child: Option<ElementId>,
        right_child: Option<ElementId>,
    }

    let root = TreeNode {
        id: ElementId::new(1),
        parent: None,  // Root has no parent
        left_child: Some(ElementId::new(2)),
        right_child: Some(ElementId::new(3)),
    };

    println!("   Root node:");
    println!("      id: {}", root.id);
    println!("      parent: {:?}", root.parent);
    println!("      left_child: {:?}", root.left_child);
    println!("      right_child: {:?}", root.right_child);

    // Pattern matching
    match root.parent {
        Some(parent_id) => println!("      Has parent: {}", parent_id),
        None => println!("      ✓ No parent (root node)"),
    }
    println!();

    // Example 5: Comparisons
    println!("5. Comparisons:");
    let a = ElementId::new(10);
    let b = ElementId::new(20);
    let c = ElementId::new(10);

    println!("   a = {:?}", a);
    println!("   b = {:?}", b);
    println!("   c = {:?}", c);
    println!("   a == c: {}", a == c);
    println!("   a == b: {}", a == b);
    println!("   a < b:  {}", a < b);
    println!();

    // Example 6: Arithmetic operations (for bitmap indexing)
    println!("6. Arithmetic Operations:");
    let base = ElementId::new(100);
    let offset = 5;

    println!("   base = {}", base);
    println!("   offset = {}", offset);
    println!("   base + offset = {:?}", base + offset);
    println!("   base - offset = {}", base - offset);
    println!("   -> Used for bitmap indexing in dirty tracking\n");

    // Example 7: Memory layout
    println!("7. Memory Layout:");

    #[repr(C)]
    struct OldComponentElement {
        _other_fields: [u8; 48],
        child: usize,  // Old: sentinel value for "none"
    }

    #[repr(C)]
    struct NewComponentElement {
        _other_fields: [u8; 48],
        child: Option<ElementId>,  // New: proper Option
    }

    println!("   Old struct size: {} bytes", size_of::<OldComponentElement>());
    println!("   New struct size: {} bytes", size_of::<NewComponentElement>());
    println!("   ✓ Same size, better semantics!\n");

    // Example 8: Conversion
    println!("8. Conversions:");
    let id = ElementId::new(999);
    let as_usize: usize = id.into();
    println!("   ElementId(999) -> usize: {}", as_usize);

    let back = ElementId::new(as_usize);
    println!("   usize(999) -> ElementId: {:?}", back);
    println!();

    // Example 9: Display formatting
    println!("9. Display Formatting:");
    let id = ElementId::new(12345);
    println!("   Debug:   {:?}", id);
    println!("   Display: {}", id);
    println!();

    // Example 10: Hash and collections
    println!("10. Collections:");
    use std::collections::HashSet;

    let mut seen = HashSet::new();
    seen.insert(ElementId::new(1));
    seen.insert(ElementId::new(2));
    seen.insert(ElementId::new(1)); // Duplicate

    println!("    Inserted: [1, 2, 1]");
    println!("    Set contains: {:?}", seen.len());
    println!("    ✓ Properly implements Hash and Eq\n");

    println!("=== Demo Complete ===");
    println!("\nKey Takeaways:");
    println!("- Option<ElementId> has zero memory overhead (niche optimization)");
    println!("- Type-safe: cannot create ElementId(0)");
    println!("- No sentinel values needed (use Option instead)");
    println!("- Supports all standard operations (Eq, Ord, Hash, Display)");
    println!("- Arithmetic for bitmap indexing (Add, Sub)");
    println!("- Better API: Option<ElementId> vs checking for sentinel");
}
