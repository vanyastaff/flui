//! Test for BoxedWidget cloning via dyn-clone
//!
//! This test verifies that `Box<dyn DynWidget>` can be cloned thanks to
//! the `DynClone` trait integration.

use flui_core::*;

#[derive(Debug, Clone)]
struct SimpleWidget {
    value: i32,
}

// StatelessWidget automatically implements Widget and DynWidget
impl StatelessWidget for SimpleWidget {
    fn build(&self) -> BoxedWidget {
        Box::new(SimpleWidget {
            value: self.value + 1,
        })
    }
}

#[test]
fn test_boxed_widget_can_clone() {
    // Create a boxed widget
    let widget: BoxedWidget = Box::new(SimpleWidget { value: 42 });

    // Clone it! This is the key feature we're testing
    let cloned = widget.clone();

    // Both should have the same value
    let original_value = widget.downcast_ref::<SimpleWidget>().unwrap().value;
    let cloned_value = cloned.downcast_ref::<SimpleWidget>().unwrap().value;

    assert_eq!(original_value, 42);
    assert_eq!(cloned_value, 42);

    // They should be different instances (different memory addresses)
    assert_ne!(
        widget.as_ref() as *const dyn DynWidget,
        cloned.as_ref() as *const dyn DynWidget,
        "Cloned widget should be a separate instance"
    );
}

#[test]
fn test_vec_of_boxed_widgets_can_clone() {
    // Create a vec of boxed widgets
    let widgets: Vec<BoxedWidget> = vec![
        Box::new(SimpleWidget { value: 1 }),
        Box::new(SimpleWidget { value: 2 }),
        Box::new(SimpleWidget { value: 3 }),
    ];

    // Clone the entire vec! This works because BoxedWidget implements Clone
    let cloned_widgets = widgets.clone();

    // Verify lengths match
    assert_eq!(widgets.len(), cloned_widgets.len());

    // Verify all values match
    for (i, (original, cloned)) in widgets.iter().zip(cloned_widgets.iter()).enumerate() {
        let original_value = original.downcast_ref::<SimpleWidget>().unwrap().value;
        let cloned_value = cloned.downcast_ref::<SimpleWidget>().unwrap().value;

        assert_eq!(
            original_value, cloned_value,
            "Value mismatch at index {}",
            i
        );

        // But they should be different instances
        assert_ne!(
            original.as_ref() as *const dyn DynWidget,
            cloned.as_ref() as *const dyn DynWidget,
            "Widget at index {} should be a separate instance",
            i
        );
    }
}

#[test]
fn test_multiple_different_widgets_clone() {
    // Define another widget type
    #[derive(Debug, Clone)]
    struct AnotherWidget {
        name: String,
    }

    impl StatelessWidget for AnotherWidget {
        fn build(&self) -> BoxedWidget {
            Box::new(AnotherWidget {
                name: self.name.clone(),
            })
        }
    }

    // Mix different widget types
    let widgets: Vec<BoxedWidget> = vec![
        Box::new(SimpleWidget { value: 100 }),
        Box::new(AnotherWidget {
            name: "test".to_string(),
        }),
        Box::new(SimpleWidget { value: 200 }),
    ];

    // Clone the mixed vec
    let cloned = widgets.clone();

    assert_eq!(widgets.len(), cloned.len());

    // Verify first widget (SimpleWidget)
    assert!(widgets[0].downcast_ref::<SimpleWidget>().is_some());
    assert_eq!(
        widgets[0].downcast_ref::<SimpleWidget>().unwrap().value,
        100
    );

    // Verify second widget (AnotherWidget)
    assert!(widgets[1].downcast_ref::<AnotherWidget>().is_some());
    assert_eq!(
        widgets[1].downcast_ref::<AnotherWidget>().unwrap().name,
        "test"
    );
}
