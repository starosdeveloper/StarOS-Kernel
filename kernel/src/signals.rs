//! Signal delivery to userspace for STAR OS kernel.
//!
//! POSIX-like signals 1-31 with per-task pending/blocked masks,
//! handler registration, and signal frame setup for ARM64.

use core::sync::atomic::{AtomicU32, Ordering};

// Signal numbers
pub const SIGHUP: u8 = 1;
pub const SIGINT: u8 = 2;
pub const SIGQUIT: u8 = 3;
pub const SIGILL: u8 = 4;
pub const SIGTRAP: u8 = 5;
pub const SIGABRT: u8 = 6;
pub const SIGBUS: u8 = 7;
pub const SIGFPE: u8 = 8;
pub const SIGKILL: u8 = 9;
pub const SIGUSR1: u8 = 10;
pub const SIGSEGV: u8 = 11;
pub const SIGUSR2: u8 = 12;
pub const SIGPIPE: u8 = 13;
pub const SIGALRM: u8 = 14;
pub const SIGTERM: u8 = 15;
pub const SIGCHLD: u8 = 17;
pub const SIGCONT: u8 = 18;
pub const SIGSTOP: u8 = 19;
pub const SIGTSTP: u8 = 20;

const MAX_SIGNALS: usize = 32;
const MAX_TASKS: usize = 256;

/// Signals that cannot be caught or blocked.
const UNCATCHABLE_MASK: u32 = (1 << SIGKILL) | (1 << SIGSTOP);

/// Saved register context for signal frame restoration.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct SavedContext {
    pub x: [u64; 31],
    pub sp: u64,
    pub pc: u64,
    pub pstate: u64,
}

/// Per-task signal state.
pub struct SignalState {
    pub pending: AtomicU32,
    pub blocked: AtomicU32,
    pub handlers: [Option<u64>; MAX_SIGNALS],
    pub saved_ctx: Option<SavedContext>,
}

impl SignalState {
    pub const fn new() -> Self {
        Self {
            pending: AtomicU32::new(0),
            blocked: AtomicU32::new(0),
            handlers: [None; MAX_SIGNALS],
            saved_ctx: None,
        }
    }
}

/// Global signal state table indexed by task_id.
static mut SIGNAL_TABLE: Option<&'static mut [SignalState]> = None;

/// Fixed backing storage for signal state (no_std compatible).
static mut SIGNAL_STORAGE: [core::mem::MaybeUninit<SignalState>; MAX_TASKS] =
    // SAFETY: MaybeUninit array doesn't require initialization.
    unsafe { core::mem::MaybeUninit::uninit().assume_init() };

static INIT_DONE: AtomicU32 = AtomicU32::new(0);

/// Initialize the signal subsystem. Must be called once at boot.
pub fn init() {
    if INIT_DONE.swap(1, Ordering::AcqRel) != 0 {
        return;
    }
    // SAFETY: Single-threaded init at boot, guarded by atomic flag.
    unsafe {
        for slot in SIGNAL_STORAGE.iter_mut() {
            slot.write(SignalState::new());
        }
        let ptr = SIGNAL_STORAGE.as_mut_ptr() as *mut SignalState;
        SIGNAL_TABLE = Some(core::slice::from_raw_parts_mut(ptr, MAX_TASKS));
    }
}

fn get_state(task_id: usize) -> Option<&'static mut SignalState> {
    if task_id >= MAX_TASKS {
        return None;
    }
    // SAFETY: Accessed after init(), task_id bounds-checked.
    unsafe { SIGNAL_TABLE.as_mut().and_then(|t| t.get_mut(task_id)) }
}

/// Send a signal to a task. Sets the pending bit.
pub fn send_signal(task_id: usize, signal: u8) -> Result<(), &'static str> {
    if signal == 0 || signal > 31 {
        return Err("invalid signal number");
    }
    let state = get_state(task_id).ok_or("invalid task_id")?;
    state.pending.fetch_or(1 << signal, Ordering::Release);
    Ok(())
}

/// Dequeue the highest-priority (lowest-numbered) deliverable signal.
/// Returns the signal number if one is pending and not blocked.
pub fn dequeue_signal(task_id: usize) -> Option<u8> {
    let state = get_state(task_id)?;
    loop {
        let pending = state.pending.load(Ordering::Acquire);
        let blocked = state.blocked.load(Ordering::Acquire);
        // Deliverable = pending & ~blocked (SIGKILL/SIGSTOP always deliverable)
        let deliverable = pending & (!blocked | UNCATCHABLE_MASK);
        if deliverable == 0 {
            return None;
        }
        let sig = deliverable.trailing_zeros() as u8;
        let bit = 1u32 << sig;
        if state.pending.compare_exchange_weak(
            pending, pending & !bit, Ordering::AcqRel, Ordering::Acquire
        ).is_ok() {
            return Some(sig);
        }
    }
}

/// Register a userspace signal handler for a task.
pub fn set_handler(task_id: usize, signal: u8, handler_addr: u64) -> Result<(), &'static str> {
    if signal == 0 || signal > 31 {
        return Err("invalid signal number");
    }
    if (1 << signal) & UNCATCHABLE_MASK != 0 {
        return Err("cannot catch SIGKILL or SIGSTOP");
    }
    let state = get_state(task_id).ok_or("invalid task_id")?;
    state.handlers[signal as usize] = Some(handler_addr);
    Ok(())
}

/// Set the blocked signal mask for a task. SIGKILL/SIGSTOP bits are forced clear.
pub fn set_signal_mask(task_id: usize, mask: u32) -> Result<(), &'static str> {
    let state = get_state(task_id).ok_or("invalid task_id")?;
    state.blocked.store(mask & !UNCATCHABLE_MASK, Ordering::Release);
    Ok(())
}

/// Address of the sigreturn trampoline mapped into every process.
const SIGRETURN_TRAMPOLINE: u64 = 0xFFFF_FFFF_0000;

/// Prepare a signal frame on the user stack for delivery.
/// Saves current registers, sets PC to handler, LR to sigreturn trampoline.
/// `ctx` is the current saved register state of the task (e.g., from interrupt frame).
pub fn setup_signal_frame(task_id: usize, signal: u8, ctx: &mut SavedContext) -> Result<(), &'static str> {
    if signal == 0 || signal > 31 {
        return Err("invalid signal number");
    }
    let state = get_state(task_id).ok_or("invalid task_id")?;

    let handler = state.handlers[signal as usize]
        .ok_or("no handler registered")?;

    // Save current context for restoration on sigreturn
    state.saved_ctx = Some(*ctx);

    // Set up the new context to invoke the handler
    ctx.x[0] = signal as u64;           // x0 = signal number (first argument)
    ctx.x[30] = SIGRETURN_TRAMPOLINE;   // LR = sigreturn trampoline
    ctx.pc = handler;                    // PC = handler address
    // SP stays the same (handler runs on user stack)

    Ok(())
}

/// Restore saved context after sigreturn syscall.
pub fn restore_signal_frame(task_id: usize, ctx: &mut SavedContext) -> Result<(), &'static str> {
    let state = get_state(task_id).ok_or("invalid task_id")?;
    let saved = state.saved_ctx.take().ok_or("no saved signal frame")?;
    *ctx = saved;
    Ok(())
}
