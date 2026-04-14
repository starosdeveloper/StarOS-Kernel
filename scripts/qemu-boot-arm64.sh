#!/bin/bash
# Quick QEMU boot test for ARM64

set -e

echo "🚀 StarOS QEMU Boot Test (ARM64)"
echo "================================"

# Build bootloader
echo "📦 Building bootloader..."
cargo build --release \
    --package staros-bootloader \
    --target aarch64-unknown-none \
    --features arm64

BOOTLOADER="target/aarch64-unknown-none/release/staros-bootloader"

if [ ! -f "$BOOTLOADER" ]; then
    echo "❌ Build failed!"
    exit 1
fi

echo "✅ Build complete: $(ls -lh $BOOTLOADER | awk '{print $5}')"

echo ""
echo "🎮 Launching QEMU..."
echo "Press Ctrl-A then X to exit"
echo ""
sleep 1

# Launch QEMU
qemu-system-aarch64 \
    -machine virt \
    -cpu cortex-a57 \
    -m 512M \
    -nographic \
    -serial mon:stdio \
    -kernel "$BOOTLOADER" \
    -bios none

echo ""
echo "✅ QEMU session ended"
