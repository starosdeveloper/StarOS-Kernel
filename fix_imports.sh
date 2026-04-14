#!/bin/bash
# Auto-replace alloc imports with prelude

set -e

cd "/home/staros-dev/Рабочий стол/STAR OS KERNEL/android/kernel/src"

echo "🔧 Replacing alloc imports with prelude..."

# Find all .rs files with direct alloc imports
COUNT=0
while IFS= read -r file; do
    echo "  Processing: $file"
    
    # Remove all lines starting with "use alloc::"
    sed -i '/^use alloc::/d' "$file"
    
    # Check if prelude is already imported
    if ! grep -q "^use crate::prelude::\*;" "$file"; then
        # Add prelude import after the first use statement
        sed -i '0,/^use /{/^use /a use crate::prelude::*;
}' "$file"
    fi
    
    COUNT=$((COUNT + 1))
done < <(find . -name "*.rs" -type f -exec grep -l "^use alloc::" {} \;)

echo "✅ Processed $COUNT files"
echo ""
echo "🧪 Testing compilation..."
cd "/home/staros-dev/Рабочий стол/STAR OS KERNEL/android/kernel"
cargo build --lib 2>&1 | tail -5

echo ""
echo "✅ Done!"
