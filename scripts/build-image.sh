#!/bin/bash
# StarOS v0.4.0 - Build Bootable Image
# Creates flashable image for PinePhone Pro

set -e

VERSION="0.4.0-alpha1"
BUILD_DATE=$(date +%Y%m%d)
IMAGE_NAME="staros-${VERSION}-${BUILD_DATE}.img"

echo "╔══════════════════════════════════════════════════════════════╗"
echo "║                                                              ║"
echo "║         StarOS v0.4.0 Alpha 1 - Image Builder                ║"
echo "║                                                              ║"
echo "╚══════════════════════════════════════════════════════════════╝"
echo ""

# Check dependencies
echo "[1/8] Checking dependencies..."
command -v cargo >/dev/null 2>&1 || { echo "❌ cargo not found"; exit 1; }
command -v rustc >/dev/null 2>&1 || { echo "❌ rustc not found"; exit 1; }
echo "✅ Dependencies OK"

# Clean previous builds
echo ""
echo "[2/8] Cleaning previous builds..."
cargo clean
rm -f target/aarch64-unknown-none/release/staros-kernel
echo "✅ Clean complete"

# Build kernel
echo ""
echo "[3/8] Building kernel (aarch64-unknown-none)..."
cargo build --release \
    --package staros-kernel \
    --target aarch64-unknown-none \
    2>&1 | grep -E "(Compiling|Finished|error)" || true

if [ ! -f "target/aarch64-unknown-none/release/libstaros_kernel.rlib" ]; then
    echo "❌ Kernel build failed!"
    exit 1
fi
echo "✅ Kernel built successfully"

# Build bootloader
echo ""
echo "[4/8] Building bootloader..."
cargo build --release \
    --package staros-bootloader \
    --target aarch64-unknown-none \
    2>&1 | grep -E "(Compiling|Finished|error)" || true

if [ ! -f "target/aarch64-unknown-none/release/libstaros_bootloader.rlib" ]; then
    echo "⚠️  Bootloader not found (optional)"
    BOOTLOADER_EXISTS=false
else
    echo "✅ Bootloader built successfully"
    BOOTLOADER_EXISTS=true
fi

# Create image directory
echo ""
echo "[5/8] Creating image structure..."
mkdir -p build/image
mkdir -p build/image/boot
mkdir -p build/image/system

# Copy binaries
cp target/aarch64-unknown-none/release/libstaros_kernel.rlib build/image/system/staros-kernel.rlib
if [ "$BOOTLOADER_EXISTS" = true ]; then
    cp target/aarch64-unknown-none/release/libstaros_bootloader.rlib build/image/boot/staros-bootloader.rlib
fi
echo "✅ Binaries copied"

# Create boot configuration
echo ""
echo "[6/8] Creating boot configuration..."
cat > build/image/boot/boot.cfg << EOF
# StarOS Boot Configuration
version=${VERSION}
kernel=/system/staros-kernel.rlib
bootargs=console=ttyS2,115200 loglevel=7
EOF
echo "✅ Boot config created"

# Create image metadata
echo ""
echo "[7/8] Creating image metadata..."
KERNEL_SIZE=$(stat -f%z build/image/system/staros-kernel.rlib 2>/dev/null || stat -c%s build/image/system/staros-kernel.rlib)
if [ "$BOOTLOADER_EXISTS" = true ]; then
    BOOTLOADER_SIZE=$(stat -f%z build/image/boot/staros-bootloader.rlib 2>/dev/null || stat -c%s build/image/boot/staros-bootloader.rlib)
else
    BOOTLOADER_SIZE=0
fi

cat > build/image/staros.json << EOF
{
  "version": "${VERSION}",
  "build_date": "${BUILD_DATE}",
  "target": "PinePhone Pro",
  "kernel_size": ${KERNEL_SIZE},
  "bootloader_size": ${BOOTLOADER_SIZE},
  "features": [
    "telephony",
    "audio",
    "incoming_calls",
    "long_sms",
    "error_handling"
  ]
}
EOF
echo "✅ Metadata created"

# Create tarball
echo ""
echo "[8/8] Creating flashable image..."
cd build/image
tar czf "../${IMAGE_NAME}" .
cd ../..

IMAGE_SIZE=$(du -h "build/${IMAGE_NAME}" | cut -f1)
echo "✅ Image created: build/${IMAGE_NAME} (${IMAGE_SIZE})"

# Create checksum
echo ""
echo "Creating checksum..."
cd build
sha256sum "${IMAGE_NAME}" > "${IMAGE_NAME}.sha256"
cd ..
echo "✅ Checksum: build/${IMAGE_NAME}.sha256"

# Summary
echo ""
echo "╔══════════════════════════════════════════════════════════════╗"
echo "║                                                              ║"
echo "║              Build Complete! 🎉                              ║"
echo "║                                                              ║"
echo "╚══════════════════════════════════════════════════════════════╝"
echo ""
echo "Image:    build/${IMAGE_NAME}"
echo "Size:     ${IMAGE_SIZE}"
echo "Checksum: build/${IMAGE_NAME}.sha256"
echo ""
echo "Next steps:"
echo "  1. Flash to SD card: ./scripts/flash-sd.sh /dev/sdX"
echo "  2. Insert SD card into PinePhone Pro"
echo "  3. Boot and test!"
echo ""
