//! `image_demo` — interactive visual check of [`RenderImage`].
//!
//! Displays a real image (JPEG/PNG decoded via the `image` crate) in an
//! interactive window showing how [`RenderImage`] scales and aligns
//! on real, variable-aspect photos.
//!
//! The demo goes through the full pipeline: View → Element → RenderImage
//! → layout → paint (fragment recording) → LayerTree → Scene → GPU.
//!
//! Run with: cargo run --example image_demo

use flui_app::run_app;
use flui_objects::{ImageAlignment, ImageFit, RenderImage};
use flui_types::painting::Image as FluiImage;
use flui_view::{BuildContext, IntoView, RenderView, StatelessView, View, ViewExt};

/// Decoded image data shared across UI rebuilds.
#[derive(Clone)]
struct SharedImage {
    data: std::sync::Arc<FluiImage>,
}

/// Render view for the image display.
#[derive(Clone)]
struct ImageDisplay {
    image: SharedImage,
}

impl RenderView for ImageDisplay {
    type Protocol = flui_rendering::protocol::BoxProtocol;
    type RenderObject = RenderImage;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderImage::from_image(
            (*self.image.data).clone(),
            ImageFit::Contain,
            ImageAlignment::Center,
        )
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        *render_object = self.create_render_object();
    }
}

flui_view::impl_render_view!(ImageDisplay);

/// Stateless app that wraps the image display.
#[derive(Clone)]
struct App {
    image: SharedImage,
}

impl StatelessView for App {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        ImageDisplay {
            image: self.image.clone(),
        }
        .boxed()
    }
}

impl View for App {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

fn load_image() -> anyhow::Result<SharedImage> {
    let input = std::env::args()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("flui_test_cat.jpg"));
    println!("Loading image: {}", input.display());

    let decoded = image::open(&input)?.to_rgba8();
    let (iw, ih) = decoded.dimensions();
    let rgba = decoded.into_raw();
    println!("Source image: {iw}x{ih} ({} bytes RGBA)", rgba.len());

    let flui_image = FluiImage::from_rgba8(iw, ih, rgba);

    // Verify the image was created correctly
    println!("FluiImage size: {:?}", flui_image.size());
    println!("FluiImage byte_count: {}", flui_image.byte_count());

    Ok(SharedImage {
        data: std::sync::Arc::new(flui_image),
    })
}

fn main() -> anyhow::Result<()> {
    let image = load_image()?;
    println!("Loaded. Starting app...");

    run_app(App { image });

    Ok(())
}
