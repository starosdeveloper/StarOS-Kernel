# Xiaomi Redmi 4X (santoni) - Device Configuration

## Hardware Specifications

**SoC:** Qualcomm Snapdragon 435 (MSM8937)
- CPU: 8x Cortex-A53 @ 1.4 GHz
- GPU: Adreno 505
- Process: 28nm

**Display:**
- Panel: Tianma NT35596
- Resolution: 720x1280 (HD)
- Size: 5.0 inches
- Technology: IPS LCD
- Refresh Rate: 60Hz

**Memory:**
- RAM: 2GB LPDDR3
- Storage: 16GB eMMC

**Modem:**
- Qualcomm X6 LTE
- Bands: LTE Cat 4
- Max Speed: 150 Mbps down, 50 Mbps up

**Audio:**
- Codec: Qualcomm WCD9335
- Speaker: Mono
- Headphone Jack: 3.5mm

**Sensors:**
- Accelerometer: Bosch BMI160
- Proximity: STK3x1x
- Ambient Light: STK3x1x

**Battery:**
- Capacity: 4100 mAh
- Charging: 5V/2A (10W)

---

## Memory Map

```
0x00000000 - 0x7FFFFFFF : Reserved
0x80000000 - 0xFFFFFFFF : RAM (2GB)

Peripherals:
0x01A00000 - 0x01A8FFFF : MDSS (Display)
0x01A94000 - 0x01A943FF : DSI Controller
0x0C0F0000 - 0x0C0F3FFF : WCD Audio Codec
0x078B7000 - 0x078B75FF : I2C (Touch)
0x078AF000 - 0x078AF1FF : UART Console
```

---

## Boot Configuration

**Kernel Load Address:** 0x80008000
**DTB Load Address:** 0x82000000
**Ramdisk Load Address:** 0x83000000

**Kernel Command Line:**
```
console=ttyMSM0,115200
androidboot.hardware=qcom
androidboot.serialno=<serial>
```

---

## Driver Configuration

### Display (MDSS)
- Base Address: 0x01A00000
- DSI Address: 0x01A94000
- Panel: Tianma NT35596 (720p)
- Backlight: PWM-based

### Touch (Goodix GT917S)
- I2C Address: 0x5D
- I2C Bus: 3
- Interrupt GPIO: 65
- Reset GPIO: 64

### Modem (Qualcomm)
- QMI Port: /dev/cdc-wdm0
- AT Port: /dev/ttyUSB2
- RmNet: rmnet_data0

### Audio (WCD9335)
- Base Address: 0x0C0F0000
- I2S Interface: Primary
- Sample Rates: 8/16/48 kHz

---

## Power Management

**CPU Frequencies:**
- Min: 400 MHz
- Max: 1400 MHz
- Governor: Interactive

**GPU Frequencies:**
- Min: 133 MHz
- Max: 450 MHz

**Thermal Limits:**
- CPU: 85°C
- GPU: 80°C
- Battery: 60°C

---

## Build Instructions

### 1. Compile Device Tree
```bash
dtc -I dts -O dtb -o santoni.dtb santoni.dts
```

### 2. Build Kernel
```bash
cargo build --release \
    --target aarch64-unknown-none \
    --features device-santoni
```

### 3. Create Boot Image
```bash
./tools/boot-image-builder \
    --device santoni \
    --kernel target/aarch64-unknown-none/release/staros-kernel \
    --dtb santoni.dtb \
    --output boot-santoni.img
```

### 4. Flash
```bash
fastboot flash boot boot-santoni.img
fastboot reboot
```

---

## Testing Checklist

### Display
- [ ] Display initializes
- [ ] Framebuffer works
- [ ] Brightness control works
- [ ] No screen tearing

### Touch
- [ ] Touch events detected
- [ ] Multi-touch works
- [ ] Gestures recognized
- [ ] Calibration accurate

### Modem
- [ ] Modem initializes
- [ ] Signal detected
- [ ] Can make calls
- [ ] Can send SMS
- [ ] Data connection works

### Audio
- [ ] Speaker works
- [ ] Earpiece works
- [ ] Headphones work
- [ ] Volume control works
- [ ] No distortion

### Power
- [ ] Battery level detected
- [ ] Charging works
- [ ] Thermal management works
- [ ] Suspend/resume works

---

## Known Issues

### Current
- Touch driver not yet implemented
- Camera not supported
- Bluetooth not implemented
- GPS not implemented

### Workarounds
- Use serial console for input
- Camera support in v0.9.0
- Bluetooth in v0.9.0

---

## Performance Targets

**Boot Time:** < 5 seconds
**App Launch:** < 500ms
**UI Rendering:** 60 FPS
**Battery Life:** 24+ hours (standby)

---

## References

- [Qualcomm MSM8937 Datasheet](https://www.qualcomm.com/products/snapdragon-435)
- [Redmi 4X Specifications](https://www.mi.com/redmi4x)
- [LineageOS Device Tree](https://github.com/LineageOS/android_device_xiaomi_santoni)

---

**Status:** Ready for testing
**Last Updated:** March 3, 2026
**Maintainer:** StarOS Team
