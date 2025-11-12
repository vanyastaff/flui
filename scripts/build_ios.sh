#!/bin/bash
set -e

# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

echo "========================================"
echo "Building FLUI Counter for iOS"
echo "========================================"
echo ""

# ============================================================================
# Configuration
# ============================================================================

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
IOS_DIR="$PROJECT_ROOT/platforms/ios"
EXAMPLE_NAME="counter_demo"

# ============================================================================
# Step 1: Check Prerequisites
# ============================================================================

echo -e "${YELLOW}[1/5] Checking prerequisites...${NC}"
echo ""

# Check if running on macOS
if [[ "$OSTYPE" != "darwin"* ]]; then
    echo -e "${RED}Error: iOS build requires macOS${NC}"
    exit 1
fi
echo "  - macOS: OK"

# Check Rust
if ! command -v rustc &> /dev/null; then
    echo -e "${RED}Error: Rust not found. Install from https://rustup.rs/${NC}"
    exit 1
fi
echo "  - Rust: $(rustc --version)"

# Check Xcode
if ! command -v xcodebuild &> /dev/null; then
    echo -e "${RED}Error: Xcode not found. Install from App Store${NC}"
    exit 1
fi
echo "  - Xcode: $(xcodebuild -version | head -n1)"

# Check CocoaPods (optional)
if ! command -v pod &> /dev/null; then
    echo -e "${YELLOW}  Warning: CocoaPods not found (optional)${NC}"
else
    echo "  - CocoaPods: $(pod --version)"
fi

echo ""

# ============================================================================
# Step 2: Add Rust Targets
# ============================================================================

echo -e "${YELLOW}[2/5] Installing Rust targets...${NC}"
echo ""

# iOS Device (ARM64)
rustup target add aarch64-apple-ios
if [ $? -ne 0 ]; then
    echo -e "${RED}Error: Failed to add iOS device target${NC}"
    exit 1
fi
echo "  - iOS Device (aarch64-apple-ios): OK"

# iOS Simulator (ARM64 for Apple Silicon Macs)
rustup target add aarch64-apple-ios-sim
if [ $? -ne 0 ]; then
    echo -e "${YELLOW}  Warning: ARM64 simulator target failed${NC}"
else
    echo "  - iOS Simulator ARM64 (aarch64-apple-ios-sim): OK"
fi

# iOS Simulator (x86_64 for Intel Macs)
rustup target add x86_64-apple-ios
if [ $? -ne 0 ]; then
    echo -e "${YELLOW}  Warning: x86_64 simulator target failed${NC}"
else
    echo "  - iOS Simulator x86_64 (x86_64-apple-ios): OK"
fi

echo ""

# ============================================================================
# Step 3: Build Rust Library
# ============================================================================

echo -e "${YELLOW}[3/5] Building Rust library...${NC}"
echo ""

cd "$PROJECT_ROOT"

# Build for iOS Device (ARM64)
echo "Building for iOS Device..."
cargo build \
    --manifest-path crates/flui_app/Cargo.toml \
    --example $EXAMPLE_NAME \
    --target aarch64-apple-ios \
    --release

if [ $? -ne 0 ]; then
    echo -e "${RED}Error: Build failed for iOS device${NC}"
    exit 1
fi
echo -e "  ${GREEN}✓ iOS Device build complete${NC}"

# Build for iOS Simulator (ARM64 - Apple Silicon)
echo ""
echo "Building for iOS Simulator (ARM64)..."
cargo build \
    --manifest-path crates/flui_app/Cargo.toml \
    --example $EXAMPLE_NAME \
    --target aarch64-apple-ios-sim \
    --release

if [ $? -ne 0 ]; then
    echo -e "${YELLOW}  Warning: ARM64 simulator build failed${NC}"
else
    echo -e "  ${GREEN}✓ iOS Simulator ARM64 build complete${NC}"
fi

# Build for iOS Simulator (x86_64 - Intel)
echo ""
echo "Building for iOS Simulator (x86_64)..."
cargo build \
    --manifest-path crates/flui_app/Cargo.toml \
    --example $EXAMPLE_NAME \
    --target x86_64-apple-ios \
    --release

if [ $? -ne 0 ]; then
    echo -e "${YELLOW}  Warning: x86_64 simulator build failed${NC}"
else
    echo -e "  ${GREEN}✓ iOS Simulator x86_64 build complete${NC}"
fi

echo ""

# ============================================================================
# Step 4: Copy Libraries to Xcode Project
# ============================================================================

echo -e "${YELLOW}[4/5] Copying libraries...${NC}"
echo ""

# Create Libraries directory
mkdir -p "$IOS_DIR/Libraries"

# Copy device library
cp "target/aarch64-apple-ios/release/examples/lib$EXAMPLE_NAME.a" \
   "$IOS_DIR/Libraries/lib${EXAMPLE_NAME}_device.a"
echo -e "  ${GREEN}✓ Device library copied${NC}"

# Copy simulator library (ARM64)
if [ -f "target/aarch64-apple-ios-sim/release/examples/lib$EXAMPLE_NAME.a" ]; then
    cp "target/aarch64-apple-ios-sim/release/examples/lib$EXAMPLE_NAME.a" \
       "$IOS_DIR/Libraries/lib${EXAMPLE_NAME}_sim_arm64.a"
    echo -e "  ${GREEN}✓ Simulator ARM64 library copied${NC}"
fi

# Copy simulator library (x86_64)
if [ -f "target/x86_64-apple-ios/release/examples/lib$EXAMPLE_NAME.a" ]; then
    cp "target/x86_64-apple-ios/release/examples/lib$EXAMPLE_NAME.a" \
       "$IOS_DIR/Libraries/lib${EXAMPLE_NAME}_sim_x64.a"
    echo -e "  ${GREEN}✓ Simulator x86_64 library copied${NC}"
fi

echo ""

# ============================================================================
# Step 5: Create Universal Library (Optional)
# ============================================================================

echo -e "${YELLOW}[5/5] Creating universal library...${NC}"
echo ""

# Create universal simulator library if both architectures exist
if [ -f "$IOS_DIR/Libraries/lib${EXAMPLE_NAME}_sim_arm64.a" ] && \
   [ -f "$IOS_DIR/Libraries/lib${EXAMPLE_NAME}_sim_x64.a" ]; then
    
    echo "Creating universal simulator library..."
    lipo -create \
        "$IOS_DIR/Libraries/lib${EXAMPLE_NAME}_sim_arm64.a" \
        "$IOS_DIR/Libraries/lib${EXAMPLE_NAME}_sim_x64.a" \
        -output "$IOS_DIR/Libraries/lib${EXAMPLE_NAME}_sim.a"
    
    echo -e "  ${GREEN}✓ Universal simulator library created${NC}"
else
    echo -e "${YELLOW}  Skipping universal library (missing architectures)${NC}"
fi

echo ""

# ============================================================================
# Summary
# ============================================================================

echo "========================================"
echo "Build Complete!"
echo "========================================"
echo ""
echo "Libraries copied to:"
echo "  $IOS_DIR/Libraries/"
echo ""
echo "Next steps:"
echo "  1. Open $IOS_DIR/FluiCounter.xcodeproj in Xcode"
echo "  2. Select target device or simulator"
echo "  3. Click Run (⌘R)"
echo ""
echo "To build from command line:"
echo "  xcodebuild -project $IOS_DIR/FluiCounter.xcodeproj \\"
echo "             -scheme FluiCounter \\"
echo "             -configuration Release \\"
echo "             -sdk iphoneos \\"
echo "             build"
echo ""
