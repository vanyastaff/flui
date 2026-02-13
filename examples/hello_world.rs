//! Hello World - Minimal FLUI application

use flui_platform::{current_platform, WindowOptions};
use flui_types::geometry::{px, Size};

fn main() {
    println!("🚀 FLUI Hello World!");
    println!("Platform: {}", std::env::consts::OS);

    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let platform = current_platform().expect("Failed to initialize platform");
    println!("✅ Platform initialized: {:?}", platform.name());

    let displays = platform.displays();
    println!("📺 Found {} display(s):", displays.len());
    for (i, display) in displays.iter().enumerate() {
        println!(
            "  Display {}: {} ({}x{} @ {:.1}x scale)",
            i + 1,
            display.name(),
            display.bounds().size.width,
            display.bounds().size.height,
            display.scale_factor()
        );
    }

    println!("\n🪟 Creating window...");

    let window_options = WindowOptions {
        title: "Hello FLUI! 👋".to_string(),
        size: Size::new(px(800.0), px(600.0)),
        resizable: true,
        visible: true,
        decorated: true,
        min_size: None,
        max_size: None,
    };

    let platform_clone = platform.clone();

    platform.run(Box::new(move || {
        match platform_clone.open_window(window_options) {
            Ok(window) => {
                println!("✅ Window created successfully!");
                println!("   Logical size: {:?}", window.logical_size());
                println!("   Physical size: {:?}", window.physical_size());
                println!("   Scale factor: {:.1}x", window.scale_factor());
                println!("\n⏱️  Window will stay open for 10 seconds...");
                std::thread::sleep(std::time::Duration::from_secs(10));
                println!("\n👋 Closing application...");
            }
            Err(e) => eprintln!("❌ Failed to create window: {}", e),
        }
        platform_clone.quit();
    }));

    println!("🏁 Application finished!");
}
