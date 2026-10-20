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
        let _ = MediaQuery::text_sizing_of(ctx).expect("MediaQuery ancestor");
        SizedBox::square(1.0)
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
pub(crate) struct StaticChild {
    pub(crate) inner: BoxedView,
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

#[derive(Clone, StatelessView)]
struct CopiedMediaText;

impl StatelessView for CopiedMediaText {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        use flui_painting::typography::TextStyle;
        use flui_view::ViewExt;
        use flui_widgets::Text;

        let mut copied = MediaQuery::of(ctx);
        copied.size.width = 600.0;
        MediaQuery::new(
            copied,
            StaticChild {
                inner: Text::new("retained paragraph")
                    .style(TextStyle::default().with_font_size(16.0))
                    .boxed(),
            },
        )
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
        text_sizing: flui_painting::TextSizing::linear(scale).expect("valid test policy"),
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

/// Provider updates must reach retained text through its inherited dependency.
pub(crate) fn a_text_scale_change_relayouts_a_preserved_text_subtree() {
    use flui_painting::typography::TextStyle;
    use flui_view::ViewExt;
    use flui_widgets::Text;

    let child = || StaticChild {
        inner: Text::new("retained paragraph")
            .style(TextStyle::default().with_font_size(16.0))
            .boxed(),
    };
    let mut laid = lay_out(MediaQuery::new(data(800.0, 1.0), child()), loose(4000.0));
    let original = laid.size(laid.root());
    laid.pump_widget(MediaQuery::new(data(800.0, 2.0), child()));
    let enlarged = laid.size(laid.root());
    assert!(
        enlarged.height > original.height * 1.5,
        "a scale update must re-layout retained text: original={original:?}, enlarged={enlarged:?}"
    );
    laid.pump_widget(MediaQuery::new(data(800.0, 1.0), child()));
    assert_eq!(
        laid.size(laid.root()),
        original,
        "restoring the preference must resolve from the original authored size"
    );

    let overridden_child = || StaticChild {
        inner: MediaQuery::new(data(800.0, 1.5), child()).boxed(),
    };
    let mut overridden = lay_out(
        MediaQuery::new(data(800.0, 1.0), overridden_child()),
        loose(4000.0),
    );
    let override_size = overridden.size(overridden.root());
    assert!(override_size.height > original.height);
    overridden.pump_widget(MediaQuery::new(data(800.0, 2.0), overridden_child()));
    assert_eq!(
        overridden.size(overridden.root()),
        override_size,
        "an outer preference update must preserve the subtree's explicit sizing"
    );

    use flui_foundation::{TextScaleProfile, TextSize, TextSizeRequest};
    use flui_painting::glyphs::FontRegistry;
    use flui_painting::{DrawOp, TextSizing};

    let exact = |answer| {
        TextSizing::exact([(
            TextSizeRequest {
                size: TextSize::new(16.0).expect("authored size"),
                profile: TextScaleProfile::Body,
            },
            TextSize::new(answer).expect("resolved size"),
        )])
        .expect("one exact answer")
    };
    // A full copied provider retains authority. An explicit Fixed provider
    // resets it, including when the outer policy has exact native answers.
    for nested in 0..3 {
        let content = || match nested {
            0 => child().boxed(),
            1 => CopiedMediaText.boxed(),
            _ => MediaQuery::new(MediaQueryData::default(), child()).boxed(),
        };
        let view = |text_sizing| {
            MediaQuery::new(
                MediaQueryData {
                    text_sizing,
                    ..MediaQueryData::default()
                },
                content(),
            )
        };
        let mut laid = lay_out(view(exact(24.0)), loose(4000.0));
        let id = laid
            .find_text("retained paragraph")
            .expect("actual paragraph");
        for (policy, answer) in [
            (exact(24.0), 24.0),
            (exact(48.0), 48.0),
            (TextSizing::linear(2.0).expect("linear policy"), 32.0),
            (TextSizing::fixed(), 16.0),
        ] {
            laid.pump_widget(view(policy));
            assert_eq!(laid.find_text("retained paragraph"), Some(id));
            let expected = if nested == 2 { 16.0 } else { answer };
            let mut painted = 0;
            let mut registry = FontRegistry::new();
            for command in laid.draw_ops() {
                let DrawOp::Paragraph { paragraph, .. } = command.op else {
                    continue;
                };
                assert_eq!(paragraph.text(), "retained paragraph");
                for run in paragraph.runs() {
                    let key = registry.prepare_run(&run).expect("real text face");
                    for glyph in run.placed_glyphs(key, (0.0, 0.0), 1.0) {
                        assert_eq!(
                            f64::from(glyph.key.size()),
                            expected,
                            "provider mode {nested}"
                        );
                        painted += 1;
                    }
                }
            }
            assert!(painted > 0, "policy must reach painted glyphs");
            if expected == 16.0 {
                assert_eq!(laid.size(id), original, "fixed resolves authored geometry");
            } else {
                assert!(
                    laid.size(id).height > original.height,
                    "policy changes layout"
                );
            }
        }
        laid.pump_widget(
            Text::new("retained paragraph").style(TextStyle::default().with_font_size(16.0)),
        );
        let unscoped = laid
            .find_text("retained paragraph")
            .expect("unscoped paragraph");
        assert_eq!(
            laid.size(unscoped),
            original,
            "removing the provider restores authored sizing"
        );
    }
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

fn keyboard_over_home_indicator(keyboard_bottom: f64) -> MediaQueryData {
    use flui_foundation::geometry::EdgeInsets;

    MediaQueryData {
        view_padding: EdgeInsets::new(0.0, 0.0, 34.0, 0.0),
        view_insets: EdgeInsets::new(0.0, 0.0, keyboard_bottom, 0.0),
        ..MediaQueryData::default()
    }
}

/// A literal built from `view_padding` and `view_insets` reports the derived
/// padding, and it never goes negative.
pub(crate) fn padding_is_derived_from_view_padding_and_view_insets() {
    use flui_foundation::geometry::EdgeInsets;

    let data = MediaQueryData {
        view_padding: EdgeInsets::new(47.0, 0.0, 34.0, 0.0),
        view_insets: EdgeInsets::new(0.0, 0.0, 20.0, 0.0),
        ..MediaQueryData::default()
    };
    assert_eq!(data.padding(), EdgeInsets::new(47.0, 0.0, 14.0, 0.0));

    let covered = MediaQueryData {
        view_insets: EdgeInsets::new(0.0, 0.0, 300.0, 0.0),
        ..data
    };
    assert_eq!(covered.padding().bottom, 0.0, "clamped, not negative");
}

/// Consuming or overriding padding keeps `padding()` equal to what the
/// consumer meant.
pub(crate) fn consuming_and_overriding_padding_updates_its_sources() {
    let mut data = keyboard_over_home_indicator(20.0);
    data.remove_bottom_padding();
    assert_eq!(data.padding().bottom, 0.0);
    assert_eq!(data.view_padding.bottom, 20.0, "the covered part stays");

    data.set_padding_bottom(50.0);
    assert_eq!(data.padding().bottom, 50.0);
}

/// A keyboard that covers the home-indicator area must leave nothing for a
/// `SafeArea` to reserve: `padding` is the part of `view_padding` the
/// `view_insets` do not already cover.
pub(crate) fn safe_area_reserves_the_home_indicator_only_while_the_keyboard_is_down() {
    use flui_widgets::SafeArea;

    let safe_area = || SafeArea::new().child(SizedBox::new(10.0, 10.0));
    let mut laid = lay_out(
        MediaQuery::new(keyboard_over_home_indicator(0.0), safe_area()),
        loose(4000.0),
    );
    assert_eq!(
        laid.size(laid.current_root()).height,
        44.0,
        "without a keyboard the home indicator is reserved"
    );

    laid.pump_widget(MediaQuery::new(
        keyboard_over_home_indicator(300.0),
        safe_area(),
    ));
    assert_eq!(
        laid.size(laid.current_root()).height,
        10.0,
        "a 300px keyboard covers the 34px home indicator, so SafeArea adds nothing above it"
    );

    laid.pump_widget(MediaQuery::new(
        keyboard_over_home_indicator(20.0),
        safe_area(),
    ));
    assert_eq!(
        laid.size(laid.current_root()).height,
        24.0,
        "a keyboard shorter than the indicator leaves only the uncovered 14px"
    );
}

/// Consuming the bottom view inset keeps all three fields consistent.
pub(crate) fn removing_the_bottom_view_inset_shrinks_view_padding_by_what_it_covered() {
    let mut data = keyboard_over_home_indicator(20.0);
    assert_eq!(data.padding().bottom, 14.0);
    data.remove_bottom_view_inset();
    assert_eq!(data.view_insets.bottom, 0.0);
    assert_eq!(data.view_padding.bottom, 14.0);
    assert_eq!(data.padding().bottom, 14.0);

    let mut covered = keyboard_over_home_indicator(300.0);
    covered.remove_bottom_view_inset();
    assert_eq!(covered.view_padding.bottom, 0.0);
    assert_eq!(covered.padding().bottom, 0.0);
}
