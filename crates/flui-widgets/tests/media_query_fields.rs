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
        SizedBox::new(size.width.0 / 100.0, 1.0)
    }
}

/// Reads `size` while `reads` is true and nothing from `MediaQuery` otherwise.
#[derive(Clone, StatelessView)]
struct DroppingReader {
    reads: Rc<Cell<bool>>,
    builds: Count,
}

impl StatelessView for DroppingReader {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        self.builds.set(self.builds.get() + 1);
        if self.reads.get() {
            let size = MediaQuery::size_of(ctx).expect("MediaQuery ancestor");
            SizedBox::new(size.width.0 / 100.0, 1.0)
        } else {
            SizedBox::new(1.0, 1.0)
        }
    }
}

/// Reads `size` only in `init_state` / `did_change_dependencies` (the
/// `FocusState` / `DraggableState` pattern) and never in `build`.
#[derive(Clone, StatefulView)]
struct LifecycleReader {
    dependency_changes: Count,
    builds: Count,
}

struct LifecycleReaderState {
    view: LifecycleReader,
    width: f32,
}

impl StatefulView for LifecycleReader {
    type State = LifecycleReaderState;
    fn create_state(&self) -> Self::State {
        LifecycleReaderState {
            view: self.clone(),
            width: 0.0,
        }
    }
}

impl ViewState<LifecycleReader> for LifecycleReaderState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        self.width = MediaQuery::size_of(ctx)
            .expect("MediaQuery ancestor")
            .width
            .0;
    }

    fn did_change_dependencies(&mut self, ctx: &dyn BuildContext) {
        let dc = &self.view.dependency_changes;
        dc.set(dc.get() + 1);
        self.width = MediaQuery::size_of(ctx)
            .expect("MediaQuery ancestor")
            .width
            .0;
    }

    fn build(&self, _view: &LifecycleReader, _ctx: &dyn BuildContext) -> impl IntoView {
        self.view.builds.set(self.view.builds.get() + 1);
        SizedBox::new(self.width / 100.0, 1.0)
    }
}

/// Reads `size` through `depend_on_fields` with an EMPTY mask.
#[derive(Clone, StatelessView)]
struct EmptyMaskReader {
    builds: Count,
}

impl StatelessView for EmptyMaskReader {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        self.builds.set(self.builds.get() + 1);
        let width =
            MediaQuery::depend_on_fields(ctx, flui_view::FieldMask::NONE, |d| d.size.width.0)
                .expect("MediaQuery ancestor");
        SizedBox::new(width / 100.0, 1.0)
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

#[test]
fn a_build_that_panics_before_reading_keeps_its_dependency() {
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
    let _ = laid.build_owner_mut().take_recovered_panics();

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
        px(10.0),
        "the reader rendered the new width after recovering"
    );
}

#[test]
fn a_build_that_stops_reading_the_provider_is_pruned_from_its_dependents() {
    // Reset-on-build prune: an element whose latest build read nothing from a
    // provider leaves that provider's dependents map (and the reverse index),
    // and no provider change rebuilds it any more.
    let reads = Rc::new(Cell::new(true));
    let builds = count();
    let reader = DroppingReader {
        reads: Rc::clone(&reads),
        builds: Rc::clone(&builds),
    };
    let wrap = |reader: &DroppingReader| {
        use flui_view::ViewExt;
        StaticChild {
            inner: reader.clone().boxed(),
        }
    };
    let mut laid = lay_out(
        MediaQuery::new(data(800.0, 1.0), wrap(&reader)),
        loose(4000.0),
    );
    assert_eq!(laid.inherited_dependent_count::<MediaQuery>(), 1);

    // The size change rebuilds it; this build reads nothing from MediaQuery.
    reads.set(false);
    laid.pump_widget(MediaQuery::new(data(900.0, 1.0), wrap(&reader)));
    assert_eq!(builds.get(), 2, "the size change reached the reader");
    assert_eq!(
        laid.inherited_dependent_count::<MediaQuery>(),
        0,
        "the entry whose mask stayed NONE was pruned"
    );

    // Neither field rebuilds it now.
    laid.pump_widget(MediaQuery::new(data(1000.0, 1.5), wrap(&reader)));
    assert_eq!(builds.get(), 2, "a pruned element is not notified");
}

#[test]
fn a_dependency_acquired_in_a_lifecycle_hook_survives_a_rebuild_that_does_not_reread_it() {
    // Reset-on-build re-derives only what `build` reads. A state that reads a
    // provider in `init_state` / `did_change_dependencies` and not in `build`
    // (FocusState, DraggableState) must stay subscribed across a rebuild from
    // another cause, or it silently stops receiving `did_change_dependencies`.
    let dependency_changes = count();
    let builds = count();
    let reader = LifecycleReader {
        dependency_changes: Rc::clone(&dependency_changes),
        builds: Rc::clone(&builds),
    };
    // No StaticChild boundary: an equal-data provider swap rebuilds the
    // reader through the parent path (a rebuild that reads nothing).
    let mut laid = lay_out(
        MediaQuery::new(data(800.0, 1.0), reader.clone()),
        loose(4000.0),
    );
    let mounted_builds = builds.get();
    let mounted_changes = dependency_changes.get();

    laid.pump_widget(MediaQuery::new(data(800.0, 1.0), reader.clone()));
    assert!(
        builds.get() > mounted_builds,
        "test setup: the parent swap must rebuild the reader"
    );
    assert_eq!(
        laid.inherited_dependent_count::<MediaQuery>(),
        1,
        "the lifecycle read survives a build that did not re-read it"
    );

    laid.pump_widget(MediaQuery::new(data(900.0, 1.0), reader));
    assert_eq!(
        dependency_changes.get(),
        mounted_changes + 1,
        "a size change still reaches did_change_dependencies"
    );
    assert_eq!(
        laid.size(laid.current_root()).width,
        px(9.0),
        "the reader rendered the width it re-read"
    );
}

#[test]
fn an_empty_mask_read_is_promoted_to_a_whole_provider_dependency() {
    // `depend_on_field(FieldMask::NONE, ..)` reads the provider; recording the
    // empty set would never intersect an update and leave the value stale, so
    // the read is promoted to the whole-provider dependency.
    let builds = count();
    let reader = EmptyMaskReader {
        builds: Rc::clone(&builds),
    };
    let wrap = |reader: &EmptyMaskReader| {
        use flui_view::ViewExt;
        StaticChild {
            inner: reader.clone().boxed(),
        }
    };
    let mut laid = lay_out(
        MediaQuery::new(data(800.0, 1.0), wrap(&reader)),
        loose(4000.0),
    );
    assert_eq!(laid.inherited_dependent_count::<MediaQuery>(), 1);

    laid.pump_widget(MediaQuery::new(data(900.0, 1.0), wrap(&reader)));
    assert_eq!(
        builds.get(),
        2,
        "the size change rebuilt the empty-mask reader"
    );
    assert_eq!(
        laid.size(laid.current_root()).width,
        px(9.0),
        "and it rendered the fresh value"
    );
}
