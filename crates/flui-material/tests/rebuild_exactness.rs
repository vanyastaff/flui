//! Rebuild-exactness — roadmap Catalog.1 exit criterion for `Theme`.
//!
//! Mounts a tree of 1,003 widgets (`Theme` → a rebuild-inert wrapper →
//! `Column` → 1,000 leaves), where exactly [`DEPENDENT_COUNT`] leaves call
//! [`Theme::of`] (dependents) and the rest never touch `Theme` at all
//! (non-dependents). Swapping `Theme`'s data at the root must rebuild
//! *exactly* the dependents — not zero of them (a broken
//! `update_should_notify`/dependents-notify path), and not the whole
//! 1,000-leaf subtree (a coarse "everything rebuilds" regression).
//!
//! ## Why a `StaticChild` wrapper is load-bearing here, not incidental
//!
//! FLUI's rebuild dispatch (`should_skip_rebuild`,
//! `crates/flui-view/src/view/view.rs`) defaults to "always rebuild" for
//! every view — by design, this is the *safe* default (see that method's
//! doc comment): a parent that re-`build()`s produces a fresh child-widget
//! value, and without an explicit opt-out the framework cannot know that
//! value is interchangeable with the old one. That default means a plain
//! root-widget swap would cascade a full rebuild through `Column` and all
//! 1,000 leaves regardless of `Theme`'s own dependents bookkeeping — which
//! would make this test pass *for the wrong reason* (everything rebuilds,
//! dependents included, by brute force) and silently fail to catch a
//! genuinely broken scoped-notify path.
//!
//! `StaticChild` (below) opts out explicitly — `should_skip_rebuild` always
//! returns `true` — so the child subtree is proven inert to `Theme`'s own
//! rebuild, and the *only* remaining channel through which a leaf can
//! rebuild is `InheritedElement`'s dependents-notify path
//! (`update_should_notify` + the per-element dependents set), which is
//! exactly the mechanism this test exists to hold accountable. This mirrors
//! Flutter's own mechanism for the same problem: `InheritedWidget.child` is
//! a stored field reused verbatim across a `pumpWidget` swap (Dart identity
//! short-circuits `Element.updateChild`), not reconstructed — `StaticChild`
//! is FLUI's explicit, opt-in equivalent (see `View::should_skip_rebuild`'s
//! doc comment on `Memo<V>`, the general-purpose version of this opt-out).

#![allow(clippy::unwrap_used)]

mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::{lay_out, loose};
use flui_material::{Theme, ThemeData};
use flui_view::element::ElementKind;
use flui_view::prelude::*;
use flui_view::{BoxedView, ProxyView, View};
use flui_widgets::{Column, SizedBox};

/// Leaves that register a `Theme::of` dependency, spread evenly through the
/// 1,000-leaf list (every 20th index) rather than clustered at one end —
/// clustering could hide an off-by-one in whatever dependents bookkeeping is
/// under test.
const TOTAL_LEAVES: usize = 1000;
const DEPENDENT_STRIDE: usize = 20;

fn is_dependent_index(index: usize) -> bool {
    index.is_multiple_of(DEPENDENT_STRIDE)
}

/// A leaf that reads [`Theme::of`] during `build()` and records the call in
/// its `Rc`-shared counter. `Rc<Cell<u32>>` (not `Arc`/`Mutex`): the test
/// harness mounts and pumps on a single thread, and every counter read here
/// happens after `pump_widget` returns (no concurrent access), so the
/// cheaper single-threaded cell is the right tool — no synchronization to
/// prove correct, no lock to poison.
#[derive(Clone, StatelessView)]
struct Dependent {
    build_count: Rc<Cell<u32>>,
}

impl StatelessView for Dependent {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        self.build_count.set(self.build_count.get() + 1);
        let _ = Theme::of(ctx);
        SizedBox::shrink()
    }
}

/// A leaf that never touches `Theme` — the control group. Structurally
/// identical to [`Dependent`] otherwise, so the only variable between the
/// two groups is whether `Theme::of` was called.
#[derive(Clone, StatelessView)]
struct NonDependent {
    build_count: Rc<Cell<u32>>,
}

impl StatelessView for NonDependent {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        self.build_count.set(self.build_count.get() + 1);
        SizedBox::shrink()
    }
}

/// Wraps a child subtree and unconditionally opts it out of rebuild — see
/// the module docs for why this is load-bearing, not a shortcut.
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
        &*self.inner.0
    }
}

/// Build the 1,000-leaf `Column`, wrapped in [`StaticChild`], from the given
/// per-leaf counters (`counters[i]` backs leaf `i`).
fn build_static_subtree(counters: &[Rc<Cell<u32>>]) -> StaticChild {
    let children: Vec<BoxedView> = counters
        .iter()
        .enumerate()
        .map(|(index, counter)| {
            if is_dependent_index(index) {
                Dependent {
                    build_count: Rc::clone(counter),
                }
                .boxed()
            } else {
                NonDependent {
                    build_count: Rc::clone(counter),
                }
                .boxed()
            }
        })
        .collect();
    StaticChild {
        inner: Column::new(children).boxed(),
    }
}

#[test]
fn swapping_theme_data_rebuilds_exactly_the_dependents() {
    let counters: Vec<Rc<Cell<u32>>> = (0..TOTAL_LEAVES).map(|_| Rc::new(Cell::new(0))).collect();
    let dependent_indices: Vec<usize> = (0..TOTAL_LEAVES)
        .filter(|&i| is_dependent_index(i))
        .collect();
    let non_dependent_indices: Vec<usize> = (0..TOTAL_LEAVES)
        .filter(|&i| !is_dependent_index(i))
        .collect();
    assert!(
        dependent_indices.len() >= 40,
        "test setup: expected a substantial dependent group, got {}",
        dependent_indices.len()
    );
    assert!(
        non_dependent_indices.len() >= TOTAL_LEAVES - 60,
        "test setup: expected most leaves to be non-dependents, got {}",
        non_dependent_indices.len()
    );

    let root = Theme::new(ThemeData::light(), build_static_subtree(&counters));
    let mut laid = lay_out(root, loose(4000.0));

    for &index in &dependent_indices {
        assert_eq!(
            counters[index].get(),
            1,
            "dependent leaf {index} should have built exactly once on initial mount"
        );
    }
    for &index in &non_dependent_indices {
        assert_eq!(
            counters[index].get(),
            1,
            "non-dependent leaf {index} should have built exactly once on initial mount"
        );
    }

    // Swap `Theme`'s data at the root. `light()` vs `dark()` differ on both
    // `color_scheme` and `text_theme`, so `Theme::update_should_notify`
    // (`self.data != old.data`) is genuinely exercised, not vacuously true.
    let root2 = Theme::new(ThemeData::dark(), build_static_subtree(&counters));
    laid.pump_widget(root2);

    // Direction 1 — must fail if `update_should_notify` is broken (returns
    // `false` when the data actually changed) or if the dependents-notify
    // path silently drops entries: every dependent must have rebuilt.
    for &index in &dependent_indices {
        assert_eq!(
            counters[index].get(),
            2,
            "dependent leaf {index} must rebuild when Theme's data changes \
             (update_should_notify or the dependents-notify path is broken)"
        );
    }

    // Direction 2 — must fail if the swap rebuilds the whole subtree instead
    // of just the dependents: every non-dependent must be untouched. This
    // checks the full non-dependent set (950 leaves), a superset of a
    // sampled check — cheap here since it is only a `Cell` read.
    for &index in &non_dependent_indices {
        assert_eq!(
            counters[index].get(),
            1,
            "non-dependent leaf {index} must NOT rebuild from a Theme-only \
             data change — it never depends on Theme, so a changed count here \
             means the whole tree rebuilt (StaticChild's should_skip_rebuild \
             boundary did not hold)"
        );
    }
}
