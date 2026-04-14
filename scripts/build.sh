#!/bin/bash
# Build script for StarOS

set -e

echo "Building StarOS..."

# Parse arguments
ARCH=${1:-riscv64}
MODE=${2:-debug}

case $ARCH in
    riscv64)
        TARGET="riscv64gc-unknown-none-elf"
        ;;
    arm64|aarch64)
        TARGET="aarch64-unknown-none"
        ;;
    *)
        echo "Error: Unknown architecture '$ARCH'"
        echo "Usage: $0 [riscv64|arm64] [debug|release]"
        exit 1
        ;;
esac

case $MODE in
    debug)
        RELEASE_FLAG=""
        ;;
    release)
        RELEASE_FLAG="--release"
        ;;
    *)
        echo "Error: Unknown build mode '$MODE'"
        echo "Usage: $0 [riscv64|arm64] [debug|release]"
        exit 1
        ;;
esac

echo "Architecture: $ARCH ($TARGET)"
echo "Build mode: $MODE"
echo ""

# Check if target is installed
if ! rustup target list --installed | grep -q "$TARGET"; then
    echo "Installing target $TARGET..."
    rustup target add $TARGET
fi

# Build HAL
echo "Building HAL..."
cargo build $RELEASE_FLAG --package staros-hal --target $TARGET

# Build kernel
echo "Building kernel..."
cargo build $RELEASE_FLAG --package staros-kernel --target $TARGET

# Build bootloader
echo "Building bootloader..."
cargo build $RELEASE_FLAG --package staros-bootloader --target $TARGET

echo ""
echo "Build completed successfully!"
echo "Artifacts location: target/$TARGET/$MODE/"
