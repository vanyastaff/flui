// Coherence Success Tests - Approaches that SHOULD work
// Run with: rustc --crate-type lib coherence_success_tests.rs

use std::fmt::Debug;

// =============================================================================
// TEST 1: Direct Impls - No Blanket Impls (✅ WORKS)
// =============================================================================

mod test1_direct_impls {
    use super::*;

    pub trait Widget: Debug + 'static {
        type Element;
        fn element_type(&self) -> &'static str;

        // Can still have default methods
        fn debug_name(&self) -> String {
            format!("{:?}", self)
        }
    }

    pub trait StatelessWidget {
        fn build(&self) -> String;
    }

    pub trait StatefulWidget {
        fn create_state(&self) -> i32;
    }

    // Example 1: Stateless widget
    #[derive(Debug)]
    struct Counter {
        count: i32,
    }

    impl StatelessWidget for Counter {
        fn build(&self) -> String {
            format!("Count: {}", self.count)
        }
    }

    impl Widget for Counter {
        type Element = String;
        fn element_type(&self) -> &'static str {
            "Stateless"
        }
    }

    // Example 2: Stateful widget
    #[derive(Debug)]
    struct Timer {
        seconds: i32,
    }

    impl StatefulWidget for Timer {
        fn create_state(&self) -> i32 {
            self.seconds
        }
    }

    impl Widget for Timer {
        type Element = i32;
        fn element_type(&self) -> &'static str {
            "Stateful"
        }
    }

    #[test]
    fn test_direct_impls() {
        let counter = Counter { count: 42 };
        assert_eq!(counter.element_type(), "Stateless");
        assert_eq!(counter.build(), "Count: 42");

        let timer = Timer { seconds: 10 };
        assert_eq!(timer.element_type(), "Stateful");
        assert_eq!(timer.create_state(), 10);
    }
}

// =============================================================================
// TEST 2: Xilem Pattern - Single Trait, No Hierarchy (✅ WORKS)
// =============================================================================

mod test2_xilem_pattern {
    use super::*;

    pub trait View: Debug + 'static {
        type Element;
        type State;

        fn build(&self) -> (Self::Element, Self::State);

        fn rebuild(&self, state: &mut Self::State) -> Self::Element {
            let (elem, new_state) = self.build();
            *state = new_state;
            elem
        }
    }

    // Stateless widget (State = ())
    #[derive(Debug)]
    struct Button {
        label: String,
    }

    impl View for Button {
        type Element = String;
        type State = ();

        fn build(&self) -> (Self::Element, Self::State) {
            (format!("Button: {}", self.label), ())
        }
    }

    // Stateful widget (State = f32)
    #[derive(Debug)]
    struct Slider {
        value: f32,
    }

    impl View for Slider {
        type Element = f32;
        type State = f32;

        fn build(&self) -> (Self::Element, Self::State) {
            (self.value, self.value)
        }
    }

    #[test]
    fn test_xilem_pattern() {
        let button = Button {
            label: "Click".to_string(),
        };
        let (elem, _) = button.build();
        assert_eq!(elem, "Button: Click");

        let slider = Slider { value: 0.5 };
        let (elem, state) = slider.build();
        assert_eq!(elem, 0.5);
        assert_eq!(state, 0.5);
    }
}

// =============================================================================
// TEST 3: Enum-Based Widget (✅ WORKS)
// =============================================================================

mod test3_enum_widget {
    use super::*;

    pub trait StatelessWidget: Debug {
        fn build(&self) -> String;
    }

    pub trait StatefulWidget: Debug {
        fn create_state(&self) -> i32;
        fn update_state(&self, state: &mut i32);
    }

    pub enum Widget {
        Stateless(Box<dyn StatelessWidget>),
        Stateful(Box<dyn StatefulWidget>),
    }

    impl Widget {
        pub fn element_type(&self) -> &'static str {
            match self {
                Widget::Stateless(_) => "Stateless",
                Widget::Stateful(_) => "Stateful",
            }
        }

        pub fn from_stateless<W: StatelessWidget + 'static>(w: W) -> Self {
            Widget::Stateless(Box::new(w))
        }

        pub fn from_stateful<W: StatefulWidget + 'static>(w: W) -> Self {
            Widget::Stateful(Box::new(w))
        }
    }

    #[derive(Debug)]
    struct Counter {
        count: i32,
    }

    impl StatelessWidget for Counter {
        fn build(&self) -> String {
            format!("{}", self.count)
        }
    }

    #[derive(Debug)]
    struct Timer {
        seconds: i32,
    }

    impl StatefulWidget for Timer {
        fn create_state(&self) -> i32 {
            self.seconds
        }

        fn update_state(&self, state: &mut i32) {
            *state += 1;
        }
    }

    #[test]
    fn test_enum_widget() {
        let counter = Counter { count: 42 };
        let widget = Widget::from_stateless(counter);
        assert_eq!(widget.element_type(), "Stateless");

        let timer = Timer { seconds: 10 };
        let widget = Widget::from_stateful(timer);
        assert_eq!(widget.element_type(), "Stateful");
    }
}

// =============================================================================
// TEST 4: Conditional Impl with Separate Types (✅ WORKS)
// =============================================================================

mod test4_separate_types {
    use super::*;

    // Instead of blanket impls, use newtype wrappers

    pub trait Widget: Debug + 'static {
        type Element;
        fn element_type(&self) -> &'static str;
    }

    pub trait StatelessWidget {
        fn build(&self) -> String;
    }

    pub trait StatefulWidget {
        fn create_state(&self) -> i32;
    }

    // Wrapper types that implement Widget
    #[derive(Debug)]
    pub struct StatelessWidgetWrapper<W>(pub W);

    #[derive(Debug)]
    pub struct StatefulWidgetWrapper<W>(pub W);

    impl<W: StatelessWidget + Debug + 'static> Widget for StatelessWidgetWrapper<W> {
        type Element = String;
        fn element_type(&self) -> &'static str {
            "Stateless"
        }
    }

    impl<W: StatefulWidget + Debug + 'static> Widget for StatefulWidgetWrapper<W> {
        type Element = i32;
        fn element_type(&self) -> &'static str {
            "Stateful"
        }
    }

    // Now users can wrap their widgets
    #[derive(Debug)]
    struct Counter {
        count: i32,
    }

    impl StatelessWidget for Counter {
        fn build(&self) -> String {
            format!("{}", self.count)
        }
    }

    #[test]
    fn test_wrapper() {
        let counter = Counter { count: 42 };
        let widget = StatelessWidgetWrapper(counter);
        assert_eq!(widget.element_type(), "Stateless");
    }
}

// =============================================================================
// TEST 5: Macro-Generated Impls (Conceptual - would need proc macro)
// =============================================================================

mod test5_macro_approach {
    use super::*;

    pub trait Widget: Debug + 'static {
        type Element;
        fn element_type(&self) -> &'static str;
    }

    pub trait StatelessWidget {
        fn build(&self) -> String;
    }

    // This would be generated by #[derive(Widget)]
    macro_rules! impl_widget_for_stateless {
        ($ty:ty) => {
            impl Widget for $ty {
                type Element = String;
                fn element_type(&self) -> &'static str {
                    "Stateless"
                }
            }
        };
    }

    #[derive(Debug)]
    struct Counter {
        count: i32,
    }

    impl StatelessWidget for Counter {
        fn build(&self) -> String {
            format!("{}", self.count)
        }
    }

    // User would write #[derive(Widget)] instead
    impl_widget_for_stateless!(Counter);

    #[test]
    fn test_macro() {
        let counter = Counter { count: 42 };
        assert_eq!(counter.element_type(), "Stateless");
    }
}

// =============================================================================
// TEST 6: Associated Const Discrimination (✅ WORKS with manual impls)
// =============================================================================

mod test6_const_discrimination {
    use super::*;

    pub trait Widget: Debug + 'static {
        const KIND: u8;
        type Element;
        fn element_type(&self) -> &'static str;
    }

    pub trait StatelessWidget {
        fn build(&self) -> String;
    }

    pub trait StatefulWidget {
        fn create_state(&self) -> i32;
    }

    // Manual impls work fine
    #[derive(Debug)]
    struct Counter {
        count: i32,
    }

    impl StatelessWidget for Counter {
        fn build(&self) -> String {
            format!("{}", self.count)
        }
    }

    impl Widget for Counter {
        const KIND: u8 = 0; // Stateless
        type Element = String;
        fn element_type(&self) -> &'static str {
            "Stateless"
        }
    }

    #[derive(Debug)]
    struct Timer {
        seconds: i32,
    }

    impl StatefulWidget for Timer {
        fn create_state(&self) -> i32 {
            self.seconds
        }
    }

    impl Widget for Timer {
        const KIND: u8 = 1; // Stateful
        type Element = i32;
        fn element_type(&self) -> &'static str {
            "Stateful"
        }
    }

    #[test]
    fn test_const_discrimination() {
        assert_eq!(Counter::KIND, 0);
        assert_eq!(Timer::KIND, 1);
    }
}

fn main() {
    println!("All coherence success tests compiled successfully! ✅");
}
