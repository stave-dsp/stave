//! Keep stdout for our own output. Plugins print to stdout (JUCE
//! announces its version); for the CLI's `--json` and the MCP server's
//! JSON-RPC stream, stdout is a data channel. [`reserve`] duplicates the
//! real stdout for us and points fd 1 (and the standard output handle,
//! on Windows) at stderr, so plugin chatter can never corrupt it.

use std::fs::File;
use std::io::Write;

/// Duplicate stdout for our own use, then redirect fd 1 to stderr.
#[cfg(unix)]
pub fn reserve() -> Option<File> {
    use std::os::fd::FromRawFd;
    extern "C" {
        fn dup(fd: i32) -> i32;
        fn dup2(src: i32, dst: i32) -> i32;
    }
    std::io::stdout().flush().ok()?;
    // SAFETY: plain fd syscalls; the dup'd fd is owned by the File.
    unsafe {
        let ours = dup(1);
        if ours < 0 || dup2(2, 1) < 0 {
            return None;
        }
        Some(File::from_raw_fd(ours))
    }
}

/// Duplicate stdout's handle for our own use, then point the standard
/// output handle (Win32 writes) and the C runtime's fd 1 (plugin
/// `printf`) at stderr.
#[cfg(windows)]
pub fn reserve() -> Option<File> {
    use std::ffi::c_void;
    use std::os::windows::io::FromRawHandle;
    #[link(name = "kernel32")]
    extern "system" {
        fn GetStdHandle(which: u32) -> *mut c_void;
        fn SetStdHandle(which: u32, handle: *mut c_void) -> i32;
        fn GetCurrentProcess() -> *mut c_void;
        fn DuplicateHandle(
            source_process: *mut c_void,
            source: *mut c_void,
            target_process: *mut c_void,
            target: *mut *mut c_void,
            access: u32,
            inherit: i32,
            options: u32,
        ) -> i32;
    }
    extern "C" {
        fn _dup2(src: i32, dst: i32) -> i32;
    }
    const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
    const STD_ERROR_HANDLE: u32 = -12i32 as u32;
    const DUPLICATE_SAME_ACCESS: u32 = 2;
    std::io::stdout().flush().ok()?;
    // SAFETY: handle calls on this process's own standard handles; the
    // duplicate is owned by the File.
    unsafe {
        let process = GetCurrentProcess();
        let mut ours = std::ptr::null_mut();
        let out = GetStdHandle(STD_OUTPUT_HANDLE);
        if out.is_null() || DuplicateHandle(process, out, process, &mut ours, 0, 0, DUPLICATE_SAME_ACCESS) == 0 {
            return None;
        }
        SetStdHandle(STD_OUTPUT_HANDLE, GetStdHandle(STD_ERROR_HANDLE));
        _dup2(2, 1);
        Some(File::from_raw_handle(ours))
    }
}
