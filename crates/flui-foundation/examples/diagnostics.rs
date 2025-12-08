//! Diagnostics example
//!
//! This example demonstrates how to use the diagnostics system
//! for debugging and introspection of UI components.

use flui_foundation::{
    DiagnosticLevel, Diagnosticable, DiagnosticsBuilder, DiagnosticsNode, DiagnosticsProperty,
    DiagnosticsTreeStyle,
};

fn main() {
    println!("=== FLUI Foundation: Diagnostics Example ===\n");

    // -------------------------------------------------------------------------
    // Basic DiagnosticsNode
    // -------------------------------------------------------------------------
    println!("1. Basic DiagnosticsNode");
    println!("   ----------------------");

    let node = DiagnosticsNode::new("Container")
        .property("width", 800)
        .property("height", 600)
        .property("color", "#FFFFFF");

    println!("{node}");

    // -------------------------------------------------------------------------
    // Nested Diagnostic Tree
    // -------------------------------------------------------------------------
    println!("2. Nested Diagnostic Tree");
    println!("   ------------------------");

    let tree = DiagnosticsNode::new("MaterialApp")
        .property("title", "My App")
        .property("theme", "light")
        .child(
            DiagnosticsNode::new("Scaffold")
                .property("hasAppBar", true)
                .property("hasFloatingActionButton", false)
                .child(
                    DiagnosticsNode::new("Column")
                        .property("mainAxisAlignment", "center")
                        .property("crossAxisAlignment", "stretch")
                        .child(
                            DiagnosticsNode::new("Text")
                                .property("data", "Hello, World!")
                                .property("fontSize", 24)
                                .property("fontWeight", "bold"),
                        )
                        .child(DiagnosticsNode::new("SizedBox").property("height", 16))
                        .child(
                            DiagnosticsNode::new("ElevatedButton")
                                .property("onPressed", "<closure>")
                                .child(DiagnosticsNode::new("Text").property("data", "Click Me")),
                        ),
                ),
        );

    println!("{tree}");

    // -------------------------------------------------------------------------
    // DiagnosticsProperty with levels
    // -------------------------------------------------------------------------
    println!("3. DiagnosticsProperty with Levels");
    println!("   ---------------------------------");

    let props = vec![
        DiagnosticsProperty::new("width", 100).with_level(DiagnosticLevel::Info),
        DiagnosticsProperty::new("DEPRECATED_field", "value").with_level(DiagnosticLevel::Warning),
        DiagnosticsProperty::new("error_count", 5).with_level(DiagnosticLevel::Error),
        DiagnosticsProperty::new("internal_id", "abc123").with_level(DiagnosticLevel::Debug),
    ];

    for prop in &props {
        println!("   [{:?}] {}", prop.level(), prop);
    }
    println!();

    // -------------------------------------------------------------------------
    // DiagnosticsBuilder
    // -------------------------------------------------------------------------
    println!("4. DiagnosticsBuilder");
    println!("   --------------------");

    let mut builder = DiagnosticsBuilder::new();
    builder
        .add("id", 42)
        .add("name", "MyWidget")
        .add_flag("visible", true, "VISIBLE")
        .add_flag("disabled", false, "DISABLED") // Won't be added
        .add_optional("tooltip", Some("Help text"))
        .add_optional::<String>("icon", None) // Won't be added
        .add_with_level("debug_hash", "0xDEADBEEF", DiagnosticLevel::Debug);

    let properties = builder.build();
    println!("   Built {} properties:", properties.len());
    for prop in &properties {
        println!("   - {prop}");
    }
    println!();

    // -------------------------------------------------------------------------
    // Custom Diagnosticable Implementation
    // -------------------------------------------------------------------------
    println!("5. Custom Diagnosticable");
    println!("   -----------------------");

    #[derive(Debug)]
    struct CustomButton {
        label: String,
        width: f32,
        height: f32,
        enabled: bool,
        on_press: Option<String>,
    }

    impl Diagnosticable for CustomButton {
        fn debug_fill_properties(&self, properties: &mut Vec<DiagnosticsProperty>) {
            properties.push(DiagnosticsProperty::new("label", &self.label));
            properties.push(DiagnosticsProperty::new(
                "size",
                format!("{}x{}", self.width, self.height),
            ));

            if !self.enabled {
                properties.push(
                    DiagnosticsProperty::new("enabled", self.enabled)
                        .with_level(DiagnosticLevel::Warning),
                );
            }

            if let Some(handler) = &self.on_press {
                properties.push(DiagnosticsProperty::new("onPress", handler));
            } else {
                properties.push(
                    DiagnosticsProperty::new("onPress", "null")
                        .with_level(DiagnosticLevel::Warning),
                );
            }
        }
    }

    let button = CustomButton {
        label: "Submit".to_string(),
        width: 120.0,
        height: 48.0,
        enabled: false,
        on_press: None,
    };

    let node = button.to_diagnostics_node();
    println!("{node}");

    // -------------------------------------------------------------------------
    // Tree Styles
    // -------------------------------------------------------------------------
    println!("6. Different Tree Styles");
    println!("   -----------------------");

    let node = DiagnosticsNode::new("Widget")
        .property("id", 1)
        .property("type", "Container")
        .with_style(DiagnosticsTreeStyle::SingleLine);

    println!("   SingleLine style:");
    for prop in node.properties() {
        println!(
            "   {}",
            prop.format_with_style(DiagnosticsTreeStyle::SingleLine)
        );
    }

    let node = DiagnosticsNode::new("Widget")
        .property("id", 1)
        .property("type", "Container")
        .with_style(DiagnosticsTreeStyle::Dense);

    println!("\n   Dense style:");
    for prop in node.properties() {
        println!("   {}", prop.format_with_style(DiagnosticsTreeStyle::Dense));
    }
    println!();

    // -------------------------------------------------------------------------
    // Conditional Properties
    // -------------------------------------------------------------------------
    println!("7. Conditional Properties");
    println!("   ------------------------");

    let is_debug = true;
    let has_error = false;

    let node = DiagnosticsNode::new("ConditionalWidget")
        .property("name", "Test")
        .flag("debug_mode", is_debug, "DEBUG")
        .flag("has_error", has_error, "ERROR") // Won't be added
        .optional(
            "error_message",
            if has_error {
                Some("Something went wrong")
            } else {
                None
            },
        );

    println!("{node}");

    println!("=== Example Complete ===");
}
