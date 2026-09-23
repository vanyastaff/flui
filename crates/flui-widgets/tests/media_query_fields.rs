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
use flui_geometry::{EdgeInsets, px};
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
        SizedBox::new(size.width.0 / 100.0, 1.0)
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

/// Reads `size` while `read_size` is true, `text_scale_factor` otherwise;
/// the switch is flipped from outside between builds.
#[derive(Clone, StatelessView)]
struct SwitchingReader {
    read_size: Rc<Cell<bool>>,
    builds: Count,
}

impl StatelessView for SwitchingReader {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        self.builds.set(self.builds.get() + 1);
        let side = if self.read_size.get() {
            MediaQuery::size_of(ctx)
                .expect("MediaQuery ancestor")
                .width
                .0
                / 100.0
        } else {
            MediaQuery::text_scale_factor_of(ctx).expect("MediaQuery ancestor")
        };
        SizedBox::new(side, 1.0)
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

fn data(width: f32, scale: f32) -> MediaQueryData {
    MediaQueryData {
        size: flui_types::Size::new(px(width), px(600.0)),
        text_scale_factor: scale,
        ..MediaQueryData::default()
    }
}

fn snapshot(c: &Counters) -> [u32; 4] {
    [c.size.get(), c.scale.get(), c.whole.get(), c.none.get()]
}

#[test]
fn a_size_only_change_rebuilds_size_and_whole_readers_only() {
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

#[test]
fn a_text_scale_only_change_rebuilds_scale_and_whole_readers_only() {
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

    laid.pump_widget(MediaQuery::new(data(800.0, 1.5), subtree(&c)));

    assert_eq!(
        snapshot(&c),
        [1, 2, 2, 1],
        "text scale changed: the scale reader and the whole-of reader rebuild; \
         the size reader and the non-dependent do not"
    );
}

#[test]
fn a_field_reader_still_rebuilds_when_its_own_field_changes_after_an_unrelated_one() {
    // Stale-read regression: an earlier unrelated change must not make the
    // registry forget the field a reader depends on.
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

    laid.pump_widget(MediaQuery::new(data(800.0, 1.5), subtree(&c)));
    assert_eq!(c.size.get(), 1, "size reader untouched by a scale change");

    laid.pump_widget(MediaQuery::new(data(900.0, 1.5), subtree(&c)));
    assert_eq!(
        c.size.get(),
        2,
        "size reader rebuilds when size finally changes"
    );
    assert_eq!(c.scale.get(), 2, "scale reader untouched by a size change");
    // The render root is the Column itself: MediaQuery and StaticChild have no
    // render object.
    let column = laid.current_root();
    assert_eq!(
        laid.size(laid.child(column, 0)).width,
        px(9.0),
        "the size reader rendered the new width (900 / 100)"
    );
}

#[test]
fn an_unchanged_provider_swap_rebuilds_nobody() {
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

    laid.pump_widget(MediaQuery::new(data(800.0, 1.0), subtree(&c)));

    assert_eq!(snapshot(&c), [1, 1, 1, 1], "equal data changes no field");
}

#[test]
fn padding_and_insets_masks_are_distinct_fields() {
    let a = MediaQueryData {
        padding: EdgeInsets::all(px(8.0)),
        ..MediaQueryData::default()
    };
    let b = MediaQueryData {
        view_insets: EdgeInsets::all(px(16.0)),
        ..a.clone()
    };
    let changed = flui_view::InheritedData::field_mask_diff(&a, &b);
    assert!(changed.intersects(MediaQueryData::FIELD_VIEW_INSETS));
    assert!(!changed.intersects(MediaQueryData::FIELD_PADDING));
    assert!(!changed.intersects(MediaQueryData::FIELD_SIZE));
}

#[test]
fn a_rebuild_re_derives_the_field_set_so_a_dropped_read_stops_depending() {
    // Reset-on-build (ADR-0074 §5.5 mapping decision): the fields an element
    // is recorded as reading are those of its LATEST build, not the union of
    // every build since mount (Flutter accumulates `_dependencies` until
    // unmount). Read `size` in the first build and `text_scale_factor` in the
    // second: a later size-only change must not rebuild the element.
    let read_size = Rc::new(Cell::new(true));
    let builds = count();
    let reader = SwitchingReader {
        read_size: Rc::clone(&read_size),
        builds: Rc::clone(&builds),
    };
    let wrap = |reader: &SwitchingReader| {
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

    // Second build, triggered by the size it read; it now reads only the scale.
    read_size.set(false);
    laid.pump_widget(MediaQuery::new(data(900.0, 1.0), wrap(&reader)));
    assert_eq!(builds.get(), 2, "the size change rebuilt the reader");

    // Size-only change: the last build did not read size, so no rebuild.
    laid.pump_widget(MediaQuery::new(data(1000.0, 1.0), wrap(&reader)));
    assert_eq!(
        builds.get(),
        2,
        "a field read only in an earlier build no longer rebuilds the element"
    );

    // The field it does read still does.
    laid.pump_widget(MediaQuery::new(data(1000.0, 1.5), wrap(&reader)));
    assert_eq!(builds.get(), 3, "a scale change rebuilds the reader");
    assert_eq!(
        laid.size(laid.current_root()).width,
        px(1.5),
        "the reader rendered the new scale"
    );
}
