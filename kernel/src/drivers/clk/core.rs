// SPDX-License-Identifier: MIT
//! Clock Framework Core
//!
//! Ported from Linux: `drivers/clk/clk.c` (~4500 lines C → ~1000 lines Rust)
//!
//! # Architecture
//!
//! ```text
//!  ┌──────────────────────────────────────────────────────────┐
//!  │  Clock Tree                                              │
//!  │                                                          │
//!  │   [root oscillator]  24 MHz                             │
//!  │          │                                               │
//!  │        [PLL]  → 1200 MHz                                │
//!  │          │                                               │
//!  │      [divider] /4 → 300 MHz                             │
//!  │          │                                               │
//!  │    [gate] ──► CPU clock                                  │
//!  └──────────────────────────────────────────────────────────┘
//! ```
//!
//! Each clock node is a [`ClkCore`] identified by a [`ClkId`].
//! The global [`CLK_REGISTRY`] holds up to [`MAX_CLOCKS`] entries.
//!
//! # Usage
//!
//! ```ignore
//! // Register a clock during board init:
//! clk_register(ClkCore::fixed("xo_24mhz", 24_000_000))?;
//!
//! // Consumer driver:
//! let id  = clk_get("xo_24mhz").unwrap();
//! clk_prepare(id)?;
//! clk_enable(id)?;
//! let hz = clk_get_rate(id);   // 24_000_000
//! clk_set_rate(id, 19_200_000)?;
//! clk_disable(id)?;
//! clk_unprepare(id)?;
//! clk_put(id);
//! ```

use core::sync::atomic::{AtomicU32, AtomicI32, Ordering};
use spin::Mutex;
use crate::error::KernelError;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of clocks in the global registry.
/// Modern SoCs (SD888, Exynos 2100) have 300-400 clocks.
pub const MAX_CLOCKS: usize = 512;

/// Maximum clock tree depth to prevent stack overflow.
const MAX_CLK_DEPTH: u32 = 15;

/// Maximum length of a clock name.
pub const CLK_NAME_LEN: usize = 32;

/// Sentinel: no parent.
pub const CLK_NO_PARENT: ClkId = ClkId(u32::MAX);

// ---------------------------------------------------------------------------
// ClkId — opaque handle
// ---------------------------------------------------------------------------

/// Opaque clock identifier returned by [`clk_get`].
///
/// Pass it to `clk_prepare`, `clk_enable`, `clk_set_rate`, etc.
/// Release with [`clk_put`] when done.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClkId(pub(crate) u32);

// ---------------------------------------------------------------------------
// ClkFlags — capability / state bitmask
// ---------------------------------------------------------------------------

pub mod clk_flags {
    /// Clock rate cannot be changed by software.
    pub const FIXED_RATE:      u32 = 1 << 0;
    /// Clock can gate (be stopped) independently.
    pub const IS_GATABLE:      u32 = 1 << 1;
    /// Rate change requires parent to be disabled first.
    pub const SET_RATE_PARENT: u32 = 1 << 2;
    /// Clock is critical — must never be gated.
    pub const IS_CRITICAL:     u32 = 1 << 3;
    /// Currently prepared (resources allocated).
    pub const PREPARED:        u32 = 1 << 4;
    /// Currently enabled (hardware running).
    pub const ENABLED:         u32 = 1 << 5;
}

// ---------------------------------------------------------------------------
// ClkOps — hardware operations vtable
// ---------------------------------------------------------------------------

/// Hardware operations a clock provider implements.
///
/// All methods are optional — set unused ones to `None`.
#[derive(Clone, Copy)]
pub struct ClkOps {
    /// Prepare the clock (allocate resources, power rails).
    pub prepare:   Option<fn(hw: u64) -> Result<(), ClkError>>,
    /// Undo preparation.
    pub unprepare: Option<fn(hw: u64)>,
    /// Enable the clock output in hardware.
    pub enable:    Option<fn(hw: u64) -> Result<(), ClkError>>,
    /// Disable the clock output in hardware.
    pub disable:   Option<fn(hw: u64)>,
    /// Return current rate in Hz, or 0 if unknown.
    pub get_rate:  Option<fn(hw: u64, parent_rate: u64) -> u64>,
    /// Set the rate; may round. Returns actual rate set.
    pub set_rate:  Option<fn(hw: u64, rate: u64, parent_rate: u64) -> Result<u64, ClkError>>,
    /// Round a requested rate to the nearest achievable value.
    pub round_rate: Option<fn(hw: u64, rate: u64, parent_rate: u64) -> u64>,
    /// Return index of the current parent (for mux clocks).
    pub get_parent: Option<fn(hw: u64) -> u32>,
    /// Switch to parent at `index`.
    pub set_parent: Option<fn(hw: u64, index: u32) -> Result<(), ClkError>>,
    /// Recalculate rate after a parent rate change.
    pub recalc_rate: Option<fn(hw: u64, parent_rate: u64) -> u64>,
}

impl ClkOps {
    pub const fn empty() -> Self {
        Self {
            prepare: None, unprepare: None,
            enable: None, disable: None,
            get_rate: None, set_rate: None, round_rate: None,
            get_parent: None, set_parent: None, recalc_rate: None,
        }
    }
}

// ---------------------------------------------------------------------------
// ClkCore — one node in the clock tree
// ---------------------------------------------------------------------------

/// A clock node in the global clock tree.
pub struct ClkCore {
    /// Human-readable name (null-padded to CLK_NAME_LEN).
    pub name: [u8; CLK_NAME_LEN],
    /// Hardware-specific context pointer (MMIO base, provider index, etc.).
    pub hw:   u64,
    /// Hardware operations.
    pub ops:  ClkOps,
    /// Capability and state flags (see [`clk_flags`]).
    pub flags: AtomicU32,
    /// Fixed or last-calculated rate in Hz.
    pub rate:  AtomicU64Wrapper,
    /// Index in the registry of the parent clock, or [`CLK_NO_PARENT`].
    pub parent: AtomicU32,
    /// Depth in clock tree (0 = root).
    pub depth: u32,
    /// Number of active `clk_get` references.
    pub ref_count: AtomicI32,
    /// Number of times prepare has been calls (nested).
    pub prepare_count: AtomicI32,
    /// Number of times enable has been called (nested).
    pub enable_count: AtomicI32,
}

/// Atomic u64 wrapper compatible with no_std (uses two u32 atomics on 32-bit).
pub struct AtomicU64Wrapper(AtomicU32, AtomicU32);

impl AtomicU64Wrapper {
    pub const fn new(v: u64) -> Self {
        Self(AtomicU32::new((v >> 32) as u32), AtomicU32::new(v as u32))
    }
    pub fn load(&self) -> u64 {
        let hi = self.0.load(Ordering::Acquire) as u64;
        let lo = self.1.load(Ordering::Acquire) as u64;
        (hi << 32) | lo
    }
    pub fn store(&self, v: u64) {
        self.0.store((v >> 32) as u32, Ordering::Release);
        self.1.store(v as u32,         Ordering::Release);
    }
}

impl ClkCore {
    /// Create a fixed-rate clock (no hardware ops needed).
    pub fn fixed(name: &str, rate_hz: u64) -> Self {
        let mut n = [0u8; CLK_NAME_LEN];
        let b = name.as_bytes();
        let len = b.len().min(CLK_NAME_LEN - 1);
        n[..len].copy_from_slice(&b[..len]);

        Self {
            name: n,
            hw:   0,
            ops:  ClkOps::empty(),
            flags: AtomicU32::new(clk_flags::FIXED_RATE),
            rate:  AtomicU64Wrapper::new(rate_hz),
            parent: AtomicU32::new(CLK_NO_PARENT.0),
            depth: 0,
            ref_count:     AtomicI32::new(0),
            prepare_count: AtomicI32::new(0),
            enable_count:  AtomicI32::new(0),
        }
    }

    /// Create a generic clock with ops.
    pub fn new(name: &str, hw: u64, ops: ClkOps, parent: ClkId, flags: u32) -> Self {
        let mut n = [0u8; CLK_NAME_LEN];
        let b = name.as_bytes();
        let len = b.len().min(CLK_NAME_LEN - 1);
        n[..len].copy_from_slice(&b[..len]);

        Self {
            name: n,
            hw,
            ops,
            flags: AtomicU32::new(flags),
            rate:  AtomicU64Wrapper::new(0),
            parent: AtomicU32::new(parent.0),
            depth: 0,
            ref_count:     AtomicI32::new(0),
            prepare_count: AtomicI32::new(0),
            enable_count:  AtomicI32::new(0),
        }
    }

    pub fn name_str(&self) -> &str {
        let end = self.name.iter().position(|&b| b == 0).unwrap_or(CLK_NAME_LEN);
        core::str::from_utf8(&self.name[..end]).unwrap_or("<bad>")
    }
}

// ---------------------------------------------------------------------------
// Global clock registry
// ---------------------------------------------------------------------------

pub(crate) struct ClkRegistry {
    pub(crate) clocks: [Option<ClkCore>; MAX_CLOCKS],
    count:  usize,
}

impl ClkRegistry {
    const fn new() -> Self {
        const NONE: Option<ClkCore> = None;
        Self {
            clocks: [NONE; MAX_CLOCKS],
            count: 0,
        }
    }
}

pub(crate) static CLK_REGISTRY: Mutex<ClkRegistry> = Mutex::new(ClkRegistry::new());

// ---------------------------------------------------------------------------
// Public API — registration
// ---------------------------------------------------------------------------

/// Register a new clock into the global tree.
///
/// Returns a [`ClkId`] on success, or an error if the registry is full.
///
/// Ported from: `clk_register()` / `__clk_core_init()`
pub fn clk_register(mut clk: ClkCore) -> Result<ClkId, ClkError> {
    let mut reg = CLK_REGISTRY.lock();
    if reg.count >= MAX_CLOCKS {
        return Err(ClkError::RegistryFull);
    }
    
    // Calculate depth and validate tree depth
    let parent_id = ClkId(clk.parent.load(Ordering::Relaxed));
    if parent_id != CLK_NO_PARENT {
        let parent_depth = reg.clocks[parent_id.0 as usize]
            .as_ref()
            .map(|p| p.depth)
            .ok_or(ClkError::NoParent)?;
        clk.depth = parent_depth + 1;
        if clk.depth > MAX_CLK_DEPTH {
            return Err(ClkError::TreeTooDeep);
        }
    }
    
    // Pre-compute parent rate before the mutable loop to avoid borrow conflict.
    let parent_rate_init = if parent_id != CLK_NO_PARENT {
        reg.clocks[parent_id.0 as usize]
            .as_ref()
            .map(|p| p.rate.load())
            .unwrap_or(0)
    } else { 0 };

    // Find first free slot.
    for (i, slot) in reg.clocks.iter_mut().enumerate() {
        if slot.is_none() {
            // Initial rate calculation for non-fixed clocks.
            if clk.flags.load(Ordering::Relaxed) & clk_flags::FIXED_RATE == 0 {
                if let Some(recalc) = clk.ops.recalc_rate {
                    let r = recalc(clk.hw, parent_rate_init);
                    clk.rate.store(r);
                }
            }
            *slot = Some(clk);
            reg.count += 1;
            return Ok(ClkId(i as u32));
        }
    }
    Err(ClkError::RegistryFull)
}

/// Unregister a clock.  Must not be called while the clock is enabled.
///
/// Ported from: `clk_unregister()`
pub fn clk_unregister(id: ClkId) -> Result<(), ClkError> {
    let mut reg = CLK_REGISTRY.lock();
    let idx = id.0 as usize;
    if idx >= MAX_CLOCKS { return Err(ClkError::InvalidId); }
    match &reg.clocks[idx] {
        None => Err(ClkError::NotFound),
        Some(c) => {
            if c.enable_count.load(Ordering::Relaxed) > 0 {
                return Err(ClkError::StillEnabled);
            }
            reg.clocks[idx] = None;
            reg.count -= 1;
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// Public API — consumer interface
// ---------------------------------------------------------------------------

/// Look up a clock by name and take a reference.
///
/// Ported from: `clk_get()` / `__clk_get()`
pub fn clk_get(name: &str) -> Option<ClkId> {
    let reg = CLK_REGISTRY.lock();
    for (i, slot) in reg.clocks.iter().enumerate() {
        if let Some(c) = slot {
            if c.name_str() == name {
                c.ref_count.fetch_add(1, Ordering::Relaxed);
                return Some(ClkId(i as u32));
            }
        }
    }
    None
}

/// Release a clock reference obtained with [`clk_get`].
///
/// Ported from: `clk_put()`
pub fn clk_put(id: ClkId) {
    let reg = CLK_REGISTRY.lock();
    if let Some(c) = reg.clocks.get(id.0 as usize).and_then(|s| s.as_ref()) {
        let prev = c.ref_count.fetch_sub(1, Ordering::Relaxed);
        debug_assert!(prev > 0, "clk_put: ref_count underflow for {}", c.name_str());
    }
}

/// Prepare a clock for use (allocate resources, power rails).
/// May be called from process context only.  Nestable.
///
/// Ported from: `clk_prepare()`
pub fn clk_prepare(id: ClkId) -> Result<(), ClkError> {
    // Build parent chain iteratively to avoid stack overflow
    let mut chain = [CLK_NO_PARENT; MAX_CLK_DEPTH as usize + 1];
    let mut depth = 0;
    
    {
        let reg = CLK_REGISTRY.lock();
        let mut current = id;
        while current != CLK_NO_PARENT && depth <= MAX_CLK_DEPTH as usize {
            chain[depth] = current;
            depth += 1;
            current = reg.clocks[current.0 as usize]
                .as_ref()
                .map(|c| ClkId(c.parent.load(Ordering::Relaxed)))
                .unwrap_or(CLK_NO_PARENT);
        }
    }
    
    // Prepare from root to leaf
    for i in (0..depth).rev() {
        let cid = chain[i];
        let reg = CLK_REGISTRY.lock();
        let clk = reg_get(&reg, cid)?;
        let prev = clk.prepare_count.fetch_add(1, Ordering::Relaxed);
        if prev == 0 {
            if let Some(prepare_fn) = clk.ops.prepare {
                let hw = clk.hw;
                drop(reg);
                prepare_fn(hw)?;
            } else {
                drop(reg);
            }
            let reg = CLK_REGISTRY.lock();
            let clk = reg_get(&reg, cid)?;
            clk.flags.fetch_or(clk_flags::PREPARED, Ordering::Relaxed);
        }
    }
    Ok(())
}

/// Undo `clk_prepare`.  Nestable — must match `clk_prepare` call count.
///
/// Ported from: `clk_unprepare()`
pub fn clk_unprepare(id: ClkId) {
    // Build parent chain iteratively
    let mut chain = [CLK_NO_PARENT; MAX_CLK_DEPTH as usize + 1];
    let mut depth = 0;
    
    {
        let reg = CLK_REGISTRY.lock();
        let mut current = id;
        while current != CLK_NO_PARENT && depth <= MAX_CLK_DEPTH as usize {
            chain[depth] = current;
            depth += 1;
            current = reg.clocks[current.0 as usize]
                .as_ref()
                .map(|c| ClkId(c.parent.load(Ordering::Relaxed)))
                .unwrap_or(CLK_NO_PARENT);
        }
    }
    
    // Unprepare from leaf to root
    for i in 0..depth {
        let cid = chain[i];
        let reg = CLK_REGISTRY.lock();
        if let Some(c) = reg.clocks.get(cid.0 as usize).and_then(|s| s.as_ref()) {
            let prev = c.prepare_count.fetch_sub(1, Ordering::Relaxed);
            if prev == 1 {
                if let Some(f) = c.ops.unprepare {
                    let hw = c.hw;
                    drop(reg);
                    f(hw);
                } else {
                    drop(reg);
                }
                let reg = CLK_REGISTRY.lock();
                if let Some(c) = reg.clocks.get(cid.0 as usize).and_then(|s| s.as_ref()) {
                    c.flags.fetch_and(!clk_flags::PREPARED, Ordering::Relaxed);
                }
            }
        }
    }
}

/// Enable the clock output in hardware.  Must call `clk_prepare` first.
/// May be called from atomic context.  Nestable.
///
/// Ported from: `clk_enable()`
pub fn clk_enable(id: ClkId) -> Result<(), ClkError> {
    // Build parent chain iteratively
    let mut chain = [CLK_NO_PARENT; MAX_CLK_DEPTH as usize + 1];
    let mut depth = 0;
    
    {
        let reg = CLK_REGISTRY.lock();
        let clk = reg_get(&reg, id)?;
        if clk.prepare_count.load(Ordering::Relaxed) == 0 {
            return Err(ClkError::NotPrepared);
        }
        
        let mut current = id;
        while current != CLK_NO_PARENT && depth <= MAX_CLK_DEPTH as usize {
            chain[depth] = current;
            depth += 1;
            current = reg.clocks[current.0 as usize]
                .as_ref()
                .map(|c| ClkId(c.parent.load(Ordering::Relaxed)))
                .unwrap_or(CLK_NO_PARENT);
        }
    }
    
    // Enable from root to leaf
    for i in (0..depth).rev() {
        let cid = chain[i];
        let reg = CLK_REGISTRY.lock();
        let clk = reg_get(&reg, cid)?;
        let prev = clk.enable_count.fetch_add(1, Ordering::Relaxed);
        if prev == 0 {
            if let Some(enable_fn) = clk.ops.enable {
                let hw = clk.hw;
                drop(reg);
                enable_fn(hw)?;
            } else {
                drop(reg);
            }
            let reg = CLK_REGISTRY.lock();
            let clk = reg_get(&reg, cid)?;
            clk.flags.fetch_or(clk_flags::ENABLED, Ordering::Relaxed);
        }
    }
    Ok(())
}

/// Disable the clock output.  Nestable — must match `clk_enable` call count.
///
/// Ported from: `clk_disable()`
pub fn clk_disable(id: ClkId) {
    // Build parent chain iteratively
    let mut chain = [CLK_NO_PARENT; MAX_CLK_DEPTH as usize + 1];
    let mut depth = 0;
    
    {
        let reg = CLK_REGISTRY.lock();
        let mut current = id;
        while current != CLK_NO_PARENT && depth <= MAX_CLK_DEPTH as usize {
            chain[depth] = current;
            depth += 1;
            current = reg.clocks[current.0 as usize]
                .as_ref()
                .map(|c| ClkId(c.parent.load(Ordering::Relaxed)))
                .unwrap_or(CLK_NO_PARENT);
        }
    }
    
    // Disable from leaf to root
    for i in 0..depth {
        let cid = chain[i];
        let reg = CLK_REGISTRY.lock();
        if let Some(c) = reg.clocks.get(cid.0 as usize).and_then(|s| s.as_ref()) {
            if c.flags.load(Ordering::Relaxed) & clk_flags::IS_CRITICAL != 0 {
                continue;
            }
            let prev = c.enable_count.fetch_sub(1, Ordering::Relaxed);
            if prev == 1 {
                if let Some(f) = c.ops.disable {
                    let hw = c.hw;
                    drop(reg);
                    f(hw);
                } else {
                    drop(reg);
                }
                let reg = CLK_REGISTRY.lock();
                if let Some(c) = reg.clocks.get(cid.0 as usize).and_then(|s| s.as_ref()) {
                    c.flags.fetch_and(!clk_flags::ENABLED, Ordering::Relaxed);
                }
            }
        }
    }
}

/// Return the current rate of the clock in Hz.
///
/// Ported from: `clk_get_rate()`
pub fn clk_get_rate(id: ClkId) -> u64 {
    let reg = CLK_REGISTRY.lock();
    reg.clocks
        .get(id.0 as usize)
        .and_then(|s| s.as_ref())
        .map(|c| {
            if let Some(get_rate) = c.ops.get_rate {
                let parent_id = ClkId(c.parent.load(Ordering::Relaxed));
                let parent_rate = parent_rate(&reg, parent_id);
                get_rate(c.hw, parent_rate)
            } else {
                c.rate.load()
            }
        })
        .unwrap_or(0)
}

/// Request a new rate for the clock.  The framework may round to the
/// nearest achievable rate and propagate the change up/down the tree.
///
/// Ported from: `clk_set_rate()`
pub fn clk_set_rate(id: ClkId, rate_hz: u64) -> Result<u64, ClkError> {
    let reg = CLK_REGISTRY.lock();
    let clk = reg_get(&reg, id)?;

    // Fixed-rate clocks reject rate changes.
    if clk.flags.load(Ordering::Relaxed) & clk_flags::FIXED_RATE != 0 {
        return Err(ClkError::FixedRate);
    }

    let parent_id = ClkId(clk.parent.load(Ordering::Relaxed));
    let p_rate = parent_rate(&reg, parent_id);
    let hw     = clk.hw;
    let ops    = clk.ops;

    // Round the rate first.
    let actual = if let Some(round) = ops.round_rate {
        round(hw, rate_hz, p_rate)
    } else {
        rate_hz
    };

    // Apply via set_rate op, or store directly if no op.
    let set = if let Some(set_fn) = ops.set_rate {
        drop(reg);
        let result = set_fn(hw, actual, p_rate)?;
        let reg = CLK_REGISTRY.lock();
        let clk = reg_get(&reg, id)?;
        clk.rate.store(result);
        result
    } else {
        clk.rate.store(actual);
        actual
    };

    // Propagate rate change to all children
    clk_propagate_rate(id, set);
    
    Ok(set)
}

/// Propagate rate change to all children recursively.
fn clk_propagate_rate(parent_id: ClkId, new_parent_rate: u64) {
    // Collect all children first to avoid holding lock during recursion
    let mut children = [(ClkId(0), 0u64); 32]; // Max 32 children per clock
    let mut child_count = 0;
    
    {
        let reg = CLK_REGISTRY.lock();
        for (i, slot) in reg.clocks.iter().enumerate() {
            if let Some(child) = slot {
                let child_parent = ClkId(child.parent.load(Ordering::Relaxed));
                if child_parent == parent_id && child_count < 32 {
                    let child_id = ClkId(i as u32);
                    
                    // Recalculate child rate
                    let new_rate = if let Some(recalc) = child.ops.recalc_rate {
                        recalc(child.hw, new_parent_rate)
                    } else {
                        child.rate.load()
                    };
                    
                    child.rate.store(new_rate);
                    children[child_count] = (child_id, new_rate);
                    child_count += 1;
                }
            }
        }
    }
    
    // Recursively propagate to all children
    for i in 0..child_count {
        let (child_id, child_rate) = children[i];
        clk_propagate_rate(child_id, child_rate);
    }
}

/// Round `rate_hz` to the nearest rate this clock can produce.
///
/// Ported from: `clk_round_rate()`
pub fn clk_round_rate(id: ClkId, rate_hz: u64) -> u64 {
    let reg = CLK_REGISTRY.lock();
    if let Some(c) = reg.clocks.get(id.0 as usize).and_then(|s| s.as_ref()) {
        if let Some(round) = c.ops.round_rate {
            let p = parent_rate(&reg, ClkId(c.parent.load(Ordering::Relaxed)));
            return round(c.hw, rate_hz, p);
        }
    }
    rate_hz
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn reg_get<'a>(reg: &'a ClkRegistry, id: ClkId) -> Result<&'a ClkCore, ClkError> {
    reg.clocks
        .get(id.0 as usize)
        .and_then(|s| s.as_ref())
        .ok_or(ClkError::NotFound)
}

fn parent_rate(reg: &ClkRegistry, parent: ClkId) -> u64 {
    if parent == CLK_NO_PARENT { return 0; }
    reg.clocks
        .get(parent.0 as usize)
        .and_then(|s| s.as_ref())
        .map(|c| c.rate.load())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClkError {
    /// Registry is full (`MAX_CLOCKS` reached).
    RegistryFull,
    /// Clock ID is out of range.
    InvalidId,
    /// Clock not found in the registry.
    NotFound,
    /// Rate cannot be changed (fixed-rate clock).
    FixedRate,
    /// `clk_enable` called before `clk_prepare`.
    NotPrepared,
    /// `clk_unregister` called while the clock is still enabled.
    StillEnabled,
    /// Hardware operation returned an error.
    HwError,
    /// Parent clock not found.
    NoParent,
    /// Rate is out of the clock's supported range.
    RateOutOfRange,
    /// Clock tree depth exceeds MAX_CLK_DEPTH.
    TreeTooDeep,
}

impl From<ClkError> for KernelError {
    fn from(_e: ClkError) -> Self {
        KernelError::Device(crate::error::DeviceError::HardwareError)
    }
}
