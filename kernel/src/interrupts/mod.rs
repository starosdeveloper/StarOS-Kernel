//! Interrupt and exception handling

pub mod exception;
pub mod controller;
pub mod timer;

pub use exception::{
    ExceptionType, ExceptionLevel, ExceptionContext, ExceptionVectorTable,
    ExceptionHandler, current_el, enable_interrupts, disable_interrupts,
};

#[cfg(target_arch = "aarch64")]
pub use exception::install_vector_table;
pub use controller::{
    InterruptController, Irq, IrqHandler, IrqPriority, IrqConfig,
};
pub use timer::{Timer, TIMER_FREQ, TICK_NS};
