#!/bin/bash
set -e

echo "=== Building StarOS Benchmark (Bare Metal) ==="

cd "$(dirname "$0")"

# Создаем временный Cargo.toml для StarOS версии
cat > Cargo_staros.toml << 'EOF'
[package]
name = "staros_bench"
version = "0.1.0"
edition = "2021"

[dependencies]

[profile.release]
opt-level = 3
lto = true
codegen-units = 1
panic = "abort"

[[bin]]
name = "staros_bench"
path = "staros_bench.rs"
EOF

# Собираем для ARM64 bare-metal
echo "Building for aarch64-unknown-none..."
cargo +nightly build \
    --release \
    --target aarch64-unknown-none \
    --manifest-path Cargo_staros.toml \
    -Z build-std=core \
    -Z build-std-features=compiler-builtins-mem

# Создаем бинарник
rust-objcopy \
    --strip-all \
    -O binary \
    target/aarch64-unknown-none/release/staros_bench \
    staros_bench.bin

echo ""
echo "=== Running in QEMU with KVM ==="
echo "CPU: Host passthrough (single core)"
echo "Mode: Bare metal, no interrupts"
echo ""

# Запускаем в QEMU с KVM и CPU passthrough
qemu-system-aarch64 \
    -machine virt,accel=kvm \
    -cpu host \
    -smp 1 \
    -m 512M \
    -nographic \
    -kernel staros_bench.bin \
    -serial mon:stdio

echo ""
echo "StarOS benchmark complete!"
