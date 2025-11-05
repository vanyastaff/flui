//! Mock Render Elements for Examples
//!
//! Заглушки для render элементов, чтобы примеры компилировались.
//! В реальной реализации эти элементы будут настоящими.

use flui_core::element::{Element, ComponentElement};

/// Mock для текстового элемента
pub fn create_text_element(text: String) -> ComponentElement {
    // В реальности создаётся TextRenderElement
    println!("Mock: Creating text element with: {}", text);
    create_mock_element(format!("Text({})", text))
}

/// Mock для кнопки
pub fn create_button_element(text: String, enabled: bool) -> ComponentElement {
    println!("Mock: Creating button '{}' (enabled: {})", text, enabled);
    create_mock_element(format!("Button({}, enabled={})", text, enabled))
}

/// Mock для текстового поля
pub fn create_textfield_element(label: String, value: String, placeholder: Option<String>) -> ComponentElement {
    println!("Mock: Creating text field '{}' with value '{}' (placeholder: {:?})", label, value, placeholder);
    create_mock_element(format!("TextField({}, {})", label, value))
}

/// Mock для checkbox
pub fn create_checkbox_element(label: String, checked: bool) -> ComponentElement {
    println!("Mock: Creating checkbox '{}' (checked: {})", label, checked);
    create_mock_element(format!("Checkbox({}, {})", label, checked))
}

/// Mock для padding
pub fn create_padding_element(padding: f32, _child: Option<Element>) -> ComponentElement {
    println!("Mock: Creating padding with padding: {}", padding);
    create_mock_element(format!("Padding({})", padding))
}

/// Mock для Row
pub fn create_row_element(spacing: f32, child_count: usize) -> ComponentElement {
    println!("Mock: Creating row with {} children (spacing: {})", child_count, spacing);
    create_mock_element(format!("Row({} children, spacing={})", child_count, spacing))
}

/// Mock для Column
pub fn create_column_element(spacing: f32, child_count: usize) -> ComponentElement {
    println!("Mock: Creating column with {} children (spacing: {})", child_count, spacing);
    create_mock_element(format!("Column({} children, spacing={})", child_count, spacing))
}

/// Mock для Container
pub fn create_container_element(child_count: usize, padding: f32) -> ComponentElement {
    println!("Mock: Creating container with {} children (padding: {})", child_count, padding);
    create_mock_element(format!("Container({} children)", child_count))
}

// Mock View implementation for examples
#[derive(Debug, Clone)]
struct MockView {
    description: String,
}

impl flui_core::view::View for MockView {
    type Element = flui_core::element::ComponentElement;
    type State = ();

    fn build(self, _ctx: &mut flui_core::BuildContext) -> (Self::Element, Self::State) {
        // Create a simple component element
        let view: Box<dyn flui_core::view::AnyView> = Box::new(self.clone());
        let state: Box<dyn std::any::Any> = Box::new(());
        let component = flui_core::element::ComponentElement::new(view, state);
        (component, ())
    }
}

/// Внутренняя функция для создания mock элемента
///
/// Создаёт простой ComponentElement с MockView.
fn create_mock_element(description: String) -> ComponentElement {
    let mock_view = MockView { description };
    let view: Box<dyn flui_core::view::AnyView> = Box::new(mock_view);
    let state: Box<dyn std::any::Any> = Box::new(());

    ComponentElement::new(view, state)
}
