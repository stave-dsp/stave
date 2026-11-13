//! What a stretch of work on this thread cost: its CPU time
//! ([`thread_cpu_time`]) and, where the CPU's counters can be read, its
//! retired instructions ([`Instructions`]). The bench, the driver and
//! `stave_test`'s perf check measure through these.
//!
//! CPU time leaves out the time the thread was preempted, the largest
//! noise in a wall-clock timing on a busy machine. An instruction count
//! is the same on every run of the same code, so it can see a change
//! timing cannot; it is read through Linux's `perf_event_open`, and is
//! unavailable elsewhere and where the kernel refuses it (an
//! unprivileged container, `kernel.perf_event_paranoid` above 2, a VM
//! without counters).

use std::time::Duration;

/// This thread's CPU time so far.
#[cfg(unix)]
pub fn thread_cpu_time() -> Duration {
    let mut now = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `now` is a valid out-pointer; the clock exists on every unix
    // the host builds for.
    let failed = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut now) } != 0;
    debug_assert!(!failed, "CLOCK_THREAD_CPUTIME_ID unavailable");
    Duration::new(now.tv_sec as u64, now.tv_nsec as u32)
}

/// Time for one block on Windows: wall time from the performance counter
/// (`Instant`). The thread's CPU time (`GetThreadTimes`) advances in
/// scheduler ticks of 15.6 ms — coarser than a block, so every block
/// measured 0 or 15.6 ms (measured in CI). Wall time also counts
/// preemption: pessimistic, never optimistic.
#[cfg(windows)]
pub fn thread_cpu_time() -> Duration {
    static ORIGIN: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    ORIGIN.get_or_init(std::time::Instant::now).elapsed()
}

/// A counter of the instructions this thread retires in user space.
///
/// ```
/// use stave_host::counters::Instructions;
///
/// // `None` where the counters cannot be read: callers report that.
/// if let Some(mut counter) = Instructions::open() {
///     let n = counter.count(|| (0..1000u64).map(|i| i * i).sum::<u64>()).1;
///     assert!(n > 0);
/// }
/// ```
pub struct Instructions {
    #[cfg(target_os = "linux")]
    fd: libc::c_int,
}

impl Instructions {
    /// Open the counter for the calling thread; `None` where the
    /// platform or the kernel will not count.
    #[cfg(target_os = "linux")]
    pub fn open() -> Option<Self> {
        // perf_event_attr, PERF_ATTR_SIZE_VER0 (64 bytes): type @0,
        // size @4, config @8, the flags bitfield @40.
        let mut attr = [0u64; 8];
        const PERF_TYPE_HARDWARE: u64 = 0;
        const PERF_COUNT_HW_INSTRUCTIONS: u64 = 1;
        const DISABLED: u64 = 1 << 0;
        const EXCLUDE_KERNEL: u64 = 1 << 5;
        const EXCLUDE_HV: u64 = 1 << 6;
        attr[0] = PERF_TYPE_HARDWARE | (64u64 << 32);
        attr[1] = PERF_COUNT_HW_INSTRUCTIONS;
        attr[5] = DISABLED | EXCLUDE_KERNEL | EXCLUDE_HV;
        // SAFETY: `attr` is a zero-filled perf_event_attr of the size its
        // size field states, alive for the call; pid 0 / cpu -1 counts
        // the calling thread on any CPU, with no group and no flags.
        let fd = unsafe {
            libc::syscall(libc::SYS_perf_event_open, attr.as_ptr(), 0 as libc::pid_t, -1 as libc::c_int, -1 as libc::c_int, 0 as libc::c_ulong)
        };
        (fd >= 0).then(|| Self { fd: fd as libc::c_int })
    }

    /// Instructions are not counted on this platform.
    #[cfg(not(target_os = "linux"))]
    pub fn open() -> Option<Self> {
        None
    }

    /// Run `work` and count the instructions it retired: its result and
    /// the count.
    #[cfg(target_os = "linux")]
    pub fn count<R>(&mut self, work: impl FnOnce() -> R) -> (R, u64) {
        // _IO('$', n): PERF_EVENT_IOC_ENABLE 0, DISABLE 1, RESET 3.
        const ENABLE: libc::c_ulong = 0x2400;
        const DISABLE: libc::c_ulong = 0x2401;
        const RESET: libc::c_ulong = 0x2403;
        // SAFETY: `fd` is the perf event this value opened and owns;
        // the ioctls take no argument.
        unsafe {
            libc::ioctl(self.fd, RESET as _, 0);
            libc::ioctl(self.fd, ENABLE as _, 0);
        }
        let result = work();
        let mut n = 0u64;
        // SAFETY: as above; `read` writes at most 8 bytes into `n`.
        let read = unsafe {
            libc::ioctl(self.fd, DISABLE as _, 0);
            libc::read(self.fd, (&mut n as *mut u64).cast(), 8)
        };
        (result, if read == 8 { n } else { 0 })
    }

    /// Never reached: [`open`](Self::open) returns `None` here.
    #[cfg(not(target_os = "linux"))]
    pub fn count<R>(&mut self, work: impl FnOnce() -> R) -> (R, u64) {
        (work(), 0)
    }
}

#[cfg(target_os = "linux")]
impl Drop for Instructions {
    fn drop(&mut self) {
        // SAFETY: `fd` is ours and closed once, here.
        unsafe {
            libc::close(self.fd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_time_moves_forward_while_the_thread_works() {
        let t0 = thread_cpu_time();
        let mut x = 0u64;
        for i in 0..2_000_000u64 {
            x = std::hint::black_box(x.wrapping_add(i * i));
        }
        assert!(thread_cpu_time() > t0, "{x}");
    }

    #[test]
    fn more_work_counts_more_instructions() {
        let Some(mut c) = Instructions::open() else {
            eprintln!("instruction counters unavailable here: skipped");
            return;
        };
        let work = |n: u64| (0..n).fold(0u64, |a, i| std::hint::black_box(a.wrapping_add(i)));
        let small = c.count(|| work(10_000)).1;
        let large = c.count(|| work(1_000_000)).1;
        assert!(large > 50 * small.max(1), "{small} vs {large}");
    }
}
