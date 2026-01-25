# Web Platform Improvements Roadmap for FLUI

**Document Version:** 1.0  
**Last Updated:** January 25, 2026  
**Target Platform:** Web (WebAssembly + WebGPU)  
**FLUI Crates:** `flui-platform` (web module), `flui_engine` (wgpu web backend)

---

## Executive Summary

This document outlines modern web platform features and improvements relevant to FLUI framework implementation. The web platform has reached a critical inflection point in 2025-2026:

- **WebGPU Universal Support** - All major browsers (Chrome, Firefox 141, Safari 26, Edge) shipped WebGPU by November 2025
- **WebAssembly 3.0** - Released September 2025 with GC, Tail Calls, SIMD improvements
- **WASI 0.3** - Expected February 2026 with async/await support, Component Model integration
- **wgpu in Firefox** - Firefox's WebGPU implementation uses Rust wgpu (same as FLUI!)
- **WebCodecs API** - Hardware video encoding/decoding in all major browsers (Safari partial)
- **WebTransport** - QUIC-based low-latency networking, replacing WebRTC for some use cases

**Key Milestones:**
1. **WebGPU Critical Mass** (November 2025) - All major browsers ship WebGPU
2. **Firefox 141** (July 2025) - First WebGPU release (Windows), uses wgpu
3. **Safari 26** (June 2025) - WebGPU enabled by default (macOS Tahoe 26, iOS 26)
4. **Wasm 3.0** (September 2025) - GC, threads, SIMD as "live" standard
5. **WASI 0.3** (Expected February 2026) - Native async, Component Model

---

## 1. WebGPU - Universal GPU API

### 1.1 Browser Support Status ⭐⭐⭐⭐⭐

**Priority:** CRITICAL  
**Status:** Universal support achieved November 2025  
**Impact:** Native GPU rendering in all major browsers

#### Browser Implementation Timeline

| Browser | Version | Release Date | Backend | Status |
|---------|---------|--------------|---------|--------|
| **Chrome** | 113+ | April 2023 | Dawn (C++) | ✅ Stable |
| **Edge** | 113+ | April 2023 | Dawn (C++) | ✅ Stable |
| **Safari** | 26+ | June 2025 | WebKit (C++) | ✅ Stable |
| **Firefox** | 141+ | July 2025 | wgpu (Rust) | ✅ Stable |

**Platform Support:**
- **Chrome 113+**: Windows (D3D12), ChromeOS (Vulkan), macOS (Metal), Android 121+ (Vulkan)
- **Safari 26+**: macOS Tahoe 26, iOS 26, iPadOS 26, visionOS 26 (Metal)
- **Firefox 141+**: Windows (D3D12 via wgpu), macOS Tahoe 26 ARM64 (Metal via wgpu)
- **Firefox 2026**: Linux (Vulkan), Android support planned

#### WebGPU Implementations

**Two Major Implementations:**

1. **Dawn (C++)** - Powers Chrome, Edge
   - Maps to Direct3D 12 (Windows), Metal (macOS/iOS), Vulkan (Linux/Android)
   - Developed by Google
   - C++ codebase

2. **wgpu (Rust)** - Powers Firefox
   - Written in Rust (same as FLUI uses!)
   - Maps to D3D12, Metal, Vulkan through unified abstraction
   - Shared with Servo, Deno
   - **Direct compatibility with FLUI's rendering stack**

#### Implications for FLUI

```rust
// crates/flui_engine/src/web/mod.rs

#[cfg(target_arch = "wasm32")]
pub struct WebGpuRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface,
}

impl WebGpuRenderer {
    pub async fn new(canvas: web_sys::HtmlCanvasElement) -> Result<Self> {
        // FLUI already uses wgpu, so web target "just works"!
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::BROWSER_WEBGPU,  // Use native WebGPU
            ..Default::default()
        });

        // Create surface from canvas
        let surface = instance.create_surface_from_canvas(&canvas)?;

        // Request adapter and device
        let adapter = instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }).await.ok_or_else(|| anyhow::anyhow!("No WebGPU adapter"))?;

        let (device, queue) = adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("FLUI WebGPU Device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
            },
            None
        ).await?;

        Ok(Self { device, queue, surface })
    }

    pub fn render(&mut self, view_tree: &ViewTree) {
        // Same rendering code as desktop!
        // wgpu abstracts WebGPU vs native backends
    }
}
```

**Key Advantage:** FLUI already uses wgpu, so the same rendering code works on:
- Native desktop (Vulkan/Metal/D3D12)
- Web (WebGPU)
- Minimal platform-specific code needed

**Crate Assignment:** `flui_engine` (web backend via wgpu)

---

### 1.2 WebGPU Features Available ⭐⭐⭐⭐⭐

**Priority:** CRITICAL  
**Status:** Full feature set in all browsers  
**Impact:** High-performance 3D graphics, compute shaders, AI

#### WebGPU Capabilities

**Core Features:**
- **Compute Shaders** - GPU-accelerated computation (physics, AI inference, image processing)
- **Render Pipelines** - Modern GPU rendering (FLUI's primary use case)
- **Texture Compression** - BC, ETC2, ASTC formats
- **Depth/Stencil** - Advanced rendering techniques
- **MSAA** - Anti-aliasing
- **Indirect Drawing** - GPU-driven rendering

**Use Cases for FLUI:**

1. **UI Rendering** - Fast 2D/3D widget rendering
2. **Effects** - Blur, shadows, gradients on GPU
3. **Animations** - Smooth 60fps+ animations
4. **Text Rendering** - GPU-accelerated glyph rasterization
5. **Compute** - Layout calculations on GPU (future optimization)

#### AI in Browser with WebGPU

**2025-2026 Trend:** AI inference directly in browser using WebGPU compute shaders.

**Example Use Cases:**
- On-device ML models (privacy-preserving)
- Real-time image processing
- Natural language processing
- Smart autocomplete/suggestions

**Implementation Example:**

```rust
// crates/flui-ai/src/web/inference.rs

#[cfg(target_arch = "wasm32")]
pub struct WebGpuInference {
    device: wgpu::Device,
    compute_pipeline: wgpu::ComputePipeline,
}

impl WebGpuInference {
    /// Run ML inference on GPU
    pub fn infer(&self, input_tensor: &[f32]) -> Vec<f32> {
        // Create GPU buffers
        let input_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Input Tensor"),
            contents: bytemuck::cast_slice(input_tensor),
            usage: wgpu::BufferUsages::STORAGE,
        });

        // Dispatch compute shader for inference
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut compute_pass = encoder.begin_compute_pass(&Default::default());
            compute_pass.set_pipeline(&self.compute_pipeline);
            compute_pass.set_bind_group(0, &bind_group, &[]);
            compute_pass.dispatch_workgroups(
                (input_tensor.len() as u32 + 63) / 64,
                1,
                1
            );
        }

        // Read results back
        // Implementation details...
        vec![]
    }
}
```

**Crate Assignment:** `flui-ai` (web ML inference)

---

### 1.3 WebGL Fallback ⭐⭐⭐

**Priority:** MEDIUM  
**Status:** wgpu supports WebGL2 backend  
**Impact:** Compatibility with older browsers/devices

#### Overview

For browsers without WebGPU support (legacy devices, older browsers), wgpu can fall back to **WebGL2**.

**wgpu Backend Selection:**

```rust
// crates/flui_engine/src/web/backend_detection.rs

#[cfg(target_arch = "wasm32")]
pub fn detect_web_backend() -> wgpu::Backends {
    use wasm_bindgen::JsCast;
    use web_sys::window;

    let window = window().expect("No window object");
    let navigator = window.navigator();

    // Check for WebGPU support
    if js_sys::Reflect::has(&navigator, &"gpu".into()).unwrap_or(false) {
        tracing::info!("WebGPU detected, using native backend");
        return wgpu::Backends::BROWSER_WEBGPU;
    }

    // Fallback to WebGL2
    tracing::warn!("WebGPU not available, falling back to WebGL2");
    wgpu::Backends::GL
}

pub async fn create_renderer(canvas: web_sys::HtmlCanvasElement) -> Result<WebGpuRenderer> {
    let backends = detect_web_backend();
    
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends,
        ..Default::default()
    });

    // Rest of initialization...
}
```

**Important Note:** When using WebGL backend, wgpu translates WGSL shaders to GLSL using **Naga**.

**Build Flag Required:**

```bash
# For wasm-pack build with WebGPU (unstable APIs)
RUSTFLAGS=--cfg=web_sys_unstable_apis wasm-pack build --target web
```

**Crate Assignment:** `flui_engine` (web backend detection)

---

## 2. WebAssembly 3.0 & WASI

### 2.1 WebAssembly 3.0 (September 2025) ⭐⭐⭐⭐⭐

**Priority:** CRITICAL  
**Status:** Released as "live" standard September 2025  
**Impact:** GC, threads, SIMD, tail calls in all browsers

#### Wasm 3.0 Features

**Officially Standardized:**

1. **Garbage Collection (GC)** ⭐⭐⭐⭐⭐
   - Supported in Safari (2024), Chrome, Firefox (2025)
   - Enables efficient compilation of Java, C#, Go, Python, Kotlin
   - Languages no longer need to bundle their own GC
   - **Rust doesn't need GC**, but other WASM modules can interop better

2. **Tail Calls** ⭐⭐⭐
   - Optimized recursive function calls
   - Important for functional languages (Scheme, OCaml, Haskell)
   - Available in Safari, Chrome, Firefox

3. **Threads (Phase 4)** ⭐⭐⭐⭐⭐
   - Multi-threaded WebAssembly
   - Requires `SharedArrayBuffer` + cross-origin isolation
   - Experimental support in most browsers
   - **Critical for FLUI performance** (parallel layout, rendering)

4. **SIMD Improvements** ⭐⭐⭐⭐
   - Vector instructions for data-parallel operations
   - 128-bit SIMD operations
   - Better performance for math-heavy code

#### Threads and SharedArrayBuffer

**Requirements:**

1. **Cross-Origin Isolation** - Must set HTTP headers:
   ```
   Cross-Origin-Opener-Policy: same-origin
   Cross-Origin-Embedder-Policy: require-corp
   ```

2. **SharedArrayBuffer** - Shared memory between main thread and workers

**Implementation:**

```rust
// crates/flui-platform/src/web/threading.rs

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;
use web_sys::{Worker, MessageEvent};

pub struct WasmThreadPool {
    workers: Vec<Worker>,
    shared_memory: Option<js_sys::SharedArrayBuffer>,
}

impl WasmThreadPool {
    pub fn new(num_threads: usize) -> Result<Self> {
        // Check if SharedArrayBuffer is available
        if !Self::is_cross_origin_isolated() {
            return Err(anyhow::anyhow!(
                "SharedArrayBuffer requires cross-origin isolation. \
                 Set COOP and COEP headers."
            ));
        }

        let mut workers = Vec::new();
        for i in 0..num_threads {
            let worker = Worker::new(&format!("worker-{}.js", i))?;
            workers.push(worker);
        }

        // Allocate shared memory
        let shared_memory = Some(js_sys::SharedArrayBuffer::new(1024 * 1024)); // 1MB

        Ok(Self { workers, shared_memory })
    }

    fn is_cross_origin_isolated() -> bool {
        use wasm_bindgen::JsCast;
        let window = web_sys::window().unwrap();
        let cross_origin_isolated = js_sys::Reflect::get(
            &window,
            &"crossOriginIsolated".into()
        ).unwrap();
        
        cross_origin_isolated.as_bool().unwrap_or(false)
    }

    /// Dispatch parallel layout computation
    pub fn parallel_layout(&self, layout_jobs: Vec<LayoutJob>) {
        // Split jobs across workers
        // Each worker computes layout on its own thread
        // Results collected via postMessage
    }
}
```

**wasm-bindgen-rayon Integration:**

```rust
// Use Rayon for parallel iterators in WASM
use wasm_bindgen_rayon::init_thread_pool;

#[wasm_bindgen]
pub async fn init_flui() {
    // Initialize rayon thread pool for WASM
    init_thread_pool(4).await;  // 4 worker threads

    // Now Rayon parallel iterators work!
    let results: Vec<_> = items.par_iter()
        .map(|item| process(item))
        .collect();
}
```

**Crate Assignment:** `flui-platform` (web threading)

---

### 2.2 WASI Preview 3 (WASI 0.3) - February 2026 ⭐⭐⭐⭐

**Priority:** HIGH  
**Status:** Expected February 2026  
**Impact:** Native async/await, Component Model, filesystem access

#### WASI Timeline

- **WASI 0.2** (January 25, 2024) - Component Model integration, wasi-cli, wasi-http, wasi-filesystem, wasi-sockets
- **WASI 0.3** (Expected February 2026) - Native async with Component Model, stream optimizations, threads API
- **WASI 1.0** (Late 2026 / Early 2027) - Full standardization

#### WASI 0.3 Features

**Native Async Support:**
- Async/await in WebAssembly Component Model
- Cancellation tokens
- Stream optimizations
- Better integration with JavaScript Promises

**New APIs:**
- `wasi-threads` - Thread creation API
- Async versions of existing WASI 0.2 APIs
- Better filesystem performance

#### Component Model

The **WebAssembly Component Model** enables:
- Language-agnostic module composition
- Interface Types (WIT - WebAssembly Interface Types)
- Virtualized imports/exports
- Better toolchain interop

**Use Case for FLUI:**
- Load plugins as WASM components
- Share UI widgets as reusable components
- Interop with non-Rust WASM modules

**Example:**

```wit
// widget.wit - Define widget interface
package flui:widgets

world widget-host {
  import logger: interface {
    log: func(msg: string)
  }

  export widget: interface {
    render: func() -> list<u8>  // Returns serialized view tree
    handle-event: func(event: string)
  }
}
```

```rust
// Implement widget as WASM component
wit_bindgen::generate!({
    world: "widget-host",
});

struct MyWidget;

impl Widget for MyWidget {
    fn render() -> Vec<u8> {
        // Serialize view tree
        vec![]
    }

    fn handle_event(event: String) {
        logger::log(&format!("Event: {}", event));
    }
}
```

**Crate Assignment:** `flui-plugin` (future crate for WASM component plugins)

---

## 3. wasm-bindgen and Web APIs

### 3.1 wasm-bindgen Integration ⭐⭐⭐⭐⭐

**Priority:** CRITICAL  
**Status:** Mature, actively maintained  
**Impact:** Rust ↔ JavaScript interop

#### Overview

**wasm-bindgen** provides bindings between Rust and JavaScript, enabling:
- Call JavaScript APIs from Rust
- Call Rust functions from JavaScript
- Share memory efficiently
- Type-safe bindings via `web-sys` and `js-sys`

**Build Configuration:**

```toml
# Cargo.toml
[target.'cfg(target_arch = "wasm32")'.dependencies]
wasm-bindgen = "0.2"
web-sys = { version = "0.3", features = [
    "Document",
    "Element",
    "HtmlCanvasElement",
    "Window",
    "Performance",
    "PerformanceTiming",
    "Navigator",
    "Gpu",  # WebGPU
    "Worker",
    "MessageEvent",
] }
js-sys = "0.3"
wasm-bindgen-futures = "0.4"  # async/await support

[profile.release]
opt-level = "z"  # Optimize for size
lto = true
codegen-units = 1
```

**Build Command:**

```bash
# Build for web
wasm-pack build --target web --release

# With WebGPU unstable APIs
RUSTFLAGS=--cfg=web_sys_unstable_apis wasm-pack build --target web
```

**Entry Point:**

```rust
// crates/flui-platform/src/web/lib.rs

use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub async fn main() -> Result<(), JsValue> {
    // Initialize panic hook for better error messages
    console_error_panic_hook::set_once();

    // Initialize logging
    tracing_wasm::set_as_global_default();

    tracing::info!("FLUI Web starting...");

    // Initialize FLUI
    let app = FluiApp::new().await?;
    app.run().await?;

    Ok(())
}

#[wasm_bindgen]
pub struct FluiApp {
    renderer: WebGpuRenderer,
    view_tree: ViewTree,
}

#[wasm_bindgen]
impl FluiApp {
    #[wasm_bindgen(constructor)]
    pub async fn new() -> Result<FluiApp, JsValue> {
        let window = web_sys::window().expect("No window");
        let document = window.document().expect("No document");
        let canvas = document
            .get_element_by_id("flui-canvas")
            .expect("No canvas")
            .dyn_into::<web_sys::HtmlCanvasElement>()?;

        let renderer = WebGpuRenderer::new(canvas).await
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        Ok(FluiApp {
            renderer,
            view_tree: ViewTree::default(),
        })
    }

    #[wasm_bindgen]
    pub fn render(&mut self) {
        self.renderer.render(&self.view_tree);
    }
}
```

**HTML Integration:**

```html
<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <title>FLUI Web App</title>
    <style>
        body { margin: 0; overflow: hidden; }
        #flui-canvas { width: 100vw; height: 100vh; }
    </style>
</head>
<body>
    <canvas id="flui-canvas"></canvas>
    <script type="module">
        import init, { FluiApp } from './pkg/flui_web.js';

        async function run() {
            await init();  // Initialize WASM module
            const app = await FluiApp.new();
            
            function render() {
                app.render();
                requestAnimationFrame(render);
            }
            requestAnimationFrame(render);
        }

        run();
    </script>
</body>
</html>
```

**Crate Assignment:** `flui-platform` (web bindings)

---

### 3.2 Performance Optimization ⭐⭐⭐⭐

**Priority:** HIGH  
**Status:** Best practices established  
**Impact:** Fast load times, small binary size

#### WASM Binary Size Optimization

**Techniques:**

1. **wasm-opt** - Optimize WASM binary
2. **LTO** - Link-Time Optimization
3. **Strip symbols** - Remove debug info
4. **Code splitting** - Load modules on demand
5. **Compression** - Brotli/gzip for network transfer

**Build Configuration:**

```toml
# Cargo.toml
[profile.release]
opt-level = "z"      # Optimize for size (not "s" for slightly better perf)
lto = true           # Link-Time Optimization
codegen-units = 1    # Better optimization, slower builds
panic = "abort"      # Smaller binary, no unwinding
strip = true         # Remove symbols
```

**Post-Build Optimization:**

```bash
# Install wasm-opt (from binaryen)
npm install -g wasm-opt

# Optimize WASM binary
wasm-opt -Oz -o output_optimized.wasm input.wasm

# With aggressive inlining
wasm-opt -Oz --inline-functions-with-loops -o output.wasm input.wasm
```

**Typical Size Reductions:**
- Debug build: ~5-10 MB
- Release build: ~500 KB - 2 MB
- After wasm-opt: ~300 KB - 1 MB
- After Brotli compression: ~100 KB - 400 KB

#### Lazy Loading

```rust
// Load heavy modules on demand
#[wasm_bindgen]
pub async fn load_video_module() -> Result<JsValue, JsValue> {
    let module = wasm_bindgen_futures::JsFuture::from(
        js_sys::eval("import('./video_module.js')")
    ).await?;
    
    Ok(module)
}
```

**Crate Assignment:** Build system configuration

---

## 4. Progressive Web Apps (PWA)

### 4.1 PWA Adoption (2025-2026) ⭐⭐⭐⭐⭐

**Priority:** CRITICAL  
**Status:** 24.5% of websites use PWA features, 3.3% fully implemented  
**Impact:** Installable apps, offline support, native-like UX

#### PWA Statistics (2025)

- **Service Workers:** 18.9% of sites (up from 1.7% in 2022 - 10x growth!)
- **Web App Manifest:** 9% of sites
- **Both features:** 3.3% (fully implemented PWAs)

**Key Trend:** Service workers no longer required for installation prompt (Chrome, Edge) - only manifest needed.

#### Web App Manifest

```json
{
  "name": "FLUI App",
  "short_name": "FLUI",
  "description": "Beautiful UI built with FLUI framework",
  "start_url": "/",
  "display": "standalone",
  "background_color": "#ffffff",
  "theme_color": "#4285f4",
  "orientation": "any",
  "icons": [
    {
      "src": "/icons/icon-192.png",
      "sizes": "192x192",
      "type": "image/png"
    },
    {
      "src": "/icons/icon-512.png",
      "sizes": "512x512",
      "type": "image/png"
    },
    {
      "src": "/icons/icon-maskable.png",
      "sizes": "512x512",
      "type": "image/png",
      "purpose": "maskable"
    }
  ],
  "categories": ["productivity", "utilities"],
  "screenshots": [
    {
      "src": "/screenshots/desktop.png",
      "sizes": "1280x720",
      "type": "image/png",
      "form_factor": "wide"
    },
    {
      "src": "/screenshots/mobile.png",
      "sizes": "750x1334",
      "type": "image/png",
      "form_factor": "narrow"
    }
  ],
  "share_target": {
    "action": "/share",
    "method": "POST",
    "enctype": "multipart/form-data",
    "params": {
      "title": "title",
      "text": "text",
      "url": "url",
      "files": [
        {
          "name": "media",
          "accept": ["image/*", "video/*"]
        }
      ]
    }
  },
  "protocol_handlers": [
    {
      "protocol": "web+flui",
      "url": "/handler?url=%s"
    }
  ]
}
```

**Features:**
- **Install Prompts** - "Add to Home Screen" on mobile/desktop
- **Standalone Display** - Runs without browser chrome
- **Share Target** - Handle shares from other apps
- **Protocol Handlers** - Register custom URL schemes
- **Screenshots** - App store-like previews

#### Service Worker (Optional but Recommended)

```javascript
// service-worker.js
const CACHE_NAME = 'flui-v1';
const urlsToCache = [
  '/',
  '/index.html',
  '/pkg/flui_web.js',
  '/pkg/flui_web_bg.wasm',
  '/styles.css',
];

// Install event - cache resources
self.addEventListener('install', event => {
  event.waitUntil(
    caches.open(CACHE_NAME)
      .then(cache => cache.addAll(urlsToCache))
  );
});

// Fetch event - serve from cache, fallback to network
self.addEventListener('fetch', event => {
  event.respondWith(
    caches.match(event.request)
      .then(response => response || fetch(event.request))
  );
});

// Activate event - clean up old caches
self.addEventListener('activate', event => {
  event.waitUntil(
    caches.keys().then(cacheNames => {
      return Promise.all(
        cacheNames.map(cacheName => {
          if (cacheName !== CACHE_NAME) {
            return caches.delete(cacheName);
          }
        })
      );
    })
  );
});
```

**Register Service Worker:**

```javascript
// main.js
if ('serviceWorker' in navigator) {
  navigator.serviceWorker.register('/service-worker.js')
    .then(reg => console.log('Service Worker registered', reg))
    .catch(err => console.error('Service Worker registration failed', err));
}
```

**Crate Assignment:** Static web assets (not Rust code)

---

### 4.2 Offline Support ⭐⭐⭐⭐

**Priority:** HIGH  
**Status:** Enabled via Service Workers + Cache API  
**Impact:** App works without network

#### IndexedDB for Offline Storage

```rust
// crates/flui-platform/src/web/storage.rs

use wasm_bindgen::prelude::*;
use web_sys::{IdbFactory, IdbDatabase, IdbObjectStore};

#[wasm_bindgen]
pub struct OfflineStorage {
    db: IdbDatabase,
}

#[wasm_bindgen]
impl OfflineStorage {
    pub async fn new(db_name: &str) -> Result<OfflineStorage, JsValue> {
        let window = web_sys::window().unwrap();
        let idb_factory = window.indexed_db()?.unwrap();

        // Open database
        let open_request = idb_factory.open_with_u32(db_name, 1)?;

        // Wait for database to open
        let db = wasm_bindgen_futures::JsFuture::from(open_request)
            .await?
            .dyn_into::<IdbDatabase>()?;

        Ok(OfflineStorage { db })
    }

    pub async fn save_data(&self, key: &str, value: &str) -> Result<(), JsValue> {
        let transaction = self.db.transaction_with_str("data")?;
        let store = transaction.object_store("data")?;
        
        store.put_with_key(&JsValue::from_str(value), &JsValue::from_str(key))?;
        
        Ok(())
    }

    pub async fn load_data(&self, key: &str) -> Result<Option<String>, JsValue> {
        let transaction = self.db.transaction_with_str("data")?;
        let store = transaction.object_store("data")?;
        
        let request = store.get(&JsValue::from_str(key))?;
        let result = wasm_bindgen_futures::JsFuture::from(request).await?;
        
        if result.is_undefined() {
            Ok(None)
        } else {
            Ok(Some(result.as_string().unwrap()))
        }
    }
}
```

**Crate Assignment:** `flui-platform` (web storage)

---

## 5. WebCodecs API

### 5.1 Video Encoding/Decoding ⭐⭐⭐⭐

**Priority:** HIGH  
**Status:** Chrome/Edge stable, Safari partial, Firefox experimental  
**Impact:** Hardware video codec access in browser

#### Browser Support (2025-2026)

| Browser | VideoDecoder | AudioDecoder | VideoEncoder | AudioEncoder |
|---------|--------------|--------------|--------------|--------------|
| **Chrome 94+** | ✅ | ✅ | ✅ | ✅ |
| **Edge 94+** | ✅ | ✅ | ✅ | ✅ |
| **Safari 26+** | ✅ | ⚠️ Preview | ✅ | ⚠️ Preview |
| **Firefox** | ❌ | ❌ | ❌ | ❌ |

**Safari Note:** AudioDecoder supported in Technology Preview, full support coming soon.

#### Codec Support

- **H.264** - Universal support
- **H.265 (HEVC)** - Chrome/Edge/Safari (hardware dependent)
- **AV1** - Chrome/Edge (newer hardware)
- **VP8/VP9** - Chrome/Edge

#### Use Cases for FLUI

1. **Video Playback** - Custom video player widget
2. **Camera Input** - Camera preview with filters
3. **Screen Recording** - Record app UI
4. **Video Export** - Export animations as video

#### Implementation

```rust
// crates/flui_media/src/web/video_decoder.rs

use wasm_bindgen::prelude::*;
use web_sys::{VideoDecoder, VideoDecoderConfig, EncodedVideoChunk};

#[wasm_bindgen]
pub struct FluiVideoDecoder {
    decoder: VideoDecoder,
}

#[wasm_bindgen]
impl FluiVideoDecoder {
    pub fn new() -> Result<FluiVideoDecoder, JsValue> {
        let on_output = Closure::wrap(Box::new(|frame: JsValue| {
            // VideoFrame received, upload to GPU texture
            tracing::info!("Decoded frame");
        }) as Box<dyn FnMut(JsValue)>);

        let on_error = Closure::wrap(Box::new(|err: JsValue| {
            tracing::error!("Decode error: {:?}", err);
        }) as Box<dyn FnMut(JsValue)>);

        let init = js_sys::Object::new();
        js_sys::Reflect::set(&init, &"output".into(), on_output.as_ref())?;
        js_sys::Reflect::set(&init, &"error".into(), on_error.as_ref())?;

        let decoder = VideoDecoder::new(&init)?;

        // Configure decoder
        let config = VideoDecoderConfig::new("avc1.42E01E");  // H.264 baseline
        decoder.configure(&config);

        on_output.forget();
        on_error.forget();

        Ok(FluiVideoDecoder { decoder })
    }

    pub fn decode(&self, chunk_data: &[u8], timestamp: f64) -> Result<(), JsValue> {
        let chunk_init = js_sys::Object::new();
        js_sys::Reflect::set(&chunk_init, &"type".into(), &"key".into())?;
        js_sys::Reflect::set(&chunk_init, &"timestamp".into(), &timestamp.into())?;
        js_sys::Reflect::set(&chunk_init, &"data".into(), &js_sys::Uint8Array::from(chunk_data))?;

        let chunk = EncodedVideoChunk::new(&chunk_init)?;
        self.decoder.decode(&chunk);

        Ok(())
    }
}
```

**Crate Assignment:** `flui_media` (future crate for video/audio)

---

## 6. WebTransport & WebRTC

### 6.1 WebTransport over HTTP/3 ⭐⭐⭐⭐

**Priority:** HIGH  
**Status:** Chrome/Edge stable, Firefox/Safari experimental  
**Impact:** Low-latency networking over QUIC

#### Overview

**WebTransport** provides bidirectional, multiplexed, low-latency communication over HTTP/3 (QUIC protocol).

**Advantages over WebSockets:**
- **Lower latency** - Built on QUIC (faster handshake)
- **Multiplexing** - Multiple independent streams
- **Unreliable datagrams** - For real-time data (game state, audio)
- **Better congestion control** - QUIC's modern algorithms

**Use Cases:**
- Real-time multiplayer games
- Video conferencing
- Live streaming
- Collaborative editing

#### Browser Support

| Browser | Status | Notes |
|---------|--------|-------|
| **Chrome 97+** | ✅ Stable | Full support |
| **Edge 97+** | ✅ Stable | Full support |
| **Firefox** | ⚠️ Experimental | Behind flag |
| **Safari** | ⚠️ Experimental | Tech Preview |

#### Implementation

```rust
// crates/flui-network/src/web/webtransport.rs

use wasm_bindgen::prelude::*;
use web_sys::WebTransport;

#[wasm_bindgen]
pub struct FluiTransport {
    transport: WebTransport,
}

#[wasm_bindgen]
impl FluiTransport {
    pub async fn connect(url: &str) -> Result<FluiTransport, JsValue> {
        let transport = WebTransport::new(url)?;

        // Wait for connection
        let ready = wasm_bindgen_futures::JsFuture::from(transport.ready()).await?;

        tracing::info!("WebTransport connected to {}", url);

        Ok(FluiTransport { transport })
    }

    /// Send reliable message over bidirectional stream
    pub async fn send_reliable(&self, data: &[u8]) -> Result<(), JsValue> {
        // Create bidirectional stream
        let streams = wasm_bindgen_futures::JsFuture::from(
            self.transport.create_bidirectional_stream()
        ).await?;

        // Get writable stream
        let writable = js_sys::Reflect::get(&streams, &"writable".into())?;
        
        // Write data
        // Implementation details...

        Ok(())
    }

    /// Send unreliable datagram (fire-and-forget)
    pub fn send_datagram(&self, data: &[u8]) -> Result<(), JsValue> {
        let datagram_writer = self.transport.datagrams();
        let writable = datagram_writer.writable();

        // Get writer
        let writer = writable.get_writer()?;

        // Write datagram
        let uint8_array = js_sys::Uint8Array::from(data);
        writer.write_with_chunk(&uint8_array)?;
        writer.release_lock();

        Ok(())
    }
}
```

**Server Side (Rust):**

```rust
// Server using quinn (QUIC implementation)
use quinn::{Endpoint, ServerConfig};

async fn run_server() -> Result<()> {
    let mut server_config = ServerConfig::with_single_cert(certs, key)?;
    
    // Enable WebTransport
    server_config.transport = Arc::new(TransportConfig::default());

    let endpoint = Endpoint::server(server_config, "0.0.0.0:4433".parse()?)?;

    while let Some(conn) = endpoint.accept().await {
        tokio::spawn(handle_connection(conn));
    }

    Ok(())
}
```

**Crate Assignment:** `flui-network` (future crate for networking)

---

### 6.2 WebRTC Improvements (2025) ⭐⭐⭐

**Priority:** MEDIUM  
**Status:** Mature, ongoing improvements  
**Impact:** Video calls, screen sharing, P2P data

#### 2025 WebRTC Trends

- **HTTP/3 Integration** - WebRTC over QUIC for lower latency
- **AV1 Codec** - Better compression for video calls
- **Simulcast Improvements** - Multi-quality streaming
- **SFU Enhancements** - Selective Forwarding Units

**Note:** WebTransport may replace some WebRTC use cases, but WebRTC remains dominant for real-time communication.

**Crate Assignment:** `flui-network` (WebRTC bindings)

---

## 7. Implementation Priority Matrix

### Critical Path Features (Q1 2026)

1. **WebGPU Rendering** ⭐⭐⭐⭐⭐
   - **Effort:** Low (wgpu already supports web)
   - **Impact:** CRITICAL (universal browser support achieved)
   - **Crate:** `flui_engine` (web backend)

2. **wasm-bindgen Integration** ⭐⭐⭐⭐⭐
   - **Effort:** Medium (2-3 weeks)
   - **Impact:** CRITICAL (Rust ↔ JS interop)
   - **Crate:** `flui-platform` (web bindings)

3. **PWA Manifest & Installation** ⭐⭐⭐⭐⭐
   - **Effort:** Low (1 week)
   - **Impact:** HIGH (installable apps)
   - **Crate:** Static assets

### High Priority Features (Q2 2026)

4. **WASM Threads (SharedArrayBuffer)** ⭐⭐⭐⭐⭐
   - **Effort:** High (4-6 weeks, requires COOP/COEP setup)
   - **Impact:** CRITICAL (parallel rendering, layout)
   - **Crate:** `flui-platform` (web threading)

5. **Binary Size Optimization** ⭐⭐⭐⭐
   - **Effort:** Medium (2-3 weeks)
   - **Impact:** HIGH (fast load times)
   - **Crate:** Build system

6. **IndexedDB Offline Storage** ⭐⭐⭐⭐
   - **Effort:** Medium (2 weeks)
   - **Impact:** HIGH (offline support)
   - **Crate:** `flui-platform` (web storage)

### Medium Priority Features (Q3 2026)

7. **Service Worker Caching** ⭐⭐⭐⭐
   - **Effort:** Low (1 week)
   - **Impact:** MEDIUM (offline, faster loads)
   - **Crate:** JavaScript (service-worker.js)

8. **WebCodecs Video Decoder** ⭐⭐⭐
   - **Effort:** Medium (3 weeks)
   - **Impact:** MEDIUM (video widget support)
   - **Crate:** `flui_media`

9. **WebTransport Networking** ⭐⭐⭐
   - **Effort:** High (4 weeks)
   - **Impact:** MEDIUM (real-time apps)
   - **Crate:** `flui-network`

### Low Priority Features (Q4 2026)

10. **WASI 0.3 Component Model** ⭐⭐⭐
    - **Effort:** High (6+ weeks)
    - **Impact:** MEDIUM (plugin system)
    - **Crate:** `flui-plugin`

11. **WebGL Fallback** ⭐⭐
    - **Effort:** Low (wgpu supports it already)
    - **Impact:** LOW (WebGPU has universal support)
    - **Crate:** `flui_engine`

12. **WebRTC Integration** ⭐⭐
    - **Effort:** High (6+ weeks)
    - **Impact:** LOW (niche use case)
    - **Crate:** `flui-network`

---

## 8. Testing Strategy

### Browser Testing Matrix

| Browser | Version | Platform | WebGPU | WASM Threads | Priority |
|---------|---------|----------|--------|--------------|----------|
| **Chrome** | 113+ | Windows/Mac/Linux/Android | ✅ | ✅ | ⭐⭐⭐⭐⭐ |
| **Edge** | 113+ | Windows/Mac | ✅ | ✅ | ⭐⭐⭐⭐⭐ |
| **Safari** | 26+ | macOS Tahoe/iOS 26 | ✅ | ✅ | ⭐⭐⭐⭐⭐ |
| **Firefox** | 141+ | Windows/Mac | ✅ | ⚠️ | ⭐⭐⭐⭐ |
| **Chrome Mobile** | 121+ | Android | ✅ | ✅ | ⭐⭐⭐⭐⭐ |
| **Safari Mobile** | iOS 26+ | iPhone/iPad | ✅ | ✅ | ⭐⭐⭐⭐⭐ |

### Performance Testing

```rust
// tests/web/performance.rs

#[cfg(target_arch = "wasm32")]
mod web_tests {
    use wasm_bindgen_test::*;
    use web_sys::Performance;

    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    async fn test_render_performance() {
        let window = web_sys::window().unwrap();
        let performance = window.performance().unwrap();

        let start = performance.now();

        // Render 1000 widgets
        let app = FluiApp::new().await.unwrap();
        app.render();

        let end = performance.now();
        let duration = end - start;

        // Should render in < 16ms (60fps)
        assert!(duration < 16.0, "Render took {}ms, expected < 16ms", duration);
    }

    #[wasm_bindgen_test]
    fn test_binary_size() {
        // Check WASM binary size
        // Should be < 500KB after optimization
    }

    #[wasm_bindgen_test]
    async fn test_webgpu_available() {
        let window = web_sys::window().unwrap();
        let navigator = window.navigator();

        let has_gpu = js_sys::Reflect::has(&navigator, &"gpu".into()).unwrap();
        assert!(has_gpu, "WebGPU not available");
    }
}
```

### Continuous Integration

```yaml
# .github/workflows/web_tests.yml
name: Web Platform Tests

on: [push, pull_request]

jobs:
  test-web:
    runs-on: ubuntu-latest
    
    steps:
      - uses: actions/checkout@v4
      
      - name: Install wasm-pack
        run: curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh
      
      - name: Build for web
        run: |
          RUSTFLAGS=--cfg=web_sys_unstable_apis wasm-pack build --target web
      
      - name: Run browser tests
        run: wasm-pack test --headless --chrome --firefox
      
      - name: Check binary size
        run: |
          SIZE=$(stat -c%s pkg/flui_web_bg.wasm)
          echo "WASM size: $SIZE bytes"
          if [ $SIZE -gt 2097152 ]; then
            echo "Binary too large (> 2MB)"
            exit 1
          fi
```

---

## 9. Risk Assessment

### Technical Risks

| Risk | Probability | Impact | Mitigation |
|------|------------|--------|------------|
| Cross-origin isolation setup complexity | High | High | Clear documentation, dev server template |
| Binary size bloat | Medium | High | Aggressive optimization, code splitting |
| WebGPU driver bugs | Low | High | Fallback to WebGL2, browser bug reports |
| WASM threads instability | Medium | Medium | Feature flag, graceful degradation |
| Service Worker caching issues | Low | Medium | Clear cache invalidation strategy |
| IndexedDB quota limits | Low | Medium | Monitor storage usage, clear old data |

### Browser Compatibility

- **WebGPU:** Universal support ✅ (Low risk)
- **WASM Threads:** Requires COOP/COEP headers (Medium risk - setup complexity)
- **WebCodecs:** Safari partial, Firefox experimental (Medium risk - fallback needed)
- **WebTransport:** Chrome/Edge only (High risk - use WebSockets fallback)

### Timeline Risks

- **Q1 2026:** Basic WebGPU rendering - **LOW RISK** (wgpu already supports web)
- **Q2 2026:** WASM threads, optimization - **MEDIUM RISK** (cross-origin isolation complexity)
- **Q3 2026:** Advanced features (WebCodecs, WebTransport) - **MEDIUM RISK** (browser support gaps)

---

## 10. Resource Requirements

### Engineering Team

- **1 Web Platform Engineer** - wasm-bindgen, WebGPU, PWA (full-time, 6 months)
- **1 Performance Engineer** - Binary optimization, threading (part-time, 3 months)
- **1 QA Engineer** - Cross-browser testing, CI/CD (part-time, 6 months)

### Infrastructure

- **BrowserStack or similar** - Cross-browser testing ($100/month)
- **CDN** - Serve WASM binaries with Brotli compression ($50/month)
- **CI/CD** - GitHub Actions (included with GitHub)

### Budget Estimate

- **Engineering:** $150k - $300k (2.5 engineers × varying durations)
- **Infrastructure:** $1.8k/year (BrowserStack + CDN)
- **Total:** $152k - $302k

---

## 11. Conclusion

The web platform has achieved critical mass for high-performance UI frameworks in 2025-2026:

**Must-Have (2026):**
- WebGPU rendering (universal browser support achieved!)
- wasm-bindgen integration (Rust ↔ JS interop)
- PWA manifest (installable apps)
- Binary size optimization (< 500KB target)

**High-Value (2026):**
- WASM threads (parallel rendering)
- IndexedDB offline storage
- Service Worker caching
- WebCodecs video (where supported)

**Nice-to-Have (2027+):**
- WASI 0.3 Component Model (plugin system)
- WebTransport (low-latency networking)
- WebRTC (real-time communication)

**Recommended Timeline:**
- **Q1 2026:** WebGPU + wasm-bindgen + PWA manifest
- **Q2 2026:** WASM threads, binary optimization, offline storage
- **Q3 2026:** Service Worker, WebCodecs, WebTransport
- **Q4 2026:** WASI 0.3, plugin system, advanced features

**Key Advantage for FLUI:** 
Since FLUI already uses **wgpu**, the same rendering code works across:
- Native desktop (Vulkan/Metal/D3D12)
- Web (WebGPU via wgpu's browser backend)
- Minimal platform-specific code

This positions FLUI as a true **cross-platform UI framework** with first-class web support.

---

**Next Steps:**
1. Review and approve this roadmap
2. Assign engineering resources
3. Set up cross-browser testing environment (BrowserStack)
4. Create starter template with COOP/COEP headers configured
5. Begin Q1 2026 implementation (WebGPU + wasm-bindgen)

---

## Sources

- [WebGPU is now supported in major browsers | Blog | web.dev](https://web.dev/blog/webgpu-supported-major-browsers)
- [WebGPU Hits Critical Mass: All Major Browsers Now Ship It](https://www.webgpu.com/news/webgpu-hits-critical-mass-all-major-browsers-now-ship-it/)
- [The State of WebAssembly – 2025 and 2026](https://platform.uno/blog/the-state-of-webassembly-2025-2026/)
- [WASI and the WebAssembly Component Model: Current Status](https://eunomia.dev/blog/2025/02/16/wasi-and-the-webassembly-component-model-current-status/)
- [Rust + WebAssembly 2025: Why WasmGC and SIMD Change Everything](https://dev.to/dataformathub/rust-webassembly-2025-why-wasmgc-and-simd-change-everything-3ldh)
- [wgpu: portable graphics library for Rust](https://wgpu.rs/)
- [Understanding SharedArrayBuffer and cross-origin isolation - LogRocket Blog](https://blog.logrocket.com/understanding-sharedarraybuffer-and-cross-origin-isolation/)
- [PWA | 2025 | The Web Almanac by HTTP Archive](https://almanac.httparchive.org/en/2025/pwa)
- [What Is a PWA? the Ultimate Guide to Progressive Web Apps in 2026](https://www.mobiloud.com/blog/progressive-web-apps)
- [WebCodecs API - Web APIs | MDN](https://developer.mozilla.org/en-US/docs/Web/API/WebCodecs_API)
- [What is WebTransport? The Complete Guide for Developers (2025 Edition)](https://www.videosdk.live/developer-hub/webtransport/what-is-webtransport)
- [What's Next for WebRTC in 2025? A Look Ahead](https://webrtc.ventures/2025/01/whats-next-for-webrtc-in-2025/)
