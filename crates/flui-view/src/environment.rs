//! Inherited presentation data and subtree-scoped presentation services:
//! [`MediaQuery`]/[`MediaQueryData`], the gesture arena and its inherited
//! input policy ([`GestureArenaScope`], [`GesturePolicy`]), the shared
//! frame registry for implicitly-animated widgets ([`VsyncScope`]), and the
//! presentation's root focus scope ([`FocusTreeRoot`]); and the platform whose
//! conventions the subtree follows ([`PlatformConvention`]).
//!
//! These are the views the runtime installs above one root and the catalog,
//! the runtime, and the packages all read; the widget catalog owns nothing
//! about them.
//!
//! # [`MediaQuery`]
//!
//! ## Implemented subset
//!
//! `size`, `device_pixel_ratio`, `text_sizing`, `font_weight_adjustment`, `view_padding`,
//! `padding`, `view_insets`, `system_gesture_insets`, `platform_brightness`, `high_contrast`,
//! `preferred_locales`, `regional_formats`, `motion` — presentation and
//! preference fields for layout and theming.
//!
//! ## Deferred (not yet implemented)
//!
//! Accessible navigation, inverted colors, display features, and
//! navigation mode. Text-weight observation
//! is implemented by the UIKit backend; other backends leave it unavailable.
//! These require platform event plumbing (accessibility bridge, IME state)
//! that lives above this layer.

mod focus_tree_root;

use std::rc::Rc;
use std::sync::Arc;

pub use focus_tree_root::{FocusTreeRoot, FocusTreeRootState};

use flui_animation::{MotionPolicy, Vsync};
use flui_foundation::geometry::{EdgeInsets, Edges, Size};
use flui_interaction::gesture::{GestureArena, SweepModel};
use flui_interaction::{GestureSettingsProvider, WheelPreferencesProvider};
use flui_painting::TextSizing;
use flui_platform::{Brightness, Locale, RegionalFormats, TargetPlatform};

use crate::prelude::*;
use crate::{FieldMask, impl_inherited_view};

/// Ambient logical-screen data provided to descendants by a [`MediaQuery`]
/// ancestor.
///
/// Construct with individual pub fields
/// directly, or start from [`Default`] and override:
///
/// ```rust,ignore
/// use flui_view::environment::MediaQueryData;
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
/// - [`regional_formats`](Self::regional_formats)
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
    pub text_sizing: TextSizing,

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
    pub motion: MotionPolicy,

    /// Ordered preferred UI languages observed by the host. `None` means
    /// unavailable; an empty list is an observed empty preference list.
    /// Application locale overrides and resource fallback belong to the
    /// application's `WidgetsApp`. Like the other fields, a nested
    /// `MediaQuery` replaces this value; copy parent data to preserve it when
    /// overriding another field.
    pub preferred_locales: Option<Arc<[Locale]>>,

    /// The user's regional formats observed by the host: first weekday,
    /// hour cycle, measurement, numbering system and region override
    /// (ADR-0278). Empty when the host reported none. A date, time or number
    /// formatter takes the resolved locale through
    /// [`RegionalFormats::apply_to`]; the locale `Localizations` resolver
    /// stays without these keys, so delegates match it as declared.
    pub regional_formats: RegionalFormats,
}

impl Default for MediaQueryData {
    fn default() -> Self {
        Self {
            size: Size::new(800.0, 600.0),
            device_pixel_ratio: 1.0,
            text_sizing: TextSizing::fixed(),
            font_weight_adjustment: 0,
            view_padding: EdgeInsets::ZERO,
            view_insets: EdgeInsets::ZERO,
            system_gesture_insets: EdgeInsets::ZERO,
            platform_brightness: Brightness::Light,
            high_contrast: false,
            motion: MotionPolicy::Full,
            preferred_locales: None,
            regional_formats: RegionalFormats::default(),
        }
    }
}

impl MediaQueryData {
    /// The part of [`view_padding`](Self::view_padding) that
    /// [`view_insets`](Self::view_insets) do not already cover, per edge: with
    /// the keyboard up over the home-indicator area the bottom value is zero.
    /// App content avoids these areas and a `SafeArea` consumes them.
    /// Computed, so it cannot disagree with its sources.
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
/// use flui_view::environment::{MediaQuery, MediaQueryData};
///
/// MediaQuery::new(
///     MediaQueryData::default(),
///     child.into_view(),
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
    pub fn motion_of(ctx: &dyn BuildContext) -> Option<MotionPolicy> {
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
    pub fn text_sizing_of(ctx: &dyn BuildContext) -> Option<TextSizing> {
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

    /// The user's regional formats, depending on `regional_formats` only.
    pub fn regional_formats_of(ctx: &dyn BuildContext) -> Option<RegionalFormats> {
        Self::depend_on_fields(ctx, MediaQueryData::FIELD_REGIONAL_FORMATS, |d| {
            d.regional_formats.clone()
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

// ---------------------------------------------------------------------------
// The platform whose conventions the subtree follows.

/// The platform whose conventions a subtree follows: which modifier jumps by
/// word, which keys refresh and zoom, how a long press selects, which
/// physics a scroll view settles with, which page transition a route runs.
///
/// The one channel for that choice (ADR-0028's capability seam): a widget
/// reads [`PlatformConvention::of`] and never branches on
/// `TargetPlatform::current()` or a theme field. The runtime installs it
/// above every root with the platform its presentation names, so a web
/// build follows the platform the host reports, not the compile target.
/// A plain wrapper overrides it for a subtree, which is how a test or a
/// preview shows the iOS conventions on any machine:
///
/// ```rust,ignore
/// PlatformConvention::new(TargetPlatform::iOS, app)
/// ```
#[derive(Clone)]
pub struct PlatformConvention {
    platform: TargetPlatform,
    child: BoxedView,
}

impl PlatformConvention {
    /// Wrap `child` in a subtree that follows `platform`'s conventions.
    #[must_use]
    pub fn new(platform: TargetPlatform, child: impl IntoView) -> Self {
        Self {
            platform,
            child: BoxedView::new(child.into_view()),
        }
    }

    /// The platform the nearest ancestor names, registering a dependency so
    /// the element rebuilds when it changes. Without one,
    /// [`TargetPlatform::Unknown`], whose conventions are the
    /// platform-neutral ones: a tree mounted outside a runtime never takes
    /// them from the machine it runs on.
    pub fn of(ctx: &dyn BuildContext) -> TargetPlatform {
        ctx.depend_on::<Self, _>(|convention| convention.platform)
            .unwrap_or(TargetPlatform::Unknown)
    }
}

impl std::fmt::Debug for PlatformConvention {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlatformConvention")
            .field("platform", &self.platform)
            .finish_non_exhaustive()
    }
}

impl InheritedView for PlatformConvention {
    type Data = TargetPlatform;

    fn data(&self) -> &Self::Data {
        &self.platform
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        self.platform != old.platform
    }
}

impl_inherited_view!(PlatformConvention);

// ---------------------------------------------------------------------------
// The gesture arena and the inherited input policy.
//
// The presentation installs its live arena once; every recognizer below
// acquires exactly that arena through [`GestureArenaScope`], and the scope
// publishes the resolved settings as one inherited node.
// ---------------------------------------------------------------------------

/// Shares one binding-driven arena and its input policy with descendants.
///
/// The presentation installs its live gesture and wheel providers at the root.
/// An authored nested scope inherits both policies unless explicitly overridden:
/// `.settings(...)` overrides gestures independently of wheel observations.
/// Without an outer scope, absent policies use framework defaults.
///
/// This composition widget resolves inheritance before publishing one inherited
/// node containing the arena and both policies. Use [`Self::of`] to acquire the
/// arena; the resolved form is the [`InheritedView`], not this configuration.
#[derive(Clone, StatefulView)]
pub struct GestureArenaScope {
    arena: GestureArena,
    settings: Option<GestureSettingsProvider>,
    wheel_preferences: Option<WheelPreferencesProvider>,
    child: BoxedView,
}

impl GestureArenaScope {
    /// Wrap `child` with the presentation's shared arena.
    ///
    /// # Panics
    /// Panics if `arena` is not binding-driven. A scope never creates a second
    /// owner of the arena's close/sweep lifecycle.
    #[must_use]
    pub fn new(arena: GestureArena, child: impl IntoView) -> Self {
        assert_eq!(
            arena.sweep_model(),
            SweepModel::BindingDriven,
            "BUG: GestureArenaScope requires a BindingDriven arena owned by the presentation \
             binding; SelfDriven arenas cannot back a presentation scope"
        );
        Self {
            arena,
            settings: None,
            wheel_preferences: None,
            child: BoxedView::new(child.into_view()),
        }
    }

    /// Explicitly override gesture policy, retaining inherited wheel policy.
    ///
    /// An authored [`flui_interaction::GestureSettings`] becomes a fixed profile;
    /// a presentation supplies its live provider. New admissions read the
    /// provider while active contacts retain their admitted settings. Beneath
    /// an outer scope, the outer provider's accessibility timing floors still
    /// raise a shorter authored long-press or double-tap timing.
    #[must_use]
    pub fn settings(mut self, settings: impl Into<GestureSettingsProvider>) -> Self {
        self.settings = Some(settings.into());
        self
    }

    /// Explicitly override wheel observations independently of gesture policy.
    ///
    /// Leaving this unset inherits the outer scope's exact provider, including
    /// future host updates. An authored fixed value replaces that inheritance.
    #[must_use]
    pub fn wheel_preferences(mut self, preferences: impl Into<WheelPreferencesProvider>) -> Self {
        self.wheel_preferences = Some(preferences.into());
        self
    }

    /// The gesture settings the nearest scope has resolved for consumers of
    /// gesture data.
    ///
    /// # Panics
    /// Panics outside a [`GestureArenaScope`]: a settings consumer is a
    /// gesture recognizer and only mounts beneath the presentation's scope.
    #[must_use]
    pub fn settings_of(ctx: &dyn BuildContext) -> GestureSettingsProvider {
        ctx.depend_on::<ResolvedGestureArenaScope, _>(|scope| scope.data.policy.settings().clone())
            .expect("BUG: gesture consumers must acquire settings beneath GestureArenaScope")
    }

    /// The scope's gesture settings, or the framework defaults outside one.
    #[must_use]
    pub fn settings_or_default_of(ctx: &dyn BuildContext) -> GestureSettingsProvider {
        ctx.depend_on::<ResolvedGestureArenaScope, _>(|scope| scope.data.policy.settings().clone())
            .unwrap_or_default()
    }

    /// The wheel observations the nearest scope has resolved. Wheel handlers
    /// that admit by pointer kind or min-target read them.
    ///
    /// # Panics
    /// Panics outside a [`GestureArenaScope`]: a wheel consumer is a gesture
    /// recognizer and only mounts beneath the presentation's scope.
    #[must_use]
    pub fn wheel_preferences_of(ctx: &dyn BuildContext) -> WheelPreferencesProvider {
        ctx.depend_on::<ResolvedGestureArenaScope, _>(|scope| scope.data.wheel_preferences.clone())
            .expect("BUG: wheel consumers must acquire preferences beneath GestureArenaScope")
    }

    /// Acquire the exact arena without registering an inherited dependency.
    ///
    /// # Panics
    /// Panics outside a presentation scope. Consumers never create a private
    /// arena or take over the binding's lifecycle.
    #[must_use]
    pub fn of(ctx: &dyn BuildContext) -> GestureArena {
        // Compositions built from the handle are checked against the scope's
        // gesture policy (ADR-0208 delay ceiling).
        ctx.get::<ResolvedGestureArenaScope, _>(|scope| scope.data.policy.arena().clone())
            .expect(
                "BUG: gesture consumers must be mounted beneath GestureArenaScope; \
             the presentation binding is the sole GestureArena lifecycle owner",
            )
    }

    /// [`Self::of`], registering a dependency: the caller's
    /// `did_change_dependencies` runs when the scope hands out a handle that
    /// admits elsewhere or another policy.
    ///
    /// # Panics
    /// Panics outside a presentation scope, as [`Self::of`] does.
    pub fn dependent_of(ctx: &dyn BuildContext) -> GestureArena {
        ctx.depend_on::<ResolvedGestureArenaScope, _>(|scope| scope.data.policy.arena().clone())
            .expect(
                "BUG: gesture consumers must be mounted beneath GestureArenaScope; \
             the presentation binding is the sole GestureArena lifecycle owner",
            )
    }

    /// [`Self::of`], or `None` outside a presentation scope.
    #[must_use]
    pub fn maybe_of(ctx: &dyn BuildContext) -> Option<GestureArena> {
        ctx.get::<ResolvedGestureArenaScope, _>(|scope| scope.data.policy.arena().clone())
    }

    /// The shared arena configured for this scope.
    #[must_use]
    pub fn arena(&self) -> &GestureArena {
        &self.arena
    }
}

/// The arena handle and the gesture settings a widget builds its recognizers
/// for. Equal policies admit alike under the same settings, so recognizers
/// built for one serve the other.
///
/// A scope resolves its policy once and every consumer beneath it shares
/// that value, so holding or cloning a policy costs one reference.
#[derive(Clone, Debug)]
pub struct GesturePolicy(Rc<PolicyParts>);

#[derive(Debug)]
struct PolicyParts {
    arena: GestureArena,
    settings: GestureSettingsProvider,
}

impl GesturePolicy {
    fn new(arena: &GestureArena, settings: GestureSettingsProvider) -> Self {
        Self(Rc::new(PolicyParts {
            // Compositions built from the handle are checked against the
            // scope's gesture policy (ADR-0208 delay ceiling).
            arena: arena.with_settings(settings.clone()),
            settings,
        }))
    }

    /// The scope's policy. Reading it depends on the scope, so the caller's
    /// `did_change_dependencies` runs when the scope changes.
    ///
    /// # Panics
    /// Panics outside a [`GestureArenaScope`], as [`GestureArenaScope::of`]
    /// does.
    pub fn of(ctx: &dyn BuildContext) -> Self {
        Self::maybe_of(ctx).expect(
            "BUG: gesture consumers must be mounted beneath GestureArenaScope; \
             the presentation binding is the sole GestureArena lifecycle owner",
        )
    }

    /// [`Self::of`], or `None` outside a presentation scope.
    #[must_use]
    pub fn maybe_of(ctx: &dyn BuildContext) -> Option<Self> {
        ctx.depend_on::<ResolvedGestureArenaScope, _>(|scope| scope.data.policy.clone())
    }

    /// The subtree's exact arena, configured with the resolved settings.
    #[must_use]
    pub fn arena(&self) -> &GestureArena {
        &self.0.arena
    }

    /// The resolved gesture settings the recognizers built for it admit with.
    #[must_use]
    pub fn settings(&self) -> &GestureSettingsProvider {
        &self.0.settings
    }
}

impl PartialEq for GesturePolicy {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
            || (self.0.settings == other.0.settings && self.0.arena.admits_like(&other.0.arena))
    }
}

impl std::fmt::Debug for GestureArenaScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GestureArenaScope")
            .field("arena", &self.arena)
            .finish_non_exhaustive()
    }
}

/// What a mounted [`GestureArenaScope`] publishes, resolved when the scope
/// or the policies above it change. The state keeps it across rebuilds, so a
/// scope whose arena and policies have not changed hands its consumers the
/// policy they already hold.
pub struct GestureArenaScopeState {
    /// What the scope itself authors, kept to resolve again when the
    /// policies above change.
    authored: Policies,
    inherited: Policies,
    data: GestureScopeData,
}

impl std::fmt::Debug for GestureArenaScopeState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GestureArenaScopeState")
            .field("policy", &self.data.policy)
            .finish_non_exhaustive()
    }
}

/// The two policies of a scope, each absent where none is given.
#[derive(Clone, Default)]
struct Policies {
    settings: Option<GestureSettingsProvider>,
    wheel_preferences: Option<WheelPreferencesProvider>,
}

impl Policies {
    /// The policies of the scope above `ctx`, registering a dependency.
    fn inherited(ctx: &dyn BuildContext) -> Self {
        ctx.depend_on::<ResolvedGestureArenaScope, _>(|scope| Self {
            settings: Some(scope.data.policy.settings().clone()),
            wheel_preferences: Some(scope.data.wheel_preferences.clone()),
        })
        .unwrap_or_default()
    }
}

impl GestureScopeData {
    /// What a scope of `arena` publishes when it authors `authored` beneath
    /// `inherited`. `current` is what it publishes now: its policy is kept
    /// while it still says the same.
    fn resolve(
        arena: &GestureArena,
        authored: &Policies,
        inherited: &Policies,
        current: Option<&Self>,
    ) -> Self {
        // An override beneath another scope still meets the host's
        // accessibility timing floors (ADR-0212 §3).
        let settings = match (authored.settings.clone(), &inherited.settings) {
            (Some(authored), Some(inherited)) => inherited.overridden_by(authored),
            (Some(authored), None) => authored,
            (None, inherited) => inherited.clone().unwrap_or_default(),
        };
        let wheel_preferences = authored
            .wheel_preferences
            .clone()
            .or_else(|| inherited.wheel_preferences.clone())
            .unwrap_or_default();
        let policy = current
            .map(|current| &current.policy)
            .filter(|current| *current.settings() == settings && current.arena().admits_like(arena))
            .map_or_else(
                || GesturePolicy::new(arena, settings.clone()),
                GesturePolicy::clone,
            );
        Self {
            policy,
            wheel_preferences,
        }
    }
}

impl StatefulView for GestureArenaScope {
    type State = GestureArenaScopeState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        let (authored, inherited) = (self.authored(), Policies::inherited(ctx));
        GestureArenaScopeState {
            data: GestureScopeData::resolve(&self.arena, &authored, &inherited, None),
            authored,
            inherited,
        }
    }
}

impl GestureArenaScope {
    fn authored(&self) -> Policies {
        Policies {
            settings: self.settings.clone(),
            wheel_preferences: self.wheel_preferences.clone(),
        }
    }
}

impl ViewState<GestureArenaScope> for GestureArenaScopeState {
    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.inherited = Policies::inherited(ctx);
        self.data = GestureScopeData::resolve(
            self.data.policy.arena(),
            &self.authored,
            &self.inherited,
            Some(&self.data),
        );
    }

    fn did_update_view(&mut self, _old: &GestureArenaScope, new: &GestureArenaScope) {
        self.authored = new.authored();
        self.data = GestureScopeData::resolve(
            &new.arena,
            &self.authored,
            &self.inherited,
            Some(&self.data),
        );
    }

    fn build(&self, view: &GestureArenaScope, _ctx: &dyn BuildContext) -> impl IntoView {
        ResolvedGestureArenaScope {
            data: self.data.clone(),
            child: view.child.clone(),
        }
    }
}

/// The resolved arena and both policies one inherited node carries.
#[derive(Clone)]
struct GestureScopeData {
    /// The arena and the settings as one shared value, which consumers hold.
    policy: GesturePolicy,
    wheel_preferences: WheelPreferencesProvider,
}

#[derive(Clone)]
struct ResolvedGestureArenaScope {
    data: GestureScopeData,
    child: BoxedView,
}

impl InheritedView for ResolvedGestureArenaScope {
    type Data = GestureScopeData;
    fn data(&self) -> &Self::Data {
        &self.data
    }
    fn child(&self) -> &dyn View {
        &self.child
    }
    /// A consumer's recognizers join the arena and follow the policy they were
    /// built with, so a change of either reaches every dependent.
    fn update_should_notify(&self, old: &Self) -> bool {
        self.data.policy != old.data.policy
            || self.data.wheel_preferences != old.data.wheel_preferences
    }
}

impl_inherited_view!(ResolvedGestureArenaScope);

// ---------------------------------------------------------------------------
// The shared frame registry ([`Vsync`]).
//
// With no ambient ticker owner, a binding hands its [`Vsync`] to a subtree as
// inherited data and every implicitly-animated widget below acquires it in
// `init_state`.
// ---------------------------------------------------------------------------

/// Provides a shared [`Vsync`] registry to its descendant implicitly-animated
/// widgets.
///
/// A binding (or a test harness) wraps the application subtree in
/// `VsyncScope::new(binding.vsync(), child)`. Every implicitly-animated widget
/// below (`AnimatedOpacity`, …) acquires this registry in `init_state`
/// through [`VsyncScope::maybe_of`] and creates an owning controller in it, so
/// the binding's `pump_frame` advances all of them on the same virtual clock —
/// deterministically, with no `thread::sleep`.
///
/// FLUI is non-singleton, so there is no ambient ticker owner: the registry is
/// handed down explicitly as inherited data, scoped to a subtree.
///
/// Without a registry, finite animations settle through the common unbound
/// policy; infinite repeats park until a registry is provided. Gesture
/// ownership is stricter and unrelated: gesture widgets require their
/// presentation's `GestureArenaScope`.
///
/// Replacing the registry notifies retained states, which rebind their owning
/// controllers in `did_change_dependencies` without restarting accepted motion.
#[derive(Clone)]
pub struct VsyncScope {
    /// The shared registry handed to descendants. Cloning the scope clones this
    /// `Rc`-backed handle, so all clones observe the same registry.
    vsync: Option<Vsync>,
    /// The wrapped subtree the registry is provided to.
    child: BoxedView,
}

impl VsyncScope {
    /// Read the subtree's registry while registering a lifecycle dependency.
    /// Rebind owned animations from init_state and did_change_dependencies.
    #[must_use]
    pub fn maybe_of(ctx: &dyn LifecycleContext) -> Option<Vsync> {
        ctx.depend_on::<Self, _>(|scope| scope.vsync.clone())
            .flatten()
    }

    /// Whether a state holding `current` still holds the subtree's registry
    /// `incoming`: both absent, or one registry. A state rebinds only when
    /// this is false, so a fresh clone of the same registry restarts nothing.
    pub fn same_registry(current: Option<&Vsync>, incoming: Option<&Vsync>) -> bool {
        match (current, incoming) {
            (None, None) => true,
            (Some(current), Some(incoming)) => current.is_same(incoming),
            _ => false,
        }
    }

    /// Wrap `child` in a scope that provides `vsync` to its descendants.
    #[must_use]
    pub fn new(vsync: Vsync, child: impl IntoView) -> Self {
        Self {
            vsync: Some(vsync),
            child: BoxedView::new(child.into_view()),
        }
    }

    /// The shared registry this scope provides — what a descendant implicitly-
    /// animated widget reads in `init_state` to register its controller against.
    #[must_use]
    pub fn vsync(&self) -> Option<&Vsync> {
        self.vsync.as_ref()
    }

    /// Provide a subtree with no frame registry. Finite animations settle.
    #[must_use]
    pub fn detached(child: impl IntoView) -> Self {
        Self {
            vsync: None,
            child: BoxedView::new(child.into_view()),
        }
    }
}

impl std::fmt::Debug for VsyncScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VsyncScope")
            .field("vsync", &self.vsync)
            .finish_non_exhaustive()
    }
}

impl InheritedView for VsyncScope {
    type Data = Option<Vsync>;

    fn data(&self) -> &Self::Data {
        &self.vsync
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        !Self::same_registry(old.vsync.as_ref(), self.vsync.as_ref())
    }
}

impl_inherited_view!(VsyncScope);
