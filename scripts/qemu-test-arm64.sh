#!/bin/bash
# QEMU test runner for ARM64 architecture with detailed boot logging

set -e

echo "========================================"
echo "  ARM64 BOOT SEQUENCE - DETAILED MODE"
echo "========================================"
echo ""

# Configuration
QEMU_BIN="qemu-system-aarch64"
MACHINE="virt"
CPU="cortex-a57"
MEMORY="512M"
KERNEL="artifacts/aarch64/staros-bootloader"
TIMEOUT=30
LOG_FILE="boot-arm64-$(date +%Y%m%d-%H%M%S).log"

echo "[$(date +%H:%M:%S.%3N)] Pre-flight checks..."

# Check if QEMU is installed
if ! command -v $QEMU_BIN &> /dev/null; then
    echo "❌ Error: $QEMU_BIN not found. Please install QEMU."
    exit 1
fi
echo "✓ QEMU binary found: $(which $QEMU_BIN)"
echo "  Version: $($QEMU_BIN --version | head -n1)"

# Check if kernel artifact exists
if [ ! -f "$KERNEL" ]; then
    echo "❌ Error: Kernel artifact not found at $KERNEL"
    echo "  Run: make build-arm64"
    exit 1
fi
echo "✓ Kernel artifact found: $KERNEL"
echo "  Size: $(du -h $KERNEL | cut -f1)"
echo "  Modified: $(stat -c %y $KERNEL | cut -d. -f1)"

# Display kernel sections
echo ""
echo "[$(date +%H:%M:%S.%3N)] Kernel binary analysis..."
if command -v rust-objdump &> /dev/null || command -v llvm-objdump &> /dev/null; then
    OBJDUMP=$(command -v rust-objdump || command -v llvm-objdump)
    echo "  Sections:"
    $OBJDUMP -h $KERNEL | grep -E "^\s+[0-9]+" | awk '{printf "    %-15s Size: %-10s VMA: %s\n", $2, $3, $4}'
else
    echo "  (objdump not available for section analysis)"
fi

echo ""
echo "[$(date +%H:%M:%S.%3N)] QEMU Configuration:"
echo "  Machine:     $MACHINE"
echo "  CPU:         $CPU (ARMv8-A)"
echo "  Memory:      $MEMORY"
echo "  Cores:       1"
echo "  Features:    NEON, Crypto, FP"
echo "  Serial:      mon:stdio (console output)"
echo "  Graphics:    none (-nographic)"
echo "  Timeout:     ${TIMEOUT}s"
echo "  Log file:    $LOG_FILE"

echo ""
echo "========================================"
echo "  STARTING BOOT SEQUENCE"
echo "========================================"
echo "[$(date +%H:%M:%S.%3N)] Launching QEMU..."
echo ""

# Run QEMU with detailed logging
timeout $TIMEOUT $QEMU_BIN \
    -machine $MACHINE \
    -cpu $CPU \
    -m $MEMORY \
    -nographic \
    -serial mon:stdio \
    -kernel $KERNEL \
    -d guest_errors,unimp \
    -D qemu-debug-arm64.log \
    -trace enable=arm_gt_tval_write \
    -trace enable=arm_gt_cval_write \
    2>&1 | tee $LOG_FILE || true

EXIT_CODE=$?

echo ""
echo "========================================"
echo "  BOOT SEQUENCE COMPLETED"
echo "========================================"
echo "[$(date +%H:%M:%S.%3N)] Exit code: $EXIT_CODE"
echo ""
echo "📊 Boot logs saved to: $LOG_FILE"
echo "🔍 QEMU debug log: qemu-debug-arm64.log"
echo ""

if [ $EXIT_CODE -eq 124 ]; then
    echo "⏱️  Boot timed out after ${TIMEOUT}s (expected for continuous run)"
elif [ $EXIT_CODE -eq 0 ]; then
    echo "✓ Boot completed successfully"
else
    echo "⚠️  Boot exited with code $EXIT_CODE"
fi

echo ""
echo "To analyze boot time, check timestamps in $LOG_FILE"
exit 0
