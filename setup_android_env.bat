@echo off
REM Android SDK and NDK Environment Setup for FLUI

echo Setting up Android environment variables...

set ANDROID_HOME=C:\Users\vanya\AppData\Local\Android\Sdk
set ANDROID_NDK_HOME=C:\Users\vanya\AppData\Local\Android\Sdk\ndk\29.0.14206865
set JAVA_HOME=C:\Users\vanya\AppData\Local\Programs\Android Studio\jbr
set PATH=%JAVA_HOME%\bin;%ANDROID_HOME%\platform-tools;%ANDROID_HOME%\cmdline-tools\latest\bin;%PATH%

echo.
echo ========================================
echo Android Environment Variables Set:
echo ========================================
echo ANDROID_HOME=%ANDROID_HOME%
echo ANDROID_NDK_HOME=%ANDROID_NDK_HOME%
echo JAVA_HOME=%JAVA_HOME%
echo.
echo Platform-tools, Java, and adb added to PATH
echo ========================================
echo.

REM Verify installation
echo Verifying installation...
echo.

if exist "%ANDROID_HOME%\platform-tools\adb.exe" (
    echo [OK] ADB found: %ANDROID_HOME%\platform-tools\adb.exe
    "%ANDROID_HOME%\platform-tools\adb.exe" version
) else (
    echo [WARNING] ADB not found
)

echo.
if exist "%ANDROID_NDK_HOME%" (
    echo [OK] NDK found: %ANDROID_NDK_HOME%
) else (
    echo [ERROR] NDK not found at %ANDROID_NDK_HOME%
)

echo.
echo Environment setup complete!
echo.
echo To make these permanent, add them to System Environment Variables:
echo 1. Search "Environment Variables" in Windows
echo 2. Add ANDROID_HOME and ANDROID_NDK_HOME to User or System variables
echo 3. Add platform-tools to PATH
echo.
