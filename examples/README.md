# FLUI Examples

This directory contains examples demonstrating FLUI framework features.

## Windows Examples

### windows11_demo - Windows 11 Modern Features

Demonstrates Windows 11-specific visual features using DWM (Desktop Window Manager) API.

**Requirements:**
- Windows 11 Build 22000+ (for Mica backdrop and rounded corners)
- Windows 10 Build 17763+ (for dark mode)

**Features shown:**
- ✨ Mica backdrop material (translucent effect showing desktop wallpaper)
- 🌙 Dark mode title bar
- 🔵 Custom title bar color (dark blue)
- ⚪ Rounded window corners
- 📐 Snap Layouts support (hover over maximize button)

**Run:**
```bash
cargo run --example windows11_demo
```

**What you should see:**
- Window with translucent Mica backdrop
- Dark title bar with custom blue tint (#141E32)
- Smooth rounded corners
- Windows 11 Snap Layouts when hovering maximize button

Close the window to exit.

## Cross-Platform Examples

### hello_world - Basic Window

Simple window creation example that works on all platforms.

**Run:**
```bash
cargo run --example hello_world
```

### window_features - Cross-Platform Window API

Demonstrates cross-platform window features from the Window trait.

**Run:**
```bash
cargo run --example window_features
```

## Building Examples

All examples can be built with:
```bash
cargo build --examples
```

Run a specific example:
```bash
cargo run --example <name>
```

## Notes

- Windows-specific examples (`windows11_*`) will only compile on Windows
- macOS-specific examples (`macos_*`) will only compile on macOS
- Linux-specific examples (`linux_*`) will only compile on Linux
- Cross-platform examples work on all supported platforms
