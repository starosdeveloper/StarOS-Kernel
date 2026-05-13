//! TLB management and SMP shootdown
//!
//! On multi-core ARM64, TLB invalidation must be broadcast to all cores.
//! This module provides the IPI-based TLB shootdown mechanism.

use core::sync::atomic::{AtomicU64, AtomicU8, Ordering};

/// TLB shootdown request (broadcast to all cores)
#[repr(C, align(64))] // Cache-line aligned to avoid false sharing
pub struct TlbShootdown {
    /// Virtual address to invalidate (0 = flush all)
    target_addr: AtomicU64,
    /// ASID to invalidate (0 = all ASIDs)
    asid: AtomicU8,
    /// Number of cores that have completed the flush
    ack_count: AtomicU8,
    /// Number of cores that need to respond
    target_cores: AtomicU8,
    /// Request pending flag
    pending: AtomicU8,
}

static SHOOTDOWN: TlbShootdown = TlbShootdown {
    target_addr: AtomicU64::new(0),
    asid: AtomicU8::new(0),
    ack_count: AtomicU8::new(0),
    target_cores: AtomicU8::new(0),
    pending: AtomicU8::new(0),
};

/// Flush TLB for a single page on ALL cores
pub fn tlb_flush_page_all(vaddr: u64, asid: u8) {
    // Set up shootdown request
    SHOOTDOWN.target_addr.store(vaddr, Ordering::Release);
    SHOOTDOWN.asid.store(asid, Ordering::Release);
    SHOOTDOWN.ack_count.store(0, Ordering::Release);
    SHOOTDOWN.target_cores.store(num_online_cpus(), Ordering::Release);
    SHOOTDOWN.pending.store(1, Ordering::Release);

    // Send IPI to all other cores
    send_ipi_all_others();

    // Flush local TLB
    local_tlb_flush_page(vaddr, asid);
    SHOOTDOWN.ack_count.fetch_add(1, Ordering::AcqRel);

    // Wait for all cores to acknowledge
    let target = SHOOTDOWN.target_cores.load(Ordering::Acquire);
    while SHOOTDOWN.ack_count.load(Ordering::Acquire) < target {
        core::hint::spin_loop();
    }

    SHOOTDOWN.pending.store(0, Ordering::Release);
}

/// Flush entire TLB on ALL cores
pub fn tlb_flush_all() {
    tlb_flush_page_all(0, 0);
}

/// Called on each core when IPI is received
pub fn tlb_shootdown_ipi_handler() {
    if SHOOTDOWN.pending.load(Ordering::Acquire) == 0 {
        return;
    }

    let addr = SHOOTDOWN.target_addr.load(Ordering::Acquire);
    let asid = SHOOTDOWN.asid.load(Ordering::Acquire);

    if addr == 0 {
        local_tlb_flush_all();
    } else {
        local_tlb_flush_page(addr, asid);
    }

    SHOOTDOWN.ack_count.fetch_add(1, Ordering::AcqRel);
}

/// Local TLB flush for a single page
#[inline]
fn local_tlb_flush_page(vaddr: u64, asid: u8) {
    #[cfg(target_arch = "aarch64")]
    // SAFETY: TLBI instruction invalidates a single TLB entry identified by
    // the virtual address and ASID. This is a privileged but non-destructive
    // cache maintenance operation required for correct virtual memory semantics.
    unsafe {
        let val = (vaddr >> 12) | ((asid as u64) << 48);
        core::arch::asm!(
            "tlbi vale1is, {0}",
            "dsb ish",
            "isb",
            in(reg) val,
            options(nostack)
        );
    }
}

/// Local full TLB flush
#[inline]
fn local_tlb_flush_all() {
    #[cfg(target_arch = "aarch64")]
    // SAFETY: TLBI vmalle1is invalidates all TLB entries for EL1.
    // This is a privileged cache maintenance operation that is always safe
    // to execute from kernel context (correctness impact only, not memory safety).
    unsafe {
        core::arch::asm!(
            "tlbi vmalle1is",
            "dsb ish",
            "isb",
            options(nostack)
        );
    }
}

/// Send IPI to all other cores via GIC
fn send_ipi_all_others() {
    #[cfg(target_arch = "aarch64")]
    // SAFETY: Writing ICC_SGI1R_EL1 sends a Software Generated Interrupt
    // (SGI #1) to all other PEs via GICv3. IRM=1 targets all other cores.
    // This requires EL1 privilege and an initialized GIC.
    unsafe {
        // GICv3: write to ICC_SGI1R_EL1 to send SGI to all other PEs
        // IRM=1 (all other), INTID=1 (TLB shootdown SGI)
        let sgi_val: u64 = (1u64 << 40) | 1; // IRM=1, INTID=1
        core::arch::asm!(
            "msr ICC_SGI1R_EL1, {0}",
            "isb",
            in(reg) sgi_val,
            options(nostack)
        );
    }
}

fn num_online_cpus() -> u8 {
    // Read from kernel state; default to 1 for safety
    1 // TODO: read from actual CPU topology
}
