//! [`MediaQuery`] and [`MediaQueryData`] — inherited presentation data.
//!
//! ## Implemented subset
//!
//! `size`, `device_pixel_ratio`, `text_scale_factor`, `font_weight_adjustment`, `padding`,
//! `view_insets`, `platform_brightness`, `high_contrast`, `preferred_locales` — presentation and
//! preference fields for layout and theming.
//!
//! ## Deferred (not yet implemented)
//!
//! View padding, system gesture insets, 24-hour-format preference,
//! accessible navigation, inverted colors, disabled
//! animations, display features, and navigation mode. Text-weight observation
//! is implemented by the UIKit backend; other backends leave it unavailable.
//! These require platform event plumbing (accessibility bridge, IME state)
//! that lives above this layer.

use flui_foundation::geometry::EdgeInsets;
use flui_foundation::geometry::Size;
use flui_platform_api::{Brightness, Locale};
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
/// - [`text_scale_factor`](Self::text_scale_factor) (a flat `f64`, not a scaler object)
/// - [`font_weight_adjustment`](Self::font_weight_adjustment)
/// - [`padding`](Self::padding)
/// - [`view_insets`](Self::view_insets)
/// - [`platform_brightness`](Self::platform_brightness)
/// - [`high_contrast`](Self::high_contrast)
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

    /// User-configured font scaling factor. `1.0` is the system default;
    /// values above `1.0` enlarge text for accessibility.
    /// Text consumers resolve values outside `1/64..=64` to `1.0` through
    /// [`MediaQuery::text_scale_factor_of`], including NaN and infinities.
    pub text_scale_factor: f64,

    /// Signed adjustment applied after authored text-style inheritance. Zero
    /// preserves authored weights. Text shaping bounds adjusted weights to
    /// `1..=1000`, including for explicitly authored nested providers.
    pub font_weight_adjustment: i32,

    /// Safe-area insets from the window edges reserved by the OS (notch,
    /// home indicator, status bar). App content should avoid rendering
    /// interactive or critical elements in these areas.
    pub padding: EdgeInsets,

    /// Insets occupied by system UI that fully obscures part of the window,
    /// such as the software keyboard when it is visible. Unlike
    /// [`padding`](Self::padding), these areas are hidden — not just reserved.
    pub view_insets: EdgeInsets,

    /// The OS-level light/dark preference as reported by the platform.
    /// An app-level `Theme` (e.g. `flui_material::Theme`) may override this
    /// for its subtree; this field reflects the platform signal only.
    pub platform_brightness: Brightness,

    /// Whether the user requests a higher-contrast palette. An unavailable
    /// platform observation projects to `false`; nested providers may override it.
    pub high_contrast: bool,

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
            text_scale_factor: 1.0,
            font_weight_adjustment: 0,
            padding: EdgeInsets::ZERO,
            view_insets: EdgeInsets::ZERO,
            platform_brightness: Brightness::Light,
            high_contrast: false,
            preferred_locales: None,
        }
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
    /// Wrap `child` in a `MediaQuery` that provides `data` to all descendants.
    #[must_use]
    pub fn new(data: MediaQueryData, child: impl IntoView) -> Self {
        Self {
            data,
            child: BoxedView(Box::new(child.into_view())),
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

    /// The effective text scale factor, depending on `text_scale_factor` only.
    /// Values outside `1/64..=64` fall back to `1.0`, matching the supported
    /// system-preference range even for directly authored nested providers.
    #[must_use]
    pub fn text_scale_factor_of(ctx: &dyn BuildContext) -> Option<f64> {
        Self::depend_on_fields(ctx, MediaQueryData::FIELD_TEXT_SCALE_FACTOR, |d| {
            if (1.0 / 64.0..=64.0).contains(&d.text_scale_factor) {
                d.text_scale_factor
            } else {
                1.0
            }
        })
    }

    /// The signed weight adjustment, depending only on that media field.
    #[must_use]
    pub fn font_weight_adjustment_of(ctx: &dyn BuildContext) -> Option<i32> {
        Self::depend_on_fields(ctx, MediaQueryData::FIELD_FONT_WEIGHT_ADJUSTMENT, |d| {
            d.font_weight_adjustment
        })
    }

    /// The safe-area padding, depending on `padding` only.
    #[must_use]
    pub fn padding_of(ctx: &dyn BuildContext) -> Option<EdgeInsets> {
        Self::depend_on_fields(ctx, MediaQueryData::FIELD_PADDING, |d| d.padding)
    }

    /// The view insets (keyboard), depending on `view_insets` only.
    #[must_use]
    pub fn view_insets_of(ctx: &dyn BuildContext) -> Option<EdgeInsets> {
        Self::depend_on_fields(ctx, MediaQueryData::FIELD_VIEW_INSETS, |d| d.view_insets)
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
