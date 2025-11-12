# FLUI Mobile Deployment

Complete setup for deploying FLUI applications to multiple platforms.

## 📱 Supported Platforms

- ✅ **Desktop** - Windows, Linux, macOS
- ✅ **Android** - ARM64, ARMv7
- ✅ **iOS** - Device (ARM64), Simulator (ARM64 + x86_64)
- ✅ **Web** - WebAssembly with WebGPU

---

## 🚀 Quick Start

### Desktop

```bash
cargo run -p flui_app --example counter_demo
```

### Android (Genymotion)

```bash
# Windows
scripts\build_android.bat

# Linux/macOS
chmod +x scripts/build_android.sh
./scripts/build_android.sh
```

### iOS (macOS only)

```bash
chmod +x scripts/build_ios.sh
./scripts/build_ios.sh
```

### Web

```bash
# Windows
scripts\build_web.bat

# Linux/macOS
chmod +x scripts/build_web.sh
./scripts/build_web.sh
```

---

## 📂 Project Structure

```
flui/
├── crates/
│   └── flui_app/
│       └── examples/
│           └── counter_demo.rs      # ✨ Universal example
│
├── platforms/                       # Platform configurations
│   ├── android/
│   │   ├── app/
│   │   │   ├── src/main/
│   │   │   │   ├── AndroidManifest.xml
│   │   │   │   └── jniLibs/         # Native libraries (.so)
│   │   │   └── build.gradle
│   │   ├── build.gradle
│   │   ├── settings.gradle
│   │   └── gradle.properties
│   │
│   ├── ios/
│   │   ├── FluiCounter/
│   │   │   ├── AppDelegate.swift
│   │   │   ├── Info.plist
│   │   │   └── Assets.xcassets/
│   │   ├── Libraries/               # Static libraries (.a)
│   │   └── FluiCounter.xcodeproj/
│   │
│   └── web/
│       ├── index.html
│       └── pkg/                     # Generated WASM files
│
└── scripts/                         # Build automation
    ├── build_android.bat
    ├── build_android.sh
    ├── build_ios.sh
    ├── build_web.bat
    └── build_web.sh
```

---

## 🔧 Prerequisites

### All Platforms

- **Rust** 1.70+ ([rustup.rs](https://rustup.rs))
- **Cargo** (comes with Rust)

### Android

- **Android Studio** or Android SDK
- **Android NDK** r25+ (install via SDK Manager)
- **Java Development Kit** (JDK 17+)
- **cargo-ndk**: `cargo install cargo-ndk`

**Environment Variables:**
```bash
# Windows
set ANDROID_HOME=C:\Users\%USERNAME%\AppData\Local\Android\Sdk
set ANDROID_NDK_HOME=%ANDROID_HOME%\ndk\27.0.12077973

# Linux/macOS
export ANDROID_HOME=$HOME/Android/Sdk
export ANDROID_NDK_HOME=$ANDROID_HOME/ndk/27.0.12077973
```

### iOS (macOS only)

- **Xcode** 14+ (from App Store)
- **Xcode Command Line Tools**: `xcode-select --install`
- **CocoaPods** (optional): `sudo gem install cocoapods`

### Web

- **wasm-pack**: `cargo install wasm-pack`
- **Chrome 113+** or Edge 113+ (for WebGPU support)

---

## 📱 Android Setup

### 1. Install Prerequisites

```bash
# Install Android Studio
# Download from: https://developer.android.com/studio

# Install cargo-ndk
cargo install cargo-ndk

# Add Rust targets
rustup target add aarch64-linux-android
rustup target add armv7-linux-androideabi
```

### 2. Configure Android SDK

Open Android Studio → SDK Manager, install:
- **Android SDK Platform** (API 33 or 34)
- **Android SDK Build-Tools** (34.0.0)
- **NDK** (Side by side) version 27.0+

### 3. Setup Genymotion (Recommended)

1. Download [Genymotion](https://www.genymotion.com/)
2. Install and create a virtual device
3. Start the device

### 4. Build and Run

```bash
# Build APK
scripts\build_android.bat

# Or manually:
cargo ndk -t arm64-v8a \
  -o platforms/android/app/src/main/jniLibs \
  --manifest-path crates/flui_app/Cargo.toml \
  build --example counter_demo --release

cd platforms/android
gradlew assembleDebug

# Install
adb install -r app/build/outputs/apk/debug/app-debug.apk

# Launch
adb shell am start -n com.vanya.flui.counter.debug/android.app.NativeActivity

# View logs
adb logcat -s FLUI
```

### Troubleshooting Android

**"NDK not found"**
```bash
# Check NDK path
ls $ANDROID_HOME/ndk/

# Set correct version
export ANDROID_NDK_HOME=$ANDROID_HOME/ndk/27.0.12077973
```

**"Build failed: linker error"**
```bash
# Clean and rebuild
cargo clean
scripts/build_android.bat
```

**"Device not found"**
```bash
# Check connected devices
adb devices

# If Genymotion not showing:
adb connect 192.168.56.101:5555  # Genymotion default IP
```

---

## 🍎 iOS Setup

### 1. Install Prerequisites (macOS only)

```bash
# Install Xcode from App Store

# Install Command Line Tools
xcode-select --install

# Add Rust targets
rustup target add aarch64-apple-ios
rustup target add aarch64-apple-ios-sim
rustup target add x86_64-apple-ios
```

### 2. Build Libraries

```bash
chmod +x scripts/build_ios.sh
./scripts/build_ios.sh
```

This creates:
- `platforms/ios/Libraries/libcounter_demo_device.a` - For real devices
- `platforms/ios/Libraries/libcounter_demo_sim.a` - For simulator (universal)

### 3. Open in Xcode

```bash
open platforms/ios/FluiCounter.xcodeproj
```

### 4. Configure Signing

1. Select project in Xcode
2. Go to "Signing & Capabilities"
3. Select your Team
4. Xcode will create provisioning profile automatically

### 5. Run

1. Select target device/simulator
2. Click Run (⌘R)

### Troubleshooting iOS

**"Library not found"**
```bash
# Ensure libraries are in correct location
ls platforms/ios/Libraries/

# Check Xcode build settings:
# Library Search Paths: $(PROJECT_DIR)/Libraries
```

**"Code signing error"**
- Add Apple ID in Xcode → Settings → Accounts
- Select your team in project settings

---

## 🌐 Web Setup

### 1. Install Prerequisites

```bash
# Install wasm-pack
cargo install wasm-pack

# Add WASM target
rustup target add wasm32-unknown-unknown
```

### 2. Build for Web

```bash
# Windows
scripts\build_web.bat

# Linux/macOS
chmod +x scripts/build_web.sh
./scripts/build_web.sh
```

This generates:
- `platforms/web/pkg/counter_demo.js`
- `platforms/web/pkg/counter_demo_bg.wasm`
- `platforms/web/pkg/package.json`

### 3. Serve Locally

```bash
cd platforms/web
python -m http.server 8080

# Or with Node.js
npx http-server . -p 8080
```

### 4. Open in Browser

Navigate to: http://localhost:8080

**Supported Browsers:**
- Chrome 113+
- Edge 113+
- Firefox with `dom.webgpu.enabled` flag

### Troubleshooting Web

**"WebGPU not supported"**
- Use Chrome 113+ or Edge 113+
- Check `chrome://gpu` → WebGPU status
- Enable in `chrome://flags` → "Unsafe WebGPU"

**"Module not found"**
- Ensure files in `platforms/web/pkg/` exist
- Check browser console for exact error

---

## 🎯 Example Application

The `counter_demo.rs` example demonstrates:

✨ **Features:**
- Reactive state with `use_signal`
- Computed values with `use_memo`
- Theme switching
- Platform detection
- Touch/mouse interactions
- Beautiful UI with FLUI widgets

**Code Structure:**
```rust
#[derive(Debug)]
struct CounterApp;

impl View for CounterApp {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        let count = use_signal(ctx, 0);
        
        Column::new()
            .children(vec![
                Text::new(format!("Count: {}", count.get(ctx))),
                Button::new("Increment")
                    .on_pressed(move || count.update(|n| *n += 1)),
            ])
    }
}

// Platform entry points
#[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
fn main() { run_app(CounterApp); }

#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(app: AndroidApp) { run_app(CounterApp); }

#[cfg(target_os = "ios")]
#[no_mangle]
pub extern "C" fn start_flui_counter() { run_app(CounterApp); }

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn wasm_main() { run_app(CounterApp); }
```

---

## 🔍 Verification

After building, verify:

### Android
```bash
# Check APK exists
ls platforms/android/app/build/outputs/apk/debug/app-debug.apk

# Check native libraries
ls platforms/android/app/src/main/jniLibs/arm64-v8a/

# Verify on device
adb shell pm list packages | grep flui
```

### iOS
```bash
# Check libraries
ls platforms/ios/Libraries/

# Check in Xcode
open platforms/ios/FluiCounter.xcodeproj
```

### Web
```bash
# Check WASM files
ls platforms/web/pkg/

# File sizes (should be reasonable)
du -h platforms/web/pkg/counter_demo_bg.wasm
```

---

## 📊 Performance Tips

### Android
- Use ARM64 for best performance
- Enable release builds for production
- Test on real devices, not just emulators

### iOS
- Use release builds: `cargo build --release`
- Test on real devices for accurate performance
- Profile with Xcode Instruments

### Web
- Use `--release` flag in wasm-pack
- Enable WASM optimizations
- Minimize bundle size with `wasm-opt`

---

## 🐛 Common Issues

### All Platforms

**Slow build times**
```bash
# Use sccache for caching
cargo install sccache
export RUSTC_WRAPPER=sccache
```

**Out of memory**
```bash
# Reduce parallel jobs
cargo build -j 2
```

### Android-Specific

**Gradle build fails**
```bash
cd platforms/android
./gradlew clean
./gradlew assembleDebug --stacktrace
```

**App crashes immediately**
- Check logcat: `adb logcat -s FLUI`
- Verify library name in AndroidManifest.xml matches Cargo.toml

### iOS-Specific

**Library architecture mismatch**
```bash
# Check library architecture
lipo -info platforms/ios/Libraries/libcounter_demo_device.a

# Should show: arm64
```

### Web-Specific

**WASM too large**
```bash
# Optimize with wasm-opt
wasm-opt -Oz platforms/web/pkg/counter_demo_bg.wasm \
  -o platforms/web/pkg/counter_demo_bg.opt.wasm
```

---

## 📚 Next Steps

1. **Customize the app** - Edit `counter_demo.rs`
2. **Add more widgets** - Explore `flui_widgets`
3. **Create your own app** - Copy the pattern
4. **Deploy to stores** - Follow platform guidelines

---

## 🆘 Getting Help

- **Issues**: [GitHub Issues](https://github.com/vanyastaff/flui/issues)
- **Discussions**: [GitHub Discussions](https://github.com/vanyastaff/flui/discussions)
- **Documentation**: [Full Docs](../../docs/)

---

## 📄 License

MIT OR Apache-2.0

---

**Happy Building! 🚀**
