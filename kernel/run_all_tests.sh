#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
KERNEL_DIR="$SCRIPT_DIR"

TOTAL_TESTS=0
PASSED_TESTS=0
FAILED_TESTS=0

print_header() {
    echo ""
    echo "========================================"
    echo "$1"
    echo "========================================"
}

run_test() {
    local test_name="$1"
    local test_cmd="$2"
    
    TOTAL_TESTS=$((TOTAL_TESTS + 1))
    echo ""
    echo "[$TOTAL_TESTS] Running: $test_name"
    
    if eval "$test_cmd" > /dev/null 2>&1; then
        echo "    ✅ PASSED"
        PASSED_TESTS=$((PASSED_TESTS + 1))
        return 0
    else
        echo "    ❌ FAILED"
        FAILED_TESTS=$((FAILED_TESTS + 1))
        return 1
    fi
}

print_header "STAR OS Kernel - Automated Test Suite"

echo "Starting tests at $(date)"

# Unit tests
print_header "Unit Tests"
run_test "Cargo test (lib)" "cargo test --lib"
run_test "Cargo test (integration)" "cargo test --test integration_tests"

# Build tests
print_header "Build Tests"
run_test "Build for Qualcomm" "RUSTFLAGS='-C link-arg=-T$KERNEL_DIR/linker-qualcomm.ld' cargo build --release --target aarch64-unknown-none"
run_test "Build for MediaTek" "RUSTFLAGS='-C link-arg=-T$KERNEL_DIR/linker-mediatek.ld' cargo build --release --target aarch64-unknown-none"
run_test "Build for Exynos" "RUSTFLAGS='-C link-arg=-T$KERNEL_DIR/linker-exynos.ld' cargo build --release --target aarch64-unknown-none"
run_test "Build for RPi4" "RUSTFLAGS='-C link-arg=-T$KERNEL_DIR/linker-rpi4.ld' cargo build --release --target aarch64-unknown-none"

# Boot image tests
print_header "Boot Image Tests"
run_test "Build boot image v2" "$KERNEL_DIR/build_real_device.sh qualcomm v2"
run_test "Build boot image v3" "$KERNEL_DIR/build_real_device.sh qualcomm v3"
run_test "Build boot image v4" "$KERNEL_DIR/build_real_device.sh qualcomm v4"

# Code quality
print_header "Code Quality"
run_test "Cargo check" "cargo check --lib"
run_test "Cargo clippy" "cargo clippy --lib -- -D warnings" || true

# Documentation
print_header "Documentation"
run_test "Cargo doc" "cargo doc --no-deps"

# Summary
print_header "Test Summary"
echo ""
echo "Total tests:  $TOTAL_TESTS"
echo "Passed:       $PASSED_TESTS ✅"
echo "Failed:       $FAILED_TESTS ❌"
echo ""

if [ $FAILED_TESTS -eq 0 ]; then
    echo "🎉 All tests passed!"
    exit 0
else
    echo "⚠️  Some tests failed"
    exit 1
fi
