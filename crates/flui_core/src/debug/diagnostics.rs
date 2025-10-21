//! Diagnostic tree printing for debugging
//!
//! Provides utilities to print element trees in a human-readable format.

use crate::element::DynElement;
use std::fmt::Write;

/// Print element tree to string with indentation
///
/// # Example
///
/// ```text
/// Container #ElementId(1)
///   ├─ Row #ElementId(2)
///   │  ├─ Text #ElementId(3)
///   │  └─ Button #ElementId(4)
///   └─ Column #ElementId(5)
/// ```
pub fn print_element_tree(element: &dyn DynElement) -> String {
    let mut output = String::new();
    print_element_recursive(element, &mut output, "", true);
    output
}

fn print_element_recursive(
    element: &dyn DynElement,
    output: &mut String,
    prefix: &str,
    is_last: bool,
) {
    // Print current element
    let connector = if is_last { "└─" } else { "├─" };
    let type_name = element.widget_type_id();
    let type_str = format!("{:?}", type_name);

    writeln!(
        output,
        "{}{} {} #{:?}",
        prefix,
        connector,
        type_str.split("::").last().unwrap_or(&type_str),
        element.id()
    )
    .ok();

    // Prepare prefix for children
    let child_prefix = if is_last {
        format!("{}   ", prefix)
    } else {
        format!("{}│  ", prefix)
    };

    // Print children
    let children: Vec<_> = element.children_iter().collect();
    for (i, child_id) in children.iter().enumerate() {
        // Note: We can't actually get child elements without tree access
        // This is a simplified version
        writeln!(
            output,
            "{}   {} Child #{:?}",
            child_prefix,
            if i == children.len() - 1 { "└─" } else { "├─" },
            child_id
        )
        .ok();
    }
}

/// Simple diagnostic info for an element
pub fn element_info(element: &dyn DynElement) -> String {
    format!(
        "{} #{:?} (lifecycle: {:?}, dirty: {})",
        format!("{:?}", element.widget_type_id())
            .split("::")
            .last()
            .unwrap_or("Unknown"),
        element.id(),
        element.lifecycle(),
        element.is_dirty()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_element_info() {
        use crate::element::ComponentElement;
        use crate::widget::StatelessWidget;
        use crate::Context;

        #[derive(Debug, Clone)]
        struct TestWidget;

        impl StatelessWidget for TestWidget {
            fn build(&self, _context: &Context) -> Box<dyn crate::widget::DynWidget> {
                Box::new(TestWidget)
            }
        }

        let element = ComponentElement::new(TestWidget);
        let info = element_info(&element);

        // Info should contain element ID
        assert!(info.contains("ElementId") || info.contains("#"));
        // Info should have some content (not empty)
        assert!(!info.is_empty());
    }
}
