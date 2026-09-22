//! Issue #1090 acceptance, `Theme` half: many consumers split by slot; changing
//! one slot rebuilds only that slot's consumers plus the whole-theme
//! consumers. Same `StaticChild` boundary as `rebuild_exactness`.

use std::cell::Cell;
use std::rc::Rc;

use crate::common::{lay_out, loose};
use flui_material::{Theme, ThemeData};
use flui_view::element::ElementKind;
use flui_view::prelude::*;
use flui_view::{BoxedView, ProxyView, View};
use flui_widgets::{Column, SizedBox};

type Count = Rc<Cell<u32>>;

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
struct TextReader {
    builds: Count,
}

impl StatelessView for TextReader {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        self.builds.set(self.builds.get() + 1);
        let _ = Theme::text_theme_of(ctx).expect("Theme ancestor");
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
        &*self.inner.0
    }
}

const PER_KIND: usize = 50;

struct Counters {
    color: Vec<Count>,
    text: Vec<Count>,
    whole: Vec<Count>,
}

fn counters() -> Counters {
    let mk = || {
        (0..PER_KIND)
            .map(|_| Rc::new(Cell::new(0)))
            .collect::<Vec<_>>()
    };
    Counters {
        color: mk(),
        text: mk(),
        whole: mk(),
    }
}

fn subtree(c: &Counters) -> StaticChild {
    use flui_view::ViewExt;
    let mut children: Vec<BoxedView> = Vec::new();
    for i in 0..PER_KIND {
        children.push(
            ColorReader {
                builds: Rc::clone(&c.color[i]),
            }
            .boxed(),
        );
        children.push(
            TextReader {
                builds: Rc::clone(&c.text[i]),
            }
            .boxed(),
        );
        children.push(
            WholeReader {
                builds: Rc::clone(&c.whole[i]),
            }
            .boxed(),
        );
    }
    StaticChild {
        inner: Column::new(children).boxed(),
    }
}

fn total(counts: &[Count]) -> u32 {
    counts.iter().map(|c| c.get()).sum()
}

#[test]
fn changing_one_theme_slot_rebuilds_that_slots_readers_and_whole_theme_readers_only() {
    let c = counters();
    let base = ThemeData::light();
    let mut laid = lay_out(Theme::new(base.clone(), subtree(&c)), loose(4000.0));
    assert_eq!(total(&c.color), PER_KIND as u32);
    assert_eq!(total(&c.text), PER_KIND as u32);
    assert_eq!(total(&c.whole), PER_KIND as u32);

    // Only the color scheme changes.
    let mut recolored = base.clone();
    recolored.color_scheme = ThemeData::dark().color_scheme;
    assert_ne!(
        recolored.color_scheme, base.color_scheme,
        "test setup: the slot must differ"
    );
    assert_eq!(
        recolored.text_theme, base.text_theme,
        "test setup: text theme untouched"
    );
    laid.pump_widget(Theme::new(recolored, subtree(&c)));

    assert_eq!(
        total(&c.color),
        2 * PER_KIND as u32,
        "color-scheme readers rebuilt"
    );
    assert_eq!(
        total(&c.whole),
        2 * PER_KIND as u32,
        "whole-theme readers rebuilt"
    );
    assert_eq!(
        total(&c.text),
        PER_KIND as u32,
        "text-theme readers did NOT rebuild"
    );
}

#[test]
fn the_whole_theme_reader_rebuilds_for_any_slot_and_slot_masks_are_distinct() {
    let base = ThemeData::light();
    let mut other = base.clone();
    other.text_theme = ThemeData::dark().text_theme;
    let changed = flui_view::InheritedData::field_mask_diff(&base, &other);
    assert!(changed.intersects(ThemeData::FIELD_TEXT_THEME));
    assert!(!changed.intersects(ThemeData::FIELD_COLOR_SCHEME));
    assert!(
        flui_view::FieldMask::ALL.intersects(changed),
        "a whole-type dependent sees every change"
    );
}
