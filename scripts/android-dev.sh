#!/usr/bin/env bash
# Android hot-reload development script for FLUI
#
# Two modes:
#
# SCENE MODE (--scene): True hot-reload — sub-second visual updates!
#   Watches scene plugin source, cross-compiles .so (~0.2s), pushes via adb+run-as.
#   The host app detects the file change and reloads via dlopen — no restart needed.
#
# FULL MODE (default): Full rebuild — APK reinstall + app restart (~5s).
#   Watches all Rust source, cross-compiles host .so, packages APK, reinstalls.
#
# Usage:
#   ./scripts/android-dev.sh --scene            # hot-reload scene plugin (FAST)
#   ./scripts/android-dev.sh                    # full rebuild (default)
#   ./scripts/android-dev.sh --crate my-app     # custom crate
#   ./scripts/android-dev.sh --release          # release build
#
# Requirements: cargo-ndk, adb, Android NDK, Gradle wrapper (full mode only)

set -euo pipefail

# ── Defaults ──────────────────────────────────────────────────────────────────
CRATE="flui-android-demo"
SCENE_CRATE="flui-android-scene"
SCENE_LIB="libflui_scene.so"
PACKAGE="com.vanya.flui.counter"
ACTIVITY="android.app.NativeActivity"
TARGET="arm64-v8a"
RELEASE=""
POLL_INTERVAL=1
PROJECT_ROOT=""
SCENE_MODE=false

# ── Colors ────────────────────────────────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
DIM='\033[2m'
RESET='\033[0m'

# ── Parse args ────────────────────────────────────────────────────────────────
while [[ $# -gt 0 ]]; do
    case "$1" in
        --scene)    SCENE_MODE=true; shift ;;
        --crate)    CRATE="$2"; shift 2 ;;
        --package)  PACKAGE="$2"; shift 2 ;;
        --target)   TARGET="$2"; shift 2 ;;
        --release)  RELEASE="--release"; shift ;;
        --help|-h)
            echo "Usage: $0 [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --scene           Scene hot-reload mode (fast, no app restart)"
            echo "  --crate <name>    Cargo crate to build (default: flui-android-demo)"
            echo "  --package <id>    Android package name (default: com.vanya.flui.counter)"
            echo "  --target <abi>    Android ABI target (default: arm64-v8a)"
            echo "  --release         Build in release mode"
            echo "  -h, --help        Show this help"
            exit 0
            ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

# ── Resolve project root ─────────────────────────────────────────────────────
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$PROJECT_ROOT"

JNILIBS_DIR="platforms/android/app/src/main/jniLibs"
GRADLE_DIR="platforms/android"
APK_PATH="platforms/build/app/outputs/apk/debug/app-debug.apk"
if [[ -n "$RELEASE" ]]; then
    APK_PATH="platforms/build/app/outputs/apk/release/app-release.apk"
fi

# ── Validate environment ─────────────────────────────────────────────────────
log()  { echo -e "${GREEN}[flui]${RESET} $*"; }
warn() { echo -e "${YELLOW}[flui]${RESET} $*"; }
err()  { echo -e "${RED}[flui]${RESET} $*"; }
dim()  { echo -e "${DIM}$*${RESET}"; }

check_tool() {
    if ! command -v "$1" &>/dev/null; then
        err "Required tool not found: $1"
        exit 1
    fi
}

check_tool cargo
check_tool adb

if ! cargo ndk --version &>/dev/null; then
    err "cargo-ndk not found. Install with: cargo install cargo-ndk"
    exit 1
fi

# Resolve NDK
if [[ -z "${ANDROID_NDK_HOME:-}" ]]; then
    # Auto-detect from ANDROID_HOME or common paths
    for base in "${ANDROID_HOME:-}" "${ANDROID_SDK_ROOT:-}" \
                "$HOME/Android/Sdk" "$LOCALAPPDATA/Android/Sdk" \
                "/usr/local/lib/android/sdk"; do
        if [[ -n "$base" && -d "$base/ndk" ]]; then
            NDK_DIR=$(ls -1d "$base/ndk"/*/ 2>/dev/null | sort -V | tail -1)
            if [[ -n "$NDK_DIR" ]]; then
                export ANDROID_NDK_HOME="$NDK_DIR"
                break
            fi
        fi
    done
fi

if [[ -z "${ANDROID_NDK_HOME:-}" ]]; then
    err "ANDROID_NDK_HOME not set and could not auto-detect NDK"
    exit 1
fi

log "NDK: ${CYAN}$ANDROID_NDK_HOME${RESET}"

# Check device
if ! adb get-state &>/dev/null; then
    err "No Android device connected (adb get-state failed)"
    exit 1
fi

DEVICE=$(adb shell getprop ro.product.model 2>/dev/null | tr -d '\r')
log "Device: ${CYAN}$DEVICE${RESET}"
if [[ "$SCENE_MODE" == true ]]; then
    log "Crate: ${CYAN}$SCENE_CRATE${RESET} (scene plugin)"
else
    log "Crate: ${CYAN}$CRATE${RESET}"
fi
log "Package: ${CYAN}$PACKAGE${RESET}"
log "Target: ${CYAN}$TARGET${RESET}"

# ── Build & Deploy ────────────────────────────────────────────────────────────
build_and_deploy() {
    local start_time
    start_time=$(date +%s)

    # Step 1: Cross-compile
    log "Compiling .so ..."
    if ! cargo ndk -t "$TARGET" -o "$JNILIBS_DIR" build -p "$CRATE" $RELEASE 2>&1; then
        err "cargo ndk build FAILED"
        return 1
    fi

    # Step 2: Package APK
    log "Packaging APK ..."
    local gradle_cmd="assembleDebug"
    if [[ -n "$RELEASE" ]]; then
        gradle_cmd="assembleRelease"
    fi
    if ! (cd "$GRADLE_DIR" && ./gradlew "$gradle_cmd" 2>&1 | tail -3); then
        err "Gradle build FAILED"
        return 1
    fi

    # Step 3: Install
    log "Installing ..."
    if ! adb install -r "$APK_PATH" 2>&1; then
        err "adb install FAILED"
        return 1
    fi

    # Step 4: Fast restart — kill process directly (avoids close animation),
    # then immediately relaunch. This is faster than force-stop which waits
    # for the app to handle the stop lifecycle.
    local old_pid
    old_pid=$(adb shell pidof "$PACKAGE" 2>/dev/null | tr -d '\r')
    if [[ -n "$old_pid" ]]; then
        adb shell kill "$old_pid" 2>/dev/null || adb shell am force-stop "$PACKAGE" 2>/dev/null
        # Brief wait for process to die before relaunch
        sleep 0.3
    fi
    adb shell am start -n "$PACKAGE/$ACTIVITY" --activity-clear-task 2>&1 | grep -v "^$"

    local end_time
    end_time=$(date +%s)
    local duration=$((end_time - start_time))
    log "${GREEN}Deployed in ${duration}s${RESET}"

    # Step 5: Show logcat (brief, non-blocking)
    sleep 0.5
    local pid
    pid=$(adb shell pidof "$PACKAGE" 2>/dev/null | tr -d '\r')
    if [[ -n "$pid" ]]; then
        dim "--- logcat (PID $pid) ---"
        adb logcat --pid="$pid" -d -t 20 2>/dev/null | grep -i "flui_app\|scene\|render\|error\|panic" | tail -10
        dim "------------------------"
    fi

    return 0
}

# ── Scene-only build & push (no APK, no restart) ─────────────────────────────
build_and_push_scene() {
    local start_time
    start_time=$(date +%s%3N 2>/dev/null || date +%s)

    # Step 1: Cross-compile scene plugin only
    log "Compiling scene plugin ..."
    if ! cargo ndk -t "$TARGET" build -p "$SCENE_CRATE" $RELEASE 2>&1; then
        err "cargo ndk build FAILED"
        return 1
    fi

    # Step 2: Determine .so path from target dir
    local profile_dir="debug"
    if [[ -n "$RELEASE" ]]; then
        profile_dir="release"
    fi
    # Map TARGET ABI to Rust target triple
    local rust_target=""
    case "$TARGET" in
        arm64-v8a)      rust_target="aarch64-linux-android" ;;
        armeabi-v7a)    rust_target="armv7-linux-androideabi" ;;
        x86_64)         rust_target="x86_64-linux-android" ;;
        x86)            rust_target="i686-linux-android" ;;
        *)              err "Unknown target: $TARGET"; return 1 ;;
    esac
    local so_path="target/${rust_target}/${profile_dir}/${SCENE_LIB}"
    if [[ ! -f "$so_path" ]]; then
        err "Built .so not found at: $so_path"
        return 1
    fi

    # Step 3: Push to device via tmp + run-as copy (SELinux workaround)
    log "Pushing to device ..."
    if ! MSYS_NO_PATHCONV=1 adb push "$so_path" "/data/local/tmp/${SCENE_LIB}" 2>&1; then
        err "adb push FAILED"
        return 1
    fi
    if ! MSYS_NO_PATHCONV=1 adb shell "run-as $PACKAGE cp /data/local/tmp/${SCENE_LIB} /data/data/${PACKAGE}/files/${SCENE_LIB}" 2>&1; then
        err "run-as copy FAILED"
        return 1
    fi

    local end_time
    end_time=$(date +%s%3N 2>/dev/null || date +%s)
    # Try millisecond precision, fall back to seconds
    if [[ ${#start_time} -gt 10 && ${#end_time} -gt 10 ]]; then
        local duration_ms=$((end_time - start_time))
        log "${GREEN}Scene pushed in ${duration_ms}ms — app will reload automatically${RESET}"
    else
        local duration=$((end_time - start_time))
        log "${GREEN}Scene pushed in ${duration}s — app will reload automatically${RESET}"
    fi

    return 0
}

# ── File change detection ─────────────────────────────────────────────────────
# Portable — works with just find + stat (no inotify/fswatch needed).
snapshot() {
    if [[ "$SCENE_MODE" == true ]]; then
        find examples/android_scene/src -name '*.rs' -newer "$MARKER_FILE" 2>/dev/null | head -1
    else
        find examples/android_demo/src crates -name '*.rs' -newer "$MARKER_FILE" 2>/dev/null | head -1
    fi
}

# ── Main loop ─────────────────────────────────────────────────────────────────

# Trap Ctrl+C
cleanup() {
    echo ""
    log "Stopping..."
    rm -f "$MARKER_FILE"
    exit 0
}
trap cleanup SIGINT SIGTERM

MARKER_FILE=$(mktemp)
touch "$MARKER_FILE"

# Select build function based on mode
if [[ "$SCENE_MODE" == true ]]; then
    build_fn="build_and_push_scene"
    log "Mode: ${CYAN}SCENE HOT-RELOAD${RESET} (watching examples/android_scene/src/)"
    log "Edit scene → save → see change on device (no restart!)"
else
    build_fn="build_and_deploy"
    log "Mode: ${CYAN}FULL REBUILD${RESET} (watching examples/android_demo/src/ + crates/)"
    log "Tip: use ${CYAN}--scene${RESET} for sub-second scene hot-reload"
fi

echo ""
log "=== Initial build ==="
if $build_fn; then
    log "${GREEN}Ready!${RESET}"
else
    warn "Initial build failed — fix errors and save to retry"
fi

# Update marker after initial build
touch "$MARKER_FILE"

echo ""
log "Watching for changes... ${DIM}(Ctrl+C to stop)${RESET}"

while true; do
    sleep "$POLL_INTERVAL"

    changed=$(snapshot)
    if [[ -n "$changed" ]]; then
        echo ""
        log "Change detected: ${CYAN}$changed${RESET}"

        if $build_fn; then
            log "${GREEN}Ready!${RESET}"
        else
            warn "Build failed — fix errors and save to retry"
        fi

        # Update marker after build attempt
        touch "$MARKER_FILE"

        echo ""
        log "Watching for changes... ${DIM}(Ctrl+C to stop)${RESET}"
    fi
done
