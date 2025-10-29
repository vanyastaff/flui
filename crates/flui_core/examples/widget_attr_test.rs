//! Test #[widget] attribute macro

use flui_core::{BoxedWidget, StatelessWidget, Widget, widget};

#[widget] // Автоматически добавляет Debug, Clone и генерирует Widget/DynWidget
struct MyWidget {
    value: i32,
}

impl StatelessWidget for MyWidget {
    fn build(&self) -> BoxedWidget {
        // Return a placeholder for now
        Box::new(MyWidget {
            value: self.value + 1,
        })
    }
}

fn main() {
    let widget = MyWidget { value: 42 };
    println!("Widget: {:?}", widget);

    let cloned = widget.clone();
    println!("Cloned: {:?}", cloned);

    // Test that Widget trait is implemented
    let _element = cloned.into_element();
    println!("✅ #[widget] attribute macro works!");
}
