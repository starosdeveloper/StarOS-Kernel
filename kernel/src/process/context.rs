//! Context switching for ARM64
//!
//! Low-level context save/restore operations

use super::task::Context;

/// Save current context and switch to new context
/// 
/// # Safety
/// Must be called with interrupts disabled
/// Both contexts must be valid
#[inline(never)]
pub unsafe fn switch_context(old: *mut Context, new: *const Context) {
    // This will be implemented in assembly
    // For now, inline assembly placeholder
    
    #[cfg(target_arch = "aarch64")]
    core::arch::asm!(
        // Save old context
        "stp x0, x1, [{old}, #0]",
        "stp x2, x3, [{old}, #16]",
        "stp x4, x5, [{old}, #32]",
        "stp x6, x7, [{old}, #48]",
        "stp x8, x9, [{old}, #64]",
        "stp x10, x11, [{old}, #80]",
        "stp x12, x13, [{old}, #96]",
        "stp x14, x15, [{old}, #112]",
        "stp x16, x17, [{old}, #128]",
        "stp x18, x19, [{old}, #144]",
        "stp x20, x21, [{old}, #160]",
        "stp x22, x23, [{old}, #176]",
        "stp x24, x25, [{old}, #192]",
        "stp x26, x27, [{old}, #208]",
        "stp x28, x29, [{old}, #224]",
        "str x30, [{old}, #240]",
        
        // Save SP and PC
        "mov x9, sp",
        "str x9, [{old}, #248]",
        "adr x9, 1f",
        "str x9, [{old}, #256]",
        
        // Save PSTATE
        "mrs x9, nzcv",
        "str x9, [{old}, #264]",
        
        // Save FP registers
        "stp q0, q1, [{old}, #272]",
        "stp q2, q3, [{old}, #304]",
        "stp q4, q5, [{old}, #336]",
        "stp q6, q7, [{old}, #368]",
        "stp q8, q9, [{old}, #400]",
        "stp q10, q11, [{old}, #432]",
        "stp q12, q13, [{old}, #464]",
        "stp q14, q15, [{old}, #496]",
        "stp q16, q17, [{old}, #528]",
        "stp q18, q19, [{old}, #560]",
        "stp q20, q21, [{old}, #592]",
        "stp q22, q23, [{old}, #624]",
        "stp q24, q25, [{old}, #656]",
        "stp q26, q27, [{old}, #688]",
        "stp q28, q29, [{old}, #720]",
        "stp q30, q31, [{old}, #752]",
        
        // Save FP control registers
        "mrs x9, fpcr",
        "str x9, [{old}, #784]",
        "mrs x9, fpsr",
        "str x9, [{old}, #792]",
        
        // Restore new context
        "ldp x0, x1, [{new}, #0]",
        "ldp x2, x3, [{new}, #16]",
        "ldp x4, x5, [{new}, #32]",
        "ldp x6, x7, [{new}, #48]",
        "ldp x8, x9, [{new}, #64]",
        "ldp x10, x11, [{new}, #80]",
        "ldp x12, x13, [{new}, #96]",
        "ldp x14, x15, [{new}, #112]",
        "ldp x16, x17, [{new}, #128]",
        "ldp x18, x19, [{new}, #144]",
        "ldp x20, x21, [{new}, #160]",
        "ldp x22, x23, [{new}, #176]",
        "ldp x24, x25, [{new}, #192]",
        "ldp x26, x27, [{new}, #208]",
        "ldp x28, x29, [{new}, #224]",
        "ldr x30, [{new}, #240]",
        
        // Restore SP
        "ldr x9, [{new}, #248]",
        "mov sp, x9",
        
        // Restore PSTATE
        "ldr x9, [{new}, #264]",
        "msr nzcv, x9",
        
        // Restore FP registers
        "ldp q0, q1, [{new}, #272]",
        "ldp q2, q3, [{new}, #304]",
        "ldp q4, q5, [{new}, #336]",
        "ldp q6, q7, [{new}, #368]",
        "ldp q8, q9, [{new}, #400]",
        "ldp q10, q11, [{new}, #432]",
        "ldp q12, q13, [{new}, #464]",
        "ldp q14, q15, [{new}, #496]",
        "ldp q16, q17, [{new}, #528]",
        "ldp q18, q19, [{new}, #560]",
        "ldp q20, q21, [{new}, #592]",
        "ldp q22, q23, [{new}, #624]",
        "ldp q24, q25, [{new}, #656]",
        "ldp q26, q27, [{new}, #688]",
        "ldp q28, q29, [{new}, #720]",
        "ldp q30, q31, [{new}, #752]",
        
        // Restore FP control registers
        "ldr x9, [{new}, #784]",
        "msr fpcr, x9",
        "ldr x9, [{new}, #792]",
        "msr fpsr, x9",
        
        // Jump to new PC
        "ldr x9, [{new}, #256]",
        "br x9",
        
        "1:",
        old = in(reg) old,
        new = in(reg) new,
        options(noreturn)
    );
    
    #[cfg(not(target_arch = "aarch64"))]
    {
        // For non-ARM64 targets (testing), just copy context
        if !old.is_null() && !new.is_null() {
            core::ptr::write(old, core::ptr::read(new));
        }
    }
}

/// Initialize context for first run
pub fn init_context(ctx: &mut Context, entry: u64, stack_top: u64, arg: u64) {
    ctx.init(entry, stack_top, arg);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_context() {
        let mut ctx = Context::new();
        init_context(&mut ctx, 0x1000, 0x2000, 0x42);
        
        assert_eq!(ctx.pc, 0x1000);
        assert_eq!(ctx.sp, 0x2000);
        assert_eq!(ctx.x[0], 0x42);
    }

    #[test]
    fn test_context_size() {
        use core::mem::size_of;
        
        // Verify context size for alignment
        let size = size_of::<Context>();
        assert!(size > 0);
        assert_eq!(size % 16, 0); // Must be 16-byte aligned
    }
}
