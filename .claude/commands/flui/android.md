---
name: Android Build
description: Build and deploy FLUI app to Android device
---

Build and deploy to Android:

1. **Build APK**:
```bash
cargo xtask build android --release
```

2. **Install on device**:
```bash
adb install -r platforms/android/app/build/outputs/apk/debug/app-debug.apk
```

3. **Launch app**:
```bash
adb shell am start -n com.vanya.flui.counter.debug/android.app.NativeActivity
```

4. **View logs**:
```bash
adb logcat -s RustStdoutStderr:V
```

If $ARGUMENTS contains "logcat", also show live logs after launching.
