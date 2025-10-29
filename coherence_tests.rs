// Coherence Rule Tests - Testing Different Approaches
// Run with: rustc --crate-type lib coherence_tests.rs

use std::fmt::Debug;

// =============================================================================
// TEST 1: WidgetMarker Approach (Expected: FAIL)
// =============================================================================

#[allow(dead_code)]
mod test1_widget_marker {
    use super::*;

    pub trait WidgetMarker {}
    impl<T: Debug + 'static> WidgetMarker for T {}

    pub trait Widget: WidgetMarker {
        type Element;
        fn element_type(&self) -> &'static str;
    }

    pub trait StatelessWidget: WidgetMarker {
        fn build(&self) -> String;
    }

    pub trait StatefulWidget: WidgetMarker {
        fn create_state(&self) -> i32;
    }

    // ❌ These SHOULD conflict according to coherence rules
    impl<W: StatelessWidget> Widget for W {
        type Element = String;
        fn element_type(&self) -> &'static str {
            "Stateless"
        }
    }

    impl<W: StatefulWidget> Widget for W {
        type Element = i32;
        fn element_type(&self) -> &'static str {
            "Stateful"
        }
    }
}

// =============================================================================
// TEST 2: Associated Type Discrimination (Expected: FAIL)
// =============================================================================

#[allow(dead_code)]
mod test2_associated_type {
    use super::*;

    pub struct StatelessMarker;
    pub struct StatefulMarker;

    pub trait Widget: Debug + 'static {
        type Marker;
        type Element;
        fn element_type(&self) -> &'static str;
    }

    pub trait StatelessWidget: Widget<Marker = StatelessMarker> {
        fn build(&self) -> String;
    }

    pub trait StatefulWidget: Widget<Marker = StatefulMarker> {
        fn create_state(&self) -> i32;
    }

    // ❌ These SHOULD still conflict because of circular dependency
    impl<W: StatelessWidget> Widget for W {
        type Marker = StatelessMarker;
        type Element = String;
        fn element_type(&self) -> &'static str {
            "Stateless"
        }
    }

    impl<W: StatefulWidget> Widget for W {
        type Marker = StatefulMarker;
        type Element = i32;
        fn element_type(&self) -> &'static str {
            "Stateful"
        }
    }
}

// =============================================================================
// TEST 3: Sealed Trait with Const (Expected: FAIL)
// =============================================================================

#[allow(dead_code)]
mod test3_sealed_const {
    use super::*;

    mod sealed {
        pub trait Sealed {
            const KIND: u8;
        }
    }

    pub trait Widget: sealed::Sealed + Debug + 'static {
        type Element;
    }

    pub trait StatelessWidget: Debug + 'static {
        fn build(&self) -> String;
    }

    pub trait StatefulWidget: Debug + 'static {
        fn create_state(&self) -> i32;
    }

    impl<W: StatelessWidget> sealed::Sealed for W {
        const KIND: u8 = 0;
    }

    impl<W: StatefulWidget> sealed::Sealed for W {
        const KIND: u8 = 1;
    }

    // ❌ These SHOULD still conflict - Rust doesn't check const values
    impl<W: StatelessWidget> Widget for W {
        type Element = String;
    }

    impl<W: StatefulWidget> Widget for W {
        type Element = i32;
    }
}

// =============================================================================
// TEST 4: Direct Impls (Expected: SUCCESS)
// =============================================================================

#[allow(dead_code)]
mod test4_direct_impls {
    use super::*;

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

    // No blanket impls - users impl both traits manually

    #[derive(Debug)]
    struct Counter {
        count: i32,
    }

    impl StatelessWidget for Counter {
        fn build(&self) -> String {
            format!("Count: {}", self.count)
        }
    }

    // ✅ Explicit impl - this works!
    impl Widget for Counter {
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

    // ✅ Explicit impl - this works!
    impl Widget for Timer {
        type Element = i32;
        fn element_type(&self) -> &'static str {
            "Stateful"
        }
    }
}

// =============================================================================
// TEST 5: Alternative - Negative Trait Bounds (Expected: FAIL - unstable)
// =============================================================================

#[allow(dead_code)]
mod test5_negative_bounds {
    use super::*;

    pub trait Widget: Debug + 'static {
        type Element;
    }

    pub trait StatelessWidget: Debug + 'static {
        fn build(&self) -> String;
    }

    pub trait StatefulWidget: Debug + 'static {
        fn create_state(&self) -> i32;
    }

    // ❌ This would be ideal but requires negative_impls feature (unstable)
    // #![feature(negative_impls)]
    // impl<W: StatelessWidget> !StatefulWidget for W {}
    // impl<W: StatefulWidget> !StatelessWidget for W {}

    // Then these would work:
    // impl<W: StatelessWidget> Widget for W { ... }
    // impl<W: StatefulWidget> Widget for W { ... }
}

// =============================================================================
// TEST 6: Xilem's Actual Pattern (Expected: SUCCESS)
// =============================================================================

#[allow(dead_code)]
mod test6_xilem_pattern {
    use super::*;

    // Xilem doesn't have sub-trait types!
    // Each widget implements View directly

    pub trait View: Debug + 'static {
        type Element;
        type State;

        fn build(&self) -> (Self::Element, Self::State);
    }

    #[derive(Debug)]
    struct Button {
        label: String,
    }

    // ✅ Direct impl - no blanket impls needed
    impl View for Button {
        type Element = String;
        type State = ();

        fn build(&self) -> (Self::Element, Self::State) {
            (self.label.clone(), ())
        }
    }

    #[derive(Debug)]
    struct Slider {
        value: f32,
    }

    // ✅ Direct impl
    impl View for Slider {
        type Element = f32;
        type State = f32;

        fn build(&self) -> (Self::Element, Self::State) {
            (self.value, self.value)
        }
    }
}

// =============================================================================
// TEST 7: Enum-Based Widget (Expected: SUCCESS)
// =============================================================================

#[allow(dead_code)]
mod test7_enum_widget {
    use super::*;

    pub trait StatelessWidget: Debug {
        fn build(&self) -> String;
    }

    pub trait StatefulWidget: Debug {
        fn create_state(&self) -> i32;
    }

    // ✅ Widget is an enum
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

    fn usage() {
        let counter = Counter { count: 42 };
        let widget = Widget::Stateless(Box::new(counter));
        println!("{}", widget.element_type());
    }
}
