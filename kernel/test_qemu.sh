#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
KERNEL_DIR="$SCRIPT_DIR"
BUILD_DIR="$KERNEL_DIR/build/qemu"

TARGET=aarch64-unknown-none
TIMEOUT=30

print_header() {
    echo ""
    echo "==================================="
    echo "$1"
    echo "==================================="
}

cleanup() {
    if [ ! -z "$QEMU_PID" ]; then
        kill $QEMU_PID 2>/dev/null || true
    fi
    rm -f "$BUILD_DIR/qemu.log"
}

trap cleanup EXIT

mkdir -p "$BUILD_DIR"

print_header "QEMU Test Suite"

echo "[1/6] Building kernel..."
cargo build --release --target aarch64-unknown-none

echo "[2/6] Extracting binary..."
rust-objcopy -O binary \
    target/$TARGET/release/kernel \
    "$BUILD_DIR/kernel.bin"

KERNEL_SIZE=$(stat -f%z "$BUILD_DIR/kernel.bin" 2>/dev/null || stat -c%s "$BUILD_DIR/kernel.bin")
echo "Kernel size: $KERNEL_SIZE bytes"

echo "[3/6] Generating DTB..."
cat > "$BUILD_DIR/virt.dts" << 'EOF'
/dts-v1/;

/ {
    compatible = "linux,dummy-virt";
    #address-cells = <2>;
    #size-cells = <2>;
    interrupt-parent = <&gic>;

    chosen {
        bootargs = "console=ttyAMA0";
    };

    memory@40000000 {
        device_type = "memory";
        reg = <0x0 0x40000000 0x0 0x40000000>;
    };

    cpus {
        #address-cells = <1>;
        #size-cells = <0>;

        cpu@0 {
            device_type = "cpu";
            compatible = "arm,cortex-a57";
            reg = <0>;
        };
    };

    timer {
        compatible = "arm,armv8-timer";
        interrupts = <1 13 0xf08>,
                     <1 14 0xf08>,
                     <1 11 0xf08>,
                     <1 10 0xf08>;
        clock-frequency = <19200000>;
    };

    gic: interrupt-controller@8000000 {
        compatible = "arm,gic-400";
        #interrupt-cells = <3>;
        interrupt-controller;
        reg = <0x0 0x08000000 0x0 0x10000>,
              <0x0 0x08010000 0x0 0x10000>;
    };

    uart@9000000 {
        compatible = "arm,pl011";
        reg = <0x0 0x09000000 0x0 0x1000>;
        interrupts = <0 1 4>;
        clock-frequency = <24000000>;
    };
};
EOF

dtc -O dtb -o "$BUILD_DIR/virt.dtb" "$BUILD_DIR/virt.dts" 2>/dev/null || {
    echo "Warning: dtc not found, using pre-built DTB"
    dd if=/dev/zero of="$BUILD_DIR/virt.dtb" bs=1024 count=4 2>/dev/null
}

echo "[4/6] Starting QEMU..."
qemu-system-aarch64 \
    -machine virt,gic-version=2 \
    -cpu cortex-a57 \
    -smp 1 \
    -m 1G \
    -kernel "$BUILD_DIR/kernel.bin" \
    -dtb "$BUILD_DIR/virt.dtb" \
    -serial stdio \
    -nographic \
    -monitor none \
    -d guest_errors \
    > "$BUILD_DIR/qemu.log" 2>&1 &

QEMU_PID=$!
echo "QEMU PID: $QEMU_PID"

echo "[5/6] Waiting for boot (timeout: ${TIMEOUT}s)..."
START_TIME=$(date +%s)

while kill -0 $QEMU_PID 2>/dev/null; do
    CURRENT_TIME=$(date +%s)
    ELAPSED=$((CURRENT_TIME - START_TIME))
    
    if [ $ELAPSED -ge $TIMEOUT ]; then
        echo "Timeout reached"
        break
    fi
    
    if grep -q "Kernel Ready" "$BUILD_DIR/qemu.log" 2>/dev/null; then
        echo "Boot successful!"
        break
    fi
    
    if grep -q "KERNEL PANIC" "$BUILD_DIR/qemu.log" 2>/dev/null; then
        echo "Kernel panic detected!"
        break
    fi
    
    sleep 1
done

echo "[6/6] Analyzing results..."
echo ""
echo "=== Boot Log ==="
cat "$BUILD_DIR/qemu.log"
echo ""

if grep -q "Kernel Ready" "$BUILD_DIR/qemu.log"; then
    echo "✅ Test PASSED: Kernel booted successfully"
    exit 0
elif grep -q "Early UART initialized" "$BUILD_DIR/qemu.log"; then
    echo "⚠️  Test PARTIAL: Early init OK, but boot incomplete"
    exit 1
else
    echo "❌ Test FAILED: No output detected"
    exit 1
fi
