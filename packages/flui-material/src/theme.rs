//! [`Theme`] — publishes [`ThemeData`] to a subtree via FLUI's
//! inherited-data mechanism.
//!
//! Implements `flui-widgets`' [`InheritedTheme`] trait so a future
//! capture/re-parent mechanism (see that trait's module docs) can wrap a
//! `Theme` the same way it wraps any other ambient theme.

use flui_sdk::view::prelude::*;
use flui_sdk::view::{BoxedView, FieldMask, InheritedData, InheritedView, impl_inherited_view};
use flui_sdk::widgets::{DefaultFocusRingStyle, DefaultSelectionStyle, InheritedTheme};

use crate::theme_data::ThemeData;

/// Provides [`ThemeData`] to its subtree via FLUI's inherited-data mechanism.
///
/// Place a `Theme` near the root of the application subtree to supply a
/// consistent Material visual identity. Any descendant reads the current
/// theme with [`Theme::of`] / [`Theme::maybe_of`].
///
/// # Example
///
/// ```rust
/// use flui_material::{Theme, ThemeData};
/// use flui_sdk::widgets::SizedBox;
///
/// let _themed = Theme::new(ThemeData::dark(), SizedBox::shrink());
/// ```
#[derive(Clone)]
pub struct Theme {
    /// The style data this node provides to descendants.
    data: ThemeData,
    /// The single child subtree this node wraps.
    child: BoxedView,
}

impl Theme {
    /// Wrap `child` in a `Theme` that provides `data` to all descendants,
    /// `data.focus_ring` to their focus rings as the inherited
    /// [`DefaultFocusRingStyle`], and `data.selection` to selection areas and
    /// text fields as the inherited [`DefaultSelectionStyle`].
    #[must_use]
    pub fn new(data: ThemeData, child: impl IntoView) -> Self {
        let (focus_ring, selection) = (data.focus_ring, data.selection);
        Self {
            data,
            child: DefaultFocusRingStyle::new(
                focus_ring,
                DefaultSelectionStyle::new(selection, child),
            )
            .boxed(),
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
    /// This crate has no `ThemeData::fallback` (every FLUI app is expected to
    /// wrap its tree in a `Theme`, so a themeless default has no consumer
    /// yet), so the missing-ancestor case panics — surfacing the mistake
    /// loudly during development.
    #[must_use]
    pub fn of(ctx: &dyn BuildContext) -> ThemeData {
        Self::maybe_of(ctx).expect(
            "Theme::of called with no Theme ancestor in the tree — wrap the subtree in a \
             Theme, or use Theme::maybe_of with a caller-chosen default",
        )
    }

    /// Look up the nearest ancestor [`Theme`]'s data, registering a
    /// dependency. Returns `None` if there is no [`Theme`] ancestor.
    ///
    /// The oracle's `Theme` has no `maybeOf` — its `Theme.of` never returns
    /// `null` (see [`of`](Self::of)'s doc comment on the fallback it uses
    /// instead), so there is no method to port here. `maybe_of` is this
    /// crate's own non-panicking counterpart to [`of`](Self::of)'s panicking
    /// behavior, not a parity port.
    #[must_use]
    pub fn maybe_of(ctx: &dyn BuildContext) -> Option<ThemeData> {
        ctx.depend_on::<Self, _>(|t| t.data.clone())
    }

    /// Depend on selected base fields of the nearest `Theme`.
    ///
    /// Dynamic component entries are not part of this mask. `Theme::of` stays
    /// the whole-theme dependency across both base fields and entries.
    pub fn depend_on_fields<R>(
        ctx: &dyn BuildContext,
        mask: FieldMask<ThemeData>,
        f: impl FnOnce(&ThemeData) -> R,
    ) -> Option<R> {
        ctx.depend_on_field::<Self, _>(mask, |t| f(&t.data))
    }

    /// The color scheme, depending on `color_scheme` only.
    #[must_use]
    pub fn color_scheme_of(ctx: &dyn BuildContext) -> Option<crate::color_scheme::ColorScheme> {
        Self::depend_on_fields(ctx, ThemeData::FIELD_COLOR_SCHEME, |d| d.color_scheme)
    }

    /// The text theme, depending on `text_theme` only.
    #[must_use]
    pub fn text_theme_of(ctx: &dyn BuildContext) -> Option<crate::text_theme::TextTheme> {
        Self::depend_on_fields(ctx, ThemeData::FIELD_TEXT_THEME, |d| d.text_theme.clone())
    }

    /// Clone component `T` from the nearest `Theme`, depending only on that entry.
    ///
    /// Returns `None` when there is no `Theme` ancestor or the nearest one does
    /// not contain `T`. A missing entry never continues into an outer `Theme`.
    #[must_use]
    pub fn component_of<T: crate::ComponentTheme>(ctx: &dyn BuildContext) -> Option<T> {
        ctx.depend_on_entry::<Self, T, _>(|theme| theme.data.component::<T>().cloned())
            .flatten()
    }

    pub(crate) fn color_and_text_of(
        ctx: &dyn BuildContext,
    ) -> Option<(crate::ColorScheme, crate::TextTheme)> {
        let colors = Self::color_scheme_of(ctx)?;
        let text_theme = Self::text_theme_of(ctx)?;
        Some((colors, text_theme))
    }

    pub(crate) fn base_of(ctx: &dyn BuildContext) -> Option<BaseTheme> {
        Self::color_and_text_of(ctx).map(|(color_scheme, text_theme)| BaseTheme {
            color_scheme,
            text_theme,
        })
    }

    pub(crate) fn required_color_scheme_of(ctx: &dyn BuildContext) -> crate::ColorScheme {
        Self::color_scheme_of(ctx).unwrap_or_else(|| Self::of(ctx).color_scheme)
    }

    pub(crate) fn required_color_and_text_of(
        ctx: &dyn BuildContext,
    ) -> (crate::ColorScheme, crate::TextTheme) {
        Self::color_and_text_of(ctx).unwrap_or_else(|| {
            let theme = Self::of(ctx);
            (theme.color_scheme, theme.text_theme)
        })
    }

    pub(crate) fn required_base_of(ctx: &dyn BuildContext) -> BaseTheme {
        Self::base_of(ctx).unwrap_or_else(|| {
            let theme = Self::of(ctx);
            BaseTheme {
                color_scheme: theme.color_scheme,
                text_theme: theme.text_theme,
            }
        })
    }
}

pub(crate) struct BaseTheme {
    pub(crate) color_scheme: crate::ColorScheme,
    pub(crate) text_theme: crate::TextTheme,
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
        // Rebuild descendants whenever any style field changes.
        self.data != old.data
    }

    fn changed_fields(&self, old: &Self) -> FieldMask<ThemeData> {
        self.data.field_mask_diff(&old.data)
    }

    fn changed_entries(&self, old: &Self) -> flui_sdk::view::EntrySet {
        self.data
            .changed_component_types(&old.data)
            .into_iter()
            .fold(flui_sdk::view::EntrySet::NONE, |changed, id| {
                changed.union(flui_sdk::view::EntrySet::from_type_id(id))
            })
    }
}

impl_inherited_view!(Theme);

impl InheritedTheme for Theme {
    fn wrap(&self, _ctx: &dyn BuildContext, child: BoxedView) -> BoxedView {
        Theme::new(self.data.clone(), child).boxed()
    }
}

/// A [`Theme`] that animates to each new [`ThemeData`].
///
/// The colour scheme and text theme interpolate; component themes use their
/// typed interpolation, which switches half way by default. A change mid-flight
/// starts from the theme shown. Under reduced
/// motion the new theme applies on the next frame.
///
/// ```
/// use flui_material::{AnimatedTheme, ThemeData};
/// use flui_sdk::widgets::{MotionBuilder, SizedBox};
/// use std::time::Duration;
///
/// let themed = AnimatedTheme::new(ThemeData::dark(), SizedBox::shrink())
///     .duration(Duration::from_millis(300));
/// # let _ = themed;
/// ```
#[derive(Clone, StatelessView)]
pub struct AnimatedTheme {
    data: ThemeData,
    motion: flui_sdk::animation::MotionSpec,
    child: BoxedView,
}

impl AnimatedTheme {
    /// Publish `data` to `child`, animating later changes with the default
    /// motion.
    pub fn new(data: ThemeData, child: impl IntoView) -> Self {
        Self {
            data,
            motion: flui_sdk::animation::MotionSpec::default(),
            child: child.into_view().boxed(),
        }
    }
}

impl flui_sdk::widgets::MotionBuilder for AnimatedTheme {
    fn motion_mut(&mut self) -> &mut flui_sdk::animation::MotionSpec {
        &mut self.motion
    }
}

impl std::fmt::Debug for AnimatedTheme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimatedTheme")
            .field("motion", &self.motion)
            .finish_non_exhaustive()
    }
}

impl StatelessView for AnimatedTheme {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        use flui_sdk::widgets::MotionBuilder;
        let child = self.child.clone();
        flui_sdk::widgets::AnimatedLerp::new(self.data.clone(), move |data: &ThemeData| {
            Theme::new(data.clone(), child.clone())
        })
        .motion(self.motion.clone())
    }
}
