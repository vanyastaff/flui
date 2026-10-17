//! `Scaffold` widget-level integration coverage — mounts a real scaffold
//! through the full render pipeline (`tests/common/mod.rs`), proving the
//! inset contract documented in `scaffold.rs`'s module docs: the app bar's
//! MEASURED height (not a re-added `padding.top`) sets `content_top`, and the
//! floating action button is positioned from `content_bottom` (which already
//! accounts for the keyboard), never from the scaffold's raw height.
//!
//! Slot ordering: `Scaffold::build` pushes `LayoutId`s in `body`, `app_bar`,
//! `floating_action_button` order (whichever are present) — see
//! `scaffold.rs`. Each test below indexes `laid.child(layout_root, n)`
//! against exactly that order for the slots it configures.

use crate::common;

use common::{lay_out, offset, size, tight};
use flui_material::{AppBar, Card, ListTile, MaterialApp, Scaffold, Theme, ThemeData};
use flui_painting::DrawOp;
use flui_painting::glyphs::FontRegistry;
use flui_sdk::geometry::EdgeInsets;
use flui_sdk::painting::Color;
use flui_sdk::view::ViewExt;
use flui_sdk::widgets::{Column, MediaQuery, MediaQueryData, SizedBox, Text};

/// The render-tree node for the scaffold's `CustomMultiChildLayout`.
///
/// The shared harness adds the production focus anchor outside the mounted
/// view, so this is resolved by render type instead of assuming a fixed depth
/// below the presentation root.
fn layout_root(laid: &common::LaidOut) -> flui_sdk::foundation::RenderId {
    laid.try_find_by_render_type("RenderCustomMultiChildLayoutBox")
        .expect("Scaffold must mount exactly one CustomMultiChildLayout")
}

pub fn body_is_positioned_below_the_app_bar_with_no_padding() {
    let laid = lay_out(
        Theme::new(
            ThemeData::light(),
            MediaQuery::new(
                MediaQueryData::default(),
                Scaffold::new()
                    .app_bar(AppBar::new().title(Text::new("Title")))
                    .body(SizedBox::new(10.0, 10.0)),
            ),
        ),
        tight(400.0, 800.0),
    );

    let layout = layout_root(&laid);
    let body = laid.child(layout, 0);
    let app_bar = laid.child(layout, 1);

    assert_eq!(
        laid.size(app_bar).height,
        56.0,
        "with no MediaQuery padding, the app bar's measured height is exactly toolbar_height",
    );
    assert_eq!(
        laid.offset(body),
        offset(0.0, 56.0),
        "the body must start exactly at the app bar's measured height, with no extra padding",
    );
}

pub fn floating_action_button_floats_above_the_keyboard() {
    let media_query = MediaQueryData {
        view_insets: EdgeInsets::new(0.0, 0.0, 300.0, 0.0),
        ..MediaQueryData::default()
    };
    let laid = lay_out(
        Theme::new(
            ThemeData::light(),
            MediaQuery::new(
                media_query,
                Scaffold::new()
                    .body(SizedBox::new(10.0, 10.0))
                    .floating_action_button(SizedBox::new(56.0, 56.0)),
            ),
        ),
        tight(400.0, 800.0),
    );

    let layout = layout_root(&laid);
    let fab = laid.child(layout, 1);

    assert_eq!(laid.size(fab), size(56.0, 56.0));
    // content_bottom = scaffold_height(800) - min_insets.bottom(300) = 500.
    // x = width(400) - margin(16) - min_insets.right(0) - fab_width(56) = 328.
    // y = content_bottom(500) - fab_height(56) - margin(16) = 428.
    assert_eq!(
        laid.offset(fab),
        offset(328.0, 428.0),
        "the FAB must be positioned from content_bottom (which already subtracts the \
         keyboard height), never from the scaffold's raw size.height — a raw-height \
         computation would place it at y = 800 - 56 - 16 = 728, under the keyboard",
    );
}

/// The colour and glyph size `text`'s paragraph was painted with.
fn painted_text_style(laid: &common::LaidOut, text: &str) -> (Color, f32) {
    let mut registry = FontRegistry::new();
    for command in laid.draw_ops() {
        let DrawOp::Paragraph {
            paragraph, color, ..
        } = command.op
        else {
            continue;
        };
        if paragraph.text() != text {
            continue;
        }
        let run = paragraph.runs().next().expect("painted run");
        let key = registry.prepare_run(&run).expect("painted face");
        let glyph = run
            .placed_glyphs(key, (0.0, 0.0), 1.0)
            .next()
            .expect("painted glyph");
        return (glyph.color.unwrap_or(color), glyph.key.size());
    }
    panic!("`{text}` was never painted");
}

/// A `Scaffold` body's plain `Text` paints in the theme's body style instead
/// of `MaterialApp`'s loud fallback, a nested `Material` (a `Card`) gives its
/// text the same style, and a component that styles its own text (a
/// `ListTile` title) still wins over the surface's style.
pub fn plain_body_text_takes_the_theme_body_style_and_components_keep_theirs() {
    let body_color = Color::rgb(12, 34, 56);
    let mut theme = ThemeData::light();
    let roles = &mut theme.text_theme;
    roles.body_medium = roles
        .body_medium
        .clone()
        .map(|style| style.with_color(body_color).with_font_size(17.0));
    roles.body_large = roles
        .body_large
        .clone()
        .map(|style| style.with_font_size(23.0));
    let on_surface = theme.color_scheme.on_surface;

    let laid = lay_out(
        MaterialApp::new(Scaffold::new().body(Column::new(vec![
            Text::new("plain body").boxed(),
            Card::new(Text::new("card body")).boxed(),
            ListTile::new().title(Text::new("tile title")).boxed(),
        ])))
        .theme(theme),
        tight(400.0, 800.0),
    );

    for text in ["plain body", "card body"] {
        assert_eq!(
            painted_text_style(&laid, text),
            (body_color, 17.0),
            "`{text}` must paint in the theme's body_medium, not the app's fallback style"
        );
    }
    assert_eq!(
        painted_text_style(&laid, "tile title"),
        (on_surface, 23.0),
        "a ListTile title keeps its own body_large/on_surface style inside a Material"
    );
}
