// SPDX-License-Identifier: MIT
//! Mouse device drivers: PS/2 and USB HID.
//!
//! Both drivers decode raw hardware packets and push [`InputEvent`]s into
//! the global [`EventQueue`].  The InputManager consumes the queue each frame.
//!
//! # PS/2 mouse
//!
//! Standard 3-byte packet format:
//! ```text
//!  Byte 0: [Y-ovf][X-ovf][Y-sign][X-sign][1][Mid][Right][Left]
//!  Byte 1:  X movement (two's complement if sign bit set)
//!  Byte 2:  Y movement (two's complement if sign bit set, **inverted**)
//! ```
//! IRQ 12, data port 0x60, status port 0x64.
//!
//! # USB HID Basic Mouse Report (Report ID 0)
//!
//! ```text
//!  Byte 0: buttons [bit0=Left, bit1=Right, bit2=Middle]
//!  Byte 1: X displacement (signed)
//!  Byte 2: Y displacement (signed)
//!  Byte 3: Wheel (signed, optional)
//! ```

use super::event::{EventQueue, InputEvent, btn};
use crate::error::KernelError;
use crate::interrupts::Irq;

// ---------------------------------------------------------------------------
// PS/2 mouse
// ---------------------------------------------------------------------------

/// Standard PC IRQ for the PS/2 mouse (secondary PS/2 port).
pub const PS2_MOUSE_IRQ: Irq = 12;

/// x86 I/O ports for the PS/2 controller.
/// On AArch64 / bare-metal these are memory-mapped — adjust to your SoC.
const PS2_DATA_PORT:   u16 = 0x60;
const PS2_STATUS_PORT: u16 = 0x64;
const PS2_CMD_PORT:    u16 = 0x64;

/// PS/2 mouse packet decoder.
///
/// Accumulates up to 3 bytes and fires an event when a complete packet arrives.
pub struct Ps2Mouse {
    buf:   [u8; 3],
    pos:   usize,
    /// Previous button state — used to detect press/release transitions.
    prev_buttons: u8,
}

impl Ps2Mouse {
    pub const fn new() -> Self {
        Self { buf: [0u8; 3], pos: 0, prev_buttons: 0 }
    }

    /// Initialise the PS/2 mouse via controller commands.
    ///
    /// # Safety
    /// 
    /// Caller must ensure:
    /// - PS/2 controller is initialized and accessible
    /// - I/O ports 0x60/0x64 are mapped correctly for the platform
    /// - No concurrent access to PS/2 controller during initialization
    /// - Called only once during system initialization
    pub unsafe fn init(&self) {
        // Enable auxiliary device (mouse).
        // SAFETY: ps2_cmd accesses I/O ports, caller guarantees PS/2 controller is ready
        ps2_cmd(0xA8);
        // Enable auxiliary interrupts in the Command Byte.
        ps2_cmd(0x20);           // Read Command Byte
        // SAFETY: ps2_read_data reads from data port, controller is ready
        let cb = ps2_read_data();
        // SAFETY: Writing to command port with valid command byte
        ps2_write_data(0x60, cb | 0x02); // set bit 1 = enable aux IRQ
        // Send "Enable Streaming" to the mouse.
        // SAFETY: ps2_write_aux sends command to auxiliary device
        ps2_write_aux(0xF4);
        let _ = ps2_read_data(); // ACK
    }

    /// Feed one raw byte from the PS/2 data port into the packet assembler.
    ///
    /// Call this from the IRQ 12 handler.  When a complete 3-byte packet is
    /// assembled, the decoded event is pushed to `queue`.
    pub fn feed(&mut self, byte: u8, queue: &EventQueue, cursor_x: u32, cursor_y: u32) {
        // Byte 0 must have bit 3 set (always-1 bit) — use it for sync.
        if self.pos == 0 && (byte & 0x08) == 0 {
            return; // de-sync, wait for a valid first byte
        }

        self.buf[self.pos] = byte;
        self.pos += 1;

        if self.pos == 3 {
            self.pos = 0;
            self.decode(queue, cursor_x, cursor_y);
        }
    }

    fn decode(&mut self, queue: &EventQueue, cursor_x: u32, cursor_y: u32) {
        let flags = self.buf[0];
        let buttons = flags & 0x07; // bits [2:0] = Middle|Right|Left

        // Signed 9-bit deltas: bit in flags byte is the sign extension.
        let raw_dx = self.buf[1] as i16
            | if flags & 0x10 != 0 { -256i16 } else { 0 };
        // Y axis is inverted in PS/2 (positive = up on screen).
        let raw_dy = -(self.buf[2] as i16
            | if flags & 0x20 != 0 { -256i16 } else { 0 });

        // Overflow flags — discard the packet if set.
        if flags & 0xC0 != 0 {
            return;
        }

        if raw_dx != 0 || raw_dy != 0 {
            queue.push(InputEvent::MouseMove { dx: raw_dx, dy: raw_dy });
        }

        // Detect button press / release transitions.
        let changed = buttons ^ self.prev_buttons;
        if changed != 0 {
            for bit in 0..3u8 {
                if changed & (1 << bit) != 0 {
                    let pressed = buttons & (1 << bit) != 0;
                    queue.push(InputEvent::MouseButton {
                        buttons: 1 << bit,
                        pressed,
                        x: cursor_x,
                        y: cursor_y,
                    });
                }
            }
        }
        self.prev_buttons = buttons;
    }
}

// ---------------------------------------------------------------------------
// USB HID Basic Mouse
// ---------------------------------------------------------------------------

/// USB HID mouse report decoder (Boot Protocol, 4 bytes).
pub struct UsbHidMouse {
    prev_buttons: u8,
}

impl UsbHidMouse {
    pub const fn new() -> Self {
        Self { prev_buttons: 0 }
    }

    /// Decode a USB HID mouse report and push events to `queue`.
    ///
    /// `report` must be at least 3 bytes long (Boot Protocol minimum).
    /// Call this from the USB interrupt handler when a new report arrives.
    pub fn handle_report(
        &mut self,
        report: &[u8],
        queue: &EventQueue,
        cursor_x: u32,
        cursor_y: u32,
    ) {
        if report.len() < 3 {
            return;
        }

        let buttons = report[0] & 0x07;
        let dx = report[1] as i8 as i16;
        let dy = report[2] as i8 as i16; // HID Y: positive = down

        if dx != 0 || dy != 0 {
            queue.push(InputEvent::MouseMove { dx, dy });
        }

        // Wheel (byte 3, optional).
        if report.len() >= 4 && report[3] != 0 {
            queue.push(InputEvent::MouseWheel {
                delta: report[3] as i8,
                x: cursor_x,
                y: cursor_y,
            });
        }

        // Button transitions.
        let changed = buttons ^ self.prev_buttons;
        if changed != 0 {
            for bit in 0..3u8 {
                if changed & (1 << bit) != 0 {
                    let pressed = buttons & (1 << bit) != 0;
                    queue.push(InputEvent::MouseButton {
                        buttons: 1 << bit,
                        pressed,
                        x: cursor_x,
                        y: cursor_y,
                    });
                }
            }
        }
        self.prev_buttons = buttons;
    }
}

// ---------------------------------------------------------------------------
// Keyboard scancode decoder (minimal, Set 2)
// ---------------------------------------------------------------------------

/// Feed one PS/2 keyboard scancode byte (Set 2).
///
/// Handles the 0xF0 break prefix.  Extended (0xE0) scancodes are passed
/// through with bit 15 set in the resulting scancode.
pub struct Ps2Keyboard {
    extended: bool,
    release:  bool,
}

impl Ps2Keyboard {
    pub const fn new() -> Self {
        Self { extended: false, release: false }
    }

    /// Feed a raw scancode byte; returns an event if a full key is decoded.
    pub fn feed(&mut self, byte: u8, queue: &EventQueue) {
        match byte {
            0xE0 => { self.extended = true;  return; }
            0xF0 => { self.release  = true;  return; }
            _ => {}
        }

        let scancode = if self.extended {
            0x0100u16 | byte as u16
        } else {
            byte as u16
        };

        if self.release {
            queue.push(InputEvent::KeyUp { scancode });
        } else {
            queue.push(InputEvent::KeyDown { scancode });
        }

        self.extended = false;
        self.release  = false;
    }
}

// ---------------------------------------------------------------------------
// IRQ handler thunks
// ---------------------------------------------------------------------------

/// Global IRQ 12 handler — reads one byte from the PS/2 data port and
/// forwards it to the PS/2 mouse state machine.
///
/// Register with `InterruptController::register_handler(PS2_MOUSE_IRQ, ps2_mouse_irq)`.
pub fn ps2_mouse_irq(_irq: Irq) -> Result<(), KernelError> {
    // Read one byte from the PS/2 data port.
    // SAFETY: Called from IRQ context, PS/2 controller is initialized
    // Reading from data port is safe when IRQ fires (data is available)
    let byte = unsafe { ps2_read_data() };

    // Forward to the global InputManager.
    super::INPUT_MANAGER.lock().feed_ps2_mouse(byte);
    Ok(())
}

/// IRQ handler for a USB HID mouse interrupt endpoint.
/// The caller must retrieve the HID report from the USB stack and call
/// `InputManager::feed_usb_hid(report)` directly — USB is handled at a
/// higher level than raw I/O ports.
pub fn usb_hid_mouse_irq(_irq: Irq) -> Result<(), KernelError> {
    // USB HID reports are delivered via the USB stack callback, not here.
    // This handler just wakes the USB processing path.
    Ok(())
}

// ---------------------------------------------------------------------------
// Low-level PS/2 I/O helpers (x86 port I/O)
// On AArch64 replace with MMIO reads to your SoC's PS/2 MMIO base.
// ---------------------------------------------------------------------------

/// Wait for PS/2 controller input buffer to be empty (ready for write)
/// 
/// # Safety
/// 
/// Accesses I/O port 0x64. Caller must ensure PS/2 controller is initialized.
unsafe fn ps2_wait_write() {
    // Spin until bit 1 (input buffer full) clears.
    let mut tries = 0u32;
    while tries < 100_000 {
        // SAFETY: Reading status port, caller guarantees PS/2 controller exists
        let status = inb(PS2_STATUS_PORT);
        if status & 0x02 == 0 { return; }
        tries += 1;
        core::hint::spin_loop();
    }
}

/// Wait for PS/2 controller output buffer to have data (ready for read)
/// 
/// # Safety
/// 
/// Accesses I/O port 0x64. Caller must ensure PS/2 controller is initialized.
unsafe fn ps2_wait_read() {
    let mut tries = 0u32;
    while tries < 100_000 {
        // SAFETY: Reading status port, caller guarantees PS/2 controller exists
        let status = inb(PS2_STATUS_PORT);
        if status & 0x01 != 0 { return; }
        tries += 1;
        core::hint::spin_loop();
    }
}

/// Send command to PS/2 controller
/// 
/// # Safety
/// 
/// Accesses I/O ports. Caller must ensure PS/2 controller is initialized.
unsafe fn ps2_cmd(cmd: u8) {
    ps2_wait_write();
    // SAFETY: Writing to command port, controller is ready (wait_write succeeded)
    outb(PS2_CMD_PORT, cmd);
}

/// Read byte from PS/2 data port
/// 
/// # Safety
/// 
/// Accesses I/O port 0x60. Caller must ensure data is available.
unsafe fn ps2_read_data() -> u8 {
    ps2_wait_read();
    // SAFETY: Reading data port, data is available (wait_read succeeded)
    inb(PS2_DATA_PORT)
}

/// Write byte to PS/2 port
/// 
/// # Safety
/// 
/// Accesses I/O ports. Caller must ensure PS/2 controller is initialized.
unsafe fn ps2_write_data(port: u16, data: u8) {
    ps2_wait_write();
    // SAFETY: Writing to port, controller is ready (wait_write succeeded)
    outb(port, data);
}

/// Write byte to PS/2 auxiliary device (mouse)
/// 
/// # Safety
/// 
/// Accesses I/O ports. Caller must ensure PS/2 controller and mouse are initialized.
unsafe fn ps2_write_aux(data: u8) {
    ps2_cmd(0xD4); // "Write to auxiliary device"
    ps2_wait_write();
    // SAFETY: Writing to data port for auxiliary device
    outb(PS2_DATA_PORT, data);
}

// Inline assembly stubs for x86 I/O ports.
// On AArch64 these map to your SoC MMIO peripheral base + offset.
#[cfg(target_arch = "x86_64")]
/// Read byte from I/O port (x86_64 only)
/// 
/// # Safety
/// 
/// Caller must ensure port is valid and accessible.
unsafe fn inb(port: u16) -> u8 {
    let val: u8;
    // SAFETY: Inline assembly for I/O port read, caller guarantees port validity
    core::arch::asm!("in al, dx", out("al") val, in("dx") port, options(nomem, nostack));
    val
}

#[cfg(target_arch = "x86_64")]
/// Write byte to I/O port (x86_64 only)
/// 
/// # Safety
/// 
/// Caller must ensure port is valid and accessible.
unsafe fn outb(port: u16, val: u8) {
    // SAFETY: Inline assembly for I/O port write, caller guarantees port validity
    core::arch::asm!("out dx, al", in("dx") port, in("al") val, options(nomem, nostack));
}

// AArch64 stub: replace with real MMIO read from your PS/2-compatible peripheral.
#[cfg(target_arch = "aarch64")]
/// Stub for AArch64 - replace with MMIO read
/// 
/// # Safety
/// 
/// This is a stub. Real implementation must access MMIO correctly.
unsafe fn inb(_port: u16) -> u8 { 0 }

#[cfg(target_arch = "aarch64")]
/// Stub for AArch64 - replace with MMIO write
/// 
/// # Safety
/// 
/// This is a stub. Real implementation must access MMIO correctly.
unsafe fn outb(_port: u16, _val: u8) {}

// Fallback for other targets (host tests).
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
/// Stub for testing
/// 
/// # Safety
/// 
/// This is a stub for testing only.
unsafe fn inb(_port: u16) -> u8 { 0 }

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
/// Stub for testing
/// 
/// # Safety
/// 
/// This is a stub for testing only.
unsafe fn outb(_port: u16, _val: u8) {}
