#!/bin/bash
# Sign boot image with post-quantum signature

set -e

if [ "$#" -ne 2 ]; then
    echo "Usage: $0 <input.img> <output-signed.img>"
    exit 1
fi

INPUT="$1"
OUTPUT="$2"

echo "🔐 Signing boot image with Dilithium3..."

# Build signing tool
cargo build --release --bin pq-sign-tool

# Sign
./target/release/pq-sign-tool sign "$INPUT" "$OUTPUT"

echo "✅ Signed: $OUTPUT"
echo "📊 Size increase: $(( $(stat -f%z "$OUTPUT") - $(stat -f%z "$INPUT") )) bytes"
