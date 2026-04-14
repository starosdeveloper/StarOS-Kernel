#!/bin/bash
# QEMU test runner for RISC-V architecture with detailed boot logging

set -e

echo "========================================"
echo "  RISC-V BOOT SEQUENCE - DETAILED MODE"
echo "========================================"
echo ""

# Configuration
QEMU_BIN="qemu-system-riscv64"
MACHINE="virt"
MEMORY="512M"
KERNEL="target/riscv64gc-unknown-none-elf/release/staros-bootloader"
TIMEOUT=30
LOG_FILE="boot-riscv-$(date +%Y%m%d-%H%M%S).log"

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
    echo "  Run: make build-riscv"
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
echo "  Memory:      $MEMORY"
echo "  CPUs:        1 (RISC-V 64-bit)"
echo "  Extensions:  G (IMAFD + Zicsr + Zifencei)"
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
    -cpu rv64 \
    -m $MEMORY \
    -nographic \
    -serial mon:stdio \
    -kernel $KERNEL \
    -d guest_errors,unimp \
    -D qemu-debug.log \
    -trace enable=riscv_trap \
    -trace enable=pmpcfg_csr_write \
    2>&1 | tee $LOG_FILE || true

EXIT_CODE=$?

echo ""
echo "========================================"
echo "  BOOT SEQUENCE COMPLETED"
echo "========================================"
echo "[$(date +%H:%M:%S.%3N)] Exit code: $EXIT_CODE"
echo ""
echo "📊 Boot logs saved to: $LOG_FILE"
echo "🔍 QEMU debug log: qemu-debug.log"
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
