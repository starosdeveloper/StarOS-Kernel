// SPDX-License-Identifier: MIT
//! Clock Framework
//!
//! Three-layer architecture:
//!
//! ```text
//!  ┌─────────────────────────────────────────────────────┐
//!  │  of.rs      — Device Tree integration               │
//!  │              of_clk_get / of_clk_set_defaults       │
//!  ├─────────────────────────────────────────────────────┤
//!  │  provider.rs — Standard clock types                 │
//!  │              fixed_rate / fixed_factor / gate /     │
//!  │              divider / mux                          │
//!  ├─────────────────────────────────────────────────────┤
//!  │  core.rs    — Clock tree & consumer API             │
//!  │              clk_register / clk_get / clk_put       │
//!  │              clk_prepare / clk_enable               │
//!  │              clk_set_rate / clk_get_rate            │
//!  └─────────────────────────────────────────────────────┘
//! ```

pub mod core;
pub mod provider;
pub mod of;
pub(crate) mod mmio;

// Core types & consumer API
pub use core::{
    ClkId, ClkCore, ClkOps, ClkError,
    CLK_NO_PARENT, MAX_CLOCKS,
    clk_flags,
    clk_register, clk_unregister,
    clk_get, clk_put,
    clk_prepare, clk_unprepare,
    clk_enable, clk_disable,
    clk_get_rate, clk_set_rate, clk_round_rate,
};

// Standard providers
pub use provider::{
    DividerType,
    clk_register_fixed_rate,
    clk_register_fixed_factor,
    clk_register_gate,
    clk_register_divider,
    clk_register_mux,
};

// Device Tree integration
pub use of::{
    OfClkProvider, AssignedClkEntry,
    ClkResolver,
    of_clk_add_provider, of_clk_del_provider,
    of_clk_get, of_clk_get_by_index, of_clk_get_by_name,
    of_clk_set_defaults,
    parse_clocks_prop,
};
