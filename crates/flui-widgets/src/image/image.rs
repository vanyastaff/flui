//! [`Image`] widget — displays a bitmap image.

use std::path::PathBuf;
use std::sync::Arc;

use flui_objects::{ImageAlignment, ImageFit, RenderImage};
use flui_rendering::protocol::BoxProtocol;
use flui_types::geometry::px;
use flui_types::{Pixels, Size, painting::Image as PixelImage};
use flui_view::prelude::StatelessView;
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
/// When [`ImageProvider::cache_key`] returns `Some(key)`, `Image` first
/// probes the decode cache synchronously — a cache hit (e.g. after
/// unmount+remount, or a second widget mounted with the same key) renders
/// immediately with **no placeholder frame**. A miss wraps the render in a
/// [`FutureBuilder`](crate::FutureBuilder) keyed by `key`: the first frame
/// shows the same empty-box placeholder a sync failure would, and the render
/// updates in place once [`ImageProvider::resolve_async`] completes. Two
/// widgets mounted with the same key while a load is in flight share ONE load
/// (`image::decode_cache`'s in-flight coalescing) rather than starting two.
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
/// `tracing::warn!`), `gaplessPlayback`,
/// `ImageConfiguration`/`devicePixelRatio`-based cache-key scaling, an
/// `evict`/`clearLiveImages` cache-management API, and font unification.
///
/// [`from_image`]: Image::from_image
/// [`memory`]: Image::memory
/// [`file`]: Image::file
/// [`new`]: Image::new
/// [`width`]: Image::width
/// [`height`]: Image::height
#[derive(Clone, Debug, StatelessView)]
pub struct Image {
    // PORT-CHECK-OK-SP3: widget view type; `flui_types::painting::Image` is the pixel-data handle — distinct concepts at different crate layers
    provider: Arc<dyn ImageProvider + Send + Sync>,
    fit: ImageFit,
    alignment: ImageAlignment,
    width: Option<Pixels>,
    height: Option<Pixels>,
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

    /// Resolves the provider synchronously, warns and clears on failure,
    /// and builds the leaf [`RawImage`] view directly — no `FutureBuilder`.
    fn build_sync(&self) -> BoxedView {
        let image = match self.provider.resolve() {
            Ok(decoded) => Some(decoded),
            Err(err) => {
                tracing::warn!(
                    provider = ?self.provider,
                    error = %err,
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

/// Async dispatch path — only compiled under `asset-images`, since it needs
/// `image::decode_cache`'s `lru`/`futures-util`-backed engine. Without this
/// feature `Image` always takes the `build_sync` path, even for a custom
/// provider that overrides [`ImageProvider::cache_key`] — see that method's
/// doc for the honest fallback contract.
#[cfg(feature = "asset-images")]
impl Image {
    fn build_dispatch(&self) -> BoxedView {
        let Some(key) = self.provider.cache_key() else {
            return self.build_sync();
        };
        if let Some(cached) = super::decode_cache::cached(&key) {
            return self.raw(Some(cached));
        }
        self.build_async(key)
    }

    /// Wraps the leaf render in a [`FutureBuilder`](crate::FutureBuilder)
    /// keyed by `key`, subscribing to [`ImageProvider::resolve_async`].
    fn build_async(&self, key: super::ImageCacheKey) -> BoxedView {
        use crate::{FutureBuilder, FutureFactory, SnapshotBuilder};

        let provider = Arc::clone(&self.provider);
        let factory: FutureFactory<PixelImage, crate::image::ImageProviderError> =
            std::rc::Rc::new(move || provider.resolve_async());

        let fit = self.fit;
        let alignment = self.alignment;
        let width = self.width;
        let height = self.height;
        let builder: SnapshotBuilder<PixelImage, crate::image::ImageProviderError> =
            std::rc::Rc::new(move |_ctx, snapshot| {
                if let Some(err) = snapshot.error() {
                    tracing::warn!(
                        error = %err,
                        "image provider failed to resolve asynchronously; showing empty \
                         placeholder box"
                    );
                }
                RawImage {
                    image: snapshot.data().cloned(),
                    fit,
                    alignment,
                    width,
                    height,
                }
                .boxed()
            });

        FutureBuilder::keyed(Some(key), factory, builder).boxed()
    }
}

#[cfg(feature = "asset-images")]
impl StatelessView for Image {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        self.build_dispatch()
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
        render.set_width(self.width);
        render.set_height(self.height);
        render
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render: &mut RenderImage,
    ) {
        // Always push layout/paint config — cheap field writes.
        render.set_fit(self.fit);
        render.set_alignment(self.alignment);
        render.set_width(self.width);
        render.set_height(self.height);

        // `set_image(None)` on a since-cleared image keeps the previous
        // `intrinsic_size` in the render object (so the box retains its
        // size) but clears the painted pixel source — the box shows nothing
        // until the next `Some`.
        render.set_image(self.image.clone());
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
    fn update_render_object_clears_the_image_but_keeps_the_intrinsic_size_when_the_image_becomes_absent()
     {
        let with_image = RawImage {
            image: Some(PixelImage::from_rgba8(40, 30, vec![0u8; 40 * 30 * 4])),
            fit: ImageFit::Contain,
            alignment: ImageAlignment::Center,
            width: None,
            height: None,
        };
        let mut render = with_image.create_render_object(&detached_ctx());

        assert!(render.image().is_some());
        let size_before = render.compute_size(&loose());
        assert_eq!(size_before, Size::new(px(40.0), px(30.0)));

        let now_absent = RawImage {
            image: None,
            ..with_image
        };
        now_absent.update_render_object(&detached_ctx(), &mut render);

        assert!(
            render.image().is_none(),
            "an absent image on update must clear the displayed image",
        );
        assert_eq!(
            render.compute_size(&loose()),
            size_before,
            "clearing the image must NOT reset the intrinsic size -- the box \
             keeps its prior layout size, only the painted content clears",
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
