//! [`DefaultTextStyle`] — the ambient text style descendant [`Text`](crate::Text) runs merge
//! with their own.
//!
//! # Flutter parity
//!
//! `.flutter/packages/flutter/lib/src/widgets/text.dart`, master
//! `3.33.0-0.0.pre-6280-g88e87cd963f`: `DefaultTextStyle` (`:55-73`) and the
//! `Text.build` consumption contract (`:716-765`): the ambient style merges
//! **under** the run's own style, and `textAlign` / `maxLines` fall back to the
//! ambient values when the run sets none.
//!
//! # Not ported, and named
//!
//! * `softWrap`, `overflow`, `textWidthBasis`, `textHeightBehavior` — FLUI's
//!   `Text` has no counterpart properties yet; they join when it grows them.
//! * `DefaultTextStyle.merge` (`:120-136`), the wrapper that layers a partial
//!   style over the enclosing scope's — a convenience over `of(context)`, which
//!   FLUI expresses as a plain `depend_on` read; add it when a caller exists.
//! * `TextStyle.inherit` — FLUI's `TextStyle` fields are all optional and unset
//!   fields always inherit via `merge`, so Flutter's `inherit: false` opt-out
//!   (`text.dart:718-720`) has no analogue: a run that wants no inheritance
//!   sets every field it cares about.

use flui_types::typography::{TextAlign, TextStyle};
use flui_view::impl_inherited_view;
use flui_view::prelude::*;

/// The text style to apply to descendant [`Text`](crate::Text) runs that do not
/// set their own. Flutter's `DefaultTextStyle` (`text.dart:55`).
///
/// A run's own [`style`](crate::Text::style) is merged **over** this one, field
/// by field; [`text_align`](Self::text_align) and [`max_lines`](Self::max_lines)
/// apply only when the run sets none. Without an enclosing `DefaultTextStyle`,
/// `Text` behaves as if this were empty (Flutter's `DefaultTextStyle.fallback`,
/// `:81-88`).
///
/// # Examples
///
/// ```rust
/// # use flui_widgets::prelude::*;
/// # use flui_types::typography::TextStyle;
/// let _ = DefaultTextStyle::new(
///     TextStyle::default().with_font_size(20.0),
///     Text::new("inherits twenty-point type"),
/// );
/// ```
#[derive(Clone)]
pub struct DefaultTextStyle {
    style: TextStyle,
    text_align: Option<TextAlign>,
    max_lines: Option<u32>,
    child: BoxedView,
}

impl DefaultTextStyle {
    /// Provide `style` to every descendant `Text` under `child`.
    pub fn new(style: TextStyle, child: impl IntoView) -> Self {
        Self {
            style,
            text_align: None,
            max_lines: None,
            child: BoxedView(Box::new(child.into_view())),
        }
    }

    /// The alignment for descendant runs that set none (`text.dart:66`, consumed
    /// at `:757`).
    #[must_use]
    pub fn text_align(mut self, text_align: TextAlign) -> Self {
        self.text_align = Some(text_align);
        self
    }

    /// The line cap for descendant runs that set none (`text.dart:69`, consumed
    /// at `:765`).
    #[must_use]
    pub fn max_lines(mut self, max_lines: u32) -> Self {
        self.max_lines = Some(max_lines);
        self
    }

    /// What a descendant `Text` reads: the ambient style and fallbacks.
    pub(crate) fn ambient(&self) -> (TextStyle, Option<TextAlign>, Option<u32>) {
        (self.style.clone(), self.text_align, self.max_lines)
    }
}

impl std::fmt::Debug for DefaultTextStyle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DefaultTextStyle")
            .field("style", &self.style)
            .field("text_align", &self.text_align)
            .field("max_lines", &self.max_lines)
            .finish_non_exhaustive()
    }
}

impl InheritedView for DefaultTextStyle {
    type Data = TextStyle;

    fn data(&self) -> &Self::Data {
        &self.style
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        // `text.dart:196-202`, minus the properties FLUI's `Text` lacks.
        self.style != old.style
            || self.text_align != old.text_align
            || self.max_lines != old.max_lines
    }
}

impl_inherited_view!(DefaultTextStyle);
