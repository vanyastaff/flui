//! Public contracts for typed component-theme storage, patching, interpolation,
//! scoped lookup and exact inherited-entry invalidation.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::common::{lay_out, loose};
use flui_material::{ComponentTheme, Theme, ThemeData, ThemeDataOverrides};
use flui_sdk::geometry::Lerp;
use flui_sdk::view::element::ElementKind;
use flui_sdk::view::prelude::*;
use flui_sdk::view::{BoxedView, ProxyView, View};
use flui_sdk::widgets::{Column, SizedBox};

#[derive(Clone, Debug, PartialEq)]
struct TestComponent {
    value: i32,
}

impl ComponentTheme for TestComponent {}

#[derive(Clone, Debug, PartialEq)]
struct OtherComponent {
    value: i32,
}

impl ComponentTheme for OtherComponent {}

#[derive(Clone, Debug, PartialEq)]
struct ContinuousComponent {
    value: f64,
}

impl ComponentTheme for ContinuousComponent {
    fn lerp_to(&self, other: &Self, t: f64) -> Self {
        Self {
            value: self.value + (other.value - self.value) * t,
        }
    }
}

pub fn entries_insert_replace_remove_and_compare_by_content() {
    let empty = ThemeData::light();
    let first = empty.clone().with_component(TestComponent { value: 1 });
    let equal_clone = empty.clone().with_component(TestComponent { value: 1 });

    assert_eq!(
        first.component::<TestComponent>().map(|entry| entry.value),
        Some(1)
    );
    assert_eq!(
        first, equal_clone,
        "equal-content entries must compare equal"
    );
    assert_eq!(empty.component::<TestComponent>(), None);

    let replaced = first.clone().with_component(TestComponent { value: 2 });
    assert_eq!(
        replaced
            .component::<TestComponent>()
            .map(|entry| entry.value),
        Some(2)
    );
    assert_ne!(replaced, first, "replacement must change theme content");

    let removed = replaced.without_component::<TestComponent>();
    assert_eq!(removed.component::<TestComponent>(), None);
    assert_eq!(
        removed, empty,
        "removing the only entry restores the base theme"
    );
}

pub fn copy_with_distinguishes_absent_insert_replace_and_remove_patches() {
    let base = ThemeData::light()
        .with_component(TestComponent { value: 1 })
        .with_component(OtherComponent { value: 7 });

    let preserved = base.copy_with(ThemeDataOverrides::default());
    assert_eq!(preserved, base, "an absent patch preserves the entry");

    let inserted = ThemeData::light()
        .copy_with(ThemeDataOverrides::default().with_component(TestComponent { value: 2 }));
    assert_eq!(
        inserted
            .component::<TestComponent>()
            .map(|entry| entry.value),
        Some(2)
    );

    let replaced =
        base.copy_with(ThemeDataOverrides::default().with_component(TestComponent { value: 3 }));
    assert_eq!(
        replaced
            .component::<TestComponent>()
            .map(|entry| entry.value),
        Some(3)
    );
    assert_eq!(
        replaced
            .component::<OtherComponent>()
            .map(|entry| entry.value),
        Some(7)
    );

    let removed =
        replaced.copy_with(ThemeDataOverrides::default().without_component::<TestComponent>());
    assert_eq!(removed.component::<TestComponent>(), None);
    assert_eq!(
        removed
            .component::<OtherComponent>()
            .map(|entry| entry.value),
        Some(7),
        "a tombstone removes only its own component type"
    );
}

pub fn lerp_uses_typed_interpolation_and_halfway_presence_switching() {
    let from = ThemeData::light()
        .with_component(TestComponent { value: 1 })
        .with_component(ContinuousComponent { value: 0.0 })
        .with_component(OtherComponent { value: 10 });
    let to = ThemeData::dark()
        .with_component(TestComponent { value: 2 })
        .with_component(ContinuousComponent { value: 10.0 });

    let early = from.lerp_to(&to, 0.25);
    assert_eq!(
        early.component::<TestComponent>().map(|entry| entry.value),
        Some(1)
    );
    assert_eq!(
        early
            .component::<ContinuousComponent>()
            .map(|entry| entry.value),
        Some(2.5)
    );
    assert_eq!(
        early.component::<OtherComponent>().map(|entry| entry.value),
        Some(10)
    );

    let halfway = from.lerp_to(&to, 0.5);
    assert_eq!(
        halfway
            .component::<TestComponent>()
            .map(|entry| entry.value),
        Some(2)
    );
    assert_eq!(
        halfway
            .component::<ContinuousComponent>()
            .map(|entry| entry.value),
        Some(5.0)
    );
    assert_eq!(halfway.component::<OtherComponent>(), None);

    let before_insert = to.lerp_to(&from, 0.49);
    assert_eq!(before_insert.component::<OtherComponent>(), None);
    let inserted = to.lerp_to(&from, 0.5);
    assert_eq!(
        inserted
            .component::<OtherComponent>()
            .map(|entry| entry.value),
        Some(10)
    );
}

type Count = Rc<Cell<u32>>;

#[derive(Clone)]
struct Counters {
    a: Count,
    b: Count,
    color: Count,
    whole: Count,
}

impl Counters {
    fn new() -> Self {
        Self {
            a: Rc::new(Cell::new(0)),
            b: Rc::new(Cell::new(0)),
            color: Rc::new(Cell::new(0)),
            whole: Rc::new(Cell::new(0)),
        }
    }

    fn values(&self) -> [u32; 4] {
        [
            self.a.get(),
            self.b.get(),
            self.color.get(),
            self.whole.get(),
        ]
    }
}

#[derive(Clone, StatelessView)]
struct AReader {
    builds: Count,
}

impl StatelessView for AReader {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        self.builds.set(self.builds.get() + 1);
        let _ = Theme::component_of::<TestComponent>(ctx);
        SizedBox::shrink()
    }
}

#[derive(Clone, StatelessView)]
struct BReader {
    builds: Count,
}

impl StatelessView for BReader {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        self.builds.set(self.builds.get() + 1);
        let _ = Theme::component_of::<OtherComponent>(ctx);
        SizedBox::shrink()
    }
}

#[derive(Clone, StatelessView)]
struct ColorReader {
    builds: Count,
}

impl StatelessView for ColorReader {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        self.builds.set(self.builds.get() + 1);
        let _ = Theme::color_scheme_of(ctx).expect("Theme ancestor");
        SizedBox::shrink()
    }
}

#[derive(Clone, StatelessView)]
struct WholeReader {
    builds: Count,
}

impl StatelessView for WholeReader {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        self.builds.set(self.builds.get() + 1);
        let _ = Theme::of(ctx);
        SizedBox::shrink()
    }
}

#[derive(Clone)]
struct StaticChild {
    inner: BoxedView,
}

impl View for StaticChild {
    fn create_element(&self) -> ElementKind {
        ElementKind::proxy(self)
    }

    fn should_skip_rebuild(&self, _prev: &Self) -> bool {
        true
    }
}

impl ProxyView for StaticChild {
    fn child(&self) -> &dyn View {
        &self.inner
    }
}

fn subtree(counters: &Counters) -> StaticChild {
    StaticChild {
        inner: Column::new(vec![
            AReader {
                builds: Rc::clone(&counters.a),
            }
            .boxed(),
            BReader {
                builds: Rc::clone(&counters.b),
            }
            .boxed(),
            ColorReader {
                builds: Rc::clone(&counters.color),
            }
            .boxed(),
            WholeReader {
                builds: Rc::clone(&counters.whole),
            }
            .boxed(),
        ])
        .boxed(),
    }
}

pub fn changed_entries_rebuild_only_their_readers_and_whole_theme_readers() {
    let counters = Counters::new();
    let base = ThemeData::light().with_component(TestComponent { value: 1 });
    let mut laid = lay_out(Theme::new(base.clone(), subtree(&counters)), loose(100.0));
    assert_eq!(counters.values(), [1, 1, 1, 1]);

    let replaced_a = base.with_component(TestComponent { value: 2 });
    laid.pump_widget(Theme::new(replaced_a.clone(), subtree(&counters)));
    assert_eq!(
        counters.values(),
        [2, 1, 1, 2],
        "replacing A rebuilds A and whole-theme readers only"
    );

    laid.pump_widget(Theme::new(replaced_a.clone(), subtree(&counters)));
    assert_eq!(
        counters.values(),
        [2, 1, 1, 2],
        "an equal-content replacement rebuilds nobody"
    );

    let added_b = replaced_a.with_component(OtherComponent { value: 7 });
    laid.pump_widget(Theme::new(added_b.clone(), subtree(&counters)));
    assert_eq!(
        counters.values(),
        [2, 2, 1, 3],
        "adding B rebuilds B and whole-theme readers only"
    );

    let removed_b = added_b.without_component::<OtherComponent>();
    laid.pump_widget(Theme::new(removed_b.clone(), subtree(&counters)));
    assert_eq!(
        counters.values(),
        [2, 3, 1, 4],
        "removing B rebuilds B and whole-theme readers only"
    );

    let mut recolored = removed_b;
    recolored.color_scheme = ThemeData::dark().color_scheme;
    laid.pump_widget(Theme::new(recolored, subtree(&counters)));
    assert_eq!(
        counters.values(),
        [2, 3, 2, 5],
        "a base color change rebuilds color and whole-theme readers only"
    );
}

#[derive(Clone, Debug)]
enum CaptureState {
    NotBuilt,
    Built(Option<TestComponent>),
}

#[derive(Clone, Debug, StatelessView)]
struct ComponentCapture {
    captured: Rc<RefCell<CaptureState>>,
}

impl StatelessView for ComponentCapture {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        *self.captured.borrow_mut() =
            CaptureState::Built(Theme::component_of::<TestComponent>(ctx));
        SizedBox::shrink()
    }
}

fn capture_nearest_component(inner: ThemeData) -> Option<TestComponent> {
    let captured = Rc::new(RefCell::new(CaptureState::NotBuilt));
    let _laid = lay_out(
        Theme::new(
            ThemeData::light().with_component(TestComponent { value: 10 }),
            Theme::new(
                inner,
                ComponentCapture {
                    captured: Rc::clone(&captured),
                },
            ),
        ),
        loose(100.0),
    );
    match captured.borrow().clone() {
        CaptureState::Built(component) => component,
        CaptureState::NotBuilt => {
            panic!("ComponentCapture must build beneath its Theme ancestors")
        }
    }
}

pub fn nearest_theme_scope_does_not_fall_through_to_an_outer_entry() {
    assert_eq!(
        capture_nearest_component(ThemeData::light()),
        None,
        "an absent entry in the nearest Theme must not use the outer value"
    );
    assert_eq!(
        capture_nearest_component(ThemeData::light().with_component(TestComponent { value: 20 })),
        Some(TestComponent { value: 20 }),
        "a present entry in the nearest Theme wins"
    );
}
