#!/bin/bash
# Cleanup script - removes all legacy and unused code

set -e

echo "🧹 Cleaning up legacy code..."

# Remove demo modules
rm -f kernel/src/demo.rs
rm -f kernel/src/demo_phase*.rs
rm -f kernel/src/demo_week*.rs
rm -f kernel/src/init_v04.rs
rm -f kernel/src/init_dtb.rs

# Remove old version modules
rm -f kernel/src/*_v04.rs
rm -f kernel/src/*_v05.rs
rm -f kernel/src/*_v06.rs
rm -f kernel/src/*_prod.rs

# Remove unused subsystems
rm -f kernel/src/ebpf.rs
rm -f kernel/src/logging.rs
rm -f kernel/src/performance.rs
rm -f kernel/src/power.rs
rm -f kernel/src/recovery.rs
rm -f kernel/src/security.rs
rm -f kernel/src/security_prod.rs
rm -f kernel/src/security_audit.rs
rm -f kernel/src/perf_tuning.rs
rm -f kernel/src/battery_opt.rs
rm -f kernel/src/panic.rs
rm -f kernel/src/task.rs
rm -f kernel/src/sync.rs
rm -f kernel/src/vfs.rs
rm -f kernel/src/diagnostics.rs
rm -f kernel/src/time.rs

# Remove UI modules (not needed in kernel)
rm -f kernel/src/gpu.rs
rm -f kernel/src/compositor.rs
rm -f kernel/src/window_manager.rs
rm -f kernel/src/gestures.rs

# Remove network stack (userspace)
rm -f kernel/src/network.rs

# Remove Android compat layer (userspace)
rm -f kernel/src/android.rs

# Remove app ecosystem (userspace)
rm -f kernel/src/pkgmgr.rs
rm -f kernel/src/appstore.rs
rm -f kernel/src/permissions.rs
rm -f kernel/src/sandbox.rs

# Remove telephony (userspace)
rm -f kernel/src/telephony.rs

# Remove old device support
rm -f kernel/src/devices.rs
rm -f kernel/src/hal.rs

# Remove old subsystem directories
rm -rf kernel/src/ebpf/
rm -rf kernel/src/gpu/
rm -rf kernel/src/network/
rm -rf kernel/src/android/
rm -rf kernel/src/task/
rm -rf kernel/src/sync/
rm -rf kernel/src/scheduler/
rm -rf kernel/src/memory/
rm -rf kernel/src/interrupts/
rm -rf kernel/src/ipc/
rm -rf kernel/src/ipc_v05/
rm -rf kernel/src/net_v05/
rm -rf kernel/src/fs_v05/
rm -rf kernel/src/security_v05/
rm -rf kernel/src/bluetooth/
rm -rf kernel/src/camera/
rm -rf kernel/src/telephony/
rm -rf kernel/src/sensors/
rm -rf kernel/src/usb/
rm -rf kernel/src/power/
rm -rf kernel/src/recovery/
rm -rf kernel/src/security/
rm -rf kernel/src/logging/
rm -rf kernel/src/performance/
rm -rf kernel/src/perf/
rm -rf kernel/src/crypto/
rm -rf kernel/src/net/
rm -rf kernel/src/ui/
rm -rf kernel/src/apps_v04/
rm -rf kernel/src/power_v04/
rm -rf kernel/src/devices/
rm -rf kernel/src/hal/

# Remove old benchmarks
rm -rf kernel/benches/

# Remove old tests
rm -f kernel/tests/*_v*.rs
rm -f kernel/tests/*_integration.rs
rm -f kernel/tests/benchmarks.rs

# Remove old docs
rm -rf docs/v0.*.md
rm -rf docs/archive/
rm -f docs/PHASE*.md
rm -f docs/*_COMPLETE.md
rm -f docs/*_PROGRESS.md
rm -f docs/*_SUMMARY.md
rm -f docs/XIAOMI_*.md
rm -f docs/DEVICE_*.md
rm -f docs/HARDWARE_*.md
rm -f docs/PRODUCTION_*.md
rm -f docs/ALPHA_*.md
rm -f docs/BETA_*.md
rm -f docs/SAFETY_*.md
rm -f docs/BENCHMARK_*.md
rm -f docs/PERFORMANCE_*.md
rm -f docs/TESTING_*.md
rm -f docs/QUICK_START*.md
rm -f docs/RELEASE_*.md
rm -f docs/BUILD.md
rm -f docs/PROJECT_*.md
rm -f docs/CODE_OF_CONDUCT.md
rm -f docs/CONTRIBUTING.md
rm -f docs/LICENSING.md
rm -f docs/BUSINESS-MODEL.md
rm -f docs/PRICING.md
rm -f docs/INSTALL_RUST.md
rm -f docs/LINUX_*.md
rm -f docs/MIGRATION_SUCCESS.md
rm -f docs/NEXT_STEPS_QEMU.md
rm -f docs/BOOTLOADER_*.md
rm -f docs/FINAL_*.md
rm -f docs/TODAY_*.md
rm -f docs/READY_*.md
rm -f docs/CLEANUP_*.md
rm -f docs/INDEX.md
rm -f docs/CHANGELOG.md
rm -f docs/PROGRESS_REPORT.md
rm -f docs/README*.md
rm -f docs/ЖЕЛЕЗОБЕТОННЫЕ_ДОКАЗАТЕЛЬСТВА.txt
rm -f docs/README_DEVICE_SUPPORT.txt

# Remove xiaomi-specific kernel
rm -rf xiaomi-kernel/

# Remove xiaomi-bootloader
rm -rf xiaomi-bootloader/

# Remove bootloader-minimal
rm -rf bootloader-minimal/

# Remove old releases
rm -rf release/
rm -rf releases/

# Remove old artifacts
rm -rf artifacts/

# Remove old scripts
rm -f scripts/test-phase*.sh
rm -f scripts/validate-*.sh
rm -f scripts/build-v*.sh
rm -f scripts/commit-*.sh
rm -f scripts/extract-binaries.sh
rm -f scripts/flash-sd.sh
rm -f scripts/hardware-status.sh
rm -f scripts/micro-bench.sh
rm -f scripts/quick-check.sh
rm -f scripts/remove-mocks.sh
rm -f scripts/run-benchmarks.sh
rm -f scripts/setup-linux.sh

# Remove old services
rm -rf services/

# Remove old HAL
rm -rf hal/

# Remove old bootloader
rm -rf bootloader/

# Remove old benches
rm -rf benches/

# Remove old tests
rm -rf tests/

# Remove old root-level files
rm -f *.txt
rm -f *.csv
rm -f *.log
rm -f *.rs
rm -f *.zip
rm -f bootloader_proof_bench
rm -f Makefile
rm -f COMMIT_MSG.txt
rm -f V1.0_BANNER.txt
rm -f LICENSE-COMMERCIAL

echo "✅ Cleanup complete!"
echo ""
echo "Remaining structure:"
echo "  kernel/src/          - Core kernel code"
echo "  kernel/src/drivers/  - Device drivers"
echo "  devices/             - Device Tree files"
echo "  scripts/             - Essential build scripts"
echo "  docs/                - Current documentation"
