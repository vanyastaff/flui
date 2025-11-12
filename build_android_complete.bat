@echo off
setlocal enabledelayedexpansion

echo ========================================
echo Building FLUI Counter for Android
echo ========================================
echo.

REM Set Android environment
set ANDROID_HOME=C:\Users\vanya\AppData\Local\Android\Sdk
set ANDROID_NDK_HOME=C:\Users\vanya\AppData\Local\Android\Sdk\ndk\29.0.14206865
set JAVA_HOME=C:\Users\vanya\AppData\Local\Programs\Android Studio\jbr
set PATH=%JAVA_HOME%\bin;%ANDROID_HOME%\platform-tools;%PATH%

echo Environment:
echo   ANDROID_HOME=%ANDROID_HOME%
echo   ANDROID_NDK_HOME=%ANDROID_NDK_HOME%
echo   JAVA_HOME=%JAVA_HOME%
echo.

REM ============================================================================
REM Step 1: Build Rust Native Libraries
REM ============================================================================

echo [Step 1/3] Building Rust native libraries...
echo.

REM Build for ARM64
echo Building for ARM64 (arm64-v8a)...
cargo ndk -t arm64-v8a ^
    -o platforms\android\app\src\main\jniLibs ^
    --manifest-path crates\flui_app\Cargo.toml ^
    build --example android_empty --release

if %errorlevel% neq 0 (
    echo ERROR: Rust build failed for ARM64
    exit /b 1
)
echo   [OK] ARM64 build complete
echo.

REM Build for ARMv7 (optional)
echo Building for ARMv7 (armeabi-v7a)...
cargo ndk -t armeabi-v7a ^
    -o platforms\android\app\src\main\jniLibs ^
    --manifest-path crates\flui_app\Cargo.toml ^
    build --example android_empty --release

if %errorlevel% neq 0 (
    echo WARNING: ARMv7 build failed (not critical)
) else (
    echo   [OK] ARMv7 build complete
)
echo.

REM ============================================================================
REM Step 2: Build APK with Gradle
REM ============================================================================

echo [Step 2/3] Building APK with Gradle...
echo.

cd platforms\android

REM Check if gradlew exists
if not exist "gradlew.bat" (
    echo ERROR: gradlew.bat not found
    cd ..\..
    exit /b 1
)

REM Build debug APK
call gradlew.bat assembleDebug

if %errorlevel% neq 0 (
    echo ERROR: Gradle build failed
    cd ..\..
    exit /b 1
)

echo   [OK] APK build complete
echo.

cd ..\..

REM ============================================================================
REM Step 3: Show Output
REM ============================================================================

echo [Step 3/3] Build Complete!
echo.
echo ========================================
echo Output Files:
echo ========================================
echo.
echo APK:
echo   platforms\android\app\build\outputs\apk\debug\app-debug.apk
echo.
echo Native Libraries:
dir /B platforms\android\app\src\main\jniLibs\arm64-v8a\*.so 2>nul
dir /B platforms\android\app\src\main\jniLibs\armeabi-v7a\*.so 2>nul
echo.
echo ========================================
echo Next Steps:
echo ========================================
echo.
echo 1. Start your Genymotion emulator
echo 2. Install APK:
echo    adb install -r platforms\android\app\build\outputs\apk\debug\app-debug.apk
echo.
echo 3. Check connected devices:
echo    adb devices
echo.
echo 4. View logs:
echo    adb logcat -s FLUI
echo.

endlocal
