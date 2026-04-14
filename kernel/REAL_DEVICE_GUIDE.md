# STAR OS Kernel - Real Device Deployment Guide

## ⚠️ SAFETY FIRST ⚠️

This guide ensures safe testing on real ARM64 devices without bricking.

## Prerequisites

### Required Tools
```bash
# Install build tools
sudo apt-get install -y \
    mkbootimg \
    android-tools-fastboot \
    qemu-system-aarch64

# Install Rust target
rustup target add aarch64-unknown-none
```

### Test Device Requirements
- ✅ Unlocked bootloader
- ✅ Fastboot access
- ✅ Cheap/expendable device (< $50)
- ✅ Known recovery method (EDL mode for Qualcomm)
- ❌ NOT your main phone!

## Build Process

### 1. Build for Specific SoC
```bash
cd kernel

# For Qualcomm (Snapdragon)
./build_real_device.sh qualcomm

# For MediaTek (Dimensity)
./build_real_device.sh mediatek

# For Samsung (Exynos)
./build_real_device.sh exynos
```

This creates:
- `kernel.bin` - Raw kernel binary
- `boot.img` - Android boot image (if mkbootimg available)

### 2. Test in QEMU First
```bash
./test_qemu.sh
```

**CRITICAL:** Always test in QEMU before real device!

## Device Deployment

### Safe Boot (Recommended)
```bash
# Boot temporarily WITHOUT writing to flash
fastboot boot boot.img
```

This is **SAFE** because:
- ✅ Loads kernel into RAM only
- ✅ No permanent changes
- ✅ Reboot returns to normal
- ✅ Can't brick device

### What You Should See

#### Success
```
Early debug: Qualcomm UART
DTB parsed
PMIC initialized
Clocks configured
Memory initialized
Kernel running!
```

#### Failure (Safe)
```
Early debug: Qualcomm UART
*** KERNEL PANIC ***
Triggering watchdog reset...
```

Device will automatically reboot to bootloader.

## Safety Features

### 1. Watchdog Timer
- Automatically resets device after 5 seconds if kernel hangs
- Cannot be disabled in production builds

### 2. Memory Protection
- Forbidden regions:
  - `0x00000000-0x00100000` - Bootloader
  - `0x0C000000-0x0D000000` - PMIC
  - `0x86000000-0x89000000` - TrustZone
- Writes to these regions are blocked

### 3. Panic Recovery
- All panics trigger watchdog reset
- Panic info printed to UART
- Device returns to bootloader

### 4. Boot Validation
- DTB magic checked
- Memory map validated
- UART tested before use

## Recovery Procedures

### Level 1: Watchdog Reset
- **Trigger:** Kernel panic or hang
- **Action:** Automatic reset after 5 seconds
- **Result:** Back to bootloader

### Level 2: Manual Reboot
- **Trigger:** Watchdog fails
- **Action:** Hold power button 10 seconds
- **Result:** Hard reset

### Level 3: Fastboot
- **Trigger:** Boot loop
- **Action:** Boot into fastboot mode (Vol Down + Power)
- **Result:** Can boot stock ROM

### Level 4: EDL Mode (Qualcomm Only)
- **Trigger:** Bootloader damaged
- **Action:** Short test points or use EDL cable
- **Tools:** QFIL, QPST
- **Result:** Full device recovery

## Recommended Test Devices

### Qualcomm
- **Xiaomi Redmi Note 7** (SD660) - ~$40
- **Poco F1** (SD845) - ~$50
- Easy EDL recovery

### MediaTek
- **Redmi Note 8** (Helio G90T) - ~$45
- SP Flash Tool recovery

### Exynos
- **Galaxy S9** (Exynos 9810) - ~$50
- Odin recovery

## Debug Output

### UART Connection
Most phones have UART test points on the PCB:
- TX, RX, GND
- 115200 baud, 8N1
- USB-TTL adapter required

### Without UART
- Watch for device behavior:
  - Quick reboot = panic + watchdog
  - Hang = watchdog timeout
  - Boot to bootloader = success

## Pre-Flight Checklist

Before first boot on real device:

- [ ] Tested in QEMU successfully
- [ ] Using cheap test device
- [ ] Bootloader unlocked
- [ ] Fastboot working
- [ ] Recovery method known
- [ ] Using `fastboot boot` (NOT flash)
- [ ] Watchdog enabled in code
- [ ] Memory protection enabled
- [ ] Panic handler tested
- [ ] Backup of stock boot.img

## What NOT To Do

### ❌ NEVER:
- Use `fastboot flash boot` for testing
- Test on expensive device first
- Disable watchdog
- Skip QEMU testing
- Write to bootloader partition
- Test without recovery plan

### ✅ ALWAYS:
- Use `fastboot boot` for testing
- Test on cheap device
- Enable watchdog
- Test in QEMU first
- Validate memory regions
- Have recovery tools ready

## Troubleshooting

### Device Won't Boot
```bash
# Boot into fastboot
# Vol Down + Power during boot

# Boot stock ROM temporarily
fastboot boot stock_boot.img
```

### Kernel Panics Immediately
- Check UART output
- Verify DTB address
- Check memory map
- Test in QEMU

### No UART Output
- Verify UART pins
- Check baud rate (115200)
- Try different SoC build
- Use early_debug_init()

### Watchdog Not Working
- Check timer initialization
- Verify timer base address
- Test in QEMU first

## Success Criteria

Your kernel is working when:
- ✅ Boots without panic
- ✅ UART output visible
- ✅ Watchdog can reset system
- ✅ Can boot again after panic
- ✅ Memory protection working
- ✅ Device always recoverable

## Next Steps

After successful boot:
1. Test all safety features
2. Verify device discovery
3. Test PMIC control
4. Test clock configuration
5. Run full test suite

## Support

If you brick your device:
1. Don't panic
2. Try fastboot mode
3. Try EDL/download mode
4. Check XDA forums for your device
5. Use manufacturer flash tools

## License

This kernel is provided AS-IS with NO WARRANTY. Use at your own risk.
Test on expendable devices only.
