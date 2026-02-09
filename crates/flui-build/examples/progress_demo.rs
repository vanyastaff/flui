//! Demo of progress reporting for build operations.
//!
//! This example shows how the progress indicators look for different platforms.

use flui_build::progress::{BuildPhase, BuildProgress, ProgressManager};
use std::time::Duration;
use tokio::time::sleep;

#[tokio::main]
async fn main() {
    let manager = ProgressManager::new();

    // Simulate building multiple platforms in parallel
    let handles = vec![
        tokio::spawn(build_android(manager.create_build("Android"))),
        tokio::spawn(build_web(manager.create_build("Web"))),
        tokio::spawn(build_desktop(manager.create_build("Desktop"))),
    ];

    // Wait for all builds to complete
    for handle in handles {
        let _ = handle.await;
    }

    manager.join();
    println!("\n✓ All builds completed!");
}

async fn build_android(mut progress: BuildProgress) {
    // Validate phase
    progress.start_phase(BuildPhase::Validate, Some("Checking tools..."));
    sleep(Duration::from_millis(500)).await;
    progress.set_message("Found cargo-ndk");
    sleep(Duration::from_millis(300)).await;
    progress.finish_phase("Environment validated");
    progress.set_progress(25);

    // Build Rust phase
    progress.start_phase(BuildPhase::BuildRust, Some("Compiling Rust..."));
    sleep(Duration::from_millis(800)).await;
    progress.set_message("Building for aarch64-linux-android");
    sleep(Duration::from_millis(1200)).await;
    progress.set_message("Linking flui_app.so");
    sleep(Duration::from_millis(400)).await;
    progress.finish_phase("Rust libraries built (2.3 MB)");
    progress.set_progress(60);

    // Build Platform phase
    progress.start_phase(BuildPhase::BuildPlatform, Some("Building APK..."));
    sleep(Duration::from_millis(600)).await;
    progress.set_message("Running Gradle assembleDebug");
    sleep(Duration::from_millis(1500)).await;
    progress.set_message("Packaging APK");
    sleep(Duration::from_millis(500)).await;
    progress.finish_phase("APK built (8.5 MB)");
    progress.set_progress(100);

    progress.finish("Android build completed");
}

async fn build_web(mut progress: BuildProgress) {
    // Validate phase
    progress.start_phase(BuildPhase::Validate, Some("Checking tools..."));
    sleep(Duration::from_millis(400)).await;
    progress.set_message("Found wasm-pack");
    sleep(Duration::from_millis(200)).await;
    progress.finish_phase("Environment validated");
    progress.set_progress(25);

    // Build Rust phase
    progress.start_phase(BuildPhase::BuildRust, Some("Compiling to WASM..."));
    sleep(Duration::from_millis(900)).await;
    progress.set_message("Building for wasm32-unknown-unknown");
    sleep(Duration::from_millis(1100)).await;
    progress.set_message("Optimizing WASM");
    sleep(Duration::from_millis(600)).await;
    progress.finish_phase("WASM built (1.2 MB)");
    progress.set_progress(70);

    // Build Platform phase
    progress.start_phase(BuildPhase::BuildPlatform, Some("Packaging web assets..."));
    sleep(Duration::from_millis(400)).await;
    progress.set_message("Copying HTML and JS");
    sleep(Duration::from_millis(300)).await;
    progress.finish_phase("Web build packaged");
    progress.set_progress(100);

    progress.finish("Web build completed");
}

async fn build_desktop(mut progress: BuildProgress) {
    // Validate phase
    progress.start_phase(BuildPhase::Validate, Some("Checking tools..."));
    sleep(Duration::from_millis(300)).await;
    progress.finish_phase("Environment validated");
    progress.set_progress(20);

    // Build Rust phase
    progress.start_phase(BuildPhase::BuildRust, Some("Compiling Rust..."));
    sleep(Duration::from_millis(700)).await;
    progress.set_message("Building for x86_64-pc-windows-msvc");
    sleep(Duration::from_millis(1000)).await;
    progress.set_message("Linking flui_app.dll");
    sleep(Duration::from_millis(300)).await;
    progress.finish_phase("Native library built (1.8 MB)");
    progress.set_progress(80);

    // Build Platform phase
    progress.start_phase(BuildPhase::BuildPlatform, Some("Copying artifacts..."));
    sleep(Duration::from_millis(200)).await;
    progress.finish_phase("Desktop build completed");
    progress.set_progress(100);

    progress.finish("Desktop build completed");
}
