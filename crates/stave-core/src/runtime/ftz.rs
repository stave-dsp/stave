//! The flush-to-zero backstop around every block.

/// RAII flush-to-zero guard ("belt and
/// braces"): engaged at wrapper process entry so any recursive state a
/// future cell forgets to flush manually can't become an x86/arm
/// denormal perf cliff. Manual flushes in stave-dsp remain the primary
/// mechanism — this is the backstop. Engage/restore is a single
/// control-register write (~ns per block).
pub struct ScopedFtz {
    #[cfg(target_arch = "aarch64")]
    saved: u64,
    #[cfg(target_arch = "x86_64")]
    saved: u32,
}

impl ScopedFtz {
    #[inline]
    pub fn engage() -> Self {
        // Miri interprets floats deterministically and cannot execute
        // the FP-control-register asm; FTZ is hardware state with no
        // meaning there. No-op under the interpreter so every wrapper
        // process path stays Miri-checkable.
        // SAFETY: reads FPCR and sets only its FZ bit — no memory, no
        // stack; `Drop` restores the saved value on this thread.
        #[cfg(all(target_arch = "aarch64", not(miri)))]
        unsafe {
            let saved: u64;
            core::arch::asm!("mrs {}, fpcr", out(reg) saved, options(nomem, nostack));
            // FZ (bit 24): flush denormal outputs/inputs to zero.
            core::arch::asm!("msr fpcr, {}", in(reg) saved | (1 << 24), options(nomem, nostack));
            Self { saved }
        }
        #[cfg(all(target_arch = "aarch64", miri))]
        {
            Self { saved: 0 }
        }
        // SAFETY: `stmxcsr` writes MXCSR to `saved` (a live, aligned
        // u32), `ldmxcsr` reads `engaged`; only MXCSR's FZ and DAZ bits
        // change, and `Drop` restores the saved value on this thread.
        #[cfg(all(target_arch = "x86_64", not(miri)))]
        unsafe {
            let mut saved: u32 = 0;
            core::arch::asm!("stmxcsr [{}]", in(reg) &mut saved, options(nostack, preserves_flags));
            // FZ (bit 15) + DAZ (bit 6).
            let engaged: u32 = saved | 0x8040;
            core::arch::asm!("ldmxcsr [{}]", in(reg) &engaged, options(nostack, preserves_flags));
            Self { saved }
        }
        #[cfg(all(target_arch = "x86_64", miri))]
        {
            Self { saved: 0 }
        }
        #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
        Self {}
    }
}

impl Drop for ScopedFtz {
    fn drop(&mut self) {
        // SAFETY: restores the control register `engage` read.
        #[cfg(all(target_arch = "aarch64", not(miri)))]
        unsafe {
            core::arch::asm!("msr fpcr, {}", in(reg) self.saved, options(nomem, nostack));
        }
        // SAFETY: as above.
        #[cfg(all(target_arch = "x86_64", not(miri)))]
        unsafe {
            core::arch::asm!("ldmxcsr [{}]", in(reg) &self.saved, options(nostack, preserves_flags));
        }
    }
}
