# Flui WebAssembly Demo

This directory contains a demonstration of running Flui in a web browser using WebAssembly and WebGPU.

## Features

- 🚀 **GPU-Accelerated Rendering** - Uses WebGPU for native performance
- 🎨 **Same Code, Multiple Platforms** - Same Rust code runs on desktop and web
- 🔒 **Type-Safe** - Full Rust type safety in the browser
- ⚡ **Reactive State** - Flutter-like hooks and signals

## Requirements

### Browser Support

- Chrome 113+ or Edge 113+  (WebGPU support)
- Or Firefox Nightly with `dom.webgpu.enabled` flag

### Build Tools

```bash
# Install wasm-pack
cargo install wasm-pack

# Install a local HTTP server (choose one)
cargo install basic-http-server
# OR
pip install http-server
# OR
npm install -g http-server
```

## Building and Running

### Step 1: Build for WebAssembly

From the project root directory:

```bash
# Build the web demo
wasm-pack build \
  --target web \
  --out-dir examples/web \
  --out-name flui_web_demo \
  crates/flui_app \
  --release
```

This will:
1. Compile the Rust code to WebAssembly
2. Generate JavaScript bindings
3. Output everything to `crates/flui_app/examples/web/`

### Step 2: Serve the Web Directory

```bash
# Using basic-http-server (Rust)
basic-http-server crates/flui_app/examples/web

# Using Python
python -m http.server -d crates/flui_app/examples/web 8080

# Using Node.js http-server
npx http-server crates/flui_app/examples/web -p 8080
```

### Step 3: Open in Browser

Navigate to http://localhost:8080 (or the port shown by your HTTP server)

## Development

For faster development iterations:

```bash
# Build in debug mode (faster compile, larger file)
wasm-pack build \
  --target web \
  --out-dir examples/web \
  --out-name flui_web_demo \
  --dev \
  crates/flui_app
```

## Troubleshooting

### "WebGPU is not supported"

- Make sure you're using Chrome 113+ or Edge 113+
- Check chrome://gpu to verify WebGPU is enabled
- Try enabling WebGPU in chrome://flags

### "Failed to compile WASM"

- Make sure you have the `wasm32-unknown-unknown` target installed:
  ```bash
  rustup target add wasm32-unknown-unknown
  ```

### "Module not found" errors

- Make sure the build output is in the correct directory
- Check that `flui_web_demo.js` and `flui_web_demo_bg.wasm` exist in the web directory

## File Structure

```
web/
├── index.html              # Main HTML page
├── README.md              # This file
├── flui_web_demo.js       # Generated JS bindings (after build)
├── flui_web_demo_bg.wasm  # Generated WASM binary (after build)
└── .gitignore             # Ignore generated files
```

## Next Steps

- Modify `crates/flui_app/examples/web_demo.rs` to create your own UI
- Add interactivity with signals and hooks
- Deploy to GitHub Pages, Netlify, or Vercel

## Resources

- [WebGPU Specification](https://www.w3.org/TR/webgpu/)
- [wasm-bindgen Guide](https://rustwasm.github.io/wasm-bindgen/)
- [Flui Documentation](../../README.md)
