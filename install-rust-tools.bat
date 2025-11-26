@echo off
REM FLUI Rust Development Tools Installation Script
REM Installs all essential cargo tools for Windows development

echo.
echo =====================================
echo FLUI Rust Development Tools Installer
echo =====================================
echo.

REM Check if cargo is installed
cargo --version >nul 2>&1
if errorlevel 1 (
    echo ERROR: Cargo is not installed or not in PATH
    echo Please install Rust first: https://rustup.rs/
    pause
    exit /b 1
)

echo Installing cargo-binstall (binary installer)...
cargo install cargo-binstall
if errorlevel 1 (
    echo WARNING: cargo-binstall installation failed, continuing with regular cargo install
    set USE_BINSTALL=false
) else (
    set USE_BINSTALL=true
)

REM Install nightly toolchain for cargo-udeps
echo.
echo Installing nightly toolchain for cargo-udeps...
rustup toolchain install nightly

REM Define installation command
if "%USE_BINSTALL%"=="true" (
    set INSTALL_CMD=cargo binstall --no-confirm
    echo Using cargo-binstall for faster installation...
) else (
    set INSTALL_CMD=cargo install
    echo Using cargo install...
)

echo.
echo Installing Core Development Tools...
%INSTALL_CMD% cargo-nextest
%INSTALL_CMD% cargo-watch
%INSTALL_CMD% bacon

echo.
echo Installing Security Tools...
%INSTALL_CMD% cargo-audit
%INSTALL_CMD% cargo-deny
%INSTALL_CMD% cargo-geiger

echo.
echo Installing Dependency Management Tools...
%INSTALL_CMD% cargo-outdated
%INSTALL_CMD% cargo-semver-checks
%INSTALL_CMD% cargo-update
%INSTALL_CMD% cargo-hack
%INSTALL_CMD% cargo-minimal-versions
%INSTALL_CMD% cargo-udeps

echo.
echo Installing Build Optimization Tools...
%INSTALL_CMD% cargo-cache
%INSTALL_CMD% cargo-sweep

echo.
echo Installing Release Management Tools...
%INSTALL_CMD% cargo-release
%INSTALL_CMD% git-cliff
%INSTALL_CMD% cargo-msrv

echo.
echo Installing Development Utilities...
%INSTALL_CMD% cargo-expand
%INSTALL_CMD% hyperfine

echo.
echo =====================================
echo Installation Summary
echo =====================================

REM Verify installations
echo.
echo Verifying installations...
echo.

REM Core tools
echo Core Development:
call :check_tool "cargo-nextest" "cargo nextest --version"
call :check_tool "cargo-watch" "cargo watch --version"
call :check_tool "bacon" "bacon --version"

echo.
echo Security:
call :check_tool "cargo-audit" "cargo audit --version"
call :check_tool "cargo-deny" "cargo deny --version"
call :check_tool "cargo-geiger" "cargo geiger --version"

echo.
echo Dependency Management:
call :check_tool "cargo-outdated" "cargo outdated --version"
call :check_tool "cargo-semver-checks" "cargo semver-checks --version"
call :check_tool "cargo-update" "cargo install-update --version"
call :check_tool "cargo-hack" "cargo hack --version"
call :check_tool "cargo-minimal-versions" "cargo minimal-versions --version"
call :check_tool "cargo-udeps" "cargo udeps --version"

echo.
echo Build Optimization:
call :check_tool "cargo-cache" "cargo cache --version"
call :check_tool "cargo-sweep" "cargo sweep --version"

echo.
echo Release Management:
call :check_tool "cargo-release" "cargo release --version"
call :check_tool "git-cliff" "git-cliff --version"
call :check_tool "cargo-msrv" "cargo msrv --version"

echo.
echo Development Utilities:
call :check_tool "cargo-expand" "cargo expand --version"
call :check_tool "hyperfine" "hyperfine --version"

echo.
echo =====================================
echo Installation Complete!
echo =====================================
echo.
echo See RUST_TOOLS.md for usage instructions.
echo.
echo Quick Start Commands:
echo   cargo nextest run          ^(faster testing^)
echo   cargo watch -x check       ^(continuous compilation^)
echo   bacon                      ^(interactive TUI^)
echo   cargo audit                ^(security scan^)
echo   cargo deny check           ^(policy compliance^)
echo   cargo +nightly udeps       ^(find unused deps^)
echo.
echo Happy coding! 🦀
pause
exit /b 0

:check_tool
set tool_name=%~1
set tool_command=%~2
%tool_command% >nul 2>&1
if errorlevel 1 (
    echo   ❌ %tool_name% - FAILED
) else (
    echo   ✅ %tool_name% - OK
)
goto :eof
