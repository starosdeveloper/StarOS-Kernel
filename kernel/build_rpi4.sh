#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
KERNEL_DIR="$SCRIPT_DIR"
OUTPUT_DIR="$KERNEL_DIR/build/rpi4"

TARGET=aarch64-unknown-none

print_header() {
    echo ""
    echo "==================================="
    echo "$1"
    echo "==================================="
}

mkdir -p "$OUTPUT_DIR"

print_header "Raspberry Pi 4 Build"

echo "[1/4] Building kernel..."
RUSTFLAGS="-C link-arg=-T$KERNEL_DIR/linker-rpi4.ld" \
    cargo build --release --target $TARGET

echo "[2/4] Extracting binary..."
rust-objcopy -O binary \
    target/$TARGET/release/kernel \
    "$OUTPUT_DIR/kernel8.img"

KERNEL_SIZE=$(stat -f%z "$OUTPUT_DIR/kernel8.img" 2>/dev/null || stat -c%s "$OUTPUT_DIR/kernel8.img")
echo "Kernel size: $KERNEL_SIZE bytes"

echo "[3/4] Creating config.txt..."
cat > "$OUTPUT_DIR/config.txt" << 'EOF'
# Raspberry Pi 4 Configuration for STAR OS

# ARM 64-bit mode
arm_64bit=1

# Kernel
kernel=kernel8.img

# UART
enable_uart=1
uart_2ndstage=1

# Memory
gpu_mem=64

# Disable rainbow splash
disable_splash=1

# Boot delay
boot_delay=0
EOF

echo "[4/4] Creating installation instructions..."
cat > "$OUTPUT_DIR/INSTALL.txt" << 'EOF'
Raspberry Pi 4 Installation Instructions
=========================================

1. Format SD card as FAT32

2. Copy these files to SD card:
   - kernel8.img (STAR OS kernel)
   - config.txt (boot configuration)
   - bootcode.bin (from Raspberry Pi firmware)
   - start4.elf (from Raspberry Pi firmware)
   - fixup4.dat (from Raspberry Pi firmware)

3. Download Raspberry Pi firmware files:
   https://github.com/raspberrypi/firmware/tree/master/boot

4. Insert SD card and power on

5. Connect UART:
   - GPIO 14 (TXD) - Pin 8
   - GPIO 15 (RXD) - Pin 10
   - GND - Pin 6

6. Serial settings:
   - Baud: 115200
   - Data: 8 bits
   - Parity: None
   - Stop: 1 bit

Expected output:
   === STAR OS Kernel Boot ===
   Stage 1: Early initialization
   Early UART initialized at 0xFE201000
   ...

Recovery:
   If boot fails, simply replace kernel8.img with original
   Raspberry Pi kernel to restore normal operation.
EOF

print_header "Build Complete"
echo ""
echo "Output files:"
echo "  Kernel:  $OUTPUT_DIR/kernel8.img"
echo "  Config:  $OUTPUT_DIR/config.txt"
echo "  Install: $OUTPUT_DIR/INSTALL.txt"
echo ""
echo "Next steps:"
echo "  1. Read $OUTPUT_DIR/INSTALL.txt"
echo "  2. Prepare SD card"
echo "  3. Copy files"
echo "  4. Boot RPi4"
