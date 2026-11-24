//! `#[view(key)]`: the derived `View::key` is the marked field, in each shape
//! a view holds a key in.

use std::rc::Rc;

use flui_foundation::{UniqueKey, ValueKey, ViewKey};
use flui_view::prelude::*;
use flui_view::{ErrorView, GlobalKey, Memo};

fn leaf() -> ErrorView {
    ErrorView::new("a keyed view's subtree")
}

/// A key the view always has.
#[derive(Clone, PartialEq, StatelessView)]
struct Always {
    #[view(key)]
    key: ValueKey<u32>,
}

impl StatelessView for Always {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        leaf()
    }
}

/// A key the view has only when its author gave one.
#[derive(Clone, StatelessView)]
struct Sometimes {
    #[view(key)]
    key: Option<ValueKey<u32>>,
}

impl StatelessView for Sometimes {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        leaf()
    }
}

/// A key whose type the view does not name, shared with whoever made it.
#[derive(Clone, StatelessView)]
struct Shared(&'static str, #[view(key)] Rc<dyn ViewKey>);

impl StatelessView for Shared {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        ErrorView::new(self.0)
    }
}

/// A boxed key that may be absent, as a builder's `.key(impl ViewKey)` stores it.
#[derive(Clone, StatelessView)]
struct Boxed {
    #[view(key)]
    key: Option<Box<dyn ViewKey>>,
}

impl StatelessView for Boxed {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        leaf()
    }
}

/// A key over the view's own parameter: the view is a `View` for the
/// parameters that make the field a key.
#[derive(Clone, StatelessView)]
struct Item<K: Clone + Eq + std::hash::Hash + 'static> {
    #[view(key)]
    key: ValueKey<K>,
}

impl<K> StatelessView for Item<K>
where
    K: Clone + Eq + std::hash::Hash + std::fmt::Debug + 'static,
{
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        leaf()
    }
}

/// A stateful view, whose global key reaches its state.
#[derive(Clone, StatefulView)]
struct Counter {
    #[view(key)]
    key: GlobalKey<CounterState>,
}

struct CounterState;

impl StatefulView for Counter {
    type State = CounterState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        CounterState
    }
}

impl ViewState<Counter> for CounterState {
    fn build(&self, _view: &Counter, _ctx: &dyn BuildContext) -> impl IntoView {
        leaf()
    }
}

fn is(view: &dyn View, expected: &dyn ViewKey) -> bool {
    view.key().is_some_and(|key| key.key_eq(expected))
}

#[test]
fn a_derived_view_reports_the_key_its_marked_field_holds() {
    let seven: ValueKey<u32> = ValueKey::new(7);
    let eight: ValueKey<u32> = ValueKey::new(8);

    let always = Always { key: seven.clone() };
    assert!(is(&always, &seven), "a key field is the view's key");
    assert!(!is(&always, &eight), "and no other key");
    assert!(
        is(&Memo::new(always), &seven),
        "a memo is matched by the key of the view it wraps"
    );

    let keyed = Sometimes {
        key: Some(seven.clone()),
    };
    assert!(is(&keyed, &seven), "an optional key that is present");
    assert!(
        Sometimes { key: None }.key().is_none(),
        "an optional key that is absent leaves the view keyless"
    );

    let unique = UniqueKey::new();
    let shared: Rc<dyn ViewKey> = Rc::new(unique);
    assert!(
        is(&Shared("shared", shared), &unique),
        "a shared trait-object key, in a positional field"
    );

    let boxed: Box<dyn ViewKey> = Box::new(eight.clone());
    assert!(
        is(&Boxed { key: Some(boxed) }, &eight),
        "a boxed trait-object key that is present"
    );
    assert!(
        Boxed { key: None }.key().is_none(),
        "and one that is absent"
    );

    let item = Item {
        key: ValueKey::new("row"),
    };
    assert!(
        is(&item, &ValueKey::new("row")),
        "a key over the view's parameter"
    );

    let global: GlobalKey<CounterState> = GlobalKey::new();
    assert!(
        is(
            &Counter {
                key: global.clone()
            },
            &global
        ),
        "a stateful view's global key"
    );
}
