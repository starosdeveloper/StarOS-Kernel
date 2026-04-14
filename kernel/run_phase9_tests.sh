#!/bin/bash
# Phase 9: Integration & Testing Runner
# Runs all integration tests, QEMU tests, and benchmarks

set -e

KERNEL_DIR="/home/staros-dev/Рабочий стол/STAR OS KERNEL/android/kernel"
cd "$KERNEL_DIR"

echo "🧪 Phase 9: Integration & Testing"
echo "=================================="
echo ""

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

# Test counters
TOTAL=0
PASSED=0
FAILED=0

run_test() {
    local name=$1
    local cmd=$2
    
    TOTAL=$((TOTAL + 1))
    echo -n "Running $name... "
    
    if eval "$cmd" > /tmp/test_$TOTAL.log 2>&1; then
        echo -e "${GREEN}✓ PASSED${NC}"
        PASSED=$((PASSED + 1))
    else
        echo -e "${RED}✗ FAILED${NC}"
        cat /tmp/test_$TOTAL.log
        FAILED=$((FAILED + 1))
    fi
}

echo "📦 1. Unit Tests"
echo "----------------"
run_test "Device Tree tests" "cargo test --lib of::"
run_test "Platform bus tests" "cargo test --lib base::"
run_test "Resource tests" "cargo test --lib resource::"
run_test "I2C tests" "cargo test --lib i2c::"
run_test "SPI tests" "cargo test --lib spi::"
run_test "Power management tests" "cargo test --lib power::"
run_test "DMA tests" "cargo test --lib dma::"
run_test "Clock tests" "cargo test --lib clk::"
echo ""

echo "🔗 2. Integration Tests"
echo "----------------------"
run_test "DT to Device flow" "cargo test --test integration_phase9 test_dt_to_device_flow"
run_test "Device-Driver binding" "cargo test --test integration_phase9 test_device_driver_binding"
run_test "I2C subsystem" "cargo test --test integration_phase9 test_i2c_subsystem"
run_test "SPI subsystem" "cargo test --test integration_phase9 test_spi_subsystem"
run_test "Power management" "cargo test --test integration_phase9 test_power_management"
run_test "DMA engine" "cargo test --test integration_phase9 test_dma_engine"
run_test "Clock framework" "cargo test --test integration_phase9 test_clock_framework"
run_test "Hot-plug" "cargo test --test integration_phase9 test_hotplug"
echo ""

echo "🖥️  3. QEMU Tests"
echo "----------------"
run_test "QEMU boot" "./test_qemu.sh"
run_test "QEMU UART" "cargo test --test qemu_integration test_qemu_uart"
run_test "QEMU VirtIO" "cargo test --test qemu_integration test_qemu_virtio"
echo ""

echo "📊 4. Performance Benchmarks"
echo "---------------------------"
run_test "DT parse benchmark" "cargo test --test benchmarks bench_dt_parse"
run_test "Device lookup benchmark" "cargo test --test benchmarks bench_device_lookup"
run_test "I2C transfer benchmark" "cargo test --test benchmarks bench_i2c_transfer"
run_test "SPI transfer benchmark" "cargo test --test benchmarks bench_spi_transfer"
run_test "DMA memcpy benchmark" "cargo test --test benchmarks bench_dma_memcpy"
run_test "PM runtime benchmark" "cargo test --test benchmarks bench_pm_runtime"
run_test "Performance targets" "cargo test --test benchmarks test_performance_targets"
echo ""

echo "🔍 5. Real Device Tests (Optional)"
echo "----------------------------------"
if [ -f "/dev/ttyUSB0" ]; then
    echo -e "${YELLOW}Real device detected, running tests...${NC}"
    run_test "Real device boot" "cargo test --test e2e_real_device test_real_device_boot"
else
    echo -e "${YELLOW}No real device detected, skipping${NC}"
fi
echo ""

echo "📈 6. Code Coverage"
echo "------------------"
echo "Generating coverage report..."
cargo tarpaulin --out Html --output-dir coverage > /dev/null 2>&1 || true
if [ -f "coverage/index.html" ]; then
    COVERAGE=$(grep -oP 'Coverage: \K[0-9.]+' coverage/index.html | head -1)
    echo -e "Coverage: ${GREEN}${COVERAGE}%${NC}"
else
    echo -e "${YELLOW}Coverage report not available${NC}"
fi
echo ""

echo "=================================="
echo "📊 Test Results Summary"
echo "=================================="
echo "Total tests:  $TOTAL"
echo -e "Passed:       ${GREEN}$PASSED${NC}"
echo -e "Failed:       ${RED}$FAILED${NC}"
echo ""

if [ $FAILED -eq 0 ]; then
    echo -e "${GREEN}✅ All tests PASSED!${NC}"
    echo ""
    echo "Phase 9 Complete! ✨"
    echo ""
    echo "Next steps:"
    echo "  - Review test coverage"
    echo "  - Check performance benchmarks"
    echo "  - Test on real device (if available)"
    echo "  - Update roadmap: Phase 9 ✅"
    exit 0
else
    echo -e "${RED}❌ Some tests FAILED${NC}"
    echo ""
    echo "Please fix failing tests before proceeding."
    exit 1
fi
