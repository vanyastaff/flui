//! `image_demo` — interactive visual check of [`RenderImage`].
//!
//! Displays a real image (JPEG/PNG decoded via the `image` crate) in an
//! interactive window, cycling through all [`ImageFit`] modes to show how
//! [`RenderImage`] scales and aligns on real, variable-aspect photos.
//!
//! The demo goes through the full pipeline: View → Element → RenderImage
//! → layout → paint (fragment recording) → LayerTree → Scene → GPU.
//!
//! Run with: cargo run --example image_demo
//!
//! **Controls:**
//! - Click or press Space/Enter to cycle to the next fit mode
//! - Press 'Q' or Escape to quit

use flui_app::run_app;
use flui_rendering::objects::{ImageAlignment, ImageFit, RenderImage};
use flui_types::painting::Image as FluiImage;
use flui_view::{BuildContext, ElementBase, IntoView, RenderView, StatelessView, View, ViewExt};

/// Decoded image data shared across UI rebuilds.
#[derive(Clone)]
struct SharedImage {
    data: std::sync::Arc<FluiImage>,
}

/// Demo state: tracks which fit mode to display.
#[derive(Clone, Copy, Debug)]
enum DemoFitMode {
    Fill,
    Contain,
    Cover,
    ScaleDown,
    None,
}

impl DemoFitMode {
    #[allow(dead_code)]
    fn next(self) -> Self {
        match self {
            Self::Fill => Self::Contain,
            Self::Contain => Self::Cover,
            Self::Cover => Self::ScaleDown,
            Self::ScaleDown => Self::None,
            Self::None => Self::Fill,
        }
    }

    #[allow(dead_code)]
    fn label(self) -> &'static str {
        match self {
            Self::Fill => "Fill (stretch to box)",
            Self::Contain => "Contain (letterbox)",
            Self::Cover => "Cover (crop to fill)",
            Self::ScaleDown => "ScaleDown (contain or natural)",
            Self::None => "None (natural size, cropped)",
        }
    }
}

/// Render view for the image display.
#[derive(Clone)]
struct ImageDisplay {
    image: SharedImage,
    fit: DemoFitMode,
}

impl RenderView for ImageDisplay {
    type Protocol = flui_rendering::protocol::BoxProtocol;
    type RenderObject = RenderImage;

    fn create_render_object(&self) -> Self::RenderObject {
        let obj = RenderImage::from_image(
            (*self.image.data).clone(),
            match self.fit {
                DemoFitMode::Fill => ImageFit::Fill,
                DemoFitMode::Contain => ImageFit::Contain,
                DemoFitMode::Cover => ImageFit::Cover,
                DemoFitMode::ScaleDown => ImageFit::ScaleDown,
                DemoFitMode::None => ImageFit::None,
            },
            ImageAlignment::Center,
        );
        obj
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
    fit: DemoFitMode,
}

impl StatelessView for App {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        ImageDisplay {
            image: self.image.clone(),
            fit: self.fit,
        }
        .boxed()
    }
}

impl View for App {
    fn create_element(&self) -> Box<dyn ElementBase> {
        Box::new(flui_view::StatelessElement::new(
            self,
            flui_view::element::StatelessBehavior,
        ))
    }
}

fn load_image() -> anyhow::Result<SharedImage> {
    use std::path::PathBuf;

    let input = PathBuf::from(std::env::temp_dir()).join("flui_test_cat.jpg");
    println!("Loading image: {}", input.display());

    let decoded = image::open(&input)?.to_rgba8();
    let (iw, ih) = decoded.dimensions();
    println!("Source image: {iw}x{ih} ({} bytes RGBA)", decoded.len());

    let flui_image = FluiImage::from_rgba8(iw, ih, decoded.as_raw().clone());
    Ok(SharedImage {
        data: std::sync::Arc::new(flui_image),
    })
}

fn main() -> anyhow::Result<()> {
    let image = load_image()?;
    println!("Loaded. Starting app...\n");
    println!("Controls:");
    println!("  Click or Space/Enter: cycle fit mode");
    println!("  Q or Escape: quit\n");
    println!("Press Enter to launch window...");
    let _ = std::io::stdin().read_line(&mut String::new());

    run_app(App {
        image,
        fit: DemoFitMode::Fill,
    });

    Ok(())
}
