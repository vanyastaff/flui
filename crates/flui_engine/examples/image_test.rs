//! Image rendering test - demonstrates image drawing capabilities
//!
//! This example shows:
//! - Loading and displaying images from network using NetworkImage
//! - Scaling images to fit different rectangles
//! - Applying opacity and transformations
//! - Partial image rendering (source rectangles)

use flui_engine::{App, AppLogic, Paint, Painter};
use flui_types::{
    events::Event,
    painting::{Image, ImageConfiguration, ImageError, ImageProvider, NetworkImage},
    Color, Offset, Point, Rect,
};
use std::sync::{mpsc, Arc};

struct ImageTestApp {
    /// The cat image loaded from URL
    cat_image: Option<Arc<Image>>,

    /// Loading state
    loading: bool,

    /// Receiver for async image loading
    image_receiver: Option<mpsc::Receiver<Result<Image, ImageError>>>,
}

impl ImageTestApp {
    fn new() -> Self {
        println!("Starting cat image download...");

        let url = "https://upload.wikimedia.org/wikipedia/commons/thumb/c/cd/Stray_kitten_Rambo002.jpg/1200px-Stray_kitten_Rambo002.jpg";

        // Use NetworkImage provider from flui_types
        let provider = NetworkImage::new(url);
        let config = ImageConfiguration::new();

        // Create channel for async communication
        let (tx, rx) = mpsc::channel();

        // Spawn async task to load the image
        std::thread::spawn(move || {
            // Create tokio runtime for async operations
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let result = provider.load(&config).await;
                if let Ok(ref image) = result {
                    println!("Image loaded: {}x{}", image.width(), image.height());
                }
                tx.send(result).ok();
            });
        });

        Self {
            cat_image: None,
            loading: true,
            image_receiver: Some(rx),
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
        // Poll for loaded image
        if let Some(ref rx) = self.image_receiver {
            if let Ok(result) = rx.try_recv() {
                match result {
                    Ok(image) => {
                        self.cat_image = Some(Arc::new(image));
                        self.loading = false;
                    }
                    Err(e) => {
                        eprintln!("Failed to load image: {}", e);
                        self.loading = false;
                    }
                }
                self.image_receiver = None; // Done with receiver
            }
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

    println!("=== FLUI Image Rendering Test ===");
    println!("Demonstrates NetworkImage provider from flui_types");
    println!();

    let app = App::new()
        .title("Image Rendering Test")
        .size(1200, 800);

    let logic = ImageTestApp::new();

    app.run(logic).unwrap();
}
