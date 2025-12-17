//! Basic ID usage example
//!
//! This example demonstrates how to use the various ID types in FLUI Foundation
//! for identifying elements in the UI tree.

use flui_foundation::{ElementId, Key, LayerId, RenderId, SemanticsId, ViewId};
use std::collections::HashMap;

fn main() {
    println!("=== FLUI Foundation: Basic ID Example ===\n");

    // -------------------------------------------------------------------------
    // Element IDs - Unique identifiers for tree nodes
    // -------------------------------------------------------------------------
    println!("1. Element IDs");
    println!("   -----------");

    let elem1 = ElementId::new(1);
    let elem2 = ElementId::new(2);
    let elem3 = ElementId::new(3);

    println!("   Created elements: {elem1}, {elem2}, {elem3}");
    println!("   elem1 < elem2: {}", elem1 < elem2);
    println!("   elem1 + 10 = {}", elem1 + 10);

    // IDs can be used as HashMap keys
    let mut element_names: HashMap<ElementId, &str> = HashMap::new();
    element_names.insert(elem1, "Root");
    element_names.insert(elem2, "Header");
    element_names.insert(elem3, "Content");

    println!("   Element names: {:?}", element_names);
    println!();

    // -------------------------------------------------------------------------
    // All ID Types - For the 5-tree architecture
    // -------------------------------------------------------------------------
    println!("2. All ID Types (5-Tree Architecture)");
    println!("   -----------------------------------");

    let view_id = ViewId::new(1);
    let element_id = ElementId::new(1);
    let render_id = RenderId::new(1);
    let layer_id = LayerId::new(1);
    let semantics_id = SemanticsId::new(1);

    println!("   ViewId:      {view_id}");
    println!("   ElementId:   {element_id}");
    println!("   RenderId:    {render_id}");
    println!("   LayerId:     {layer_id}");
    println!("   SemanticsId: {semantics_id}");
    println!();

    // -------------------------------------------------------------------------
    // Memory Efficiency - Niche optimization
    // -------------------------------------------------------------------------
    println!("3. Memory Efficiency");
    println!("   ------------------");

    println!(
        "   Size of ElementId:          {} bytes",
        std::mem::size_of::<ElementId>()
    );
    println!(
        "   Size of Option<ElementId>:  {} bytes (same due to niche optimization!)",
        std::mem::size_of::<Option<ElementId>>()
    );
    println!();

    // -------------------------------------------------------------------------
    // Keys - Widget identity for efficient reconciliation
    // -------------------------------------------------------------------------
    println!("4. Keys for Widget Identity");
    println!("   -------------------------");

    let key1 = Key::new();
    let key2 = Key::new();
    let key3 = Key::from_str("header");
    let key4 = Key::from_str("header"); // Same as key3

    println!("   key1 (auto): {key1}");
    println!("   key2 (auto): {key2}");
    println!("   key3 (str):  {key3}");
    println!("   key4 (str):  {key4}");
    println!("   key1 == key2: {}", key1 == key2);
    println!("   key3 == key4: {}", key3 == key4); // true - same string
    println!();

    println!("=== Example Complete ===");
}
