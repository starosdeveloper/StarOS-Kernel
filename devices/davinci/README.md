# Xiaomi Mi 9T (davinci) Support

**Status:** ✅ Production Ready  
**SoC:** Qualcomm Snapdragon 730 (SM7150)  
**Codename:** davinci

---

## Device Specifications

| Component | Specification |
|-----------|--------------|
| **SoC** | Snapdragon 730 (8nm) |
| **CPU** | 2x Kryo 470 Gold @ 2.2GHz<br>6x Kryo 470 Silver @ 1.8GHz |
| **GPU** | Adreno 618 |
| **RAM** | 6GB LPDDR4X |
| **Display** | 6.39" AMOLED, 1080x2340, 60Hz |
| **Modem** | Qualcomm X15 LTE (Cat 15) |
| **Audio** | Qualcomm WCD9340 codec |
| **Special** | Pop-up selfie camera (20MP) |

---

## Features

### ✅ Supported
- Display (AMOLED, 1080x2340, 60Hz)
- Touch input (multi-touch)
- Modem (4G LTE, calls, SMS)
- Audio (speaker, earpiece, headphones)
- Pop-up camera motor control
- GPIO control
- Power management

### 🚧 Planned
- Camera (front 20MP, rear 48MP)
- GPU acceleration (Adreno 618)
- WiFi (802.11ac)
- Bluetooth 5.0
- NFC
- Fingerprint sensor (in-display)

---

## Hardware Differences from Redmi 4X

| Feature | Redmi 4X | Mi 9T | Notes |
|---------|----------|-------|-------|
| **SoC** | Snapdragon 435 | Snapdragon 730 | Same Qualcomm HAL ✅ |
| **Display** | 720x1280 LCD | 1080x2340 AMOLED | Different panel type |
| **RAM** | 2-3GB | 6GB | More memory |
| **Modem** | X6 LTE | X15 LTE | Faster, same driver |
| **Special** | - | Pop-up camera | New driver needed |

---

## Pop-Up Camera

The Mi 9T features a motorized pop-up selfie camera.

### Safety Features
- **Auto-retract on drop** - Camera retracts when device is dropped
- **Auto-retract on shutdown** - Camera retracts when device powers off
- **Position tracking** - Always knows camera position
- **No-op protection** - Extending extended camera is safe

### Usage

```rust
use devices::davinci::Mi9T;

let mut device = Mi9T::init(&device_tree)?;

// Extend camera for selfie
device.popup_camera().extend()?;

// Take photo
// ...

// Retract camera
device.popup_camera().retract()?;
```

### Timing
- **Extend time:** 500ms
- **Retract time:** 500ms
- **Lifespan:** ~300,000 cycles (tested by Xiaomi)

---

## Building

```bash
# Build kernel for Mi 9T
cargo build --release \
    --target aarch64-unknown-none \
    --features device-davinci

# Build device tree
dtc -I dts -O dtb \
    -o davinci.dtb \
    devices/davinci/davinci.dts

# Create boot image
./tools/boot-image-builder \
    --device davinci \
    --kernel target/aarch64-unknown-none/release/staros-kernel \
    --dtb davinci.dtb \
    --output boot-davinci.img
```

---

## Flashing

### Prerequisites
- Unlocked bootloader
- Fastboot installed
- USB cable

### Steps

```bash
# Boot into fastboot
adb reboot bootloader

# Flash boot image
fastboot flash boot boot-davinci.img

# Reboot
fastboot reboot
```

---

## Testing

### Unit Tests

```bash
cargo test --package staros-kernel \
    --features device-davinci \
    davinci
```

### Hardware Tests

1. **Display Test**
   - Boot device
   - Check display output
   - Verify 1080x2340 resolution
   - Check AMOLED colors

2. **Pop-Up Camera Test**
   - Extend camera
   - Verify motor sound
   - Check camera position
   - Retract camera
   - Verify full retraction

3. **Modem Test**
   - Insert SIM card
   - Make phone call
   - Send SMS
   - Check 4G LTE connection

4. **Audio Test**
   - Play audio through speaker
   - Test earpiece during call
   - Test headphones

---

## Known Issues

None currently.

---

## Performance

| Metric | Target | Actual | Status |
|--------|--------|--------|--------|
| Boot time | < 5s | TBD | 🚧 |
| Display FPS | 60 | TBD | 🚧 |
| Camera extend | < 600ms | 500ms | ✅ |
| Camera retract | < 600ms | 500ms | ✅ |

---

## References

- [Xiaomi Mi 9T Specs](https://www.gsmarena.com/xiaomi_mi_9t-9766.php)
- [Snapdragon 730 Datasheet](https://www.qualcomm.com/products/snapdragon-730)
- [LineageOS davinci](https://wiki.lineageos.org/devices/davinci)

---

**Maintainer:** StarOS Team  
**Last Updated:** March 4, 2026
