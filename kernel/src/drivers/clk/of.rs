// SPDX-License-Identifier: MIT
//! Clock Device Tree Integration
//!
//! Ported from Linux: `drivers/clk/clk-conf.c` (~200 lines C → ~400 lines Rust)
//!
//! Provides:
//! - [`of_clk_get`]          — look up a clock by Device Tree phandle/index
//! - [`of_clk_set_defaults`] — apply `assigned-clocks` / `assigned-clock-rates`
//!                             from a DT node at boot
//!
//! # DT properties consumed
//!
//! ```text
//! node {
//!     clocks = <&gcc GCC_BLSP1_UART2_APPS_CLK>;
//!     clock-names = "uart_clk";
//!
//!     assigned-clocks = <&gcc GCC_BLSP1_UART2_APPS_CLK>;
//!     assigned-clock-rates = <115200>;
//!     assigned-clock-parents = <&xo_board>;
//! };
//! ```

use super::core::{ClkId, ClkError, clk_set_rate};

// ---------------------------------------------------------------------------
// Clock provider table
// ---------------------------------------------------------------------------
//
// In Linux this is handled by a global list of `of_clk_provider` structs.
// Here we use a fixed-size static table (no alloc).

/// Maximum number of registered DT clock providers.
pub const MAX_OF_PROVIDERS: usize = 16;
/// Maximum clock-names entries per node.
pub const MAX_CLK_NAMES: usize = 16;
/// Maximum length of a clock-name string.
pub const CLK_NAME_LEN: usize = 32;
/// Maximum `assigned-clocks` entries per node.
pub const MAX_ASSIGNED_CLOCKS: usize = 8;

/// A function that resolves a (specifier_cells × u32 args) tuple to a ClkId.
///
/// The `args` slice length equals the `#clock-cells` value from the provider
/// DT node.  For simple providers with `#clock-cells = <0>` it is empty.
pub type ClkResolver = fn(args: &[u32]) -> Option<ClkId>;

/// One registered Device Tree clock provider.
#[derive(Clone, Copy)]
pub struct OfClkProvider {
    /// phandle of the provider node (set by DT parser during unflatten).
    pub phandle: u32,
    /// Function that turns clock specifier args into a ClkId.
    pub resolver: ClkResolver,
}

struct ProviderTable {
    entries: [Option<OfClkProvider>; MAX_OF_PROVIDERS],
    count:   usize,
}

impl ProviderTable {
    const fn new() -> Self {
        Self {
            entries: [None; MAX_OF_PROVIDERS],
            count: 0,
        }
    }
}

static OF_PROVIDERS: spin::Mutex<ProviderTable> =
    spin::Mutex::new(ProviderTable::new());

// ---------------------------------------------------------------------------
// Provider registration
// ---------------------------------------------------------------------------

/// Register a Device Tree clock provider.
///
/// Call this during board / SoC clock init for each clock controller node
/// (GCC, RPM, LPASS, etc.).
///
/// ```ignore
/// // gcc clock controller: #clock-cells = <1>, specifier = clock ID.
/// of_clk_add_provider(gcc_phandle, |args| {
///     gcc_clk_lookup(args.get(0).copied().unwrap_or(0))
/// })?;
/// ```
///
/// Ported from: `of_clk_add_provider()`
pub fn of_clk_add_provider(
    phandle:  u32,
    resolver: ClkResolver,
) -> Result<(), ClkError> {
    let mut tbl = OF_PROVIDERS.lock();
    if tbl.count >= MAX_OF_PROVIDERS {
        return Err(ClkError::RegistryFull);
    }
    for slot in &mut tbl.entries {
        if slot.is_none() {
            *slot = Some(OfClkProvider { phandle, resolver });
            tbl.count += 1;
            return Ok(());
        }
    }
    Err(ClkError::RegistryFull)
}

/// Unregister a DT clock provider.
///
/// Ported from: `of_clk_del_provider()`
pub fn of_clk_del_provider(phandle: u32) {
    let mut tbl = OF_PROVIDERS.lock();
    for slot in &mut tbl.entries {
        if let Some(p) = slot {
            if p.phandle == phandle {
                *slot = None;
                tbl.count -= 1;
                return;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// of_clk_get — consumer API
// ---------------------------------------------------------------------------

/// Resolve a clock from a Device Tree `clocks` specifier.
///
/// * `phandle` — phandle of the clock provider node.
/// * `args`    — clock specifier cells (e.g., `[GCC_BLSP1_UART2_APPS_CLK]`).
///
/// Returns the `ClkId` on success, or `None` if no provider is registered
/// for that phandle or the resolver returns `None`.
///
/// Ported from: `of_clk_get_from_provider()` / `__of_clk_get()`
pub fn of_clk_get(phandle: u32, args: &[u32]) -> Option<ClkId> {
    let tbl = OF_PROVIDERS.lock();
    for slot in &tbl.entries {
        if let Some(p) = slot {
            if p.phandle == phandle {
                return (p.resolver)(args);
            }
        }
    }
    None
}

/// Resolve a clock by provider phandle + single specifier index (most common).
///
/// Convenience wrapper for `#clock-cells = <1>` providers.
///
/// ```ignore
/// let uart2_clk = of_clk_get_by_index(gcc_phandle, GCC_BLSP1_UART2_APPS_CLK);
/// ```
pub fn of_clk_get_by_index(phandle: u32, index: u32) -> Option<ClkId> {
    of_clk_get(phandle, &[index])
}

/// Look up a clock by its `clock-names` entry in a parsed node.
///
/// `clock_names` is the node's `clock-names` string list.
/// `clocks_phandles` is the parallel list of (phandle, specifier) pairs.
///
/// Ported from: `of_clk_get_by_name()`
pub fn of_clk_get_by_name<'a>(
    name:             &str,
    clock_names:      &[&'a str],
    clocks_phandles:  &[(u32, &[u32])],
) -> Option<ClkId> {
    let idx = clock_names.iter().position(|&n| n == name)?;
    let (phandle, args) = clocks_phandles.get(idx)?;
    of_clk_get(*phandle, args)
}

// ---------------------------------------------------------------------------
// of_clk_set_defaults — apply assigned-clocks at boot
// ---------------------------------------------------------------------------

/// Parsed representation of a single `assigned-clocks` entry.
#[derive(Clone, Copy, Debug)]
pub struct AssignedClkEntry {
    pub phandle: u32,
    /// Clock specifier args (up to 8 cells for exotic hardware).
    pub args:    [u32; 8],
    pub arg_len: usize,
}

/// Apply `assigned-clocks` and `assigned-clock-rates` from a DT node.
///
/// Call this for every device node during probe.  It:
/// 1. Resolves each clock in `assigned_clocks`.
/// 2. If a rate is given in `rates`, calls `clk_set_rate`.
/// 3. If a parent is given in `parents`, switches the mux.
///
/// Ported from: `of_clk_set_defaults()` in `clk-conf.c`
///
/// # Arguments
/// * `assigned_clocks`  — slice from the `assigned-clocks` DT property.
/// * `rates`            — slice from `assigned-clock-rates` (0 = skip).
/// * `parent_phandles`  — slice from `assigned-clock-parents` (0 = skip).
pub fn of_clk_set_defaults(
    assigned_clocks:   &[AssignedClkEntry],
    rates:             &[u64],
    parent_phandles:   &[(u32, &[u32])],
) -> Result<(), ClkError> {
    for (i, entry) in assigned_clocks.iter().enumerate() {
        // 1. Resolve the clock.
        let id = of_clk_get(entry.phandle, &entry.args[..entry.arg_len])
            .ok_or(ClkError::NotFound)?;

        // 2. Apply rate if provided and non-zero.
        if let Some(&rate) = rates.get(i) {
            if rate > 0 {
                clk_set_rate(id, rate)?;
            }
        }

        // 3. Switch parent if provided.
        if let Some(&(pp_phandle, pp_args)) = parent_phandles.get(i) {
            if pp_phandle != 0 {
                if let Some(parent_id) = of_clk_get(pp_phandle, pp_args) {
                    // Re-parent: find current clock's mux and switch.
                    // Full implementation requires propagating through the
                    // clock tree; simplified version stores the parent directly.
                    let _ = reparent_clock(id, parent_id);
                }
            }
        }
    }
    Ok(())
}

/// Switch a clock's active parent in the registry.
///
/// In Linux this drives through `clk_core_set_parent_nolock()`.
/// Here we update the registry entry directly using atomic operations.
fn reparent_clock(id: ClkId, new_parent: ClkId) -> Result<(), ClkError> {
    use super::core::CLK_REGISTRY;
    let reg = CLK_REGISTRY.lock();
    let idx = id.0 as usize;
    if let Some(clk) = reg.clocks.get(idx).and_then(|s| s.as_ref()) {
        clk.parent.store(new_parent.0, core::sync::atomic::Ordering::Relaxed);
        Ok(())
    } else {
        Err(ClkError::NotFound)
    }
}

// ---------------------------------------------------------------------------
// Helper: parse clock specifier from a raw DT cell stream
// ---------------------------------------------------------------------------

/// Parse a `clocks` DT property cell stream into `AssignedClkEntry` slices.
///
/// The cell stream alternates: [phandle, specifier_0, …, specifier_N, …]
/// where N is determined by the provider's `#clock-cells` property.
///
/// `cells_per_clock` maps provider phandle → `#clock-cells`.
///
/// Returns the number of entries written to `out`.
pub fn parse_clocks_prop(
    cells:            &[u32],
    cells_per_clock:  &[(u32, usize)],  // (phandle → #clock-cells)
    out:              &mut [AssignedClkEntry],
) -> usize {
    let mut pos = 0usize;
    let mut n   = 0usize;

    while pos < cells.len() && n < out.len() {
        let phandle = cells[pos];
        pos += 1;

        let ncells = cells_per_clock.iter()
            .find(|&&(ph, _)| ph == phandle)
            .map(|&(_, c)| c)
            .unwrap_or(0);

        let mut args = [0u32; 8];
        let arg_len = ncells.min(8);
        for i in 0..arg_len {
            if pos < cells.len() {
                args[i] = cells[pos];
                pos += 1;
            }
        }

        out[n] = AssignedClkEntry { phandle, args, arg_len };
        n += 1;
    }

    n
}
