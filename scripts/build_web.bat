@echo off
setlocal

echo ========================================
echo Building FLUI Counter for Web
echo ========================================
echo.

:: ============================================================================
:: Configuration
:: ============================================================================

set PROJECT_ROOT=%~dp0..
set WEB_DIR=%PROJECT_ROOT%\platforms\web
set EXAMPLE_NAME=counter_demo

:: ============================================================================
:: Step 1: Check Prerequisites
:: ============================================================================

echo [1/4] Checking prerequisites...
echo.

:: Check Rust
where rustc >nul 2>&1
if %errorlevel% neq 0 (
    echo Error: Rust not found. Install from https://rustup.rs/
    exit /b 1
)
echo   - Rust: OK

:: Check wasm-pack
where wasm-pack >nul 2>&1
if %errorlevel% neq 0 (
    echo Warning: wasm-pack not found. Installing...
    cargo install wasm-pack
    if %errorlevel% neq 0 (
        echo Error: Failed to install wasm-pack
        exit /b 1
    )
)
echo   - wasm-pack: OK

echo.

:: ============================================================================
:: Step 2: Add Rust Target
:: ============================================================================

echo [2/4] Installing Rust WASM target...
echo.

rustup target add wasm32-unknown-unknown
if %errorlevel% neq 0 (
    echo Error: Failed to add WASM target
    exit /b 1
)
echo   - wasm32-unknown-unknown: OK

echo.

:: ============================================================================
:: Step 3: Build WebAssembly
:: ============================================================================

echo [3/4] Building WebAssembly...
echo.

cd /d "%PROJECT_ROOT%"

:: Build with wasm-pack
wasm-pack build ^
    --target web ^
    --out-dir ../../platforms/web/pkg ^
    --out-name %EXAMPLE_NAME% ^
    crates/flui_app ^
    --release ^
    -- --example %EXAMPLE_NAME%

if %errorlevel% neq 0 (
    echo Error: WASM build failed
    exit /b 1
)

echo   ✓ WebAssembly build complete
echo.

:: ============================================================================
:: Step 4: Setup Web Server
:: ============================================================================

echo [4/4] Setting up web server...
echo.

:: Check if Python is available (for local server)
where python >nul 2>&1
if %errorlevel% equ 0 (
    set PYTHON_CMD=python
    goto :python_found
)

where python3 >nul 2>&1
if %errorlevel% equ 0 (
    set PYTHON_CMD=python3
    goto :python_found
)

echo Warning: Python not found. Cannot auto-start web server.
goto :manual_server

:python_found
echo Python found. Starting local web server...
echo.

cd /d "%WEB_DIR%"
echo ========================================
echo Server running at:
echo   http://localhost:8080
echo.
echo Press Ctrl+C to stop
echo ========================================
echo.
%PYTHON_CMD% -m http.server 8080

goto :end

:manual_server
echo.
echo ========================================
echo Build Complete!
echo ========================================
echo.
echo Files location:
echo   %WEB_DIR%\pkg\
echo.
echo To run locally, start a web server:
echo.
echo   cd platforms\web
echo   python -m http.server 8080
echo.
echo   OR
echo.
echo   npx http-server platforms\web -p 8080
echo.
echo Then open: http://localhost:8080
echo.

:end
cd /d "%PROJECT_ROOT%"
endlocal
