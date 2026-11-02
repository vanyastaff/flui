//! [`Image`] widget — displays a bitmap image.

use std::path::PathBuf;
use std::sync::Arc;

use flui_foundation::geometry::{Rect, Size};
use flui_objects::RenderImage;
use flui_painting::paint::Image as PixelImage;
use flui_painting::paint::ImageRepeat;
use flui_painting::{Alignment, BoxFit, ImagePlacement};
use flui_rendering::protocol::BoxProtocol;
use flui_view::prelude::{LifecycleContext, StatefulView, ViewState};
use flui_view::{BoxedView, BuildContext, IntoView, RenderView, View, ViewExt, impl_render_view};

use crate::image::provider::{DirectImageProvider, FileImage, ImageProvider, MemoryImage};

/// The widget's placement before any builder call.
const DEFAULT_PLACEMENT: ImagePlacement =
    ImagePlacement::fitted(BoxFit::Contain, Alignment::CENTER);

/// Displays a bitmap image.
///
/// Resolves the image source synchronously or asynchronously — see
/// [`ImageProvider::cache_key`] — and displays it via a private `RawImage`
/// leaf render view (`Image` is the resolver; `RawImage` is the dumb leaf that
/// just paints an already-decoded image).
///
/// On resolution failure the widget renders an empty zero-sized box — no
/// panic; a `WARN`-level trace event is emitted so the failure is visible.
///
/// # Constructors
///
/// | Constructor | Source | Path |
/// |-------------|--------|------|
/// | [`from_image`] | Already-decoded [`PixelImage`] | Sync, O(1) Arc clone |
/// | [`memory`] | Encoded bytes in memory | Async — decoded on the compute lane, once per bytes |
/// | [`file`] | Local file read + decode | Async — read on the IO lane, decoded on the compute lane, once per path |
/// | `asset` | `flui-assets` asset path | Async — cached, coalesced, off-thread |
/// | `network` | HTTP/HTTPS URL | Async — cached, coalesced, off-thread |
/// | [`new`] | Any [`ImageProvider`] impl | Provider-dependent |
///
/// `asset`/`network` require the `asset-images` feature (hence the plain,
/// non-linked names above — they do not exist in this doc build); it is off
/// by default so stable builds do not pull in `flui-assets` unless asked
/// for.
///
/// # Async dispatch
///
/// When [`ImageProvider::cache_key`] returns `Some(key)`, `Image` subscribes
/// to that key for as long as it stays mounted. Subscribing first probes
/// [`ImageProvider::cached`] synchronously — a hit (e.g. after
/// unmount+remount, or a second widget mounted with the same key) renders
/// immediately with **no placeholder frame**. A miss shows the same empty-box
/// placeholder a sync failure would, and the render updates in place once the
/// load answers: the provider's [`DecodeJob`](super::DecodeJob), run on the
/// UI runtime's background lanes, or else its
/// [`ImageProvider::resolve_async`]. On a host without background lanes a
/// decode job runs on the owner thread, so the image still shows, a frame
/// late. `memory` and `file` decode per widget; for `asset`/`network`, the
/// asset registry keeps the decoded images, and two widgets mounted with the
/// same source while a load is in flight share ONE load rather than starting
/// two; unmounting the last of them cancels it.
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
/// the same frame the change lands — a stale image under a changed caption is
/// the failure that default exists to prevent.
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
/// # Scope
///
/// `Image` is a one-shot resolver, not an image stream: no chunk/progress
/// events, no multi-frame (animated-image) support — the view is
/// single-frame. Revisit when animated images land.
///
/// Deferred (tracked, not silently missing): frame, loading and error
/// builders (an error renders the same empty box as no data, with a
/// `tracing::warn!`), device-pixel-ratio-based cache-key scaling, font
/// unification, and a decoded-image cache owned by the UI runtime that
/// `memory` and `file` widgets with equal keys would share (`docs/gaps.md`
/// entry 41).
///
/// [`from_image`]: Image::from_image
/// [`memory`]: Image::memory
/// [`file`]: Image::file
/// [`new`]: Image::new
/// [`width`]: Image::width
/// [`height`]: Image::height
#[derive(Clone, Debug, StatefulView)]
pub struct Image {
    provider: Arc<dyn ImageProvider + Send + Sync>,
    placement: ImagePlacement,
    width: Option<f64>,
    height: Option<f64>,
    gapless_playback: bool,
}

impl Image {
    /// Creates an `Image` widget backed by the given provider.
    ///
    /// Defaults: [`BoxFit::Contain`] at [`Alignment::CENTER`], not repeated,
    /// no forced width or height.
    ///
    /// `provider` must be `'static` because the widget is stored in the
    /// element tree; it must be `Send + Sync` (implied by [`ImageProvider`]'s
    /// supertraits) because the tree may be accessed from multiple threads.
    pub fn new(provider: impl ImageProvider + 'static) -> Self {
        Self {
            provider: Arc::new(provider),
            placement: DEFAULT_PLACEMENT,
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

    /// Creates an `Image` that decodes `bytes` (PNG, JPEG, GIF, …) on the
    /// compute lane, once for as long as a rebuild passes equal bytes.
    ///
    /// Shows the placeholder until the decode answers. Requires the
    /// `flui-widgets/images` feature; without it the widget renders an empty
    /// box.
    pub fn memory(bytes: impl Into<Vec<u8>>) -> Self {
        Self::new(MemoryImage::new(bytes))
    }

    /// Creates an `Image` that reads a local file on the IO lane and decodes
    /// it on the compute lane, once for as long as a rebuild passes the same
    /// path.
    ///
    /// Shows the placeholder until the decode answers. Requires the
    /// `flui-widgets/images` feature; without it the widget renders an empty
    /// box.
    pub fn file(path: impl Into<PathBuf>) -> Self {
        Self::new(FileImage::new(path))
    }

    /// Creates an `Image` that loads and decodes `path` asynchronously
    /// through `registry`, a `flui-assets` asset registry.
    ///
    /// `registry` is an explicit argument, so the load runs on whichever
    /// background runtime and byte-loader cache the application already owns.
    /// See the [async dispatch](#async-dispatch) section above.
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
    /// Requires the `flui-widgets/asset-images` feature. The URL is fetched with
    /// the host's HTTP capability (`flui_platform::http::HttpCapability`).
    #[cfg(feature = "asset-images")]
    pub fn network(registry: Arc<flui_assets::AssetRegistry>, url: impl Into<String>) -> Self {
        Self::new(super::NetworkImage::new(registry, url))
    }

    /// Sets how the image is scaled into the laid-out box.
    ///
    /// Defaults to [`BoxFit::Contain`]. Replaces a centre slice, which always
    /// fills the box.
    #[must_use]
    pub fn fit(self, fit: BoxFit) -> Self {
        self.fitted(|placed, _, _| *placed = fit)
    }

    /// Sets where the fitted image sits in the box; on a cropping fit, also
    /// which part of the image stays visible.
    ///
    /// Defaults to [`Alignment::CENTER`]. Replaces a centre slice.
    #[must_use]
    pub fn alignment(self, alignment: Alignment) -> Self {
        self.fitted(|_, aligned, _| *aligned = alignment)
    }

    /// Repeats the fitted image to cover the rest of the box.
    ///
    /// Defaults to [`ImageRepeat::NoRepeat`]. Replaces a centre slice.
    #[must_use]
    pub fn repeat(self, repeat: ImageRepeat) -> Self {
        self.fitted(|_, _, repeated| *repeated = repeat)
    }

    /// Paints the image as a nine-slice that fills the box: `center`, in
    /// decoded image pixels, stretches; the corners keep their size.
    ///
    /// Replaces any fit, alignment or repetition.
    #[must_use]
    pub fn center_slice(mut self, center: Rect) -> Self {
        self.placement = ImagePlacement::sliced(center);
        self
    }

    /// Sets the complete placement at once.
    #[must_use]
    pub fn placement(mut self, placement: ImagePlacement) -> Self {
        self.placement = placement;
        self
    }

    /// Edits the fitted placement, starting from the default when the image
    /// was sliced.
    fn fitted(mut self, edit: impl FnOnce(&mut BoxFit, &mut Alignment, &mut ImageRepeat)) -> Self {
        if !matches!(self.placement, ImagePlacement::Fitted { .. }) {
            self.placement = DEFAULT_PLACEMENT;
        }
        if let ImagePlacement::Fitted {
            fit,
            alignment,
            repeat,
        } = &mut self.placement
        {
            edit(fit, alignment, repeat);
        }
        self
    }

    /// Forces a specific logical width in pixels.
    ///
    /// Folded into the layout constraints (`tighten`). If height is not also
    /// forced, the image's aspect ratio determines the height axis.
    #[must_use]
    pub fn width(mut self, width_px: f64) -> Self {
        self.width = Some(width_px);
        self
    }

    /// Forces a specific logical height in pixels.
    ///
    /// Folded into the layout constraints (`tighten`). If width is not also
    /// forced, the image's aspect ratio determines the width axis.
    #[must_use]
    pub fn height(mut self, height_px: f64) -> Self {
        self.height = Some(height_px);
        self
    }

    /// Keeps the previously decoded frame on screen while a new provider
    /// loads, instead of clearing to the placeholder the moment the provider
    /// key changes.
    ///
    /// Defaults to `false`, because when the image is coupled to other content that has
    /// already changed (an avatar beside a name), holding the old frame shows
    /// a combination that was never true. Turn it on for a sequence of frames
    /// that are all views of the same thing, where a placeholder flash is the
    /// worse artifact.
    ///
    /// Applies to every provider with a [`cache_key`](ImageProvider::cache_key),
    /// [`memory`](Image::memory) and [`file`](Image::file) among them. A
    /// provider with no key resolves inline on every build and has no frame
    /// to hold on to.
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
            placement: self.placement,
            width: self.width,
            height: self.height,
        }
        .boxed()
    }
}

/// Async resolution is a subscription with a lifecycle (start, cancel,
/// retire), held in the state.
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
#[derive(Debug)]
pub struct ImageState {
    resolver: super::resolve::ImageResolver,
}

impl ViewState<Image> for ImageState {
    /// Resolve the provider the widget mounted with.
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.resolver.init(ctx);
    }

    /// Paint whatever frame the resolver has published.
    ///
    /// A provider that opted out of async resolution (no cache key) never
    /// publishes anything, and resolves inline here instead.
    fn build(&self, view: &Image, _ctx: &dyn BuildContext) -> impl IntoView {
        if !self.resolver.is_subscribed() {
            return view.build_sync();
        }
        view.raw(self.resolver.frame())
    }

    /// Re-resolve when the cache key changed.
    fn did_update_view(&mut self, _old_view: &Image, new_view: &Image) {
        self.resolver
            .did_update(Arc::clone(&new_view.provider), new_view.gapless_playback);
    }

    /// Cancel the load this widget owns.
    fn dispose(&mut self) {
        self.resolver.dispose();
    }
}

/// The leaf render view [`Image`] builds into once its provider has been
/// resolved (or has failed): a dumb view over an
/// already-decoded (or absent) image, with no provider, no resolution logic.
///
/// Private: [`Image`] is the only public entry point.
#[derive(Clone, Debug)]
struct RawImage {
    image: Option<PixelImage>,
    placement: ImagePlacement,
    width: Option<f64>,
    height: Option<f64>,
}

impl RenderView for RawImage {
    type Protocol = BoxProtocol;
    type RenderObject = RenderImage;

    fn create_render_object(&self, _ctx: &flui_view::RenderObjectContext<'_>) -> RenderImage {
        // `intrinsic_size = Size::ZERO` gives `constraints.smallest()` under
        // loose layout, so an absent image occupies no space and does not
        // panic.
        let mut render = match &self.image {
            Some(decoded) => RenderImage::from_image(decoded.clone(), self.placement),
            None => RenderImage::new(Size::ZERO, self.placement),
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
        let mut impact = render.set_placement(self.placement)
            | render.set_width(self.width)
            | render.set_height(self.height);

        // The box is sized by the image it is showing (the smallest
        // constraints while there is no image). `RenderImage` itself *retains* the last
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
