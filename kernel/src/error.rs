//! Unified kernel error system
//!
//! This module provides a comprehensive error hierarchy for the entire kernel.
//! All subsystems should use these error types instead of panicking.

#[cfg(not(feature = "std"))]
use core::fmt;
#[cfg(feature = "std")]
use std::fmt;

// Explicit imports for derive macros
#[cfg(not(feature = "std"))]
use core::convert::From;
#[cfg(feature = "std")]
use std::convert::From;

/// Top-level kernel error type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KernelError {
    Memory(MemoryError),
    Interrupt(InterruptError),
    Process(ProcessError),
    Filesystem(FilesystemError),
    Network(NetworkError),
    Device(DeviceError),
    Security(SecurityError),
    NotInitialized,
    InvalidParameter(&'static str),
    InvalidAddress,
    NotFound,
    NotSupported,
    Timeout,
    ResourceExhausted,
    OperationFailed,
    NotImplemented,
}

impl fmt::Display for KernelError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Memory(e) => write!(f, "Memory error: {:?}", e),
            Self::Interrupt(e) => write!(f, "Interrupt error: {:?}", e),
            Self::Process(e) => write!(f, "Process error: {:?}", e),
            Self::Filesystem(e) => write!(f, "Filesystem error: {:?}", e),
            Self::Network(e) => write!(f, "Network error: {:?}", e),
            Self::Device(e) => write!(f, "Device error: {:?}", e),
            Self::Security(e) => write!(f, "Security error: {:?}", e),
            Self::NotInitialized => write!(f, "Not initialized"),
            Self::InvalidParameter(s) => write!(f, "Invalid parameter: {}", s),
            Self::InvalidAddress => write!(f, "Invalid address"),
            Self::NotFound => write!(f, "Not found"),
            Self::NotSupported => write!(f, "Not supported"),
            Self::Timeout => write!(f, "Timeout"),
            Self::ResourceExhausted => write!(f, "Resource exhausted"),
            Self::OperationFailed => write!(f, "Operation failed"),
            Self::NotImplemented => write!(f, "Not implemented"),
        }
    }
}

/// Memory subsystem errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryError {
    OutOfMemory,
    InvalidAddress,
    InvalidAlignment,
    PermissionDenied,
    RegionNotFound,
    AlreadyMapped,
    NotMapped,
    AllocationFailed,
    InvalidSize,
    TooManyRegions,
    DoubleFree,
}

/// Interrupt subsystem errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterruptError {
    NotInitialized,
    InvalidIrq,
    AlreadyRegistered,
    NotRegistered,
    HandlerFailed,
}

/// Process subsystem errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessError {
    InvalidProcessId,
    ProcessNotFound,
    TooManyProcesses,
    InvalidPriority,
    AlreadyTerminated,
    SpawnFailed,
}

/// Device subsystem errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceError {
    NotInitialized,
    NotReady,
    Timeout,
    IoError,
    InvalidCommand,
    HardwareError,
}

/// Security subsystem errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityError {
    PermissionDenied,
    InvalidCredentials,
    VerificationFailed,
    AccessDenied,
}

/// Filesystem errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilesystemError {
    NotFound,
    PermissionDenied,
    AlreadyExists,
    InvalidPath,
    IoError,
    CorruptedData,
}

/// Network errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkError {
    NotConnected,
    Timeout,
    InvalidAddress,
    ProtocolError,
    ConnectionRefused,
}

/// Conversion implementations
impl From<MemoryError> for KernelError {
    fn from(e: MemoryError) -> Self {
        Self::Memory(e)
    }
}

impl From<InterruptError> for KernelError {
    fn from(e: InterruptError) -> Self {
        Self::Interrupt(e)
    }
}

impl From<ProcessError> for KernelError {
    fn from(e: ProcessError) -> Self {
        Self::Process(e)
    }
}

impl From<DeviceError> for KernelError {
    fn from(e: DeviceError) -> Self {
        Self::Device(e)
    }
}

impl From<SecurityError> for KernelError {
    fn from(e: SecurityError) -> Self {
        Self::Security(e)
    }
}

impl From<FilesystemError> for KernelError {
    fn from(e: FilesystemError) -> Self {
        Self::Filesystem(e)
    }
}

impl From<NetworkError> for KernelError {
    fn from(e: NetworkError) -> Self {
        Self::Network(e)
    }
}

/// Result type alias for kernel operations
#[cfg(not(feature = "std"))]
pub type KernelResult<T> = core::result::Result<T, KernelError>;

#[cfg(feature = "std")]
pub type KernelResult<T> = std::result::Result<T, KernelError>;

// Deprecated alias
#[cfg(not(feature = "std"))]
pub type Result<T> = core::result::Result<T, KernelError>;

#[cfg(feature = "std")]
pub type Result<T> = std::result::Result<T, KernelError>;

