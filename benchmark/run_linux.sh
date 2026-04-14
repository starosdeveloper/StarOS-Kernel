#!/bin/bash
set -e

echo "=== Building Linux Benchmark (Docker) ==="

cd "$(dirname "$0")"

# Собираем Docker образ
docker build -t staros-bench-linux .

echo ""
echo "=== Running Benchmark with CPU Pinning ==="
echo "Pinning to CPU core 0"
echo "Setting maximum CPU priority"
echo ""

# Запускаем с CPU pinning и максимальным приоритетом
docker run --rm \
    --cpuset-cpus="0" \
    --cpu-shares=1024 \
    --cpu-quota=100000 \
    staros-bench-linux

echo ""
echo "Linux benchmark complete!"
