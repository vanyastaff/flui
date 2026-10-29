//! Layout test for [`Text`] — proves the widget measures real text headlessly
//! through `RenderParagraph` (a non-empty box), and that it composes as a leaf
//! inside other widgets.

use std::cell::RefCell;
use std::rc::Rc;

use crate::common::{lay_out, loose};
use flui_painting::typography::TextStyle;
use flui_widgets::{DefaultTextStyle, MediaQuery, MediaQueryData, Text};

/// An icon's authored sizing policy applies to its box and painted glyph once.
pub(crate) fn icon_text_sizing_keeps_box_and_glyph_together() {
    use flui_foundation::{TextScaleProfile, TextSize, TextSizeRequest};
    use flui_painting::TextSizing;
    use flui_painting::display_list::DrawOp;
    use flui_painting::glyphs::FontRegistry;
    use flui_view::ViewExt;
    use flui_widgets::{Icon, IconData, IconTheme, IconThemeData};

    for (code, theme_size, explicit_size, scaling, side) in [
        (Some(0xe88a), None, None, None, 24.0),
        (Some(0xe88a), Some(18.0), None, Some(false), 18.0),
        (Some(0xe88a), Some(18.0), Some(27.5), Some(false), 27.5),
        (Some(0xe88a), Some(18.0), None, Some(true), 18.0),
        (Some(0xe88a), Some(18.0), Some(27.5), Some(true), 27.5),
        (None, Some(18.0), None, Some(false), 18.0),
        (None, Some(18.0), None, Some(true), 18.0),
        (Some(0xd800), Some(18.0), None, Some(true), 18.0),
    ] {
        let content = || {
            let icon = match code {
                Some(code) => Icon::new(IconData::new(code).with_font_family("Material Icons")),
                None => Icon::none(),
            };
            let icon = match explicit_size {
                Some(size) => icon.size(size),
                None => icon,
            };
            IconTheme::new(
                IconThemeData {
                    size: theme_size,
                    apply_text_scaling: scaling,
                    ..IconThemeData::default()
                },
                crate::media_query_fields::StaticChild {
                    inner: icon.boxed(),
                },
            )
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
        let exact = |answers: [f64; 3]| {
            TextSizing::exact([18.0, 24.0, 27.5].into_iter().zip(answers).map(
                |(authored, answer)| {
                    (
                        TextSizeRequest {
                            size: TextSize::new(authored).expect("icon authored size"),
                            profile: TextScaleProfile::Body,
                        },
                        TextSize::new(answer).expect("resolved icon size"),
                    )
                },
            ))
            .expect("distinct icon requests")
        };
        for tight in [false, true] {
            let constraints = if tight {
                crate::common::tight(40.25, 33.75)
            } else {
                loose(10000.0)
            };
            let mut laid = lay_out(view(TextSizing::fixed()), constraints);
            let icon_id = laid.find_by_render_type("RenderIcon");
            let answer_index = [18.0, 24.0, 27.5]
                .iter()
                .position(|size| *size == side)
                .expect("authored fixture size");
            let mut policies: Vec<_> = [1.0, 2.0, 0.75, 1.0 / 64.0, 64.0, 1.0]
                .into_iter()
                .map(|factor| {
                    (
                        TextSizing::linear(factor).expect("linear icon policy"),
                        side * factor,
                    )
                })
                .collect();
            for answers in [[29.0, 36.0, 43.25], [31.5, 37.0, 49.0]] {
                policies.push((exact(answers), answers[answer_index]));
            }
            policies.push((TextSizing::fixed(), side));
            for (policy, resolved) in policies {
                laid.pump_widget(view(policy));
                let expected = if scaling == Some(true) {
                    resolved
                } else {
                    side
                };
                let box_size = laid.pipeline_owner().with(|owner| {
                    owner
                        .render_tree()
                        .get(laid.root())
                        .expect("icon box")
                        .size()
                        .expect("laid out box")
                });
                // BoxProtocol normalizes constraints to hundredths. The actual
                // raster size must retain the authored fractional value below.
                let allocated = if tight {
                    crate::common::size(40.25, 33.75)
                } else {
                    crate::common::size(expected, expected)
                };
                assert!(
                    (box_size.width - allocated.width).abs() <= 0.005 + f64::EPSILON
                        && (box_size.height - allocated.height).abs() <= 0.005 + f64::EPSILON,
                    "icon box must use resolved size within constraints: actual={box_size:?}, allocated={allocated:?}"
                );
                assert_eq!(laid.find_by_render_type("RenderIcon"), icon_id);
                let mut registry = FontRegistry::new();
                let mut painted = 0;
                for command in laid.draw_ops() {
                    if let DrawOp::Paragraph { paragraph, .. } = command.op {
                        for run in paragraph.runs() {
                            let key = registry.prepare_run(&run).expect("real icon face");
                            for glyph in run.placed_glyphs(key, (0.0, 0.0), 1.0) {
                                assert_ne!(glyph.key.glyph_id(), 0, "the icon must not be tofu");
                                assert_eq!(
                                    f64::from(glyph.key.size()),
                                    expected,
                                    "painted icon retains resolved size under constraints: theme={theme_size:?}, explicit={explicit_size:?}, scaling={scaling:?}, tight={tight}"
                                );
                                painted += 1;
                            }
                        }
                    }
                }
                assert_eq!(
                    painted,
                    usize::from(code.is_some_and(|point| char::from_u32(point).is_some())),
                    "one actual glyph must reach paint; absent or invalid icons only reserve space"
                );
            }
        }
    }
}

/// Only an authored icon description is exposed to assistive technology.
pub(crate) fn icon_labels_reach_the_assembled_accessibility_tree() {
    use flui_testing::a11y::Role;
    use flui_widgets::{Icon, IconData};

    for code in [Some(0xe88a), None, Some(0xd800)] {
        let view = |label: Option<&str>| {
            let icon = match code {
                Some(code) => Icon::new(IconData::new(code).with_font_family("Material Icons")),
                None => Icon::none(),
            };
            match label {
                Some(label) => icon.semantic_label(label),
                None => icon,
            }
        };
        let mut laid = lay_out(view(None), loose(100.0));
        laid.enable_semantics();
        laid.tick();
        let id = laid.find_by_render_type("RenderIcon");
        for label in [None, Some("Open home"), Some("Go home"), None] {
            laid.pump_widget(view(label));
            assert_eq!(laid.find_by_render_type("RenderIcon"), id);
            let tree = laid.a11y_tree().expect("assembled accessibility tree");
            assert!(
                tree.find_by_label("\u{e88a}").is_err(),
                "a codepoint is not an icon description"
            );
            assert!(tree.find_by_label("Open home").is_err() || label == Some("Open home"));
            assert!(tree.find_by_label("Go home").is_err() || label == Some("Go home"));
            let images = tree.find_all(Role::Image);
            match label {
                Some(label) => {
                    assert_eq!(
                        images.len(),
                        1,
                        "labelled icons are images, including absent glyphs"
                    );
                    assert_eq!(images[0].label(), Some(label));
                    assert_eq!(tree.find_all_by_label(label).len(), 1);
                }
                None => assert!(images.is_empty(), "decorative icons are not announced"),
            }
        }
    }
}

/// The system text scale grows each span once, from that span's own size.
///
/// Spans of different sizes, unstyled text, ambient styles and a fixed span
/// sit under the runtime's root policy with no provider of their own. The
/// paragraph's intrinsics agree with its layout under every scale, and
/// clearing the observation restores the authored geometry.
pub(crate) fn the_system_text_scale_grows_each_span_once() {
    use flui_foundation::TextSizingIntent;
    use flui_painting::display_list::DrawOp;
    use flui_painting::glyphs::FontRegistry;
    use flui_painting::typography::TextSpan;
    use flui_platform::SystemPreferences;
    use flui_rendering::storage::IntrinsicDimension;
    use flui_view::ViewExt;
    use flui_widgets::RichText;

    fn fixed(size: f64) -> TextStyle {
        sized(size).with_sizing(TextSizingIntent::Fixed)
    }
    fn sized(size: f64) -> TextStyle {
        TextStyle::default().with_font_size(size)
    }
    let cases = [
        (
            "spans of different sizes, unstyled text and a fixed span",
            RichText::new(TextSpan::with_children(vec![
                TextSpan::new("unstyled "),
                TextSpan::styled("fixed ", fixed(20.0)),
                TextSpan::styled("small ", sized(12.0)),
                TextSpan::styled("large", sized(20.0)),
            ]))
            .boxed(),
            vec![14.0, 20.0, 12.0, 20.0],
            vec![28.0, 20.0, 24.0, 40.0],
        ),
        (
            "an ambient style is scaled by the paragraph, not before it",
            DefaultTextStyle::new(sized(18.0), Text::new("ambient")).boxed(),
            vec![18.0],
            vec![36.0],
        ),
        (
            "an ambient fixed style keeps its subtree at the authored size",
            DefaultTextStyle::new(fixed(18.0), Text::new("ambient")).boxed(),
            vec![18.0],
            vec![18.0],
        ),
    ];
    // Painted glyph sizes span by span: runs come in visual order, and a
    // fallback face can split one span into neighbouring runs of one size.
    let glyph_sizes = |laid: &crate::common::LaidOut| {
        let mut registry = FontRegistry::new();
        let mut sizes = Vec::new();
        for command in laid.draw_ops() {
            let DrawOp::Paragraph { paragraph, .. } = command.op else {
                continue;
            };
            for run in paragraph.runs() {
                let key = registry.prepare_run(&run).expect("real text face");
                sizes.extend(
                    run.placed_glyphs(key, (0.0, 0.0), 1.0)
                        .map(|glyph| glyph.key.size()),
                );
            }
        }
        sizes.dedup();
        sizes
    };
    // Intrinsics and layout must measure the same resolved sizes.
    let geometry = |laid: &crate::common::LaidOut, id| {
        let size = laid.size(id);
        let max_width = laid.intrinsic_dimension(id, IntrinsicDimension::MaxWidth, f64::INFINITY);
        let height = laid.intrinsic_dimension(id, IntrinsicDimension::MinHeight, size.width);
        assert!(
            (max_width - size.width).abs() < 0.01 && (height - size.height).abs() < 0.01,
            "intrinsics measure what layout laid out: size={size:?}, max width={max_width}, height={height}"
        );
        size
    };

    for (case, view, authored, doubled) in cases {
        let mut laid = lay_out(view, loose(10_000.0));
        let id = laid.find_by_render_type("RenderParagraph");
        assert_eq!(glyph_sizes(&laid), authored, "{case}");
        let original = geometry(&laid, id);

        laid.set_system_preferences(
            SystemPreferences::default()
                .with_text_scale(2.0)
                .expect("supported scale"),
        );
        laid.tick();
        assert_eq!(laid.find_by_render_type("RenderParagraph"), id, "{case}");
        assert_eq!(glyph_sizes(&laid), doubled, "{case}");
        let scaled = geometry(&laid, id);
        if authored == doubled {
            assert_eq!(scaled, original, "{case}");
        } else {
            assert!(
                scaled.width > original.width && scaled.height > original.height,
                "{case}: original={original:?}, scaled={scaled:?}"
            );
        }

        laid.set_system_preferences(SystemPreferences::default());
        laid.tick();
        assert_eq!(glyph_sizes(&laid), authored, "{case}");
        assert_eq!(geometry(&laid, id), original, "{case}");
    }
}

/// A preference must reach paragraph layout, not merely a data-reader widget.
pub(crate) fn media_text_scaling_changes_the_laid_out_text() {
    // Invalid policies cannot be authored through a writable scalar field.
    // Finite arithmetic overflow is separately refused by measurement.
    for factor in [0.0, -1.0, f64::INFINITY, f64::NAN] {
        assert!(flui_painting::TextSizing::linear(factor).is_err());
    }
    // Exercise snapshot admission through the actual paragraph consumer. A
    // failed native observation falls back to unknown, never a non-finite size.
    for observed in [f64::MAX, f64::from_bits(1)] {
        let preferences = flui_platform::SystemPreferences::default()
            .with_text_scale(observed)
            .unwrap_or_default();
        let paragraph = lay_out(
            MediaQuery::new(
                MediaQueryData {
                    text_sizing: flui_painting::TextSizing::linear(
                        preferences.text_scale().unwrap_or(1.0),
                    )
                    .expect("admitted preference"),
                    ..MediaQueryData::default()
                },
                Text::new("safe preference").style(TextStyle::default().with_font_size(16.0)),
            ),
            loose(1000.0),
        );
        let baseline = lay_out(
            Text::new("safe preference").style(TextStyle::default().with_font_size(16.0)),
            loose(1000.0),
        );
        assert_eq!(
            paragraph.size(paragraph.root()),
            baseline.size(baseline.root())
        );
    }
    for observed in [1.0 / 64.0, 64.0] {
        let preferences = flui_platform::SystemPreferences::default()
            .with_text_scale(observed)
            .expect("supported scale boundary");
        let paragraph = lay_out(
            MediaQuery::new(
                MediaQueryData {
                    text_sizing: flui_painting::TextSizing::linear(
                        preferences.text_scale().expect("observed scale"),
                    )
                    .expect("admitted preference"),
                    ..MediaQueryData::default()
                },
                Text::new("boundary").style(TextStyle::default().with_font_size(16.0)),
            ),
            loose(100_000.0),
        );
        let size = paragraph.size(paragraph.root());
        assert!(size.width.is_finite() && size.width > 0.0);
        assert!(size.height.is_finite() && size.height > 0.0);
    }
    let text =
        || Text::new("accessibility sizing").style(TextStyle::default().with_font_size(16.0));
    let normal = lay_out(
        MediaQuery::new(MediaQueryData::default(), text()),
        loose(1000.0),
    );
    let enlarged = lay_out(
        MediaQuery::new(
            MediaQueryData {
                text_sizing: flui_painting::TextSizing::linear(2.0).expect("valid test policy"),
                ..MediaQueryData::default()
            },
            text(),
        ),
        loose(1000.0),
    );
    let normal_size = normal.size(normal.root());
    let enlarged_size = enlarged.size(enlarged.root());
    assert!(normal_size.width > 0.0 && normal_size.height > 0.0);
    assert!(
        enlarged_size.width > normal_size.width * 1.5
            && enlarged_size.height > normal_size.height * 1.5,
        "text scaling must enlarge the shaped paragraph: normal={normal_size:?}, enlarged={enlarged_size:?}"
    );
}

// ============================================================================
// DefaultTextStyle (text.dart:55-136, consumed by Text.build :716-765)
// ============================================================================

/// An enclosing `DefaultTextStyle` styles a bare `Text` run: the ambient
/// `font_size` shapes the glyphs, so the box grows with it (`text.dart:720`).
///
/// Red-check: drop the `depend_on::<DefaultTextStyle, _>` read from `Text::build`
/// — both boxes measure identically.
pub(crate) fn an_enclosing_default_text_style_styles_a_bare_run() {
    let bare = lay_out(Text::new("ambient type"), loose(1000.0));
    let styled = lay_out(
        DefaultTextStyle::new(
            TextStyle::default().with_font_size(40.0),
            Text::new("ambient type"),
        ),
        loose(1000.0),
    );

    assert!(
        styled.size(styled.root()).height > bare.size(bare.root()).height,
        "the ambient 40pt style must produce a taller box than the default type"
    );
}

// ============================================================================
// Links
// ============================================================================

/// The bytes of [`linked_paragraph`]'s runs in its plain text.
const PLAIN: std::ops::Range<usize> = 0..6;
const LINK: std::ops::Range<usize> = 6..10;
const HELP: std::ops::Range<usize> = 10..15;

/// `RichText` with one link between plain runs: "plain ", "link" (to
/// `app://terms`), then " help", whose span asks for the help cursor.
fn linked_paragraph(opened: &Rc<RefCell<Vec<String>>>) -> flui_widgets::RichText {
    use flui_painting::typography::TextSpan;
    use flui_platform::CursorIcon;

    let opened = Rc::clone(opened);
    flui_widgets::RichText::new(
        TextSpan::new("plain ")
            .with_child(TextSpan::new("link").with_link("app://terms"))
            .with_child(TextSpan::new(" help").with_mouse_cursor(CursorIcon::Help)),
    )
    .on_link(move |_cx, link| opened.borrow_mut().push(link.to_string()))
}

/// The window rectangles of the paragraph's glyphs for the plain-text bytes
/// `range`, one per line, as the paragraph laid them out last.
fn glyph_boxes(
    laid: &crate::common::LaidOut,
    paragraph: flui_foundation::RenderId,
    range: std::ops::Range<usize>,
) -> Vec<flui_foundation::geometry::Rect<f64>> {
    use flui_objects::RenderParagraph;

    let boxes = laid.pipeline_owner().with(|owner| {
        // The one concrete render object `RichText` mounts, reached through
        // the storage layer's erasure.
        let render = owner
            .render_tree()
            .get(paragraph)
            .and_then(|node| node.as_box())
            .and_then(|node| node.render_object().downcast_ref::<RenderParagraph>())
            .expect("a mounted paragraph");
        render
            .painter()
            .get_boxes_for_selection(range.start, range.end)
    });
    let origin = laid.absolute_offset(paragraph);
    boxes
        .into_iter()
        .map(|text_box| {
            let rect = text_box.rect;
            flui_foundation::geometry::Rect::from_ltwh(
                origin.dx + rect.left(),
                origin.dy + rect.top(),
                rect.width(),
                rect.height(),
            )
        })
        .collect()
}

/// The window position of the middle of the paragraph's glyphs for the
/// plain-text bytes `range`, as the paragraph laid them out last.
fn glyph_center(
    laid: &crate::common::LaidOut,
    paragraph: flui_foundation::RenderId,
    range: std::ops::Range<usize>,
) -> (f64, f64) {
    let center = glyph_boxes(laid, paragraph, range)
        .into_iter()
        .reduce(|bounds, rect| bounds.union(&rect))
        .expect("the range has glyphs")
        .center();
    (center.x, center.y)
}

fn scale_text(laid: &mut crate::common::LaidOut, scale: f64) {
    laid.set_system_preferences(
        flui_platform::SystemPreferences::default()
            .with_text_scale(scale)
            .expect("supported scale"),
    );
    laid.tick();
}

/// A tap on a link's glyphs calls `on_link` with that link; a tap on the
/// text around it calls nothing. Both hold after the system text scale
/// doubles the glyphs, because the paragraph answers from the layout it
/// committed: the link's old position is then inside the enlarged plain run.
///
/// Red-checks: drop the paragraph's hit-test override (no tap reaches
/// `on_link`), or route a link from a layout at another scale (the tap at
/// the link's unscaled position opens it).
pub(crate) fn a_tap_on_a_link_reaches_on_link_at_the_current_text_scale() {
    let opened = Rc::new(RefCell::new(Vec::new()));
    let mut laid = lay_out(linked_paragraph(&opened), loose(10_000.0));
    let paragraph = laid.find_by_render_type("RenderParagraph");
    let tap = |laid: &crate::common::LaidOut, (x, y): (f64, f64)| {
        laid.dispatch_pointer_down(x, y);
        laid.dispatch_pointer_up(x, y);
    };

    let unscaled = glyph_center(&laid, paragraph, LINK);
    tap(&laid, glyph_center(&laid, paragraph, PLAIN));
    assert!(opened.borrow().is_empty(), "plain text is not a link");
    tap(&laid, unscaled);
    assert_eq!(
        *opened.borrow(),
        ["app://terms"],
        "a tap on the link opens it"
    );

    scale_text(&mut laid, 2.0);
    let scaled = glyph_center(&laid, paragraph, LINK);
    assert!(
        scaled.0 > unscaled.0 * 1.5,
        "precondition: the scale moved the link: unscaled={unscaled:?}, scaled={scaled:?}"
    );
    tap(&laid, unscaled);
    assert_eq!(
        opened.borrow().len(),
        1,
        "the link's unscaled position is plain text at twice the size"
    );
    tap(&laid, scaled);
    assert_eq!(
        *opened.borrow(),
        ["app://terms", "app://terms"],
        "a tap on the scaled link opens it"
    );
}

/// The pointer over a link is a pointing hand, over a span that sets a
/// cursor that cursor, and elsewhere the arrow, at a text scale that moves
/// every run from where it would be unscaled.
///
/// Red-check: drop the cursor from the paragraph's hit (every position shows
/// the arrow).
pub(crate) fn a_link_shows_a_pointing_hand_and_a_span_its_own_cursor() {
    use flui_platform::CursorIcon;

    let opened = Rc::new(RefCell::new(Vec::new()));
    let mut laid = lay_out(linked_paragraph(&opened), loose(10_000.0));
    scale_text(&mut laid, 1.5);
    let paragraph = laid.find_by_render_type("RenderParagraph");
    for (range, expected) in [
        (LINK, CursorIcon::Pointer),
        (PLAIN, CursorIcon::Default),
        (HELP, CursorIcon::Help),
        (LINK, CursorIcon::Pointer),
    ] {
        let (x, y) = glyph_center(&laid, paragraph, range.clone());
        laid.dispatch_pointer_hover(x, y);
        assert_eq!(laid.cursor(), expected, "hovering bytes {range:?}");
    }
    assert!(opened.borrow().is_empty(), "hovering opens nothing");
}

/// A paragraph with a handled link publishes the link as a link node that
/// assistive technology clicks to reach `on_link`, between nodes for the
/// text around it, so no text is read twice; each node is bounded by its
/// glyphs at the current text scale. Without a handler the paragraph is one
/// plain label.
///
/// Red-checks: drop the paragraph's semantics regions (no link node), or
/// keep the whole-text label beside them (the link is read twice).
pub(crate) fn a_handled_link_is_a_link_node_assistive_technology_clicks() {
    use flui_painting::typography::TextSpan;
    use flui_testing::a11y::{Action, ActionRequest, NodeId, Role, TreeId};

    let opened = Rc::new(RefCell::new(Vec::new()));
    let mut laid = lay_out(linked_paragraph(&opened), loose(10_000.0));
    laid.enable_semantics();
    laid.pump();
    let link_node = |laid: &crate::common::LaidOut| -> (NodeId, f64) {
        let tree = laid.a11y_tree().expect("semantics enabled");
        let link = tree
            .find_by_role_and_label(Role::Link, "link")
            .unwrap_or_else(|error| panic!("{error}\n{}", tree.describe()));
        assert!(link.supports_action(Action::Click), "{}", tree.describe());
        let plain = tree
            .find_by_label("plain ")
            .unwrap_or_else(|error| panic!("{error}\n{}", tree.describe()));
        tree.find_by_label(" help")
            .unwrap_or_else(|error| panic!("{error}\n{}", tree.describe()));
        assert!(
            tree.find_all_by_label("plain link help").is_empty(),
            "the link is not read twice:\n{}",
            tree.describe()
        );
        let (link_bounds, plain_bounds) = (
            link.bounds().expect("link bounds"),
            plain.bounds().expect("plain bounds"),
        );
        assert!(
            link_bounds.x0 >= plain_bounds.x1 - 0.5,
            "the link follows the text before it: link={link_bounds:?}, plain={plain_bounds:?}"
        );
        (link.id(), link_bounds.width())
    };
    let (_, unscaled) = link_node(&laid);
    scale_text(&mut laid, 2.0);
    let (node, scaled) = link_node(&laid);
    assert!(
        scaled > unscaled * 1.5,
        "the link node is bounded by its scaled glyphs: unscaled={unscaled}, scaled={scaled}"
    );

    laid.invoke_semantics_action(ActionRequest {
        action: Action::Click,
        target_tree: TreeId::ROOT,
        target_node: node,
        data: None,
    })
    .expect("a click on a link node resolves");
    assert_eq!(
        *opened.borrow(),
        ["app://terms"],
        "the click opens the link"
    );

    let mut unhandled = lay_out(
        flui_widgets::RichText::new(
            TextSpan::new("plain ").with_child(TextSpan::new("link").with_link("app://terms")),
        ),
        loose(10_000.0),
    );
    unhandled.enable_semantics();
    unhandled.pump();
    let tree = unhandled.a11y_tree().expect("semantics enabled");
    tree.find_by_label("plain link")
        .unwrap_or_else(|error| panic!("{error}\n{}", tree.describe()));
    assert!(
        tree.find_all(Role::Link).is_empty(),
        "an unhandled link is plain text:\n{}",
        tree.describe()
    );
}

/// Two links between two buttons: "go to ", "first" (`app://first`), " or ",
/// "second link" (`app://second`), and the presses of the button after them.
fn links_between_buttons(
    opened: &Rc<RefCell<Vec<String>>>,
    after: &Rc<std::cell::Cell<u32>>,
    width: f64,
) -> impl flui_view::View {
    use flui_painting::typography::TextSpan;
    use flui_view::ViewExt;
    use flui_widgets::{Column, RawButton, SizedBox};

    let (opened, after) = (Rc::clone(opened), Rc::clone(after));
    Column::new(vec![
        RawButton::new(Text::new("before"))
            .on_press(|_cx| {})
            .boxed(),
        SizedBox::new(width, 100.0)
            .child(
                flui_widgets::RichText::new(
                    TextSpan::new("go to ")
                        .with_child(TextSpan::new("first").with_link("app://first"))
                        .with_child(TextSpan::new(" or "))
                        .with_child(TextSpan::new("second link").with_link("app://second")),
                )
                .on_link(move |_cx, link| opened.borrow_mut().push(link.to_string())),
            )
            .boxed(),
        RawButton::new(Text::new("after"))
            .on_press(move |_cx| after.set(after.get() + 1))
            .boxed(),
    ])
}

/// Press `key` (or Shift+Tab) as the platform delivers it; then a frame.
fn press_key(laid: &mut crate::common::LaidOut, pressed: flui_platform::keyboard::Key) {
    let manager = laid.focus_manager();
    laid.enter_owner_scope(|| crate::keyboard_focus::press(&manager, &pressed));
    laid.pump();
}

fn shift_tab(laid: &mut crate::common::LaidOut) {
    use flui_platform::keyboard::{Key, Modifiers, NamedKey};

    let manager = laid.focus_manager();
    laid.enter_owner_scope(|| {
        crate::keyboard_focus::chord(&manager, &Key::Named(NamedKey::Tab), Modifiers::SHIFT)
    });
    laid.pump();
}

/// Tab stops on each link of a paragraph in reading order between the
/// controls around it, Shift+Tab walks back, and Enter opens the focused
/// link while Space does not (a link is not a button), at a text scale other
/// than 1.0.
///
/// Red-check: drop the per-link `Focus` (Tab goes from the first button to
/// the second, and Enter presses it).
pub(crate) fn tab_stops_on_each_link_and_enter_opens_it() {
    use flui_platform::keyboard::{Key, NamedKey};

    let opened = Rc::new(RefCell::new(Vec::new()));
    let after = Rc::new(std::cell::Cell::new(0));
    let mut laid = lay_out(links_between_buttons(&opened, &after, 400.0), loose(400.0));
    scale_text(&mut laid, 1.5);
    let open = |laid: &mut crate::common::LaidOut| {
        press_key(laid, Key::Named(NamedKey::Enter));
        opened.borrow().last().cloned()
    };

    press_key(&mut laid, Key::Named(NamedKey::Tab));
    press_key(&mut laid, Key::Named(NamedKey::Tab));
    assert_eq!(
        open(&mut laid).as_deref(),
        Some("app://first"),
        "the first link is the stop after the button"
    );
    press_key(&mut laid, Key::character(" "));
    assert_eq!(opened.borrow().len(), 1, "Space does not open a link");
    press_key(&mut laid, Key::Named(NamedKey::Tab));
    assert_eq!(
        open(&mut laid).as_deref(),
        Some("app://second"),
        "then the second link"
    );
    press_key(&mut laid, Key::Named(NamedKey::Tab));
    press_key(&mut laid, Key::Named(NamedKey::Enter));
    assert_eq!(after.get(), 1, "then the button after the paragraph");
    assert_eq!(
        opened.borrow().len(),
        2,
        "Enter on the button opens no link"
    );
    shift_tab(&mut laid);
    assert_eq!(
        open(&mut laid).as_deref(),
        Some("app://second"),
        "Shift+Tab walks back to the second link"
    );
    assert_eq!(opened.borrow().len(), 3);
}

/// A link focused from the keyboard draws the ring just outside each line of
/// its glyphs, at a text scale that moves and wraps them; focus moving on
/// takes the ring away.
///
/// Red-checks: drop the ring from the paragraph's paint (no band), or draw it
/// around the paragraph's box (one band, not one per line).
pub(crate) fn a_focused_link_rings_each_line_of_its_glyphs() {
    use flui_painting::display_list::DrawOp;
    use flui_painting::styling::Color;
    use flui_platform::keyboard::{Key, NamedKey};
    use flui_widgets::{DefaultFocusRingStyle, FocusRingStyle};

    const STYLE: FocusRingStyle = FocusRingStyle {
        outer: Color::rgb(200, 0, 0),
        inner: Color::rgb(0, 200, 0),
        outer_width: 2.0,
        inner_width: 1.0,
    };
    // The inner band of every ring painted, in window coordinates.
    let rings = |laid: &crate::common::LaidOut| -> Vec<flui_foundation::geometry::Rect<f64>> {
        laid.draw_ops()
            .into_iter()
            .filter_map(|command| match command.op {
                DrawOp::DRRect { inner, paint, .. } if paint.color == STYLE.inner => {
                    Some(command.transform.transform_rect(&inner.rect))
                }
                _ => None,
            })
            .collect()
    };
    let opened = Rc::new(RefCell::new(Vec::new()));
    let after = Rc::new(std::cell::Cell::new(0));
    // At 1.5 the words of "second link" do not fit one 90 px line.
    let mut laid = lay_out(
        DefaultFocusRingStyle::new(STYLE, links_between_buttons(&opened, &after, 90.0)),
        loose(400.0),
    );
    scale_text(&mut laid, 1.5);
    let paragraph = laid
        .find_text("go to first or second link")
        .expect("the paragraph with the links");
    press_key(&mut laid, Key::Named(NamedKey::Tab));
    press_key(&mut laid, Key::Named(NamedKey::Tab));
    press_key(&mut laid, Key::Named(NamedKey::Tab));
    let second = glyph_boxes(&laid, paragraph, 15..26);
    assert!(
        second.len() >= 2,
        "precondition: the link wraps: {second:?}"
    );
    let painted = rings(&laid);
    assert_eq!(
        painted.len(),
        second.len(),
        "one ring per line of the focused link: painted={painted:?}, glyphs={second:?}"
    );
    for (ring, glyphs) in painted.iter().zip(&second) {
        for (edge, (got, want)) in [
            ("left", (ring.left(), glyphs.left())),
            ("top", (ring.top(), glyphs.top())),
            ("right", (ring.right(), glyphs.right())),
            ("bottom", (ring.bottom(), glyphs.bottom())),
        ] {
            assert!(
                (got - want).abs() < 0.01,
                "the ring's {edge} edge hugs the glyphs: ring={ring:?}, glyphs={glyphs:?}"
            );
        }
    }

    press_key(&mut laid, Key::Named(NamedKey::Tab));
    assert!(
        rings(&laid).len() <= 1,
        "focus moved on: the link's ring is gone"
    );
    assert!(
        rings(&laid)
            .iter()
            .all(|ring| second.iter().all(|glyphs| ring != glyphs)),
        "no ring is left around the link"
    );
}

/// Assistive technology finds each link focusable, focuses one through its
/// node, sees it focused, and Enter then opens it.
///
/// Red-check: drop the Focus action from the link node (the request is
/// refused and Enter opens nothing).
pub(crate) fn assistive_technology_focuses_a_link() {
    use flui_platform::keyboard::{Key, NamedKey};
    use flui_testing::a11y::{Action, ActionRequest, Role, TreeId};

    let opened = Rc::new(RefCell::new(Vec::new()));
    let after = Rc::new(std::cell::Cell::new(0));
    let mut laid = lay_out(links_between_buttons(&opened, &after, 400.0), loose(400.0));
    laid.enable_semantics();
    laid.pump();
    let tree = laid.a11y_tree().expect("semantics enabled");
    let second = tree
        .find_by_role_and_label(Role::Link, "second link")
        .unwrap_or_else(|error| panic!("{error}\n{}", tree.describe()));
    assert!(second.supports_action(Action::Focus), "{}", tree.describe());
    let node = second.id();

    laid.invoke_semantics_action(ActionRequest {
        action: Action::Focus,
        target_tree: TreeId::ROOT,
        target_node: node,
        data: None,
    })
    .expect("a focus request on a link node resolves");
    laid.pump();
    let tree = laid.a11y_tree().expect("semantics enabled");
    let focused = tree
        .focus()
        .map(|focused| focused.label().map(str::to_owned));
    assert_eq!(
        focused,
        Some(Some("second link".to_owned())),
        "the link node holds the focus:\n{}",
        tree.describe()
    );
    press_key(&mut laid, Key::Named(NamedKey::Enter));
    assert_eq!(
        *opened.borrow(),
        ["app://second"],
        "Enter opens the focused link"
    );
}

/// Tab to a link far down a paragraph taller than its scroll view scrolls
/// the link's glyphs, not the paragraph's top, into view, at a text scale
/// other than 1.0.
///
/// Red-check: measure a link's focus node by the paragraph's box (the
/// paragraph's top is already visible, so nothing scrolls).
pub(crate) fn tab_to_a_link_below_the_view_scrolls_it_into_view() {
    use flui_painting::typography::TextSpan;
    use flui_platform::keyboard::{Key, NamedKey};
    use flui_widgets::{RichText, Scrollable, SizedBox};

    const VIEWPORT: f64 = 120.0;
    let filler = "words to scroll past ".repeat(40);
    let bottom = {
        let start = "start top".len() + filler.len();
        start..start + "bottom".len()
    };
    let mut laid = lay_out(
        SizedBox::new(200.0, VIEWPORT).child(
            Scrollable::new().child(
                RichText::new(
                    TextSpan::new("start ")
                        .with_child(TextSpan::new("top").with_link("app://top"))
                        .with_child(TextSpan::new(filler))
                        .with_child(TextSpan::new("bottom").with_link("app://bottom")),
                )
                .on_link(|_cx, _link| {}),
            ),
        ),
        loose(400.0),
    );
    scale_text(&mut laid, 1.5);
    let paragraph = laid.find_by_render_type("RenderParagraph");
    let below = glyph_boxes(&laid, paragraph, bottom.clone());
    assert!(
        below.iter().all(|glyphs| glyphs.top() > VIEWPORT),
        "precondition: the last link starts below the view: {below:?}"
    );

    press_key(&mut laid, Key::Named(NamedKey::Tab));
    press_key(&mut laid, Key::Named(NamedKey::Tab));
    laid.pump_for(std::time::Duration::from_secs(2));
    let shown = glyph_boxes(&laid, paragraph, bottom);
    assert!(
        shown
            .iter()
            .all(|glyphs| glyphs.top() >= -0.5 && glyphs.bottom() <= VIEWPORT + 0.5),
        "the focused link is in view: {shown:?}"
    );
}

/// A link whose only text is a child's own link links nothing: Tab skips it
/// and Enter never opens it. A textless span's semantics label is still read
/// beside the routed link inside it.
///
/// Red-checks: count every span that sets a link (Tab stops on the outer
/// link first and Enter opens it), or drop a stretch with no glyphs (the
/// label "Read " is not published).
pub(crate) fn a_shadowed_link_takes_no_stop_and_a_textless_label_is_still_read() {
    use flui_painting::typography::TextSpan;
    use flui_platform::keyboard::{Key, NamedKey};
    use flui_testing::a11y::Role;
    use flui_view::ViewExt;
    use flui_widgets::{Column, RawButton, RichText};

    let opened = Rc::new(RefCell::new(Vec::new()));
    let after = Rc::new(std::cell::Cell::new(0));
    let (record, pressed) = (Rc::clone(&opened), Rc::clone(&after));
    let mut laid = lay_out(
        Column::new(vec![
            RawButton::new(Text::new("before"))
                .on_press(|_cx| {})
                .boxed(),
            RichText::new(
                TextSpan::with_children(vec![TextSpan::new("inner").with_link("app://inner")])
                    .with_link("app://outer"),
            )
            .on_link(move |_cx, link| record.borrow_mut().push(link.to_string()))
            .boxed(),
            RichText::new(
                TextSpan::with_children(vec![TextSpan::new("terms").with_link("app://terms")])
                    .with_semantics_label("Read "),
            )
            .on_link(|_cx, _link| {})
            .boxed(),
            RawButton::new(Text::new("after"))
                .on_press(move |_cx| pressed.set(pressed.get() + 1))
                .boxed(),
        ]),
        loose(400.0),
    );
    laid.enable_semantics();
    laid.pump();
    let tree = laid.a11y_tree().expect("semantics enabled");
    tree.find_by_label("Read ")
        .unwrap_or_else(|error| panic!("the textless span's label: {error}\n{}", tree.describe()));
    tree.find_by_role_and_label(Role::Link, "terms")
        .unwrap_or_else(|error| panic!("{error}\n{}", tree.describe()));

    press_key(&mut laid, Key::Named(NamedKey::Tab));
    press_key(&mut laid, Key::Named(NamedKey::Tab));
    press_key(&mut laid, Key::Named(NamedKey::Enter));
    assert_eq!(
        *opened.borrow(),
        ["app://inner"],
        "the first stop after the button is the link with text"
    );
    press_key(&mut laid, Key::Named(NamedKey::Tab));
    press_key(&mut laid, Key::Named(NamedKey::Tab));
    press_key(&mut laid, Key::Named(NamedKey::Enter));
    assert_eq!(after.get(), 1, "then the terms link, then the button after");
    assert_eq!(opened.borrow().len(), 1, "the shadowed link never opened");
}

/// A focused link inside a button opens on Select as on Enter
/// (`ActivateLinkIntent`, ADR-0079's amendment for links), and Space neither
/// opens it nor presses the button around it.
///
/// Red-checks: drop the paragraph's Select binding (Select opens nothing),
/// or the link's declined `ActivateIntent` (Space's release presses the
/// button).
pub(crate) fn select_opens_a_link_and_space_activates_nothing_around_it() {
    use flui_painting::typography::TextSpan;
    use flui_platform::keyboard::{Key, NamedKey};
    use flui_widgets::{RawButton, RichText};

    let opened = Rc::new(RefCell::new(Vec::new()));
    let pressed = Rc::new(std::cell::Cell::new(0));
    let (record, count) = (Rc::clone(&opened), Rc::clone(&pressed));
    let mut laid = lay_out(
        RawButton::new(
            RichText::new(
                TextSpan::new("see ").with_child(TextSpan::new("help").with_link("app://help")),
            )
            .on_link(move |_cx, link| record.borrow_mut().push(link.to_string())),
        )
        .on_press(move |_cx| count.set(count.get() + 1)),
        loose(400.0),
    );
    laid.pump();
    // The button, then the link inside it.
    press_key(&mut laid, Key::Named(NamedKey::Tab));
    press_key(&mut laid, Key::Named(NamedKey::Tab));
    press_key(&mut laid, Key::character(" "));
    assert_eq!(pressed.get(), 0, "Space on the link presses no button");
    assert!(opened.borrow().is_empty(), "Space opens no link");
    press_key(&mut laid, Key::Named(NamedKey::Select));
    assert_eq!(*opened.borrow(), ["app://help"], "Select opens the link");
    press_key(&mut laid, Key::Named(NamedKey::Enter));
    assert_eq!(opened.borrow().len(), 2, "and so does Enter");
    assert_eq!(pressed.get(), 0, "the button is never pressed");
}

/// A link that `max_lines` cut is no Tab stop: it shows nothing to focus,
/// ring or announce. The link the line keeps still is one.
///
/// Red-check: leave every link focusable (Tab stops on the cut link and
/// Enter opens it).
pub(crate) fn a_link_the_layout_cut_takes_no_focus() {
    use flui_painting::typography::TextSpan;
    use flui_platform::keyboard::{Key, NamedKey};
    use flui_view::ViewExt;
    use flui_widgets::{Column, RawButton, RichText, SizedBox};

    let opened = Rc::new(RefCell::new(Vec::new()));
    let after = Rc::new(std::cell::Cell::new(0));
    let (record, pressed) = (Rc::clone(&opened), Rc::clone(&after));
    let mut laid = lay_out(
        Column::new(vec![
            RawButton::new(Text::new("before"))
                .on_press(|_cx| {})
                .boxed(),
            SizedBox::new(80.0, 100.0)
                .child(
                    RichText::new(
                        TextSpan::new("")
                            .with_child(TextSpan::new("shown").with_link("app://shown"))
                            .with_child(TextSpan::new(" and a long run of words before "))
                            .with_child(TextSpan::new("cut").with_link("app://cut")),
                    )
                    .max_lines(1)
                    .on_link(move |_cx, link| record.borrow_mut().push(link.to_string())),
                )
                .boxed(),
            RawButton::new(Text::new("after"))
                .on_press(move |_cx| pressed.set(pressed.get() + 1))
                .boxed(),
        ]),
        loose(400.0),
    );
    // The paragraph's first layout reports which links it shows.
    laid.pump();

    press_key(&mut laid, Key::Named(NamedKey::Tab));
    press_key(&mut laid, Key::Named(NamedKey::Tab));
    press_key(&mut laid, Key::Named(NamedKey::Enter));
    assert_eq!(*opened.borrow(), ["app://shown"], "the kept link is a stop");
    press_key(&mut laid, Key::Named(NamedKey::Tab));
    press_key(&mut laid, Key::Named(NamedKey::Enter));
    assert_eq!(after.get(), 1, "the next stop is the button after");
    assert_eq!(opened.borrow().len(), 1, "the cut link never opened");
}

/// `RichText` whose links are `names`, each linking `app://<name>`, between
/// single spaces.
fn named_links(opened: &Rc<RefCell<Vec<String>>>, names: &[&str]) -> flui_widgets::RichText {
    use flui_painting::typography::TextSpan;

    let mut spans = Vec::new();
    for (position, name) in names.iter().enumerate() {
        if position > 0 {
            spans.push(TextSpan::new(" "));
        }
        spans.push(TextSpan::new(*name).with_link(format!("app://{name}")));
    }
    let opened = Rc::clone(opened);
    flui_widgets::RichText::new(TextSpan::with_children(spans))
        .on_link(move |_cx, link| opened.borrow_mut().push(link.to_string()))
}

/// A link is the same link across rebuilds by what it links to: a link
/// added before the focused one, or removed from before it, leaves the focus
/// on it, and a press on a link that spans such a rebuild opens that link.
///
/// Red-check: match links to their focus nodes and recognizers by their
/// place in the text (Enter opens the link now at the focused place, and the
/// press opens the link that took the pressed one's place).
pub(crate) fn links_keep_their_focus_and_presses_when_links_before_them_change() {
    use flui_platform::keyboard::{Key, NamedKey};

    let opened = Rc::new(RefCell::new(Vec::new()));
    let last = || opened.borrow().last().cloned();
    let mut laid = lay_out(named_links(&opened, &["terms", "privacy"]), loose(400.0));
    laid.pump();
    press_key(&mut laid, Key::Named(NamedKey::Tab));
    press_key(&mut laid, Key::Named(NamedKey::Tab));

    laid.pump_widget(named_links(&opened, &["cookies", "terms", "privacy"]));
    laid.pump();
    press_key(&mut laid, Key::Named(NamedKey::Enter));
    assert_eq!(
        last().as_deref(),
        Some("app://privacy"),
        "a link added before"
    );

    laid.pump_widget(named_links(&opened, &["terms", "privacy"]));
    laid.pump();
    press_key(&mut laid, Key::Named(NamedKey::Enter));
    assert_eq!(
        last().as_deref(),
        Some("app://privacy"),
        "a link removed before"
    );

    let paragraph = laid.find_by_render_type("RenderParagraph");
    let (x, y) = glyph_center(&laid, paragraph, 0.."terms".len());
    laid.dispatch_pointer_down(x, y);
    laid.pump_widget(named_links(&opened, &["cookies", "terms", "privacy"]));
    laid.dispatch_pointer_up(x, y);
    assert_eq!(
        last().as_deref(),
        Some("app://terms"),
        "the press opens the link it began on"
    );
}

/// Unmounting a paragraph with links releases what its links registered in
/// the owner lane, with the handler they hold, and a press on a link that
/// spans the unmount opens nothing.
///
/// Red-check: leave the links' lane registrations behind at unmount (the
/// handler outlives the paragraph).
pub(crate) fn unmounting_a_linked_paragraph_releases_its_links() {
    use flui_widgets::SizedBox;

    let captured = Rc::new(());
    let witness = Rc::downgrade(&captured);
    let opened = Rc::new(RefCell::new(Vec::new()));
    let record = Rc::clone(&opened);
    let mut laid = lay_out(
        flui_widgets::RichText::new(
            flui_painting::typography::TextSpan::new("read ").with_child(
                flui_painting::typography::TextSpan::new("more").with_link("app://more"),
            ),
        )
        .on_link(move |_cx, link| {
            std::hint::black_box(&captured);
            record.borrow_mut().push(link.to_string());
        }),
        loose(400.0),
    );
    laid.pump();
    let paragraph = laid.find_by_render_type("RenderParagraph");
    let (x, y) = glyph_center(&laid, paragraph, 5..9);
    laid.dispatch_pointer_down(x, y);
    laid.pump_widget(SizedBox::shrink());
    laid.dispatch_pointer_up(x, y);
    laid.pump();
    assert!(
        opened.borrow().is_empty(),
        "the press in flight opened nothing"
    );
    assert!(
        witness.upgrade().is_none(),
        "the links' handler leaves the owner lane with the paragraph"
    );
}
