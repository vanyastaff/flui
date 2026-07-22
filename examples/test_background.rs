//! Test Background Color - Simple test to verify background works
//!
//! This example creates a window with NO background to test if
//! our WM_ERASEBKGND handler works correctly.
//!
//! **Windows-only.** A stub `main` keeps the workspace `--all-targets`
//! build green on non-Windows hosts (the example uses Win32-specific
//! `WindowsPlatform`); the real body is gated on `target_os = "windows"`.

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("test_background is Windows-only; nothing to do on this platform.");
}

#[cfg(target_os = "windows")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use flui_platform::{WindowOptions, WindowsPlatform};
    use flui_types::geometry::{Size, px};

    println!("Testing Background Handling");
    println!();

    let platform: Box<dyn flui_platform::Platform> = Box::new(WindowsPlatform::new()?);

    let options = WindowOptions {
        title: "Background Test - Should be BLACK not white".to_string(),
        size: Size::new(px(800.0), px(600.0)),
        resizable: true,
        visible: true,
        decorated: true,
        min_size: None,
        max_size: None,
    };

    // Create window before running the event loop (run() takes ownership)
    let window = platform.open_window(options)?;

    println!("Window created");
    println!();
    println!("If background is:");
    println!("  - WHITE = WM_ERASEBKGND not working (Windows default)");
    println!("  - BLACK = WM_ERASEBKGND working! (no background drawn)");
    println!();

    platform.run(Box::new(move |_platform| {
        // Keep window alive via closure capture
        let _window = window;
    }));

    Ok(())
}
