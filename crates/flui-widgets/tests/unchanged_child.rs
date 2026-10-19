//! A child its parent hands back unchanged is not built again; one the parent
//! builds anew is; one that depends on what changed is, unchanged or not.
//!
//! The parent here is a `Directionality` whose direction a root swap flips:
//! it updates in place and offers its child again, either the view it was
//! given before or a fresh one, so the build counts tell the three apart.

use std::cell::Cell;
use std::rc::Rc;

use flui_painting::typography::TextDirection;
use flui_view::prelude::*;
use flui_view::{BoxedView, ViewExt};
use flui_widgets::{Directionality, SizedBox};

use crate::common::harness::mount;

/// Counts its builds; reads the ambient direction, and so depends on it, when
/// `dependent`.
#[derive(Clone, StatelessView)]
struct Counted {
    builds: Rc<Cell<u32>>,
    dependent: bool,
}

impl std::fmt::Debug for Counted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Counted")
    }
}

impl StatelessView for Counted {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        if self.dependent {
            let _ = Directionality::of(ctx);
        }
        self.builds.set(self.builds.get() + 1);
        SizedBox::shrink()
    }
}

/// How many times the child built after its parent was updated twice with a
/// new direction, given how it is offered: `fresh` builds a new view each
/// time, `rebox` hands the shared one back through a `Box<dyn View>`.
fn builds(dependent: bool, fresh: bool, rebox: bool) -> u32 {
    let builds = Rc::new(Cell::new(0));
    let make = {
        let builds = Rc::clone(&builds);
        move || {
            Counted {
                builds: Rc::clone(&builds),
                dependent,
            }
            .boxed()
        }
    };
    let kept = make();
    let offer = move || -> BoxedView {
        if fresh {
            make()
        } else if rebox {
            BoxedView::from(Box::new(kept.clone()) as Box<dyn View>)
        } else {
            kept.clone()
        }
    };
    let mut harness = mount(Directionality::new(TextDirection::Ltr, offer()));
    for direction in [TextDirection::Rtl, TextDirection::Ltr] {
        harness.swap_root(Directionality::new(direction, offer()));
    }
    builds.get()
}

/// Rows: (case, dependent, fresh, rebox, builds across the mount and two
/// updates).
pub(crate) fn a_child_handed_back_unchanged_is_built_only_when_something_it_reads_changes() {
    let rows = [
        ("unchanged, reads nothing", false, false, false, 1),
        (
            "unchanged, boxed again, reads nothing",
            false,
            false,
            true,
            1,
        ),
        ("built anew, reads nothing", false, true, false, 3),
        ("unchanged, reads the direction", true, false, false, 3),
    ];
    for (case, dependent, fresh, rebox, expected) in rows {
        assert_eq!(builds(dependent, fresh, rebox), expected, "{case}");
    }
}

/// A lazily mounted item the builder hands back unchanged after the list is
/// updated is not built again, as for a child mounted with its parent.
pub(crate) fn a_lazily_mounted_item_handed_back_unchanged_is_not_built_again() {
    use crate::common::{lay_out, tight};
    use flui_widgets::ListView;

    const COUNT: usize = 3;
    let builds = Rc::new(Cell::new(0));
    let items: Vec<BoxedView> = (0..COUNT)
        .map(|_| {
            Counted {
                builds: Rc::clone(&builds),
                dependent: false,
            }
            .boxed()
        })
        .collect();
    let list = move || {
        let items = items.clone();
        ListView::builder(COUNT, 48.0, move |index| items.get(index).cloned())
            .repaint_boundaries(false)
    };
    let mut laid = lay_out(list(), tight(200.0, 200.0));
    let after_mount = builds.get();
    assert_eq!(
        after_mount, COUNT as u32,
        "every item is visible and built once"
    );
    laid.pump_widget(list());
    assert_eq!(
        builds.get(),
        after_mount,
        "the same items were offered again"
    );
}
