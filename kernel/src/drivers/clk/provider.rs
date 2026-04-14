// SPDX-License-Identifier: MIT
//! Clock Providers
//!
//! Ported from Linux: `drivers/clk/clk-provider.c` (~1200 lines C → ~600 lines Rust)
//!
//! Provides five standard clock types that cover the vast majority of SoC
//! clock trees without needing custom drivers:
//!
//! | Type         | Linux equiv         | Description                        |
//! |---|---|---|
//! | Fixed Rate   | `clk_fixed_rate`    | Constant Hz, no HW control         |
//! | Fixed Factor | `clk_fixed_factor`  | Parent × mult / div (PLL output)   |
//! | Gate         | `clk_gate`          | Enable/disable one MMIO bit        |
//! | Divider      | `clk_divider`       | Configurable integer divider       |
//! | Mux          | `clk_mux`           | Select one of N parent clocks      |

use super::mmio::{read_reg, rmw_reg};
use spin::Mutex;

use super::core::{
    ClkCore, ClkOps, ClkId, ClkError, CLK_NO_PARENT,
    clk_flags, clk_register,
};

// ---------------------------------------------------------------------------
// 1. Fixed Rate Clock
// ---------------------------------------------------------------------------
//
// Rate is constant; no hardware access required.
// Ported from: `clk_register_fixed_rate()`

/// Register a fixed-rate clock.
///
/// ```ignore
/// let xo = clk_register_fixed_rate("xo_24mhz", 24_000_000)?;
/// ```
pub fn clk_register_fixed_rate(name: &str, rate_hz: u64) -> Result<ClkId, ClkError> {
    clk_register(ClkCore::fixed(name, rate_hz))
}

// ---------------------------------------------------------------------------
// 2. Fixed Factor Clock
// ---------------------------------------------------------------------------
//
// Output = parent_rate * mult / div.  Commonly used for PLL outputs.
// Ported from: `clk_register_fixed_factor()`

/// State stored in the `hw` field (packed into u64).
#[repr(C)]
#[derive(Clone, Copy)]
struct FixedFactorHw {
    mult: u32,
    div:  u32,
}

impl Default for FixedFactorHw {
    fn default() -> Self {
        Self { mult: 1, div: 1 }
    }
}

fn fixed_factor_get_rate(hw: u64, parent_rate: u64) -> u64 {
    let idx = hw as usize;
    let (mult, div) = {
        let table = FF_HW_TABLE.lock();
        let ff = &table[idx];
        (ff.mult, ff.div)
    };
    if div == 0 { return 0; }
    parent_rate * mult as u64 / div as u64
}

fn fixed_factor_recalc(hw: u64, parent_rate: u64) -> u64 {
    fixed_factor_get_rate(hw, parent_rate)
}

/// Static table of FixedFactorHw descriptors (no heap).
const MAX_FF_CLOCKS: usize = 32;
static FF_HW_TABLE: Mutex<[FixedFactorHw; MAX_FF_CLOCKS]> =
    Mutex::new([FixedFactorHw { mult: 1, div: 1 }; MAX_FF_CLOCKS]);
static FF_COUNT: core::sync::atomic::AtomicUsize =
    core::sync::atomic::AtomicUsize::new(0);

/// Register a fixed-factor clock: `output = parent × mult / div`.
///
/// ```ignore
/// // PLL: 24 MHz × 50 / 1 = 1200 MHz
/// let pll = clk_register_fixed_factor("pll0", xo_id, 50, 1)?;
/// ```
pub fn clk_register_fixed_factor(
    name:   &str,
    parent: ClkId,
    mult:   u32,
    div:    u32,
) -> Result<ClkId, ClkError> {
    if div == 0 { return Err(ClkError::RateOutOfRange); }

    let idx = FF_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    if idx >= MAX_FF_CLOCKS { return Err(ClkError::RegistryFull); }

    let mut table = FF_HW_TABLE.lock();
    table[idx] = FixedFactorHw { mult, div };
    drop(table);

    const OPS: ClkOps = ClkOps {
        get_rate:   Some(fixed_factor_get_rate),
        recalc_rate: Some(fixed_factor_recalc),
        round_rate: Some(|hw, _rate, parent| fixed_factor_get_rate(hw, parent)),
        prepare: None, unprepare: None,
        enable: None, disable: None,
        set_rate: None, get_parent: None, set_parent: None,
    };

    clk_register(ClkCore::new(name, idx as u64, OPS, parent, 0))
}

// ---------------------------------------------------------------------------
// 3. Gate Clock
// ---------------------------------------------------------------------------
//
// Controls a single bit in an MMIO register to enable/disable the clock.
// Ported from: `clk_register_gate()`

#[repr(C)]
#[derive(Clone, Copy)]
struct GateHw {
    /// MMIO register address.
    reg:     u64,
    /// Bit index within the register (0–31).
    bit:     u8,
    /// If true, writing 0 enables the clock (inverted polarity).
    inverted: bool,
}

impl Default for GateHw {
    fn default() -> Self {
        Self { reg: 0, bit: 0, inverted: false }
    }
}

fn gate_enable(hw: u64) -> Result<(), ClkError> {
    let idx = hw as usize;
    let table = GATE_HW_TABLE.lock();
    let g = &table[idx];
    let mask = 1u32 << g.bit;
    let val = if g.inverted { 0 } else { mask };
    let reg = g.reg;
    drop(table);
    rmw_reg(reg, mask, val);
    Ok(())
}

fn gate_disable(hw: u64) {
    let idx = hw as usize;
    let table = GATE_HW_TABLE.lock();
    let g = &table[idx];
    let mask = 1u32 << g.bit;
    let val = if g.inverted { mask } else { 0 };
    let reg = g.reg;
    drop(table);
    rmw_reg(reg, mask, val);
}

const MAX_GATE_CLOCKS: usize = 64;
static GATE_HW_TABLE: Mutex<[GateHw; MAX_GATE_CLOCKS]> =
    Mutex::new([GateHw { reg: 0, bit: 0, inverted: false }; MAX_GATE_CLOCKS]);
static GATE_COUNT: core::sync::atomic::AtomicUsize =
    core::sync::atomic::AtomicUsize::new(0);

/// Register a gate clock.
///
/// ```ignore
/// // Enable bit 3 of GCC_BLSP1_UART2_APPS_CBCR to gate UART2 clock.
/// let uart2_clk = clk_register_gate("uart2", uart2_src, 0x0C1C0, 3, false)?;
/// ```
pub fn clk_register_gate(
    name:     &str,
    parent:   ClkId,
    reg_addr: u64,
    bit:      u8,
    inverted: bool,
) -> Result<ClkId, ClkError> {
    let idx = GATE_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    if idx >= MAX_GATE_CLOCKS { return Err(ClkError::RegistryFull); }

    let mut table = GATE_HW_TABLE.lock();
    table[idx] = GateHw { reg: reg_addr, bit, inverted };
    drop(table);

    const OPS: ClkOps = ClkOps {
        enable:  Some(gate_enable),
        disable: Some(gate_disable),
        prepare: None, unprepare: None,
        get_rate: None, set_rate: None, round_rate: None,
        get_parent: None, set_parent: None, recalc_rate: None,
    };

    clk_register(ClkCore::new(
        name, idx as u64, OPS, parent,
        clk_flags::IS_GATABLE,
    ))
}

// ---------------------------------------------------------------------------
// 4. Divider Clock
// ---------------------------------------------------------------------------
//
// Integer divider: output = parent_rate / div_value.
// div_value is read from/written to a bit-field in an MMIO register.
// Ported from: `clk_register_divider()`

/// Maximum supported divider table size (for table-based dividers).
pub const MAX_DIV_TABLE: usize = 32;

/// Divider type.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DividerType {
    /// Linear: register value N → divisor N+1.
    Linear,
    /// Power-of-two: register value N → divisor 2^N.
    PowerOfTwo,
    /// Table lookup: register value N → divisor from a provided table.
    Table,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct DividerHw {
    reg:      u64,
    /// LSB of the divider field in the register.
    shift:    u8,
    /// Number of bits in the divider field.
    width:    u8,
    div_type: DividerType,
    /// For Table dividers: (register_value, divisor) pairs.
    table:    [(u8, u32); MAX_DIV_TABLE],
    table_len: usize,
}

impl Default for DividerHw {
    fn default() -> Self {
        Self {
            reg: 0, shift: 0, width: 0,
            div_type: DividerType::Linear,
            table: [(0, 0); MAX_DIV_TABLE],
            table_len: 0,
        }
    }
}

fn divider_get_div(idx: usize) -> u32 {
    let table = DIV_HW_TABLE.lock();
    let hw = &table[idx];
    let mask = (1u32 << hw.width) - 1;
    let raw  = read_reg(hw.reg);
    let val  = (raw >> hw.shift) & mask;

    match hw.div_type {
        DividerType::Linear     => val + 1,
        DividerType::PowerOfTwo => 1u32 << val,
        DividerType::Table      => {
            for &(v, d) in &hw.table[..hw.table_len] {
                if v as u32 == val { return d; }
            }
            1 // fallback
        }
    }
}

fn divider_recalc(hw: u64, parent_rate: u64) -> u64 {
    let div = divider_get_div(hw as usize);
    if div == 0 { return 0; }
    parent_rate / div as u64
}

fn divider_round_rate(hw: u64, rate: u64, parent_rate: u64) -> u64 {
    let idx = hw as usize;
    let table = DIV_HW_TABLE.lock();
    let d = &table[idx];
    if parent_rate == 0 || rate == 0 { return 0; }
    let best_div = (parent_rate / rate).max(1).min((1u64 << d.width));
    parent_rate / best_div
}

fn divider_set_rate(hw: u64, rate: u64, parent_rate: u64) -> Result<u64, ClkError> {
    let idx = hw as usize;
    let table = DIV_HW_TABLE.lock();
    let d = &table[idx];
    if parent_rate == 0 || rate == 0 { return Err(ClkError::RateOutOfRange); }

    let div = (parent_rate / rate).max(1).min((1u64 << d.width)) as u32;
    let mask = ((1u32 << d.width) - 1) << d.shift;
    let val_bits = match d.div_type {
        DividerType::Linear     => (div - 1) << d.shift,
        DividerType::PowerOfTwo => (div.trailing_zeros()) << d.shift,
        DividerType::Table      => {
            let mut found = 0u32;
            for &(v, dv) in &d.table[..d.table_len] {
                if dv == div { found = v as u32; break; }
            }
            found << d.shift
        }
    };
    let reg = d.reg;
    drop(table);

    rmw_reg(reg, mask, val_bits);
    Ok(parent_rate / div as u64)
}

const MAX_DIV_CLOCKS: usize = 64;
static DIV_HW_TABLE: Mutex<[DividerHw; MAX_DIV_CLOCKS]> =
    Mutex::new([DividerHw {
        reg: 0, shift: 0, width: 0,
        div_type: DividerType::Linear,
        table: [(0, 0); MAX_DIV_TABLE],
        table_len: 0,
    }; MAX_DIV_CLOCKS]);
static DIV_COUNT: core::sync::atomic::AtomicUsize =
    core::sync::atomic::AtomicUsize::new(0);

/// Register a divider clock.
///
/// ```ignore
/// // 4-bit linear divider at offset 0x1234, bits [3:0].
/// let div = clk_register_divider("cpu_div", pll_id, 0x1234, 0, 4,
///                                DividerType::Linear)?;
/// ```
pub fn clk_register_divider(
    name:     &str,
    parent:   ClkId,
    reg_addr: u64,
    shift:    u8,
    width:    u8,
    div_type: DividerType,
) -> Result<ClkId, ClkError> {
    let idx = DIV_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    if idx >= MAX_DIV_CLOCKS { return Err(ClkError::RegistryFull); }

    let mut table = DIV_HW_TABLE.lock();
    table[idx] = DividerHw {
        reg: reg_addr, shift, width, div_type,
        table: [(0, 0); MAX_DIV_TABLE],
        table_len: 0,
    };
    drop(table);

    const OPS: ClkOps = ClkOps {
        recalc_rate: Some(divider_recalc),
        round_rate:  Some(divider_round_rate),
        set_rate:    Some(divider_set_rate),
        prepare: None, unprepare: None,
        enable: None, disable: None,
        get_rate: None, get_parent: None, set_parent: None,
    };

    clk_register(ClkCore::new(name, idx as u64, OPS, parent, 0))
}

// ---------------------------------------------------------------------------
// 5. Mux Clock
// ---------------------------------------------------------------------------
//
// Selects one of N parent clocks via a bit-field in an MMIO register.
// Ported from: `clk_register_mux()`

/// Maximum parents a Mux clock can have.
pub const MAX_MUX_PARENTS: usize = 8;

#[repr(C)]
#[derive(Clone, Copy)]
struct MuxHw {
    reg:        u64,
    shift:      u8,
    width:      u8,
    /// ClkIds of all possible parents, in hardware index order.
    parents:    [ClkId; MAX_MUX_PARENTS],
    parent_cnt: usize,
}

impl Default for MuxHw {
    fn default() -> Self {
        Self {
            reg: 0, shift: 0, width: 0,
            parents: [CLK_NO_PARENT; MAX_MUX_PARENTS],
            parent_cnt: 0,
        }
    }
}

fn mux_get_parent(hw: u64) -> u32 {
    let idx = hw as usize;
    let table = MUX_HW_TABLE.lock();
    let m = &table[idx];
    let mask = (1u32 << m.width) - 1;
    let raw  = read_reg(m.reg);
    (raw >> m.shift) & mask
}

fn mux_set_parent(hw: u64, index: u32) -> Result<(), ClkError> {
    let idx = hw as usize;
    let table = MUX_HW_TABLE.lock();
    let m = &table[idx];
    if index as usize >= m.parent_cnt { return Err(ClkError::NoParent); }
    let mask = ((1u32 << m.width) - 1) << m.shift;
    let val = index << m.shift;
    let reg = m.reg;
    drop(table);
    rmw_reg(reg, mask, val);
    Ok(())
}

const MAX_MUX_CLOCKS: usize = 32;
static MUX_HW_TABLE: Mutex<[MuxHw; MAX_MUX_CLOCKS]> =
    Mutex::new([MuxHw {
        reg: 0, shift: 0, width: 0,
        parents: [CLK_NO_PARENT; MAX_MUX_PARENTS],
        parent_cnt: 0,
    }; MAX_MUX_CLOCKS]);
static MUX_COUNT: core::sync::atomic::AtomicUsize =
    core::sync::atomic::AtomicUsize::new(0);

/// Register a mux clock.
///
/// `parents` lists the [`ClkId`]s in the order matching hardware index 0, 1, …
///
/// ```ignore
/// let cpu_mux = clk_register_mux(
///     "cpu_mux", 0x1238, 0, 2,
///     &[xo_id, pll_id, gpll0_id],
/// )?;
/// ```
pub fn clk_register_mux(
    name:     &str,
    reg_addr: u64,
    shift:    u8,
    width:    u8,
    parents:  &[ClkId],
) -> Result<ClkId, ClkError> {
    if parents.is_empty() || parents.len() > MAX_MUX_PARENTS {
        return Err(ClkError::NoParent);
    }

    let idx = MUX_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    if idx >= MAX_MUX_CLOCKS { return Err(ClkError::RegistryFull); }

    let mut table = MUX_HW_TABLE.lock();
    let mut hw = MuxHw {
        reg: reg_addr, shift, width,
        parents: [CLK_NO_PARENT; MAX_MUX_PARENTS],
        parent_cnt: parents.len(),
    };
    hw.parents[..parents.len()].copy_from_slice(parents);
    table[idx] = hw;
    
    // Initial parent = whatever hardware currently selects.
    let cur_idx = mux_get_parent(idx as u64) as usize;
    let init_parent = if cur_idx < parents.len() {
        parents[cur_idx]
    } else {
        parents[0]
    };
    drop(table);

    const OPS: ClkOps = ClkOps {
        get_parent: Some(mux_get_parent),
        set_parent: Some(mux_set_parent),
        prepare: None, unprepare: None,
        enable: None, disable: None,
        get_rate: None, set_rate: None, round_rate: None, recalc_rate: None,
    };

    clk_register(ClkCore::new(name, idx as u64, OPS, init_parent, 0))
}
