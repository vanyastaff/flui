//! [`MediaQuery`] and [`MediaQueryData`] — ambient logical-screen data.
//!
//! Flutter parity: `widgets/media_query.dart` (`MediaQuery` / `MediaQueryData`).
//!
//! ## Implemented subset
//!
//! `size`, `device_pixel_ratio`, `text_scale_factor`, `padding`,
//! `view_insets`, `platform_brightness` — the six fields a typical widget
//! tree needs for layout and theming.
//!
//! ## Deferred (not yet implemented)
//!
//! `viewPadding`, `systemGestureInsets`, `alwaysUse24HourFormat`,
//! `accessibleNavigation`, `invertColors`, `highContrast`,
//! `disableAnimations`, `boldText`, `displayFeatures`, `navigationMode`.
//! These require platform event plumbing (accessibility bridge, IME state)
//! that lives above this layer.

use flui_geometry::{EdgeInsets, px};
use flui_types::Size;
use flui_types::platform::Brightness;
use flui_view::prelude::*;
use flui_view::{BoxedView, InheritedView, impl_inherited_view};

/// Ambient logical-screen data provided to descendants by a [`MediaQuery`]
/// ancestor.
///
/// Mirrors Flutter's `MediaQueryData`. Construct with individual pub fields
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
/// | Field | Flutter equivalent |
/// |---|---|
/// | [`size`](Self::size) | `MediaQueryData.size` |
/// | [`device_pixel_ratio`](Self::device_pixel_ratio) | `MediaQueryData.devicePixelRatio` |
/// | [`text_scale_factor`](Self::text_scale_factor) | `MediaQueryData.textScaler` (flat `f32`, not `TextScaler`) |
/// | [`padding`](Self::padding) | `MediaQueryData.padding` |
/// | [`view_insets`](Self::view_insets) | `MediaQueryData.viewInsets` |
/// | [`platform_brightness`](Self::platform_brightness) | `MediaQueryData.platformBrightness` |
#[derive(Debug, Clone, PartialEq)]
pub struct MediaQueryData {
    /// Logical size of the current display surface (window or full screen).
    ///
    /// In logical pixels: divide by [`device_pixel_ratio`](Self::device_pixel_ratio)
    /// to get physical pixels.
    pub size: Size,

    /// Physical pixels per logical pixel (e.g. `2.0` on a Retina display,
    /// `3.0` on some high-DPI phones). Always positive and finite.
    pub device_pixel_ratio: f32,

    /// User-configured font scaling factor. `1.0` is the system default;
    /// values above `1.0` enlarge text for accessibility.
    pub text_scale_factor: f32,

    /// Safe-area insets from the window edges reserved by the OS (notch,
    /// home indicator, status bar). App content should avoid rendering
    /// interactive or critical elements in these areas.
    pub padding: EdgeInsets,

    /// Insets occupied by system UI that fully obscures part of the window,
    /// such as the software keyboard when it is visible. Unlike
    /// [`padding`](Self::padding), these areas are hidden — not just reserved.
    pub view_insets: EdgeInsets,

    /// The OS-level light/dark preference as reported by the platform.
    /// An app-level [`Theme`](super::theme::Theme) may override this for its
    /// subtree; this field reflects the platform signal only.
    pub platform_brightness: Brightness,
}

impl Default for MediaQueryData {
    fn default() -> Self {
        Self {
            size: Size::new(px(800.0), px(600.0)),
            device_pixel_ratio: 1.0,
            text_scale_factor: 1.0,
            padding: EdgeInsets::ZERO,
            view_insets: EdgeInsets::ZERO,
            platform_brightness: Brightness::Light,
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
/// ## Flutter parity
///
/// Mirrors Flutter's `MediaQuery` inherited widget
/// (`widgets/media_query.dart`). Flutter's `MediaQueryData.fromWindow` /
/// `.fromView` constructors, which bootstrap data from the platform window,
/// are deferred: in FLUI the platform layer will construct [`MediaQueryData`]
/// and provide it here. The inherited-data mechanism itself is identical.
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
    ///
    /// Flutter parity: `MediaQuery.of(context)`.
    #[must_use]
    pub fn of(ctx: &dyn BuildContext) -> MediaQueryData {
        ctx.depend_on::<Self, _>(|mq| mq.data.clone())
            .expect("MediaQuery::of called with no MediaQuery ancestor in the tree")
    }

    /// Look up the nearest ancestor [`MediaQuery`]'s data, registering a
    /// dependency. Returns `None` if there is no [`MediaQuery`] ancestor.
    ///
    /// Flutter parity: `MediaQuery.maybeOf(context)`.
    #[must_use]
    pub fn maybe_of(ctx: &dyn BuildContext) -> Option<MediaQueryData> {
        ctx.depend_on::<Self, _>(|mq| mq.data.clone())
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
        // Rebuild descendants when any field of the media data changes — the
        // same contract as Flutter's `MediaQueryData.==`.
        self.data != old.data
    }
}

impl_inherited_view!(MediaQuery);
