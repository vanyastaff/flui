@echo off
setlocal enabledelayedexpansion

echo ========================================
echo Building FLUI Counter for Android
echo ========================================
echo.

:: ============================================================================
:: Configuration
:: ============================================================================

set PROJECT_ROOT=%~dp0..
set ANDROID_DIR=%PROJECT_ROOT%\platforms\android
set EXAMPLE_NAME=counter_demo

:: Colors for output (if terminal supports ANSI)
set GREEN=[92m
set YELLOW=[93m
set RED=[91m
set RESET=[0m

:: ============================================================================
:: Step 1: Check Prerequisites
:: ============================================================================

echo %YELLOW%[1/5] Checking prerequisites...%RESET%
echo.

:: Check Rust
where rustc >nul 2>&1
if %errorlevel% neq 0 (
    echo %RED%Error: Rust not found. Install from https://rustup.rs/%RESET%
    exit /b 1
)
echo   - Rust: OK

:: Check cargo-ndk
where cargo-ndk >nul 2>&1
if %errorlevel% neq 0 (
    echo %YELLOW%  Warning: cargo-ndk not found. Installing...%RESET%
    cargo install cargo-ndk
    if %errorlevel% neq 0 (
        echo %RED%Error: Failed to install cargo-ndk%RESET%
        exit /b 1
    )
)
echo   - cargo-ndk: OK

:: Check Android SDK
if not defined ANDROID_HOME (
    echo %RED%Error: ANDROID_HOME not set%RESET%
    echo Please set ANDROID_HOME environment variable
    exit /b 1
)
echo   - Android SDK: %ANDROID_HOME%

:: Check Gradle wrapper
if not exist "%ANDROID_DIR%\gradlew.bat" (
    echo %RED%Error: Gradle wrapper not found%RESET%
    echo Run: gradle wrapper in %ANDROID_DIR%
    exit /b 1
)
echo   - Gradle: OK

:: Check ADB (optional, for automatic installation)
where adb >nul 2>&1
if %errorlevel% neq 0 (
    echo %YELLOW%  Warning: adb not in PATH. Won't auto-install APK.%RESET%
    set ADB_AVAILABLE=0
) else (
    echo   - ADB: OK
    set ADB_AVAILABLE=1
)

echo.

:: ============================================================================
:: Step 2: Add Rust Targets
:: ============================================================================

echo %YELLOW%[2/5] Installing Rust targets...%RESET%
echo.

rustup target add aarch64-linux-android
if %errorlevel% neq 0 (
    echo %RED%Error: Failed to add ARM64 target%RESET%
    exit /b 1
)
echo   - ARM64 (aarch64-linux-android): OK

:: Optional: Add ARMv7 for older devices
rustup target add armv7-linux-androideabi
if %errorlevel% neq 0 (
    echo %YELLOW%  Warning: ARMv7 target failed (not critical)%RESET%
) else (
    echo   - ARMv7 (armv7-linux-androideabi): OK
)

echo.

:: ============================================================================
:: Step 3: Build Rust Library
:: ============================================================================

echo %YELLOW%[3/5] Building Rust library...%RESET%
echo.

cd /d "%PROJECT_ROOT%"

:: Clean previous builds (optional)
:: cargo clean

:: Build for ARM64 (primary target)
echo Building for ARM64...
cargo ndk -t arm64-v8a ^
    -o platforms\android\app\src\main\jniLibs ^
    --manifest-path crates\flui_app\Cargo.toml ^
    build --example %EXAMPLE_NAME% --release

if %errorlevel% neq 0 (
    echo %RED%Error: Rust build failed for ARM64%RESET%
    exit /b 1
)
echo   %GREEN%✓ ARM64 build complete%RESET%

:: Build for ARMv7 (optional, for older devices)
echo.
echo Building for ARMv7...
cargo ndk -t armeabi-v7a ^
    -o platforms\android\app\src\main\jniLibs ^
    --manifest-path crates\flui_app\Cargo.toml ^
    build --example %EXAMPLE_NAME% --release

if %errorlevel% neq 0 (
    echo %YELLOW%  Warning: ARMv7 build failed (not critical)%RESET%
) else (
    echo   %GREEN%✓ ARMv7 build complete%RESET%
)

echo.

:: ============================================================================
:: Step 4: Build APK
:: ============================================================================

echo %YELLOW%[4/5] Building APK with Gradle...%RESET%
echo.

cd /d "%ANDROID_DIR%"

:: Build debug APK
call gradlew assembleDebug

if %errorlevel% neq 0 (
    echo %RED%Error: Gradle build failed%RESET%
    cd /d "%PROJECT_ROOT%"
    exit /b 1
)

echo   %GREEN%✓ APK build complete%RESET%
echo.

:: ============================================================================
:: Step 5: Install APK (if ADB available)
:: ============================================================================

echo %YELLOW%[5/5] Installing APK...%RESET%
echo.

if %ADB_AVAILABLE%==1 (
    :: Check if device is connected
    adb devices | findstr "device$" >nul
    if %errorlevel% neq 0 (
        echo %YELLOW%No device connected. Skipping installation.%RESET%
        goto :show_output
    )

    :: Install APK
    adb install -r app\build\outputs\apk\debug\app-debug.apk
    if %errorlevel% neq 0 (
        echo %RED%Error: Installation failed%RESET%
        goto :show_output
    )

    echo   %GREEN%✓ APK installed successfully%RESET%
    echo.

    :: Launch app (optional)
    echo Launching app...
    adb shell am start -n com.vanya.flui.counter.debug/android.app.NativeActivity
    
    echo.
    echo %GREEN%App is running on device!%RESET%
    echo.
    echo To view logs:
    echo   adb logcat -s FLUI
) else (
    echo %YELLOW%ADB not available. Please install manually:%RESET%
    goto :show_output
)

:: ============================================================================
:: Show Output Location
:: ============================================================================

:show_output
echo.
echo ========================================
echo Build Complete!
echo ========================================
echo.
echo APK Location:
echo   %ANDROID_DIR%\app\build\outputs\apk\debug\app-debug.apk
echo.
echo Native Libraries:
echo   %ANDROID_DIR%\app\src\main\jniLibs\arm64-v8a\lib%EXAMPLE_NAME%.so
echo   %ANDROID_DIR%\app\src\main\jniLibs\armeabi-v7a\lib%EXAMPLE_NAME%.so
echo.

if %ADB_AVAILABLE%==0 (
    echo To install manually:
    echo   1. Connect Android device
    echo   2. Enable USB debugging
    echo   3. Run: adb install -r app\build\outputs\apk\debug\app-debug.apk
    echo.
)

cd /d "%PROJECT_ROOT%"
endlocal
