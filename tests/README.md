# Global Test Infrastructure

Repository-wide testing infrastructure for STAR OS Kernel.

## Architecture

### Dual-Target Strategy
- **Target**: `aarch64-unknown-none` - Real hardware
- **Host**: Native architecture - Testing with sanitizers

### Data-Driven Testing
All tests use real hardware data from `assets/vectors/`:
- Real DTB files from QEMU, RPi4, Qualcomm, MediaTek, Exynos
- Memory dumps for allocator testing
- Firmware blobs when needed

### Test Categories

#### 1. Integration Tests (`tests/*.rs`)
- `dtb_integration.rs` - DTB parsing with real files
- `bus_integration.rs` - Bus infrastructure lifecycle
- `memory_integration.rs` - Allocator testing

#### 2. Common Infrastructure (`tests/common/`)
- DTB loading and validation
- Fuzzing utilities (mutation testing)
- Hardware mocking for host testing
- Memory dump utilities

## Usage

### Running All Tests
```bash
# Standard tests
cargo test

# With sanitizers (recommended)
RUSTFLAGS="-Z sanitizer=address" cargo +nightly test --target x86_64-unknown-linux-gnu

# Fuzzing tests (1M iterations)
cargo test --test dtb_integration -- --ignored
```

### Adding New Tests

```rust
use common::{DtbTestSuite, Platform};

#[test]
fn test_my_feature() {
    let suite = DtbTestSuite::load_all().unwrap();
    
    for vector in suite.vectors() {
        // Test with real DTB
        my_parser(vector.data());
    }
}
```

### Fuzzing

```rust
use common::fuzzing::DtbMutator;

#[test]
fn fuzz_my_parser() {
    let suite = DtbTestSuite::load_all().unwrap();
    let mut mutator = DtbMutator::new(42);
    
    for vector in suite.vectors() {
        for _ in 0..1000 {
            let mutated = mutator.mutate(vector.data());
            // Should never panic
            let _ = my_parser(&mutated);
        }
    }
}
```

## Coverage Requirements

### DTB Parsing
- ✅ Load all real DTB files
- ✅ Validate FDT header
- ✅ Parse device tree structure
- ✅ Extract properties
- ✅ Handle malformed data gracefully

### Bus Infrastructure
- ✅ Full lifecycle: register → enumerate → match → probe → remove
- ✅ Address translation through ranges
- ✅ IRQ mapping
- ✅ Platform device creation

### Memory Management
- ✅ Allocation/deallocation
- ✅ No leaks (verified with ASan)
- ✅ No use-after-free
- ✅ No buffer overflows

## CI/CD Integration

### GitHub Actions
```yaml
- name: Run tests with sanitizers
  run: |
    RUSTFLAGS="-Z sanitizer=address" cargo +nightly test
    RUSTFLAGS="-Z sanitizer=memory" cargo +nightly test
```

### Pre-commit Hook
```bash
#!/bin/bash
cargo test --all
cargo clippy -- -D warnings
```

## Sanitizers

### AddressSanitizer (ASan)
Detects:
- Use-after-free
- Heap buffer overflow
- Stack buffer overflow
- Memory leaks

```bash
RUSTFLAGS="-Z sanitizer=address" cargo +nightly test
```

### MemorySanitizer (MSan)
Detects:
- Uninitialized memory reads

```bash
RUSTFLAGS="-Z sanitizer=memory" cargo +nightly test
```

### UndefinedBehaviorSanitizer (UBSan)
Detects:
- Integer overflow
- Null pointer dereference
- Misaligned pointer access

```bash
RUSTFLAGS="-Z sanitizer=undefined" cargo +nightly test
```

## Adding Test Vectors

1. Obtain DTB from hardware:
   ```bash
   # QEMU
   qemu-system-aarch64 -machine virt -machine dumpdtb=qemu-virt.dtb
   
   # Real device
   cat /sys/firmware/fdt > device.dtb
   ```

2. Copy to assets:
   ```bash
   cp device.dtb assets/vectors/dtb/platform-model.dtb
   ```

3. Tests automatically pick it up!

## Regression Testing

Every commit runs full regression suite:
- All DTB files must parse successfully
- No panics during fuzzing
- No memory leaks
- No undefined behavior

## Performance Benchmarks

```bash
cargo bench --bench dtb_parsing
cargo bench --bench bus_operations
```

## Documentation

- `assets/README.md` - Test data documentation
- `tests/common/mod.rs` - API documentation
- Individual test files - Usage examples
