//! [`Image`] widget — displays a bitmap image.

use std::path::PathBuf;
use std::sync::Arc;

use flui_objects::{ImageAlignment, ImageFit, RenderImage};
use flui_rendering::protocol::BoxProtocol;
use flui_types::geometry::px;
use flui_types::{Pixels, Size, painting::Image as PixelImage};
#[cfg(not(feature = "asset-images"))]
use flui_view::prelude::StatelessView;
#[cfg(feature = "asset-images")]
use flui_view::prelude::{StatefulView, ViewState};
use flui_view::{BoxedView, BuildContext, IntoView, RenderView, View, ViewExt, impl_render_view};

use crate::image::provider::{DirectImageProvider, FileImage, ImageProvider, MemoryImage};

/// Displays a bitmap image.
///
/// Resolves the image source synchronously or asynchronously — see
/// [`ImageProvider::cache_key`] — and displays it via a private `RawImage`
/// leaf render view (Flutter's `Image`-wraps-`RawImage` split:
/// `widgets/image.dart` `Image` is the stateful/stateless resolver, wrapping
/// `rendering/image.dart` `RawImage`, the dumb leaf that just paints an
/// already-decoded image).
///
/// On resolution failure the widget renders an empty zero-sized box — no
/// panic; a `WARN`-level trace event is emitted so the failure is visible.
///
/// # Constructors
///
/// | Constructor | Source | Path |
/// |-------------|--------|------|
/// | [`from_image`] | Already-decoded [`PixelImage`] | Sync, O(1) Arc clone |
/// | [`memory`] | Encoded bytes in memory | Sync, full decode per rebuild |
/// | [`file`] | Local file read + decode | Sync, blocking I/O + decode |
/// | `asset` | `flui-assets` asset path | Async — cached, coalesced, off-thread |
/// | `network` | HTTP/HTTPS URL | Async — cached, coalesced, off-thread |
/// | [`new`] | Any [`ImageProvider`] impl | Provider-dependent |
///
/// `asset`/`network` require the `asset-images`/`network-images` features
/// respectively (hence the plain, non-linked names above — they do not exist
/// in this doc build); both are off by default so stable builds do not pull
/// in `flui-assets`/`futures-util`/`lru` unless asked for.
///
/// For static or frequently-rebuilt sync images, pre-decode once and use
/// [`from_image`] to avoid per-rebuild cost.
///
/// # Async dispatch
///
/// When [`ImageProvider::cache_key`] returns `Some(key)`, `Image` subscribes
/// to that key for as long as it stays mounted. Subscribing first probes the
/// decode cache synchronously — a hit (e.g. after unmount+remount, or a
/// second widget mounted with the same key) renders immediately with **no
/// placeholder frame**. A miss shows the same empty-box placeholder a sync
/// failure would, and the render updates in place once
/// [`ImageProvider::resolve_async`] completes. Two widgets mounted with the
/// same key while a load is in flight share ONE load
/// (`image::decode_cache`'s in-flight coalescing) rather than starting two.
///
/// The subscription is keyed on [`cache_key`](ImageProvider::cache_key), not
/// on provider instance identity: rebuilding with a freshly constructed
/// provider for the same path resolves nothing again. A rebuild that *does*
/// change the key cancels the in-flight load and retires the generation it
/// was issued under, so a result that settles afterwards can no longer reach
/// this widget: an out-of-order pair of loads cannot show the loser.
///
/// # Gapless playback
///
/// Changing the provider key clears the displayed frame to the placeholder in
/// the same frame the change lands, matching Flutter's default
/// (`gaplessPlayback: false`) — a stale image under a changed caption is the
/// failure that default exists to prevent.
/// [`gapless_playback(true)`](Image::gapless_playback) keeps the last decoded
/// frame on screen until the new one is ready instead.
///
/// # Layout
///
/// Under unconstrained (loose) layout the widget takes the image's intrinsic
/// size. [`width`] and [`height`] fold into the constraints via
/// `BoxConstraints::tighten`; omitting one lets the image's aspect ratio
/// determine the other axis.
///
/// # Flutter parity
///
/// Mirrors `widgets/image.dart` `Image` over `rendering/image.dart`
/// `RenderImage`. `Image` here is a one-shot resolver, not a port of
/// Flutter's `ImageStream`: no chunk/progress events, no multi-frame
/// (animated-image) support — FLUI's `Image` view is single-frame. Revisit
/// when animated images land.
///
/// Deferred (tracked, not silently missing): `frameBuilder`, `loadingBuilder`,
/// `errorBuilder` (an error renders the same empty box as no data, with a
/// `tracing::warn!`), `ImageConfiguration`/`devicePixelRatio`-based cache-key
/// scaling, an `evict`/`clearLiveImages` cache-management API, and font
/// unification.
///
/// [`from_image`]: Image::from_image
/// [`memory`]: Image::memory
/// [`file`]: Image::file
/// [`new`]: Image::new
/// [`width`]: Image::width
/// [`height`]: Image::height
#[derive(Clone, Debug)]
// Async resolution is a subscription with lifecycle (start, cancel, retire),
// so `Image` is stateful exactly where that subscription exists — Flutter's
// `Image` is a `StatefulWidget` unconditionally, but without `asset-images`
// there is no decode cache to subscribe to and every provider resolves inline
// in `build`.
#[cfg_attr(feature = "asset-images", derive(StatefulView))]
#[cfg_attr(not(feature = "asset-images"), derive(StatelessView))]
pub struct Image {
    // PORT-CHECK-OK-SP3: widget view type; `flui_types::painting::Image` is the pixel-data handle — distinct concepts at different crate layers
    provider: Arc<dyn ImageProvider + Send + Sync>,
    fit: ImageFit,
    alignment: ImageAlignment,
    width: Option<Pixels>,
    height: Option<Pixels>,
    gapless_playback: bool,
}

impl Image {
    /// Creates an `Image` widget backed by the given provider.
    ///
    /// Defaults: [`ImageFit::Contain`], [`ImageAlignment::Center`], no forced
    /// width or height.
    ///
    /// `provider` must be `'static` because the widget is stored in the
    /// element tree; it must be `Send + Sync` (implied by [`ImageProvider`]'s
    /// supertraits) because the tree may be accessed from multiple threads.
    pub fn new(provider: impl ImageProvider + 'static) -> Self {
        Self {
            provider: Arc::new(provider),
            fit: ImageFit::Contain,
            alignment: ImageAlignment::Center,
            width: None,
            height: None,
            gapless_playback: false,
        }
    }

    /// Creates an `Image` from an already-decoded [`PixelImage`].
    ///
    /// The most efficient path: `resolve()` is O(1) on every rebuild (the
    /// pixel buffer is `Arc`-backed). Prefer this when the image is decoded
    /// outside the widget tree or constructed procedurally.
    pub fn from_image(decoded: PixelImage) -> Self {
        Self::new(DirectImageProvider::new(decoded))
    }

    /// Creates an `Image` that decodes `bytes` (PNG, JPEG, GIF, …) on each
    /// rebuild.
    ///
    /// Requires the `flui-widgets/images` feature; without it the widget
    /// renders an empty box. For static images in frequently-rebuilt trees,
    /// pre-decode once and use [`from_image`](Image::from_image) instead.
    pub fn memory(bytes: impl Into<Vec<u8>>) -> Self {
        Self::new(MemoryImage::new(bytes))
    }

    /// Creates an `Image` that reads and decodes a local file synchronously
    /// on each rebuild.
    ///
    /// Requires the `flui-widgets/images` feature; without it the widget
    /// renders an empty box. For static file images, pre-decode once and use
    /// [`from_image`](Image::from_image) instead.
    pub fn file(path: impl Into<PathBuf>) -> Self {
        Self::new(FileImage::new(path))
    }

    /// Creates an `Image` that loads and decodes `path` asynchronously
    /// through `registry`, a `flui-assets` asset registry.
    ///
    /// `registry` is an explicit argument — never
    /// [`AssetRegistry::global()`](flui_assets::AssetRegistry::global) — so
    /// the load runs on whichever background runtime and byte-loader cache
    /// the application already owns. See the [async dispatch](#async-dispatch)
    /// section above.
    ///
    /// Requires the `flui-widgets/asset-images` feature.
    #[cfg(feature = "asset-images")]
    pub fn asset(registry: Arc<flui_assets::AssetRegistry>, path: impl Into<String>) -> Self {
        Self::new(super::AssetImage::new(registry, path))
    }

    /// Creates an `Image` that fetches and decodes a URL asynchronously
    /// through `registry`, a `flui-assets` asset registry.
    ///
    /// Same registry-injection contract as [`asset`](Image::asset).
    ///
    /// Requires the `flui-widgets/network-images` feature.
    #[cfg(feature = "network-images")]
    pub fn network(registry: Arc<flui_assets::AssetRegistry>, url: impl Into<String>) -> Self {
        Self::new(super::NetworkImage::new(registry, url))
    }

    /// Sets how the image is scaled to fit the laid-out box.
    ///
    /// Defaults to [`ImageFit::Contain`].
    #[must_use]
    pub fn fit(mut self, fit: ImageFit) -> Self {
        self.fit = fit;
        self
    }

    /// Sets how the image is aligned within the box.
    ///
    /// Defaults to [`ImageAlignment::Center`].
    #[must_use]
    pub fn alignment(mut self, alignment: ImageAlignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// Forces a specific logical width in pixels.
    ///
    /// Folded into the layout constraints (`tighten`). If height is not also
    /// forced, the image's aspect ratio determines the height axis.
    #[must_use]
    pub fn width(mut self, width_px: f32) -> Self {
        self.width = Some(px(width_px));
        self
    }

    /// Forces a specific logical height in pixels.
    ///
    /// Folded into the layout constraints (`tighten`). If width is not also
    /// forced, the image's aspect ratio determines the width axis.
    #[must_use]
    pub fn height(mut self, height_px: f32) -> Self {
        self.height = Some(px(height_px));
        self
    }

    /// Keeps the previously decoded frame on screen while a new provider
    /// loads, instead of clearing to the placeholder the moment the provider
    /// key changes.
    ///
    /// Defaults to `false` — Flutter's `gaplessPlayback` default, and for the
    /// same reason: when the image is coupled to other content that has
    /// already changed (an avatar beside a name), holding the old frame shows
    /// a combination that was never true. Turn it on for a sequence of frames
    /// that are all views of the same thing, where a placeholder flash is the
    /// worse artifact.
    ///
    /// Only reached on the async path — a provider with no
    /// [`cache_key`](ImageProvider::cache_key) resolves inline on every build
    /// and has no frame to hold on to.
    #[must_use]
    pub fn gapless_playback(mut self, gapless: bool) -> Self {
        self.gapless_playback = gapless;
        self
    }

    /// Resolves the provider synchronously, warns and clears on failure,
    /// and builds the leaf [`RawImage`] view directly — no subscription.
    fn build_sync(&self) -> BoxedView {
        let image = match self.provider.resolve() {
            Ok(decoded) => Some(decoded),
            Err(err) => {
                // Neither `?provider` nor `%err`: the provider's `Debug` holds
                // the file path or URL, and the error's `Display` interpolates
                // it. For a user-picked photo that is their data.
                tracing::warn!(
                    error_kind = err.kind(),
                    "image provider failed to resolve; showing empty placeholder box"
                );
                None
            }
        };
        self.raw(image)
    }

    /// Builds the leaf [`RawImage`] view carrying `image` (or the empty
    /// placeholder when `None`) with this widget's current layout config.
    fn raw(&self, image: Option<PixelImage>) -> BoxedView {
        RawImage {
            image,
            fit: self.fit,
            alignment: self.alignment,
            width: self.width,
            height: self.height,
        }
        .boxed()
    }
}

/// Async dispatch — only compiled under `asset-images`, since the
/// subscription needs `image::decode_cache`'s `lru`/`futures-util`-backed
/// engine. Without this feature `Image` always takes the `build_sync` path,
/// even for a custom provider that overrides [`ImageProvider::cache_key`] —
/// see that method's doc for the honest fallback contract.
#[cfg(feature = "asset-images")]
impl StatefulView for Image {
    type State = ImageState;

    fn create_state(&self) -> Self::State {
        // `ViewState::init_state` is handed a `BuildContext` but NOT the view,
        // so the provider the first resolve needs is copied here. Later
        // rebuilds reach the fresh view through `did_update_view` / `build`.
        ImageState {
            resolver: super::resolve::ImageResolver::new(
                Arc::clone(&self.provider),
                self.gapless_playback,
            ),
        }
    }
}

/// Persistent state for [`Image`] — **opaque**.
///
/// `pub` only because it is the `State` associated type of a public
/// [`StatefulView`] impl and Rust forbids a crate-private type there. It has
/// no public fields and no public methods; construct it only through
/// `Image::create_state`.
#[cfg(feature = "asset-images")]
#[derive(Debug)]
pub struct ImageState {
    resolver: super::resolve::ImageResolver,
}

#[cfg(feature = "asset-images")]
impl ViewState<Image> for ImageState {
    /// `_ImageState.initState`: resolve the provider the widget mounted with.
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        self.resolver.init(ctx);
    }

    /// `_ImageState.build`: paint whatever frame the resolver has published.
    ///
    /// A provider that opted out of async resolution (no cache key) never
    /// publishes anything, and resolves inline here instead — the same
    /// `build_sync` path a build without `asset-images` takes.
    fn build(&self, view: &Image, _ctx: &dyn BuildContext) -> impl IntoView {
        if !self.resolver.is_subscribed() {
            return view.build_sync();
        }
        view.raw(self.resolver.frame())
    }

    /// `_ImageState.didUpdateWidget`: re-resolve when the cache key changed.
    fn did_update_view(&mut self, _old_view: &Image, new_view: &Image) {
        self.resolver
            .did_update(Arc::clone(&new_view.provider), new_view.gapless_playback);
    }

    /// `_ImageState.dispose`: cancel the load this widget owns.
    fn dispose(&mut self) {
        self.resolver.dispose();
    }
}

#[cfg(not(feature = "asset-images"))]
impl StatelessView for Image {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        self.build_sync()
    }
}

/// The leaf render view [`Image`] builds into once its provider has been
/// resolved (or has failed) — Flutter's `RawImage`: a dumb view over an
/// already-decoded (or absent) image, with no provider, no resolution logic.
///
/// Private: [`Image`] is the only public entry point, matching Flutter's
/// convention of not exposing `RawImage` as a widget-catalog type.
#[derive(Clone, Debug)]
struct RawImage {
    image: Option<PixelImage>,
    fit: ImageFit,
    alignment: ImageAlignment,
    width: Option<Pixels>,
    height: Option<Pixels>,
}

impl RenderView for RawImage {
    type Protocol = BoxProtocol;
    type RenderObject = RenderImage;

    fn create_render_object(&self, _ctx: &flui_view::RenderObjectContext<'_>) -> RenderImage {
        // `intrinsic_size = Size::ZERO` gives `constraints.smallest()` under
        // loose layout, so an absent image occupies no space and does not
        // panic.
        let mut render = match &self.image {
            Some(decoded) => RenderImage::from_image(decoded.clone(), self.fit, self.alignment),
            None => RenderImage::new(Size::ZERO, self.fit, self.alignment),
        };
        let initial_impact = render.set_width(self.width) | render.set_height(self.height);
        debug_assert_eq!(
            initial_impact,
            if self.width.is_some() || self.height.is_some() {
                flui_rendering::RenderUpdateImpact::LAYOUT
            } else {
                flui_rendering::RenderUpdateImpact::NONE
            },
        );
        render
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render: &mut RenderImage,
    ) -> flui_rendering::RenderUpdateImpact {
        let mut impact = render.set_fit(self.fit)
            | render.set_alignment(self.alignment)
            | render.set_width(self.width)
            | render.set_height(self.height);

        // The box is sized by the image it is showing, matching Flutter's
        // `RenderImage._sizeForConstraints` (`constraints.smallest` while
        // `_image == null`). `RenderImage` itself *retains* the last
        // intrinsic size across `set_image(None)` — a deliberate superset so
        // a caller can reserve space for a not-yet-loaded image — so the
        // widget layer drives that dimension explicitly rather than
        // inheriting a size from an image it is no longer showing.
        impact |= render.set_intrinsic_size(match &self.image {
            Some(decoded) => decoded.size(),
            None => Size::ZERO,
        });
        impact |= render.set_image(self.image.clone());
        impact
    }

    fn has_children(&self) -> bool {
        false
    }

    fn visit_child_views(&self, _visitor: &mut dyn FnMut(&dyn View)) {}
}

impl_render_view!(RawImage);

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use flui_rendering::constraints::BoxConstraints;

    use super::*;
    use crate::image::provider::ImageProviderError;

    #[derive(Debug)]
    struct AlwaysFails;

    impl ImageProvider for AlwaysFails {
        fn resolve(&self) -> Result<PixelImage, ImageProviderError> {
            Err(ImageProviderError::DecodeFailed {
                reason: "always fails".to_string(),
            })
        }
    }

    /// Succeeds with a 40x30 image on the FIRST `resolve()` call, then fails
    /// on every subsequent call -- models a provider whose backing source
    /// (a file, a network response) becomes unavailable between rebuilds.
    #[derive(Debug)]
    struct FailsAfterFirstCall {
        calls: AtomicUsize,
    }

    impl FailsAfterFirstCall {
        fn new() -> Self {
            Self {
                calls: AtomicUsize::new(0),
            }
        }
    }

    impl ImageProvider for FailsAfterFirstCall {
        fn resolve(&self) -> Result<PixelImage, ImageProviderError> {
            if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                Ok(PixelImage::from_rgba8(40, 30, vec![0u8; 40 * 30 * 4]))
            } else {
                Err(ImageProviderError::DecodeFailed {
                    reason: "source became unavailable".to_string(),
                })
            }
        }
    }

    fn loose() -> BoxConstraints {
        BoxConstraints::loose(Size::new(px(1000.0), px(1000.0)))
    }

    fn detached_ctx() -> flui_view::RenderObjectContext<'static> {
        flui_view::RenderObjectContext::detached()
    }

    #[test]
    fn create_render_object_uses_a_zero_size_placeholder_for_an_absent_image() {
        let raw = RawImage {
            image: None,
            fit: ImageFit::Contain,
            alignment: ImageAlignment::Center,
            width: None,
            height: None,
        };
        let render = raw.create_render_object(&detached_ctx());

        assert!(render.image().is_none());
        assert_eq!(render.compute_size(&loose()), Size::ZERO);
    }

    #[test]
    fn update_render_object_collapses_the_box_when_the_image_becomes_absent() {
        let with_image = RawImage {
            image: Some(PixelImage::from_rgba8(40, 30, vec![0u8; 40 * 30 * 4])),
            fit: ImageFit::Contain,
            alignment: ImageAlignment::Center,
            width: None,
            height: None,
        };
        let mut render = with_image.create_render_object(&detached_ctx());

        assert!(render.image().is_some());
        assert_eq!(render.compute_size(&loose()), Size::new(px(40.0), px(30.0)));

        let now_absent = RawImage {
            image: None,
            ..with_image
        };
        let impact = now_absent.update_render_object(&detached_ctx(), &mut render);
        assert_eq!(impact, flui_rendering::RenderUpdateImpact::LAYOUT);

        assert!(
            render.image().is_none(),
            "an absent image on update must clear the displayed image",
        );
        assert_eq!(
            render.compute_size(&loose()),
            Size::ZERO,
            "a cleared image collapses the box, matching Flutter's \
             `RenderImage._sizeForConstraints` returning `constraints.smallest` \
             while `_image == null` -- a widget that has stopped showing an \
             image must not keep reserving that image's space",
        );
    }

    #[test]
    fn update_render_object_keeps_a_forced_dimension_when_the_image_becomes_absent() {
        let with_image = RawImage {
            image: Some(PixelImage::from_rgba8(40, 30, vec![0u8; 40 * 30 * 4])),
            fit: ImageFit::Contain,
            alignment: ImageAlignment::Center,
            width: Some(px(100.0)),
            height: None,
        };
        let mut render = with_image.create_render_object(&detached_ctx());

        let now_absent = RawImage {
            image: None,
            ..with_image
        };
        let _ = now_absent.update_render_object(&detached_ctx(), &mut render);

        assert_eq!(
            render.compute_size(&loose()),
            Size::new(px(100.0), px(0.0)),
            "clearing the image collapses only the axes the image was sizing \
             -- a forced width still reserves its width",
        );
    }

    #[test]
    fn width_and_height_overrides_reach_the_render_object() {
        let raw = RawImage {
            image: None,
            fit: ImageFit::Contain,
            alignment: ImageAlignment::Center,
            width: Some(px(100.0)),
            height: Some(px(80.0)),
        };
        let render = raw.create_render_object(&detached_ctx());

        assert_eq!(render.width(), Some(px(100.0)));
        assert_eq!(render.height(), Some(px(80.0)));
    }

    #[test]
    fn raw_image_update_unions_only_changed_configuration_impacts() {
        let initial = RawImage {
            image: None,
            fit: ImageFit::Contain,
            alignment: ImageAlignment::Center,
            width: None,
            height: None,
        };
        let mut render = initial.create_render_object(&detached_ctx());
        assert_eq!(
            initial.update_render_object(&detached_ctx(), &mut render),
            flui_rendering::RenderUpdateImpact::NONE,
        );

        let paint_only = RawImage {
            fit: ImageFit::Cover,
            alignment: ImageAlignment::TopLeft,
            ..initial.clone()
        };
        assert_eq!(
            paint_only.update_render_object(&detached_ctx(), &mut render),
            flui_rendering::RenderUpdateImpact::PAINT,
        );

        let layout_and_paint = RawImage {
            width: Some(px(100.0)),
            ..initial
        };
        assert_eq!(
            layout_and_paint.update_render_object(&detached_ctx(), &mut render),
            flui_rendering::RenderUpdateImpact::LAYOUT,
            "LAYOUT already contains the eventual paint implied by changing width",
        );
    }

    #[test]
    fn raw_image_has_children_is_always_false() {
        let raw = RawImage {
            image: None,
            fit: ImageFit::Contain,
            alignment: ImageAlignment::Center,
            width: None,
            height: None,
        };
        assert!(!raw.has_children());
    }

    #[test]
    fn image_new_stores_a_failing_provider_without_panicking() {
        // Smoke test that the public `Image::new` constructor still accepts a
        // custom `ImageProvider` after the RenderView -> StatelessView split.
        let _widget = Image::new(AlwaysFails);
    }

    #[test]
    fn image_new_stores_a_provider_that_fails_after_first_call() {
        let widget = Image::new(FailsAfterFirstCall::new());
        assert_eq!(widget.fit, ImageFit::Contain);
    }
}
