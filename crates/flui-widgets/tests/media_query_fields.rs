//! Issue #1090 acceptance, `MediaQuery` half: a dependent that read one field
//! rebuilds only when that field changes; a whole-`of` dependent rebuilds for
//! any field; non-dependents never rebuild from a provider-only change.
//!
//! The provider is swapped through `pump_widget`, and the subtree below it
//! sits behind a `should_skip_rebuild` boundary (the `rebuild_exactness`
//! pattern), so the only way a leaf rebuilds is through the dependency path.

use std::cell::Cell;
use std::rc::Rc;

use crate::common::{lay_out, loose};
use flui_view::element::ElementKind;
use flui_view::prelude::*;
use flui_view::{BoxedView, ProxyView, View};
use flui_widgets::{Column, MediaQuery, MediaQueryData, SizedBox};

type Count = Rc<Cell<u32>>;

fn count() -> Count {
    Rc::new(Cell::new(0))
}

#[derive(Clone, StatelessView)]
struct SizeReader {
    builds: Count,
}

impl StatelessView for SizeReader {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        self.builds.set(self.builds.get() + 1);
        let size = MediaQuery::size_of(ctx).expect("MediaQuery ancestor");
        SizedBox::new(size.width / 100.0, 1.0)
    }
}

#[derive(Clone, StatelessView)]
struct TextScaleReader {
    builds: Count,
}

impl StatelessView for TextScaleReader {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        self.builds.set(self.builds.get() + 1);
        let scale = MediaQuery::text_scale_factor_of(ctx).expect("MediaQuery ancestor");
        SizedBox::new(scale, 1.0)
    }
}

#[derive(Clone, StatelessView)]
struct WholeReader {
    builds: Count,
}

impl StatelessView for WholeReader {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        self.builds.set(self.builds.get() + 1);
        let _ = MediaQuery::of(ctx);
        SizedBox::square(1.0)
    }
}

#[derive(Clone, StatelessView)]
struct NonDependent {
    builds: Count,
}

impl StatelessView for NonDependent {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        self.builds.set(self.builds.get() + 1);
        SizedBox::square(1.0)
    }
}

/// Reads `size`, but panics before reading it while `fail` is set.
#[derive(Clone, StatelessView)]
struct FailingSizeReader {
    fail: Rc<Cell<bool>>,
    builds: Count,
}

impl StatelessView for FailingSizeReader {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        self.builds.set(self.builds.get() + 1);
        assert!(!self.fail.get(), "test-induced build failure");
        let size = MediaQuery::size_of(ctx).expect("MediaQuery ancestor");
        SizedBox::new(size.width / 100.0, 1.0)
    }
}

/// Stops parent-driven rebuilds so only dependency notifications reach the
/// leaves.
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

struct Counters {
    size: Count,
    scale: Count,
    whole: Count,
    none: Count,
}

fn subtree(c: &Counters) -> StaticChild {
    use flui_view::ViewExt;
    StaticChild {
        inner: Column::new(vec![
            SizeReader {
                builds: Rc::clone(&c.size),
            }
            .boxed(),
            TextScaleReader {
                builds: Rc::clone(&c.scale),
            }
            .boxed(),
            WholeReader {
                builds: Rc::clone(&c.whole),
            }
            .boxed(),
            NonDependent {
                builds: Rc::clone(&c.none),
            }
            .boxed(),
        ])
        .boxed(),
    }
}

fn data(width: f64, scale: f64) -> MediaQueryData {
    MediaQueryData {
        size: flui_foundation::geometry::Size::new(width, 600.0),
        text_scale_factor: scale,
        ..MediaQueryData::default()
    }
}

fn snapshot(c: &Counters) -> [u32; 4] {
    [c.size.get(), c.scale.get(), c.whole.get(), c.none.get()]
}

pub(crate) fn a_size_only_change_rebuilds_size_and_whole_readers_only() {
    let c = Counters {
        size: count(),
        scale: count(),
        whole: count(),
        none: count(),
    };
    let mut laid = lay_out(
        MediaQuery::new(data(800.0, 1.0), subtree(&c)),
        loose(4000.0),
    );
    assert_eq!(
        snapshot(&c),
        [1, 1, 1, 1],
        "every leaf builds once on mount"
    );

    laid.pump_widget(MediaQuery::new(data(1000.0, 1.0), subtree(&c)));

    assert_eq!(
        snapshot(&c),
        [2, 1, 2, 1],
        "size changed: the size reader and the whole-of reader rebuild; \
         the text-scale reader and the non-dependent do not"
    );
}

pub(crate) fn a_build_that_panics_before_reading_keeps_its_dependency() {
    // Reset-on-build must not treat a recovered panic as "read nothing": the
    // element would lose its dependency and never rebuild after the failing
    // condition clears. The recovered build keeps its previous masks.
    let fail = Rc::new(Cell::new(false));
    let builds = count();
    let reader = FailingSizeReader {
        fail: Rc::clone(&fail),
        builds: Rc::clone(&builds),
    };
    let wrap = |reader: &FailingSizeReader| {
        use flui_view::ViewExt;
        StaticChild {
            inner: reader.clone().boxed(),
        }
    };
    let mut laid = lay_out(
        MediaQuery::new(data(800.0, 1.0), wrap(&reader)),
        loose(4000.0),
    );
    assert_eq!(builds.get(), 1);

    // The size change rebuilds it; this build panics before reading size.
    fail.set(true);
    laid.pump_widget(MediaQuery::new(data(900.0, 1.0), wrap(&reader)));
    assert_eq!(builds.get(), 2, "the size change reached the reader");
    let _ = laid.with_build_owner_mut(flui_view::BuildOwner::take_recovered_panics);

    // Condition fixed: the next size change must still rebuild it.
    fail.set(false);
    laid.pump_widget(MediaQuery::new(data(1000.0, 1.0), wrap(&reader)));
    assert_eq!(
        builds.get(),
        3,
        "a recovered build keeps the dependency it had before the panic"
    );
    assert_eq!(
        laid.size(laid.current_root()).width,
        10.0,
        "the reader rendered the new width after recovering"
    );
}
