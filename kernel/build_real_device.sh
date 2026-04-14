#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
KERNEL_DIR="$SCRIPT_DIR"
OUTPUT_DIR="$KERNEL_DIR/build"

SOC_TYPE="${1:-qualcomm}"
BOOT_VERSION="${2:-v3}"

print_usage() {
    echo "Usage: $0 <soc_type> [boot_version]"
    echo ""
    echo "SoC types:"
    echo "  qualcomm  - Qualcomm Snapdragon (default)"
    echo "  mediatek  - MediaTek Dimensity"
    echo "  exynos    - Samsung Exynos"
    echo ""
    echo "Boot versions:"
    echo "  v2 - Android Boot Image v2"
    echo "  v3 - Android Boot Image v3 (default)"
    echo "  v4 - Android Boot Image v4"
    exit 1
}

if [[ "$1" == "-h" ]] || [[ "$1" == "--help" ]]; then
    print_usage
fi

case "$SOC_TYPE" in
    qualcomm|mediatek|exynos)
        ;;
    *)
        echo "Error: Unknown SoC type: $SOC_TYPE"
        print_usage
        ;;
esac

case "$BOOT_VERSION" in
    v2|v3|v4)
        ;;
    *)
        echo "Error: Unknown boot version: $BOOT_VERSION"
        print_usage
        ;;
esac

echo "=== STAR OS Kernel Build ==="
echo "SoC Type: $SOC_TYPE"
echo "Boot Version: $BOOT_VERSION"
echo ""

mkdir -p "$OUTPUT_DIR"

echo "[1/5] Building kernel..."
RUSTFLAGS="-C link-arg=-T$KERNEL_DIR/linker-$SOC_TYPE.ld" \
    cargo build --release --target aarch64-unknown-none

KERNEL_BIN="$OUTPUT_DIR/kernel-$SOC_TYPE.bin"
rust-objcopy -O binary \
    target/aarch64-unknown-none/release/kernel \
    "$KERNEL_BIN"

echo "[2/5] Extracting DTB..."
DTB_FILE="$KERNEL_DIR/../devices/*/$(ls $KERNEL_DIR/../devices/*/*.dtb 2>/dev/null | head -1 | xargs basename)"
if [ ! -f "$DTB_FILE" ]; then
    echo "Warning: No DTB found, using placeholder"
    dd if=/dev/zero of="$OUTPUT_DIR/device.dtb" bs=1024 count=4 2>/dev/null
    DTB_FILE="$OUTPUT_DIR/device.dtb"
fi

echo "[3/5] Creating boot image..."
python3 << EOF
import struct
import sys

BOOT_MAGIC = b'ANDROID!'
PAGE_SIZE = 2048

def align_to_page(data):
    remainder = len(data) % PAGE_SIZE
    if remainder != 0:
        data += b'\x00' * (PAGE_SIZE - remainder)
    return data

with open('$KERNEL_BIN', 'rb') as f:
    kernel = f.read()

with open('$DTB_FILE', 'rb') as f:
    dtb = f.read()

if '$BOOT_VERSION' == 'v2':
    header = struct.pack('<8sIIIIIIIIII16s512s32sIQIIQ',
        BOOT_MAGIC,
        len(kernel), 0x80000000 if '$SOC_TYPE' == 'qualcomm' else 0x40000000,
        0, 0x81000000 if '$SOC_TYPE' == 'qualcomm' else 0x41000000,
        0, 0,
        0x80000100 if '$SOC_TYPE' == 'qualcomm' else 0x40000100,
        PAGE_SIZE, 2, 0,
        b'STAR OS\x00' * 2,
        b'console=ttyMSM0,115200\x00' + b'\x00' * 489,
        b'\x00' * 32,
        0, 0,
        1664,
        len(dtb), 0x82000000 if '$SOC_TYPE' == 'qualcomm' else 0x42000000
    )
elif '$BOOT_VERSION' == 'v3':
    header = struct.pack('<8sIIIIIIIII1536s',
        BOOT_MAGIC,
        len(kernel), 0, 0,
        1580, 0, 0, 0, 0, 3,
        b'console=ttyMSM0,115200\x00' + b'\x00' * 1513
    )
else:  # v4
    header = struct.pack('<8sIIIIIIIII1536sI',
        BOOT_MAGIC,
        len(kernel), 0, 0,
        1584, 0, 0, 0, 0, 4,
        b'console=ttyMSM0,115200\x00' + b'\x00' * 1513,
        0
    )

output = align_to_page(header)
output += align_to_page(kernel)
output += align_to_page(dtb)

with open('$OUTPUT_DIR/boot-$SOC_TYPE-$BOOT_VERSION.img', 'wb') as f:
    f.write(output)

print(f"Boot image size: {len(output)} bytes")
EOF

echo "[4/5] Generating flash script..."
cat > "$OUTPUT_DIR/flash-$SOC_TYPE.sh" << 'FLASH_SCRIPT'
#!/bin/bash
set -e

BOOT_IMG="$(dirname "$0")/boot-SOC_TYPE-BOOT_VERSION.img"

if [ ! -f "$BOOT_IMG" ]; then
    echo "Error: Boot image not found: $BOOT_IMG"
    exit 1
fi

echo "=== STAR OS Kernel Flash ==="
echo "Boot image: $BOOT_IMG"
echo ""
echo "WARNING: This will boot the kernel temporarily (fastboot boot)"
echo "         Your device will NOT be modified permanently"
echo ""
read -p "Continue? (y/N) " -n 1 -r
echo
if [[ ! $REPLY =~ ^[Yy]$ ]]; then
    exit 1
fi

echo "Checking fastboot..."
if ! command -v fastboot &> /dev/null; then
    echo "Error: fastboot not found"
    exit 1
fi

echo "Waiting for device..."
fastboot devices

echo "Booting kernel..."
fastboot boot "$BOOT_IMG"

echo ""
echo "Kernel booted! Check UART output for logs."
FLASH_SCRIPT

sed -i "s/SOC_TYPE/$SOC_TYPE/g" "$OUTPUT_DIR/flash-$SOC_TYPE.sh"
sed -i "s/BOOT_VERSION/$BOOT_VERSION/g" "$OUTPUT_DIR/flash-$SOC_TYPE.sh"
chmod +x "$OUTPUT_DIR/flash-$SOC_TYPE.sh"

echo "[5/5] Build complete!"
echo ""
echo "Output files:"
echo "  Kernel:      $KERNEL_BIN"
echo "  Boot image:  $OUTPUT_DIR/boot-$SOC_TYPE-$BOOT_VERSION.img"
echo "  Flash script: $OUTPUT_DIR/flash-$SOC_TYPE.sh"
echo ""
echo "To flash (temporary boot):"
echo "  cd $OUTPUT_DIR && ./flash-$SOC_TYPE.sh"
echo ""
echo "Or manually:"
echo "  fastboot boot $OUTPUT_DIR/boot-$SOC_TYPE-$BOOT_VERSION.img"
