#!/bin/bash
# Build kernel and bootloader for hardware deployment

set -e

echo "🔨 Building StarOS for Hardware Deployment"
echo "==========================================="
echo ""

TARGET_ARCH=${1:-aarch64}

if [ "$TARGET_ARCH" = "aarch64" ]; then
    TARGET="aarch64-unknown-none"
    LINKER_SCRIPT="kernel/linker-arm64.ld"
    echo "Target: ARM64 (PinePhone Pro)"
elif [ "$TARGET_ARCH" = "riscv64" ]; then
    TARGET="riscv64gc-unknown-none-elf"
    LINKER_SCRIPT="kernel/linker-riscv.ld"
    echo "Target: RISC-V"
else
    echo "❌ Unknown architecture: $TARGET_ARCH"
    echo "Usage: $0 [aarch64|riscv64]"
    exit 1
fi

echo ""
echo "📦 Building kernel..."
RUSTFLAGS="-C link-arg=-T$LINKER_SCRIPT" \
    cargo build --release \
    --package staros-kernel \
    --target $TARGET \
    --lib

echo ""
echo "📦 Building bootloader..."
cargo build --release \
    --package staros-bootloader \
    --target $TARGET \
    --lib

echo ""
echo "✅ Build complete!"
echo ""
echo "Artifacts:"
echo "  Kernel:     target/$TARGET/release/libstaros_kernel.a"
echo "  Bootloader: target/$TARGET/release/libstaros_bootloader.a"
echo ""
echo "Next steps:"
echo "  1. Create bootable image: ./scripts/create-image.sh $TARGET_ARCH"
echo "  2. Flash to device: ./scripts/flash-device.sh $TARGET_ARCH"
echo "  3. Test in QEMU: ./scripts/qemu-test-$TARGET_ARCH.sh"
