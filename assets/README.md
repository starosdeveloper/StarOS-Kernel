# Test Assets - Real Hardware Data

This directory contains real hardware data for comprehensive testing.

## Structure

```
assets/
├── vectors/
│   ├── dtb/           # Device Tree Binaries from real hardware
│   ├── memory/        # Memory dumps for allocator testing
│   └── firmware/      # Firmware blobs (if needed)
└── README.md
```

## DTB Files

Place real `.dtb` files here from:
- QEMU virt machine
- Raspberry Pi 4
- Pine64
- Qualcomm devices (Snapdragon)
- MediaTek devices
- Samsung Exynos devices

### How to obtain DTB files

#### From QEMU:
```bash
qemu-system-aarch64 -machine virt -machine dumpdtb=qemu-virt.dtb
```

#### From Linux device:
```bash
# On running device
cat /sys/firmware/fdt > device.dtb

# Or from boot partition
cp /boot/*.dtb .
```

#### From Android device:
```bash
adb pull /proc/device-tree device.dtb
```

## Usage in Tests

```rust
use common::{DtbTestSuite, Platform};

#[test]
fn test_with_real_dtb() {
    let suite = DtbTestSuite::load_all().unwrap();
    
    for vector in suite.vectors() {
        // Test with real DTB
        assert!(vector.is_valid());
    }
}
```

## Fuzzing

The test infrastructure includes mutation testing:

```rust
use common::fuzzing::DtbMutator;

#[test]
fn fuzz_dtb_parser() {
    let suite = DtbTestSuite::load_all().unwrap();
    let mut mutator = DtbMutator::new(42);
    
    for vector in suite.vectors() {
        for _ in 0..1000 {
            let mutated = mutator.mutate(vector.data());
            // Parser should handle gracefully
            let _ = parse_dtb(&mutated);
        }
    }
}
```

## Adding New Vectors

1. Obtain DTB file from hardware
2. Copy to `assets/vectors/dtb/`
3. Name descriptively: `platform-model.dtb`
4. Tests will automatically pick it up

## CI/CD Integration

All tests run automatically on:
- Every commit
- Pull requests
- Nightly builds

With:
- AddressSanitizer (ASan)
- MemorySanitizer (MSan)
- UndefinedBehaviorSanitizer (UBSan)
