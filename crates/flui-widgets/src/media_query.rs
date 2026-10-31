//! [`MediaQuery`] and [`MediaQueryData`] — inherited presentation data.
//!
//! ## Implemented subset
//!
//! `size`, `device_pixel_ratio`, `text_sizing`, `font_weight_adjustment`, `view_padding`,
//! `padding`, `view_insets`, `system_gesture_insets`, `platform_brightness`, `high_contrast`,
//! `preferred_locales`, `motion` — presentation and
//! preference fields for layout and theming.
//!
//! ## Deferred (not yet implemented)
//!
//! 24-hour-format preference, accessible navigation, inverted colors, display features, and
//! navigation mode. Text-weight observation
//! is implemented by the UIKit backend; other backends leave it unavailable.
//! These require platform event plumbing (accessibility bridge, IME state)
//! that lives above this layer.

use flui_foundation::geometry::Size;
use flui_foundation::geometry::{EdgeInsets, Edges};
use flui_platform::{Brightness, Locale};
use flui_view::prelude::*;
use flui_view::{BoxedView, FieldMask, InheritedData, InheritedView, impl_inherited_view};
use std::sync::Arc;

/// Ambient logical-screen data provided to descendants by a [`MediaQuery`]
/// ancestor.
///
/// Construct with individual pub fields
/// directly, or start from [`Default`] and override:
///
/// ```rust,ignore
/// use flui_widgets::MediaQueryData;
///
/// let data = MediaQueryData {
///     device_pixel_ratio: 2.0,
///     ..MediaQueryData::default()
/// };
/// ```
///
/// ## Implemented subset
///
/// - [`size`](Self::size)
/// - [`device_pixel_ratio`](Self::device_pixel_ratio)
/// - [`text_sizing`](Self::text_sizing)
/// - [`font_weight_adjustment`](Self::font_weight_adjustment)
/// - [`view_padding`](Self::view_padding)
/// - [`padding`](Self::padding)
/// - [`view_insets`](Self::view_insets)
/// - [`system_gesture_insets`](Self::system_gesture_insets)
/// - [`platform_brightness`](Self::platform_brightness)
/// - [`high_contrast`](Self::high_contrast)
/// - [`motion`](Self::motion)
/// - [`preferred_locales`](Self::preferred_locales)
#[derive(Debug, Clone, PartialEq, flui_view::prelude::InheritedData)]
pub struct MediaQueryData {
    /// Logical size of the current display surface (window or full screen).
    ///
    /// In logical pixels: multiply by [`device_pixel_ratio`](Self::device_pixel_ratio)
    /// to get physical pixels.
    pub size: Size,

    /// Physical pixels per logical pixel (e.g. `2.0` on a Retina display,
    /// `3.0` on some high-DPI phones). Always positive and finite.
    pub device_pixel_ratio: f64,

    /// Numeric sizing authority applied after authored span inheritance.
    /// The nearest provider replaces the outer policy, including when fixed.
    /// Copy the parent data to preserve its policy while overriding another field.
    pub text_sizing: flui_painting::TextSizing,

    /// Signed adjustment applied after authored text-style inheritance. Zero
    /// preserves authored weights. Text shaping bounds adjusted weights to
    /// `1..=1000`, including for explicitly authored nested providers.
    pub font_weight_adjustment: i32,

    /// Safe-area insets reserved by the OS (notch, home indicator, status
    /// bar), whether or not other system UI covers them. Unlike
    /// [`padding`](Self::padding) it is not reduced by
    /// [`view_insets`](Self::view_insets).
    pub view_padding: EdgeInsets,

    /// Insets occupied by system UI that fully obscures part of the window,
    /// such as the software keyboard when it is visible. Unlike
    /// [`padding`](Self::padding), these areas are hidden, not just reserved.
    /// While the keyboard moves, each frame carries its occlusion at that
    /// frame's time.
    pub view_insets: EdgeInsets,

    /// Edges where a system gesture (back, home, the notification shade)
    /// takes precedence over the application's own drag: a horizontal drag or
    /// an edge slider pads itself away from them. Zero where the platform
    /// reports none.
    pub system_gesture_insets: EdgeInsets,

    /// The OS-level light/dark preference as reported by the platform.
    /// An app-level `Theme` (e.g. `flui_material::Theme`) may override this
    /// for its subtree; this field reflects the platform signal only.
    pub platform_brightness: Brightness,

    /// Whether the user requests a higher-contrast palette. An unavailable
    /// platform observation projects to `false`; nested providers may override it.
    pub high_contrast: bool,

    /// Resolved application motion policy. Controllers obey the presentation
    /// clock; this inherited value lets widgets choose a static composition.
    pub motion: flui_animation::MotionPolicy,

    /// Ordered preferred UI languages observed by the host. `None` means
    /// unavailable; an empty list is an observed empty preference list.
    /// Application locale overrides and resource fallback belong to
    /// [`WidgetsApp`](crate::WidgetsApp). Like the other fields, a nested
    /// `MediaQuery` replaces this value; copy parent data to preserve it when
    /// overriding another field.
    pub preferred_locales: Option<Arc<[Locale]>>,
}

impl Default for MediaQueryData {
    fn default() -> Self {
        Self {
            size: Size::new(800.0, 600.0),
            device_pixel_ratio: 1.0,
            text_sizing: flui_painting::TextSizing::fixed(),
            font_weight_adjustment: 0,
            view_padding: EdgeInsets::ZERO,
            view_insets: EdgeInsets::ZERO,
            system_gesture_insets: EdgeInsets::ZERO,
            platform_brightness: Brightness::Light,
            high_contrast: false,
            motion: flui_animation::MotionPolicy::Full,
            preferred_locales: None,
        }
    }
}

impl MediaQueryData {
    /// The part of [`view_padding`](Self::view_padding) that
    /// [`view_insets`](Self::view_insets) do not already cover, per edge: with
    /// the keyboard up over the home-indicator area the bottom value is zero.
    /// App content avoids these areas and [`SafeArea`](crate::SafeArea)
    /// consumes them. Computed, so it cannot disagree with its sources.
    #[must_use]
    pub fn padding(&self) -> EdgeInsets {
        (self.view_padding - self.view_insets).clamp_non_negative()
    }

    /// Mark the bottom view inset as consumed by a descendant that resizes
    /// around it (a scaffold body above the keyboard): the inset becomes zero
    /// and `view_padding.bottom` shrinks by what the inset covered, leaving
    /// [`padding`](Self::padding) unchanged.
    pub fn remove_bottom_view_inset(&mut self) {
        self.view_padding.bottom = (self.view_padding.bottom - self.view_insets.bottom).max(0.0);
        self.view_insets.bottom = 0.0;
    }

    /// Mark the flagged edges of [`padding`](Self::padding) as consumed by a
    /// descendant that reserves them itself (a `SafeArea`, an app bar): their
    /// padding becomes zero and `view_padding` shrinks by what was consumed.
    pub fn remove_padding(&mut self, edges: Edges<bool>) {
        let padding = self.padding();
        if edges.top {
            self.view_padding.top -= padding.top;
        }
        if edges.right {
            self.view_padding.right -= padding.right;
        }
        if edges.bottom {
            self.view_padding.bottom -= padding.bottom;
        }
        if edges.left {
            self.view_padding.left -= padding.left;
        }
    }

    /// [`remove_padding`](Self::remove_padding) for the top edge alone.
    pub fn remove_top_padding(&mut self) {
        self.remove_padding(Edges::new(true, false, false, false));
    }

    /// [`remove_padding`](Self::remove_padding) for the bottom edge alone.
    pub fn remove_bottom_padding(&mut self) {
        self.remove_padding(Edges::new(false, false, true, false));
    }

    /// Make the bottom [`padding`](Self::padding) exactly `bottom`, for a
    /// descendant that reports an area it obstructs itself (a translucent tab
    /// bar), by raising `view_padding.bottom` above the covered inset.
    pub fn set_padding_bottom(&mut self, bottom: f64) {
        self.view_padding.bottom = self.view_insets.bottom + bottom;
    }
}

/// Provides [`MediaQueryData`] to its subtree via FLUI's inherited-data
/// mechanism.
///
/// Place a `MediaQuery` near the root of the application subtree (or wrap the
/// top-level route) and read ambient media information from any descendant
/// with [`MediaQuery::of`].
///
/// ## Bootstrapping
///
/// Data derived from the platform window is platform-specific: the app runner
/// constructs [`MediaQueryData`] and provides it here.
///
/// ## Example
///
/// ```rust,ignore
/// use flui_widgets::{MediaQuery, MediaQueryData, SizedBox};
///
/// MediaQuery::new(
///     MediaQueryData::default(),
///     SizedBox::shrink(),
/// )
/// ```
#[derive(Clone)]
pub struct MediaQuery {
    /// The data this node provides to descendants.
    data: MediaQueryData,
    /// The single child subtree this node wraps.
    child: BoxedView,
}

impl MediaQuery {
    /// Depend on the resolved presentation motion policy.
    pub fn motion_of(ctx: &dyn BuildContext) -> Option<flui_animation::MotionPolicy> {
        Self::depend_on_fields(ctx, MediaQueryData::FIELD_MOTION, |data| data.motion)
    }
    /// Wrap `child` in a `MediaQuery` that provides `data` to all descendants.
    #[must_use]
    pub fn new(data: MediaQueryData, child: impl IntoView) -> Self {
        Self {
            data,
            child: BoxedView::new(child.into_view()),
        }
    }

    /// Access the [`MediaQueryData`] from the nearest ancestor [`MediaQuery`],
    /// registering a dependency so this element rebuilds when the data
    /// changes.
    ///
    /// # Panics
    ///
    /// Panics if there is no [`MediaQuery`] ancestor. Use
    /// [`maybe_of`](Self::maybe_of) for a non-panicking variant.
    #[must_use]
    pub fn of(ctx: &dyn BuildContext) -> MediaQueryData {
        ctx.depend_on::<Self, _>(|mq| mq.data.clone())
            .expect("MediaQuery::of called with no MediaQuery ancestor in the tree")
    }

    /// Look up the nearest ancestor [`MediaQuery`]'s data, registering a
    /// dependency. Returns `None` if there is no [`MediaQuery`] ancestor.
    #[must_use]
    pub fn maybe_of(ctx: &dyn BuildContext) -> Option<MediaQueryData> {
        ctx.depend_on::<Self, _>(|mq| mq.data.clone())
    }

    /// Depend on **one field group** of the nearest `MediaQuery` (issue
    /// #1090): `mask` is one or more `MediaQueryData::FIELD_*` constants, and
    /// this element rebuilds only when a masked field changes. `None` without
    /// a `MediaQuery` ancestor. The `*_of` accessors below are the common
    /// single-field forms.
    pub fn depend_on_fields<R>(
        ctx: &dyn BuildContext,
        mask: FieldMask<MediaQueryData>,
        f: impl FnOnce(&MediaQueryData) -> R,
    ) -> Option<R> {
        ctx.depend_on_field::<Self, _>(mask, |mq| f(&mq.data))
    }

    /// The window size, depending on `size` only.
    #[must_use]
    pub fn size_of(ctx: &dyn BuildContext) -> Option<Size> {
        Self::depend_on_fields(ctx, MediaQueryData::FIELD_SIZE, |d| d.size)
    }

    /// The device pixel ratio, depending on `device_pixel_ratio` only.
    #[must_use]
    pub fn device_pixel_ratio_of(ctx: &dyn BuildContext) -> Option<f64> {
        Self::depend_on_fields(ctx, MediaQueryData::FIELD_DEVICE_PIXEL_RATIO, |d| {
            d.device_pixel_ratio
        })
    }

    /// The nearest numeric sizing policy, depending on `text_sizing` only.
    /// No provider means that the render pipeline's policy is inherited.
    #[must_use]
    pub fn text_sizing_of(ctx: &dyn BuildContext) -> Option<flui_painting::TextSizing> {
        Self::depend_on_fields(ctx, MediaQueryData::FIELD_TEXT_SIZING, |d| {
            d.text_sizing.clone()
        })
    }

    /// The signed weight adjustment, depending only on that media field.
    #[must_use]
    pub fn font_weight_adjustment_of(ctx: &dyn BuildContext) -> Option<i32> {
        Self::depend_on_fields(ctx, MediaQueryData::FIELD_FONT_WEIGHT_ADJUSTMENT, |d| {
            d.font_weight_adjustment
        })
    }

    /// The safe-area padding, depending on `view_padding` and `view_insets` only.
    #[must_use]
    pub fn padding_of(ctx: &dyn BuildContext) -> Option<EdgeInsets> {
        Self::depend_on_fields(
            ctx,
            MediaQueryData::FIELD_VIEW_PADDING | MediaQueryData::FIELD_VIEW_INSETS,
            MediaQueryData::padding,
        )
    }

    /// The view insets (keyboard), depending on `view_insets` only.
    #[must_use]
    pub fn view_insets_of(ctx: &dyn BuildContext) -> Option<EdgeInsets> {
        Self::depend_on_fields(ctx, MediaQueryData::FIELD_VIEW_INSETS, |d| d.view_insets)
    }

    /// The system-gesture edges, depending on `system_gesture_insets` only.
    #[must_use]
    pub fn system_gesture_insets_of(ctx: &dyn BuildContext) -> Option<EdgeInsets> {
        Self::depend_on_fields(ctx, MediaQueryData::FIELD_SYSTEM_GESTURE_INSETS, |d| {
            d.system_gesture_insets
        })
    }

    /// The platform brightness, depending on `platform_brightness` only.
    #[must_use]
    pub fn platform_brightness_of(ctx: &dyn BuildContext) -> Option<Brightness> {
        Self::depend_on_fields(ctx, MediaQueryData::FIELD_PLATFORM_BRIGHTNESS, |d| {
            d.platform_brightness
        })
    }

    /// The contrast preference, depending on `high_contrast` only.
    #[must_use]
    pub fn high_contrast_of(ctx: &dyn BuildContext) -> Option<bool> {
        Self::depend_on_fields(ctx, MediaQueryData::FIELD_HIGH_CONTRAST, |d| {
            d.high_contrast
        })
    }
}

impl std::fmt::Debug for MediaQuery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaQuery")
            .field("data", &self.data)
            .finish_non_exhaustive()
    }
}

impl InheritedView for MediaQuery {
    type Data = MediaQueryData;

    fn data(&self) -> &Self::Data {
        &self.data
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        // Rebuild descendants when any field of the media data changes.
        self.data != old.data
    }

    fn changed_fields(&self, old: &Self) -> FieldMask<MediaQueryData> {
        self.data.field_mask_diff(&old.data)
    }
}

impl_inherited_view!(MediaQuery);
