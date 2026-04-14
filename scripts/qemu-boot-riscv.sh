#!/bin/bash
# Quick QEMU boot test for RISC-V

set -e

echo "🚀 StarOS QEMU Boot Test (RISC-V)"
echo "================================"

BOOTLOADER="target/riscv64gc-unknown-none-elf/release/staros-bootloader"

# Check if bootloader exists
if [ ! -f "$BOOTLOADER" ]; then
    echo "📦 Building bootloader..."
    cargo build --release \
        --package staros-bootloader \
        --target riscv64gc-unknown-none-elf \
        --features riscv
    echo "✅ Build complete: $(ls -lh $BOOTLOADER | awk '{print $5}')"
else
    echo "✅ Using existing bootloader: $(ls -lh $BOOTLOADER | awk '{print $5}')"
    echo "   (run 'cargo clean' to rebuild)"
fi

# Create minimal device tree for QEMU virt (only if dtc available)
if command -v dtc &> /dev/null && [ ! -f /tmp/staros-riscv.dtb ]; then
    echo "📝 Creating device tree..."
    cat > /tmp/staros-riscv.dts << 'EOF'
/dts-v1/;

/ {
    #address-cells = <2>;
    #size-cells = <2>;
    compatible = "riscv-virtio";
    model = "riscv-virtio,qemu";

    chosen {
        bootargs = "";
        stdout-path = "/uart@10000000";
    };

    memory@80000000 {
        device_type = "memory";
        reg = <0x0 0x80000000 0x0 0x20000000>; // 512MB
    };

    cpus {
        #address-cells = <1>;
        #size-cells = <0>;
        timebase-frequency = <10000000>; // 10MHz

        cpu@0 {
            device_type = "cpu";
            reg = <0>;
            compatible = "riscv";
        };
    };

    uart@10000000 {
        compatible = "ns16550a";
        reg = <0x0 0x10000000 0x0 0x100>;
        clock-frequency = <3686400>;
    };

    plic@c000000 {
        compatible = "riscv,plic0";
        interrupt-controller;
        #interrupt-cells = <1>;
        reg = <0x0 0x0c000000 0x0 0x4000000>;
    };
};
EOF

    dtc -O dtb -o /tmp/staros-riscv.dtb /tmp/staros-riscv.dts 2>/dev/null
fi

echo "🎮 Launching QEMU... (Ctrl-A then X to exit)"

# Launch QEMU with timeout and fast boot options
timeout 5s qemu-system-riscv64 \
    -machine virt \
    -cpu rv64 \
    -m 512M \
    -nographic \
    -serial mon:stdio \
    -kernel "$BOOTLOADER" \
    -bios none \
    -no-reboot \
    -d guest_errors \
    ${1:+-dtb /tmp/staros-riscv.dtb} || {
    EXIT_CODE=$?
    if [ $EXIT_CODE -eq 124 ]; then
        echo ""
        echo "⚠️  QEMU timeout (5s) - kernel may be hanging"
        exit 1
    fi
}

echo ""
echo "✅ QEMU session ended"
