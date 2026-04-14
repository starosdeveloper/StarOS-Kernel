//! System call interface
//!
//! Provides user-space to kernel-space communication via SVC instruction

#[cfg(not(feature = "std"))]
use core::sync::atomic::{AtomicU64, Ordering};
#[cfg(feature = "std")]
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::KernelError;
use crate::process::TaskId;
use crate::interrupts::ExceptionContext;

/// System call numbers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum SyscallNumber {
    // Process management
    Exit = 0,
    Fork = 1,
    Exec = 2,
    Wait = 3,
    Kill = 4,
    GetPid = 5,
    GetPPid = 6,
    
    // Memory management
    Mmap = 10,
    Munmap = 11,
    Mprotect = 12,
    Brk = 13,
    
    // IPC
    Send = 20,
    Receive = 21,
    Signal = 22,
    
    // I/O
    Read = 30,
    Write = 31,
    Open = 32,
    Close = 33,
    
    // Time
    Sleep = 40,
    GetTime = 41,
    
    // Synchronization
    MutexLock = 50,
    MutexUnlock = 51,
    SemWait = 52,
    SemSignal = 53,
    
    // Post-Quantum Crypto
    PQKeyGen = 60,
    PQEncapsulate = 61,
    PQDecapsulate = 62,
    PQSign = 63,
    PQVerify = 64,
}

impl SyscallNumber {
    pub fn from_u64(val: u64) -> Option<Self> {
        match val {
            0 => Some(Self::Exit),
            1 => Some(Self::Fork),
            2 => Some(Self::Exec),
            3 => Some(Self::Wait),
            4 => Some(Self::Kill),
            5 => Some(Self::GetPid),
            6 => Some(Self::GetPPid),
            10 => Some(Self::Mmap),
            11 => Some(Self::Munmap),
            12 => Some(Self::Mprotect),
            13 => Some(Self::Brk),
            20 => Some(Self::Send),
            21 => Some(Self::Receive),
            22 => Some(Self::Signal),
            30 => Some(Self::Read),
            31 => Some(Self::Write),
            32 => Some(Self::Open),
            33 => Some(Self::Close),
            40 => Some(Self::Sleep),
            41 => Some(Self::GetTime),
            50 => Some(Self::MutexLock),
            51 => Some(Self::MutexUnlock),
            52 => Some(Self::SemWait),
            53 => Some(Self::SemSignal),
            60 => Some(Self::PQKeyGen),
            61 => Some(Self::PQEncapsulate),
            62 => Some(Self::PQDecapsulate),
            63 => Some(Self::PQSign),
            64 => Some(Self::PQVerify),
            _ => None,
        }
    }
}

/// System call arguments (passed in registers x0-x5)
#[derive(Debug, Clone, Copy)]
pub struct SyscallArgs {
    pub arg0: u64,
    pub arg1: u64,
    pub arg2: u64,
    pub arg3: u64,
    pub arg4: u64,
    pub arg5: u64,
}

impl SyscallArgs {
    pub fn from_context(ctx: &ExceptionContext) -> Self {
        Self {
            arg0: ctx.x[0],
            arg1: ctx.x[1],
            arg2: ctx.x[2],
            arg3: ctx.x[3],
            arg4: ctx.x[4],
            arg5: ctx.x[5],
        }
    }
}

/// System call result
pub type SyscallResult = Result<u64, KernelError>;

/// System call handler trait
pub trait SyscallHandler {
    fn handle(&self, syscall: SyscallNumber, args: SyscallArgs, caller: TaskId) -> SyscallResult;
}

/// System call dispatcher
pub struct SyscallDispatcher {
    handlers: [Option<fn(SyscallArgs, TaskId) -> SyscallResult>; 64],
    total_syscalls: AtomicU64,
    invalid_syscalls: AtomicU64,
}

impl SyscallDispatcher {
    pub const fn new() -> Self {
        Self {
            handlers: [None; 64],
            total_syscalls: AtomicU64::new(0),
            invalid_syscalls: AtomicU64::new(0),
        }
    }

    /// Register syscall handler
    pub fn register(&mut self, syscall: SyscallNumber, handler: fn(SyscallArgs, TaskId) -> SyscallResult) {
        let idx = syscall as usize;
        if idx < 64 {
            self.handlers[idx] = Some(handler);
        }
    }

    /// Dispatch syscall from exception context
    pub fn dispatch(&self, ctx: &ExceptionContext, caller: TaskId) -> SyscallResult {
        self.total_syscalls.fetch_add(1, Ordering::Relaxed);

        // Syscall number in x8 (ARM64 convention)
        let syscall_num = ctx.x[8];
        
        let syscall = match SyscallNumber::from_u64(syscall_num) {
            Some(s) => s,
            None => {
                self.invalid_syscalls.fetch_add(1, Ordering::Relaxed);
                return Err(KernelError::InvalidParameter("Invalid syscall number"));
            }
        };

        let args = SyscallArgs::from_context(ctx);

        let idx = syscall as usize;
        if idx >= 64 {
            self.invalid_syscalls.fetch_add(1, Ordering::Relaxed);
            return Err(KernelError::InvalidParameter("Syscall out of range"));
        }

        if let Some(handler) = self.handlers[idx] {
            handler(args, caller)
        } else {
            self.invalid_syscalls.fetch_add(1, Ordering::Relaxed);
            Err(KernelError::NotImplemented)
        }
    }

    pub fn total_syscalls(&self) -> u64 {
        self.total_syscalls.load(Ordering::Relaxed)
    }

    pub fn invalid_syscalls(&self) -> u64 {
        self.invalid_syscalls.load(Ordering::Relaxed)
    }
}

unsafe impl Send for SyscallDispatcher {}
unsafe impl Sync for SyscallDispatcher {}

// Built-in syscall handlers

/// sys_exit - Terminate current process
pub fn sys_exit(args: SyscallArgs, _caller: TaskId) -> SyscallResult {
    let exit_code = args.arg0;
    // TODO: Terminate task with exit_code
    Ok(exit_code)
}

/// sys_getpid - Get current process ID
pub fn sys_getpid(_args: SyscallArgs, caller: TaskId) -> SyscallResult {
    Ok(caller.as_u64())
}

/// sys_sleep - Sleep for given milliseconds
pub fn sys_sleep(args: SyscallArgs, _caller: TaskId) -> SyscallResult {
    let millis = args.arg0;
    // TODO: Block task for millis milliseconds
    Ok(millis)
}

/// sys_gettime - Get current time in nanoseconds
pub fn sys_gettime(_args: SyscallArgs, _caller: TaskId) -> SyscallResult {
    // TODO: Get time from timer
    Ok(0)
}

/// sys_write - Write to file descriptor
pub fn sys_write(args: SyscallArgs, _caller: TaskId) -> SyscallResult {
    let _fd = args.arg0;
    let _buf = args.arg1 as *const u8;
    let len = args.arg2;
    
    // TODO: Validate buffer, write to fd
    // For now, just return length
    Ok(len)
}

/// sys_read - Read from file descriptor
pub fn sys_read(args: SyscallArgs, _caller: TaskId) -> SyscallResult {
    let _fd = args.arg0;
    let _buf = args.arg1 as *mut u8;
    let len = args.arg2;
    
    // TODO: Validate buffer, read from fd
    Ok(len)
}

/// Initialize syscall dispatcher with built-in handlers
pub fn init_syscalls(dispatcher: &mut SyscallDispatcher) {
    dispatcher.register(SyscallNumber::Exit, sys_exit);
    dispatcher.register(SyscallNumber::GetPid, sys_getpid);
    dispatcher.register(SyscallNumber::Sleep, sys_sleep);
    dispatcher.register(SyscallNumber::GetTime, sys_gettime);
    dispatcher.register(SyscallNumber::Write, sys_write);
    dispatcher.register(SyscallNumber::Read, sys_read);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_syscall_number() {
        assert_eq!(SyscallNumber::from_u64(0), Some(SyscallNumber::Exit));
        assert_eq!(SyscallNumber::from_u64(5), Some(SyscallNumber::GetPid));
        assert_eq!(SyscallNumber::from_u64(999), None);
    }

    #[test]
    fn test_syscall_args() {
        let mut ctx = ExceptionContext::new();
        ctx.x[0] = 1;
        ctx.x[1] = 2;
        ctx.x[2] = 3;
        
        let args = SyscallArgs::from_context(&ctx);
        assert_eq!(args.arg0, 1);
        assert_eq!(args.arg1, 2);
        assert_eq!(args.arg2, 3);
    }

    #[test]
    fn test_dispatcher_register() {
        let mut dispatcher = SyscallDispatcher::new();
        dispatcher.register(SyscallNumber::Exit, sys_exit);
        
        let mut ctx = ExceptionContext::new();
        ctx.x[8] = SyscallNumber::Exit as u64;
        ctx.x[0] = 42; // exit code
        
        let result = dispatcher.dispatch(&ctx, TaskId::new(1));
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn test_dispatcher_invalid_syscall() {
        let dispatcher = SyscallDispatcher::new();
        
        let mut ctx = ExceptionContext::new();
        ctx.x[8] = 999; // Invalid syscall
        
        let result = dispatcher.dispatch(&ctx, TaskId::new(1));
        assert!(result.is_err());
        assert_eq!(dispatcher.invalid_syscalls(), 1);
    }

    #[test]
    fn test_dispatcher_unregistered() {
        let dispatcher = SyscallDispatcher::new();
        
        let mut ctx = ExceptionContext::new();
        ctx.x[8] = SyscallNumber::Fork as u64;
        
        let result = dispatcher.dispatch(&ctx, TaskId::new(1));
        assert!(result.is_err());
    }

    #[test]
    fn test_sys_getpid() {
        let args = SyscallArgs {
            arg0: 0, arg1: 0, arg2: 0,
            arg3: 0, arg4: 0, arg5: 0,
        };
        
        let result = sys_getpid(args, TaskId::new(123));
        assert_eq!(result.unwrap(), 123);
    }

    #[test]
    fn test_init_syscalls() {
        let mut dispatcher = SyscallDispatcher::new();
        init_syscalls(&mut dispatcher);
        
        // Test that handlers are registered
        let mut ctx = ExceptionContext::new();
        ctx.x[8] = SyscallNumber::GetPid as u64;
        
        let result = dispatcher.dispatch(&ctx, TaskId::new(1));
        assert!(result.is_ok());
    }

    #[test]
    fn test_syscall_statistics() {
        let mut dispatcher = SyscallDispatcher::new();
        init_syscalls(&mut dispatcher);
        
        let mut ctx = ExceptionContext::new();
        ctx.x[8] = SyscallNumber::GetPid as u64;
        
        dispatcher.dispatch(&ctx, TaskId::new(1)).unwrap();
        dispatcher.dispatch(&ctx, TaskId::new(1)).unwrap();
        
        assert_eq!(dispatcher.total_syscalls(), 2);
    }
}
