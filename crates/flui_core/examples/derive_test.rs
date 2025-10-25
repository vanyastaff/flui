//! Test derive macros

use flui_core::{Widget, StatelessWidget, BoxedWidget, DeriveStatelessWidget};

#[derive(DeriveStatelessWidget, Debug, Clone)]
struct MyWidget {
    value: i32,
}

impl StatelessWidget for MyWidget {
    fn build(&self) -> BoxedWidget {
        // Return a placeholder for now
        Box::new(MyWidget { value: self.value + 1 })
    }
}

fn main() {
    let widget = MyWidget { value: 42 };
    println!("Widget: {:?}", widget);

    // Test that Widget trait is implemented
    let _element = widget.into_element();
    println!("✅ Derive macro works!");
}
