//! Diagnostics Demo
//!
//! This example demonstrates the diagnostics system for debugging UI trees.

use flui_core::foundation::{DiagnosticsNode, DiagnosticsProperty, DiagnosticLevel, DiagnosticsTreeStyle};
use flui_core::debug::diagnostics::{format_tree_structure, format_element_info, format_diagnostics_tree, element_to_diagnostics_node};

fn main() {
    println!("=== Diagnostics Demo ===\n");

    // Part 1: Foundation Diagnostics (Flutter-like)
    println!("1. Foundation Diagnostics System:");
    println!("===================================\n");

    demo_foundation_diagnostics();

    // Part 2: Element Tree Printing
    println!("\n2. Element Tree Printing:");
    println!("==========================\n");

    demo_element_tree_printing();

    println!("\n=== Demo Complete ===");
}

fn demo_foundation_diagnostics() {
    // Create a diagnostics tree for a UI widget
    let container = DiagnosticsNode::new("Container")
        .property("width", 800)
        .property("height", 600)
        .property("padding", 16)
        .with_level(DiagnosticLevel::Info)
        .with_style(DiagnosticsTreeStyle::Sparse)
        .child(
            DiagnosticsNode::new("Row")
                .property("mainAxisAlignment", "center")
                .property("crossAxisAlignment", "start")
                .property("spacing", 8)
                .child(
                    DiagnosticsNode::new("Text")
                        .property("content", "Hello, World!")
                        .property("fontSize", 18)
                        .property("color", "#000000")
                )
                .child(
                    DiagnosticsNode::new("Button")
                        .property("label", "Click Me")
                        .property("enabled", true)
                        .property("onPressed", "<closure>")
                )
        )
        .child(
            DiagnosticsNode::new("Column")
                .property("mainAxisAlignment", "start")
                .child(
                    DiagnosticsNode::new("Image")
                        .property("src", "logo.png")
                        .property("width", 200)
                        .property("height", 200)
                )
                .child(
                    DiagnosticsNode::new("Text")
                        .property("content", "Caption text")
                        .property("fontSize", 12)
                        .property("color", "#666666")
                )
        );

    println!("Widget Tree:");
    println!("{}", container);

    // Demonstrate different diagnostic levels
    println!("\nDiagnostic Levels:");
    for level in [
        DiagnosticLevel::Hidden,
        DiagnosticLevel::Fine,
        DiagnosticLevel::Debug,
        DiagnosticLevel::Info,
        DiagnosticLevel::Warning,
        DiagnosticLevel::Hint,
        DiagnosticLevel::Error,
    ] {
        println!(
            "  {}: visible={}, error={}, warning={}",
            level,
            level.is_visible(),
            level.is_error(),
            level.is_warning()
        );
    }

    // Demonstrate properties with defaults
    println!("\nProperties with Defaults:");
    let prop1 = DiagnosticsProperty::new("width", 100).with_default("100");
    let prop2 = DiagnosticsProperty::new("height", 200).with_default("100");

    println!("  Property 1 (same as default): hidden={}", prop1.is_hidden());
    println!("  Property 2 (different from default): hidden={}", prop2.is_hidden());

    // Demonstrate tree styles
    println!("\nTree Styles:");
    for style in [
        DiagnosticsTreeStyle::Sparse,
        DiagnosticsTreeStyle::Shallow,
        DiagnosticsTreeStyle::Dense,
        DiagnosticsTreeStyle::SingleLine,
        DiagnosticsTreeStyle::ErrorProperty,
    ] {
        println!("  {}: compact={}", style, style.is_compact());
    }
}

fn demo_element_tree_printing() {
    // Simulate an element tree structure:
    //
    // 0: Container
    //   ├─ 1: Row
    //   │  ├─ 2: Text
    //   │  └─ 3: Button
    //   └─ 4: Column
    //      ├─ 5: Image
    //      └─ 6: Text

    let get_children = |id: usize| -> Vec<usize> {
        match id {
            0 => vec![1, 4],
            1 => vec![2, 3],
            4 => vec![5, 6],
            _ => vec![],
        }
    };

    let get_type_name = |id: usize| -> String {
        match id {
            0 => "Container",
            1 => "Row",
            2 => "Text",
            3 => "Button",
            4 => "Column",
            5 => "Image",
            6 => "Text",
            _ => "Unknown",
        }
        .to_string()
    };

    // Method 1: Simple tree structure
    println!("Simple Tree Structure:");
    let tree = format_tree_structure(0, get_children, get_type_name);
    println!("{}", tree);

    // Method 2: Element info
    println!("Element Info:");
    let info = format_element_info(
        0,
        "Container",
        vec![
            ("width", "800".to_string()),
            ("height", "600".to_string()),
            ("dirty", "false".to_string()),
        ],
    );
    println!("  {}", info);

    // Method 3: Diagnostics tree with properties
    println!("Diagnostics Tree:");

    let get_diagnostics = |id: usize| -> DiagnosticsNode {
        let type_name = get_type_name(id);

        match id {
            0 => element_to_diagnostics_node(
                id,
                type_name,
                vec![
                    DiagnosticsProperty::new("width", 800),
                    DiagnosticsProperty::new("height", 600),
                ],
            ),
            1 => element_to_diagnostics_node(
                id,
                type_name,
                vec![DiagnosticsProperty::new("spacing", 8)],
            ),
            2 => element_to_diagnostics_node(
                id,
                type_name,
                vec![DiagnosticsProperty::new("content", "Hello!")],
            ),
            3 => element_to_diagnostics_node(
                id,
                type_name,
                vec![DiagnosticsProperty::new("label", "Click Me")],
            ),
            4 => element_to_diagnostics_node(
                id,
                type_name,
                vec![DiagnosticsProperty::new("alignment", "start")],
            ),
            5 => element_to_diagnostics_node(
                id,
                type_name,
                vec![
                    DiagnosticsProperty::new("src", "logo.png"),
                    DiagnosticsProperty::new("width", 200),
                ],
            ),
            6 => element_to_diagnostics_node(
                id,
                type_name,
                vec![DiagnosticsProperty::new("content", "Caption")],
            ),
            _ => DiagnosticsNode::new("Unknown"),
        }
    };

    let diagnostics_output = format_diagnostics_tree(0, get_children, get_diagnostics);
    println!("{}", diagnostics_output);
}
