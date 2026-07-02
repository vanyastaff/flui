//! [`Image`] widget — displays a bitmap image.

use std::path::PathBuf;
use std::sync::Arc;

use flui_objects::{ImageAlignment, ImageFit, RenderImage};
use flui_rendering::protocol::BoxProtocol;
use flui_types::geometry::px;
use flui_types::{Pixels, Size, painting::Image as PixelImage};
use flui_view::{RenderView, View, impl_render_view};

use crate::image::provider::{DirectImageProvider, FileImage, ImageProvider, MemoryImage};

/// Displays a bitmap image.
///
/// Wraps [`RenderImage`] and resolves the image source synchronously on each
/// rebuild. On resolution failure the widget renders an empty zero-sized box —
/// no panic; a `WARN`-level trace event is emitted so the failure is visible.
///
/// # Constructors
///
/// | Constructor | Source | Cost per rebuild |
/// |-------------|--------|------------------|
/// | [`from_image`] | Already-decoded [`PixelImage`] | O(1) Arc clone |
/// | [`memory`] | Encoded bytes in memory | Full decode |
/// | [`file`] | Local file read + decode | Blocking I/O + decode |
/// | [`new`] | Any [`ImageProvider`] impl | Provider-dependent |
///
/// The `network-images` feature exposes an HTTP/HTTPS placeholder constructor
/// (`Image::network`) while async image loading is being wired. It is disabled
/// by default so stable builds do not advertise a constructor that always
/// returns `AsyncNotWired`.
///
/// For static or frequently-rebuilt images, pre-decode once and use
/// [`from_image`] to avoid per-rebuild cost.
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
/// `RenderImage`. The sync-first [`ImageProvider`] design diverges from
/// Flutter's stream+cache provider to match FLUI's current sync rebuild path.
///
/// Deferred: async/stream loading, `loadingBuilder`, `errorBuilder`, image
/// cache. See [`ImageProvider`]'s module doc for the documented async
/// extension point.
///
/// [`from_image`]: Image::from_image
/// [`memory`]: Image::memory
/// [`file`]: Image::file
/// [`new`]: Image::new
/// [`width`]: Image::width
/// [`height`]: Image::height
#[derive(Clone, Debug)]
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

    /// Creates a typed placeholder for HTTP/HTTPS loading.
    ///
    /// Always renders an empty box until async network loading is integrated
    /// with the FLUI view layer. Pre-decode the image outside the widget tree
    /// and supply it via [`from_image`](Image::from_image) as a workaround.
    #[cfg(feature = "network-images")]
    pub fn network(url: impl Into<String>) -> Self {
        Self::new(crate::image::provider::NetworkImage::new(url))
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
}

impl RenderView for Image {
    type Protocol = BoxProtocol;
    type RenderObject = RenderImage;

    fn create_render_object(&self) -> RenderImage {
        // Resolve eagerly; on failure emit a warning and render a zero-sized
        // placeholder box. `intrinsic_size = Size::ZERO` gives
        // `constraints.smallest()` under loose layout so the box occupies no
        // space and does not panic.
        let mut render = match self.provider.resolve() {
            Ok(decoded) => RenderImage::from_image(decoded, self.fit, self.alignment),
            Err(err) => {
                tracing::warn!(
                    provider = ?self.provider,
                    error = %err,
                    "image provider failed on first render; showing empty placeholder box"
                );
                RenderImage::new(Size::ZERO, self.fit, self.alignment)
            }
        };
        render.set_width(self.width);
        render.set_height(self.height);
        render
    }

    fn update_render_object(&self, render: &mut RenderImage) {
        // Always push layout/paint config — cheap field writes.
        render.set_fit(self.fit);
        render.set_alignment(self.alignment);
        render.set_width(self.width);
        render.set_height(self.height);

        // Re-resolve on every update.
        //
        // For `DirectImageProvider` this is O(1) (Arc clone of the pixel
        // buffer). For `MemoryImage` and `FileImage`, the cost is a full
        // decode / I/O + decode per rebuild; callers who need to avoid that
        // cost should pre-decode and use [`Image::from_image`].
        //
        // Calling `set_image(None)` on failure keeps the previous
        // `intrinsic_size` in the render object (so the box retains its size)
        // but clears the painted pixel source — the box shows nothing until
        // the next successful resolution.
        match self.provider.resolve() {
            Ok(decoded) => render.set_image(Some(decoded)),
            Err(err) => {
                tracing::warn!(
                    provider = ?self.provider,
                    error = %err,
                    "image provider failed on update; clearing displayed image"
                );
                render.set_image(None);
            }
        }
    }

    fn has_children(&self) -> bool {
        false
    }

    fn visit_child_views(&self, _visitor: &mut dyn FnMut(&dyn View)) {}
}

impl_render_view!(Image);

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use flui_rendering::constraints::BoxConstraints;
    use flui_view::RenderView;

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

    #[test]
    fn create_render_object_uses_a_zero_size_placeholder_on_decode_failure() {
        let widget = Image::new(AlwaysFails);
        let render = widget.create_render_object();

        assert!(render.image().is_none());
        assert_eq!(render.compute_size(&loose()), Size::ZERO);
    }

    #[test]
    fn update_render_object_clears_the_image_but_keeps_the_intrinsic_size_on_resolve_failure() {
        let widget = Image::new(FailsAfterFirstCall::new());
        let mut render = widget.create_render_object();

        assert!(render.image().is_some(), "first resolve must succeed");
        let size_before = render.compute_size(&loose());
        assert_eq!(size_before, Size::new(px(40.0), px(30.0)));

        // Second resolve (inside update_render_object) fails.
        widget.update_render_object(&mut render);

        assert!(
            render.image().is_none(),
            "a failed re-resolve must clear the displayed image",
        );
        assert_eq!(
            render.compute_size(&loose()),
            size_before,
            "a failed re-resolve must NOT reset the intrinsic size -- the box \
             keeps its prior layout size, only the painted content clears",
        );
    }

    #[test]
    fn width_and_height_overrides_reach_the_render_object() {
        let widget = Image::new(AlwaysFails).width(100.0).height(80.0);
        let render = widget.create_render_object();

        assert_eq!(render.width(), Some(px(100.0)));
        assert_eq!(render.height(), Some(px(80.0)));
    }

    #[test]
    fn has_children_is_always_false() {
        let widget = Image::new(AlwaysFails);
        assert!(!widget.has_children());
    }
}
