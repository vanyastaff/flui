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
/// new direction, given how it is offered.
fn builds(dependent: bool, fresh: bool) -> u32 {
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
    let offer = move || -> BoxedView { if fresh { make() } else { kept.clone() } };
    let mut harness = mount(Directionality::new(TextDirection::Ltr, offer()));
    for direction in [TextDirection::Rtl, TextDirection::Ltr] {
        harness.swap_root(Directionality::new(direction, offer()));
    }
    builds.get()
}

/// Rows: (case, dependent, fresh, builds across the mount and two updates).
pub(crate) fn a_child_handed_back_unchanged_is_built_only_when_something_it_reads_changes() {
    let rows = [
        ("unchanged, reads nothing", false, false, 1),
        ("built anew, reads nothing", false, true, 3),
        ("unchanged, reads the direction", true, false, 3),
    ];
    for (case, dependent, fresh, expected) in rows {
        assert_eq!(builds(dependent, fresh), expected, "{case}");
    }
}
