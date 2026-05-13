//! Kernel boot and initialization
//!
//! Entry point and early initialization

pub mod entry;

use core::cell::UnsafeCell;
use crate::error::KernelError;
use crate::drivers::{DeviceTree, DeviceDiscovery, Uart};
use crate::memory::{PhysicalAllocator, PhysAddr, PAGE_SIZE};
use crate::interrupts::{ExceptionVectorTable, InterruptController, Timer, TIMER_FREQ};
use crate::process::{Scheduler, Task, Priority};
use crate::syscall::{SyscallDispatcher, init_syscalls};

// Helper for printing (avoids macro issues in tests)
#[cfg(not(feature = "std"))]
fn kprint(s: &str) {
    use crate::drivers::uart::UART0;
    UART0.write_str(s);
}

#[cfg(feature = "std")]
fn kprint(_s: &str) {
    // No-op in tests
}

/// Boot information passed from bootloader
#[repr(C)]
pub struct BootInfo {
    pub dtb_addr: u64,
    pub initrd_addr: u64,
    pub initrd_size: u64,
    pub memory_base: u64,
    pub memory_size: u64,
}

/// Kernel state after initialization
pub struct KernelState {
    pub phys_allocator: PhysicalAllocator,
    pub scheduler: Scheduler,
    pub interrupt_controller: InterruptController,
    pub timer: Timer,
    pub syscall_dispatcher: SyscallDispatcher,
    pub exception_table: ExceptionVectorTable,
}

impl KernelState {
    pub const fn new() -> Self {
        Self {
            phys_allocator: PhysicalAllocator::new(PhysAddr::new(0), 0),
            scheduler: Scheduler::new(),
            interrupt_controller: InterruptController::new(),
            timer: Timer::new(TIMER_FREQ),
            syscall_dispatcher: SyscallDispatcher::new(),
            exception_table: ExceptionVectorTable::new(),
        }
    }
}

/// Global kernel state
pub struct KernelCell(UnsafeCell<KernelState>);
unsafe impl Sync for KernelCell {}
impl KernelCell {
    pub const fn new() -> Self { Self(UnsafeCell::new(KernelState::new())) }
    /// # Safety: caller must ensure exclusive access
    pub unsafe fn get(&self) -> &mut KernelState { &mut *self.0.get() }
}
pub static KERNEL: KernelCell = KernelCell::new();

/// Early boot - called from assembly
/// 
/// # Safety
/// Must be called once at boot with valid boot_info
#[no_mangle]
pub unsafe extern "C" fn kernel_early_boot(boot_info: *const BootInfo) -> ! {
    // Initialize no-op logger (LTO will optimize away)
    crate::logger::init();
    
    // Parse boot info
    let boot_info = &*boot_info;
    
    // Initialize early console
    early_console_init(boot_info.dtb_addr as usize);
    
    kprint("STAR OS Microkernel v1.0");
    kprint("Booting...");
    
    // Parse device tree
    kprint("Parsing device tree...");
    let dt = DeviceTree::from_ptr(boot_info.dtb_addr as usize)
        .expect("Failed to parse device tree");
    
    // Discover devices
    kprint("Discovering devices...");
    let mut discovery = DeviceDiscovery::new();
    discovery.discover(&dt).expect("Failed to discover devices");
    
    // Initialize memory
    kprint("Initializing memory...");
    init_memory(boot_info);
    
    // Initialize interrupts
    kprint("Initializing interrupts...");
    init_interrupts();
    
    // Initialize syscalls
    kprint("Initializing syscalls...");
    init_syscalls(&mut KERNEL.get().syscall_dispatcher);
    
    // Initialize scheduler
    kprint("Initializing scheduler...");
    init_scheduler();
    
    kprint("Boot complete!");
    
    // Enter kernel main loop
    kernel_main()
}

/// Initialize early console
unsafe fn early_console_init(dtb_addr: usize) {
    // Try to parse DTB and find UART
    if let Ok(dt) = DeviceTree::from_ptr(dtb_addr) {
        let mut discovery = DeviceDiscovery::new();
        if discovery.discover(&dt).is_ok() {
            if let Some(uart_base) = discovery.uart_base() {
                let uart = Uart::new(uart_base);
                let _ = uart.init(115200, 24_000_000);
                return;
            }
        }
    }
    
    // Fallback to default QEMU address
    let uart = Uart::new(0x0900_0000);
    let _ = uart.init(115200, 24_000_000);
}

/// Initialize memory subsystem
unsafe fn init_memory(boot_info: &BootInfo) {
    let base = PhysAddr::new(boot_info.memory_base as usize);
    let pages = (boot_info.memory_size as usize) / PAGE_SIZE;
    
    KERNEL.get().phys_allocator = PhysicalAllocator::new(base, pages);
    KERNEL.get().phys_allocator.init().expect("Failed to init physical allocator");
    
    kprint("Memory initialized\n");
}

/// Initialize interrupt subsystem
unsafe fn init_interrupts() {
    // Register exception handlers
    fn irq_handler(_ctx: &crate::interrupts::ExceptionContext) -> Result<(), KernelError> {
        // Handle IRQ
        unsafe {
            KERNEL.get().interrupt_controller.handle(30)?; // Timer IRQ
        }
        Ok(())
    }
    
    KERNEL.get().exception_table.register_irq_handler(irq_handler);
    
    // Initialize timer
    KERNEL.get().timer.init().expect("Failed to init timer");
    
    kprint("Interrupts enabled");
}

/// Initialize scheduler
unsafe fn init_scheduler() {
    // Create idle task
    let idle_stack = KERNEL.get().phys_allocator.alloc_page()
        .expect("Failed to allocate idle stack");
    
    let idle_task = Task::new(
        idle_task_entry as *const () as u64,
        0,
        crate::memory::VirtAddr::new(idle_stack.as_usize()),
        64 * 1024,
        Priority::IDLE,
        None,
        1024,
    ).expect("Failed to create idle task");
    
    KERNEL.get().scheduler.set_idle_task(idle_task.id());
    
    kprint("Scheduler ready");
}

/// Idle task entry point
extern "C" fn idle_task_entry() -> ! {
    loop {
        // Wait for interrupt
        #[cfg(target_arch = "aarch64")]
        unsafe { core::arch::asm!("wfi") };
        
        core::hint::spin_loop();
    }
}

/// Kernel main loop
fn kernel_main() -> ! {
    kprint("Entering main loop...");
    
    loop {
        // Schedule next task
        unsafe {
            if let Some(next_id) = KERNEL.get().scheduler.schedule() {
                KERNEL.get().scheduler.set_current(next_id);
                // TODO: Context switch to next_id
            }
        }
        
        // Wait for interrupt
        #[cfg(target_arch = "aarch64")]
        unsafe { core::arch::asm!("wfi") };
        
        core::hint::spin_loop();
    }
}

// Panic handler moved to src/safety/panic.rs

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_boot_info_size() {
        use core::mem::size_of;
        assert_eq!(size_of::<BootInfo>(), 40);
    }

    #[test]
    fn test_kernel_state() {
        let state = KernelState::new();
        assert_eq!(state.scheduler.current(), None);
    }
}
