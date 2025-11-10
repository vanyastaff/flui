//! WebAssembly demo for Flui
//!
//! This example demonstrates running Flui in a web browser using WebAssembly.
//!
//! # Building for Web
//!
//! 1. Install wasm-pack:
//!    ```bash
//!    cargo install wasm-pack
//!    ```
//!
//! 2. Build for web:
//!    ```bash
//!    wasm-pack build --target web --out-dir crates/flui_app/examples/web crates/flui_app --example web_demo
//!    ```
//!
//! 3. Serve the web directory:
//!    ```bash
//!    # Using Python
//!    python -m http.server -d crates/flui_app/examples/web 8080
//!
//!    # Or using a simple HTTP server
//!    npx http-server crates/flui_app/examples/web -p 8080
//!    ```
//!
//! 4. Open http://localhost:8080 in your browser
//!
//! # Requirements
//!
//! - Browser with WebGPU support (Chrome 113+, Edge 113+)
//! - wasm-pack for building

use flui_core::render::LeafRender;
use flui_core::view::{AnyView, BuildContext, IntoElement, LeafView};
use flui_types::{BoxConstraints, Offset, Size};
use wasm_bindgen::prelude::*;

/// Simple demo view for WebAssembly
#[derive(Debug, Clone)]
struct WebDemo;

/// Simple render object that fills the canvas
#[derive(Debug)]
struct WebDemoRender;

impl LeafRender for WebDemoRender {
    type Metadata = ();

    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Fill available space
        constraints.biggest()
    }

    fn paint(&self, _offset: Offset) -> flui_core::BoxedLayer {
        // Just return an empty container
        // In a real app, you'd draw something here
        Box::new(flui_engine::ContainerLayer::new())
    }
}

impl View for WebDemo {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        Leaf(WebDemoRender, ())
    }
}

/// Entry point for WebAssembly
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn start() {
    // Initialize panic hook for better error messages
    flui_app::wasm::init_panic_hook();

    // Log to browser console
    wasm_bindgen_futures::spawn_local(async {
        // Create root view
        let root_view: Box<dyn AnyView> = Box::new(WebDemo);

        // Run in browser
        flui_app::wasm::run_in_browser_impl(root_view).await;
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("This example is for WebAssembly only.");
    eprintln!("Build with: wasm-pack build --target web --out-dir crates/flui_app/examples/web crates/flui_app --example web_demo");
}
