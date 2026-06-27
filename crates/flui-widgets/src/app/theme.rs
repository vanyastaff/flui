//! [`Theme`] and [`ThemeData`] — ambient visual-style data.
//!
//! Flutter parity: `material/theme.dart` (`Theme`) and
//! `material/theme_data.dart` (`ThemeData`).
//!
//! ## Implemented subset
//!
//! `brightness`, `primary_color`, `background_color`, `body_text_style` —
//! the minimal set needed for an app to render coherently in light/dark mode
//! with a branded primary color and legible body text.
//!
//! ## Deferred (not yet implemented)
//!
//! Full `ColorScheme` (Material color roles), complete `textTheme` (15 type
//! roles), component themes (`ButtonThemeData`, `InputDecorationTheme`, …),
//! `iconTheme`, `extensions`, `useMaterial3`, `platform`. These require the
//! Material Design component layer which has not yet landed.

use flui_types::platform::Brightness;
use flui_types::styling::Color;
use flui_types::typography::TextStyle;
use flui_view::prelude::*;
use flui_view::{BoxedView, InheritedView, impl_inherited_view};

/// Visual-style configuration provided to descendants by a [`Theme`] ancestor.
///
/// Mirrors Flutter's `ThemeData`. Two ready-made presets are available:
/// [`ThemeData::light()`] and [`ThemeData::dark()`]. Construct a custom
/// value directly via the pub fields:
///
/// ```rust,ignore
/// use flui_widgets::ThemeData;
/// use flui_types::styling::Color;
///
/// let data = ThemeData {
///     primary_color: Color::rgb(0, 120, 212),
///     ..ThemeData::light()
/// };
/// ```
///
/// ## Implemented subset
///
/// | Field | Flutter equivalent |
/// |---|---|
/// | [`brightness`](Self::brightness) | `ThemeData.brightness` |
/// | [`primary_color`](Self::primary_color) | `ThemeData.primaryColor` |
/// | [`background_color`](Self::background_color) | `ThemeData.colorScheme.surface` (approximation) |
/// | [`body_text_style`](Self::body_text_style) | `ThemeData.textTheme.bodyMedium` |
///
/// ## Deferred (not yet implemented)
///
/// Full `ColorScheme` (Material color roles), complete `textTheme` (15 type
/// roles), component themes (`ButtonThemeData`, `InputDecorationTheme`, etc.),
/// `iconTheme`, `extensions`, `useMaterial3`.
#[derive(Debug, Clone, PartialEq)]
pub struct ThemeData {
    /// Whether this is a light or dark theme.
    pub brightness: Brightness,

    /// The primary brand color used for interactive elements (button fills,
    /// focused outlines, active indicators, etc.).
    pub primary_color: Color,

    /// Default surface/background color for scaffolds, dialogs, and cards.
    pub background_color: Color,

    /// Default text style for body content (paragraph text, item labels).
    /// Widgets that render text without an explicit style inherit this.
    pub body_text_style: TextStyle,
}

impl ThemeData {
    /// A light theme with Flutter-inspired Material defaults.
    ///
    /// - `brightness`: [`Brightness::Light`]
    /// - `primary_color`: Material Purple 600 (`#6200EE`)
    /// - `background_color`: White (`#FFFFFF`)
    /// - `body_text_style`: default (color unset — inherits from the
    ///   platform; black on most systems)
    #[must_use]
    pub fn light() -> Self {
        Self {
            brightness: Brightness::Light,
            primary_color: Color::rgb(98, 0, 238), // Material Purple 600
            background_color: Color::rgb(255, 255, 255), // White
            body_text_style: TextStyle::default(),
        }
    }

    /// A dark theme with Flutter-inspired Material defaults.
    ///
    /// - `brightness`: [`Brightness::Dark`]
    /// - `primary_color`: Material Purple 200 (`#BB86FC`)
    /// - `background_color`: Near-black (`#121212`)
    /// - `body_text_style`: white text color for legibility on dark surfaces
    #[must_use]
    pub fn dark() -> Self {
        Self {
            brightness: Brightness::Dark,
            primary_color: Color::rgb(187, 134, 252), // Material Purple 200
            background_color: Color::rgb(18, 18, 18), // Near-black (#121212)
            body_text_style: TextStyle {
                color: Some(Color::rgb(255, 255, 255)),
                ..TextStyle::default()
            },
        }
    }
}

/// Provides [`ThemeData`] to its subtree via FLUI's inherited-data mechanism.
///
/// Place a `Theme` near the root of the application subtree to supply a
/// consistent visual identity. Any descendant reads the current theme with
/// [`Theme::of`].
///
/// ## Flutter parity
///
/// Mirrors Flutter's `Theme` inherited widget (`material/theme.dart`).
/// `ThemeData.of` (deprecated in Flutter) and `Theme.of` are both covered by
/// [`Theme::of`] here.
///
/// ## Example
///
/// ```rust,ignore
/// use flui_widgets::{Theme, ThemeData, Text};
///
/// Theme::new(ThemeData::dark(), Text::new("Hello"))
/// ```
#[derive(Clone)]
pub struct Theme {
    /// The style data this node provides to descendants.
    data: ThemeData,
    /// The single child subtree this node wraps.
    child: BoxedView,
}

impl Theme {
    /// Wrap `child` in a `Theme` that provides `data` to all descendants.
    #[must_use]
    pub fn new(data: ThemeData, child: impl IntoView) -> Self {
        Self {
            data,
            child: BoxedView(Box::new(child.into_view())),
        }
    }

    /// Access the [`ThemeData`] from the nearest ancestor [`Theme`],
    /// registering a dependency so this element rebuilds when the theme
    /// changes.
    ///
    /// # Panics
    ///
    /// Panics if there is no [`Theme`] ancestor. Use
    /// [`maybe_of`](Self::maybe_of) for a non-panicking variant.
    ///
    /// Flutter parity: `Theme.of(context)`.
    #[must_use]
    pub fn of(ctx: &dyn BuildContext) -> ThemeData {
        ctx.depend_on::<Self, _>(|t| t.data.clone())
            .expect("Theme::of called with no Theme ancestor in the tree")
    }

    /// Look up the nearest ancestor [`Theme`]'s data, registering a
    /// dependency. Returns `None` if there is no [`Theme`] ancestor.
    ///
    /// Flutter parity: `Theme.maybeOf(context)`.
    #[must_use]
    pub fn maybe_of(ctx: &dyn BuildContext) -> Option<ThemeData> {
        ctx.depend_on::<Self, _>(|t| t.data.clone())
    }
}

impl std::fmt::Debug for Theme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Theme")
            .field("data", &self.data)
            .finish_non_exhaustive()
    }
}

impl InheritedView for Theme {
    type Data = ThemeData;

    fn data(&self) -> &Self::Data {
        &self.data
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        // Rebuild descendants whenever any style field changes — same contract
        // as Flutter's `ThemeData.==`.
        self.data != old.data
    }
}

impl_inherited_view!(Theme);
