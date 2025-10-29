//! Image rendering test - demonstrates image drawing capabilities
//!
//! This example shows:
//! - Loading and displaying images
//! - Scaling images to fit different rectangles
//! - Applying opacity and transformations
//! - Partial image rendering (source rectangles)

use flui_engine::{App, AppLogic, Paint, Painter};
use flui_types::{events::Event, painting::Image, Color, Offset, Point, Rect};
use std::sync::Arc;

struct ImageTestApp {
    /// The cat image loaded from URL
    cat_image: Option<Arc<Image>>,

    /// Loading state
    loading: bool,
}

impl ImageTestApp {
    fn new() -> Self {
        Self {
            cat_image: None,
            loading: true,
        }
    }

    /// Download and decode the cat image
    fn load_cat_image(&mut self) {
        println!("Downloading cat image...");

        // Download the image
        let url = "https://upload.wikimedia.org/wikipedia/commons/thumb/c/cd/Stray_kitten_Rambo002.jpg/1200px-Stray_kitten_Rambo002.jpg";

        match ureq::get(url).call() {
            Ok(response) => {
                let mut bytes = Vec::new();
                if let Ok(_) = response.into_reader().read_to_end(&mut bytes) {
                    // Decode JPEG to RGBA8
                    match image::load_from_memory(&bytes) {
                        Ok(img) => {
                            let rgba = img.to_rgba8();
                            let (width, height) = rgba.dimensions();

                            println!("Image loaded: {}x{}", width, height);

                            self.cat_image = Some(Arc::new(Image::from_rgba8(
                                width,
                                height,
                                rgba.into_raw(),
                            )));
                            self.loading = false;
                        }
                        Err(e) => {
                            eprintln!("Failed to decode image: {}", e);
                            self.loading = false;
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("Failed to download image: {}", e);
                self.loading = false;
            }
        }
    }
}

impl AppLogic for ImageTestApp {
    fn on_event(&mut self, event: &Event) -> bool {
        match event {
            Event::Window(window_event) => {
                if let flui_types::events::WindowEvent::CloseRequested = window_event {
                    return false;
                }
            }
            _ => {}
        }
        true
    }

    fn update(&mut self, _delta_time: f32) {
        // Load image on first frame
        if self.loading && self.cat_image.is_none() {
            self.load_cat_image();
        }
    }

    fn render(&mut self, painter: &mut dyn Painter) {
        // Background
        painter.rect(
            Rect::from_xywh(0.0, 0.0, 1200.0, 800.0),
            &Paint::fill(Color::rgb(245, 245, 250)),
        );

        // Title
        painter.text(
            "Image Rendering Test",
            Point::new(450.0, 30.0),
            24.0,
            &Paint::fill(Color::rgb(40, 40, 40)),
        );

        if self.loading {
            // Show loading message
            painter.text(
                "Loading cat image...",
                Point::new(500.0, 400.0),
                18.0,
                &Paint::fill(Color::rgb(100, 100, 100)),
            );
            return;
        }

        if let Some(ref image) = self.cat_image {
            // 1. Original size (scaled to fit)
            painter.text(
                "1. Original (scaled to fit)",
                Point::new(50.0, 70.0),
                14.0,
                &Paint::fill(Color::BLACK),
            );
            painter.draw_image(
                image,
                None, // Full image
                Rect::from_xywh(50.0, 90.0, 250.0, 250.0),
                &Paint::default(),
            );

            // 2. Stretched (different aspect ratio)
            painter.text(
                "2. Stretched",
                Point::new(350.0, 70.0),
                14.0,
                &Paint::fill(Color::BLACK),
            );
            painter.draw_image(
                image,
                None,
                Rect::from_xywh(350.0, 90.0, 150.0, 250.0),
                &Paint::default(),
            );

            // 3. With opacity
            painter.text(
                "3. With 50% Opacity",
                Point::new(550.0, 70.0),
                14.0,
                &Paint::fill(Color::BLACK),
            );
            painter.save();
            painter.set_opacity(0.5);
            painter.draw_image(
                image,
                None,
                Rect::from_xywh(550.0, 90.0, 200.0, 200.0),
                &Paint::default(),
            );
            painter.restore();

            // 4. Rotated
            painter.text(
                "4. Rotated 45°",
                Point::new(800.0, 70.0),
                14.0,
                &Paint::fill(Color::BLACK),
            );
            painter.save();
            painter.translate(Offset::new(900.0, 200.0));
            painter.rotate(std::f32::consts::PI / 4.0);
            painter.draw_image(
                image,
                None,
                Rect::from_xywh(-100.0, -100.0, 200.0, 200.0),
                &Paint::default(),
            );
            painter.restore();

            // 5. Partial image (cropped)
            painter.text(
                "5. Cropped (center portion)",
                Point::new(50.0, 370.0),
                14.0,
                &Paint::fill(Color::BLACK),
            );

            let img_width = image.width() as f32;
            let img_height = image.height() as f32;
            let crop_size = img_width.min(img_height) * 0.4;

            painter.draw_image(
                image,
                Some(Rect::from_xywh(
                    (img_width - crop_size) / 2.0,
                    (img_height - crop_size) / 2.0,
                    crop_size,
                    crop_size,
                )),
                Rect::from_xywh(50.0, 390.0, 200.0, 200.0),
                &Paint::default(),
            );

            // 6. Tiled (small repeated)
            painter.text(
                "6. Scaled down",
                Point::new(300.0, 370.0),
                14.0,
                &Paint::fill(Color::BLACK),
            );
            painter.draw_image(
                image,
                None,
                Rect::from_xywh(300.0, 390.0, 100.0, 100.0),
                &Paint::default(),
            );

            // 7. Very large
            painter.text(
                "7. Scaled up",
                Point::new(450.0, 370.0),
                14.0,
                &Paint::fill(Color::BLACK),
            );
            painter.draw_image(
                image,
                None,
                Rect::from_xywh(450.0, 390.0, 350.0, 350.0),
                &Paint::default(),
            );

            // Info
            painter.text(
                &format!("Image: {}x{} pixels", image.width(), image.height()),
                Point::new(50.0, 760.0),
                12.0,
                &Paint::fill(Color::rgb(100, 100, 100)),
            );
        } else {
            // Show error message
            painter.text(
                "Failed to load image. Check console for errors.",
                Point::new(400.0, 400.0),
                18.0,
                &Paint::fill(Color::rgb(200, 50, 50)),
            );
        }
    }
}

fn main() {
    env_logger::init();

    println!("Starting image test...");
    println!("This will download a cat image from Wikimedia Commons");

    let app = App::new()
        .title("Image Rendering Test")
        .size(1200, 800);

    let logic = ImageTestApp::new();

    app.run(logic).unwrap();
}
