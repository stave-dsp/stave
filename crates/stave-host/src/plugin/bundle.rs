//! A `.vst3` bundle on disk, opened the way a DAW opens it, and the
//! binary inside it:
//!
//! * macOS: a CoreFoundation `CFBundle`. The executable comes from the
//!   bundle's own `Info.plist` (`CFBundleExecutable`, XML or binary), and
//!   the `CFBundleRef` is what `bundleEntry` receives — JUCE plugins keep
//!   it to find their resources.
//! * Linux: `Contents/<arch>-linux/<name>.so` (the SDK's layout).
//! * Windows: `Contents/<arch>-win/<name>.vst3` (a DLL), or a single-file
//!   `<name>.vst3` DLL (the older layout).
//!
//! A path that is a file, not a directory, is the binary itself on every
//! platform (a bare `.so`, `.dll` or `.dylib`).

use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
pub use mac::Bundle;
#[cfg(not(target_os = "macos"))]
pub use plain::Bundle;

/// The binary of a bundle laid out as the VST3 SDK's Linux and Windows
/// bundles are: `Contents/<arch>-<os>/<stem><ext>`.
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn sdk_executable(bundle: &Path, os: &str, ext: &str) -> PathBuf {
    let arch = match std::env::consts::ARCH {
        "aarch64" if os == "win" => "arm64",
        a => a,
    };
    bundle
        .join("Contents")
        .join(format!("{arch}-{os}"))
        .join(format!("{}{ext}", stem(bundle)))
}

fn stem(bundle: &Path) -> &str {
    bundle.file_stem().and_then(|s| s.to_str()).unwrap_or("plugin")
}

#[cfg(not(target_os = "macos"))]
mod plain {
    use std::ffi::c_void;
    use std::path::{Path, PathBuf};

    /// An opened bundle directory (or a bare binary).
    pub struct Bundle {
        path: PathBuf,
    }

    impl Bundle {
        /// `path`, as a bundle: nothing is read until [`executable`](Self::executable).
        pub fn open(path: &Path) -> Self {
            Self { path: path.to_path_buf() }
        }

        pub fn path(&self) -> &Path {
            &self.path
        }

        /// No bundle object off macOS: the module entry receives the
        /// library handle instead (Linux) or nothing (Windows).
        pub fn cf_ref(&self) -> *mut c_void {
            std::ptr::null_mut()
        }

        /// The bundle's binary: the path itself when it is a file, else
        /// the SDK layout for this platform.
        pub fn executable(&self) -> PathBuf {
            if self.path.is_file() {
                return self.path.clone();
            }
            if cfg!(windows) {
                super::sdk_executable(&self.path, "win", ".vst3")
            } else {
                super::sdk_executable(&self.path, "linux", ".so")
            }
        }
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::{c_void, CStr};
    use std::path::{Path, PathBuf};
    use std::ptr::NonNull;

    type CFTypeRef = *const c_void;

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFURLCreateFromFileSystemRepresentation(
            allocator: CFTypeRef,
            buffer: *const u8,
            len: isize,
            is_directory: u8,
        ) -> CFTypeRef;
        fn CFBundleCreate(allocator: CFTypeRef, url: CFTypeRef) -> CFTypeRef;
        fn CFBundleCopyExecutableURL(bundle: CFTypeRef) -> CFTypeRef;
        fn CFURLGetFileSystemRepresentation(
            url: CFTypeRef,
            resolve_against_base: u8,
            buffer: *mut u8,
            max_len: isize,
        ) -> u8;
        fn CFRelease(cf: CFTypeRef);
    }

    /// One owned CoreFoundation reference.
    struct Cf(NonNull<c_void>);

    impl Cf {
        fn adopt(r: CFTypeRef) -> Option<Self> {
            NonNull::new(r.cast_mut()).map(Self)
        }
        fn get(&self) -> CFTypeRef {
            self.0.as_ptr()
        }
    }

    impl Drop for Cf {
        fn drop(&mut self) {
            // SAFETY: we own exactly one reference (Create/Copy rule).
            unsafe { CFRelease(self.get()) };
        }
    }

    /// An opened bundle directory.
    pub struct Bundle {
        path: PathBuf,
        cf: Option<Cf>,
    }

    impl Bundle {
        /// Open `path` as a CFBundle. A directory CoreFoundation refuses
        /// still opens (executable falls back to the naming convention).
        pub fn open(path: &Path) -> Self {
            Self {
                path: path.to_path_buf(),
                cf: cf_bundle(path),
            }
        }

        pub fn path(&self) -> &Path {
            &self.path
        }

        /// The `CFBundleRef` for `bundleEntry` (null if CF refused the bundle).
        pub fn cf_ref(&self) -> *mut c_void {
            self.cf
                .as_ref()
                .map_or(std::ptr::null_mut(), |c| c.get().cast_mut())
        }

        /// The bundle's executable: the path itself when it is a file, else
        /// `CFBundleExecutable` when declared, else `Contents/MacOS/<bundle
        /// stem>`.
        pub fn executable(&self) -> PathBuf {
            if self.path.is_file() {
                return self.path.clone();
            }
            self.cf
                .as_ref()
                .and_then(|cf| executable_from_cf(cf.get()))
                .unwrap_or_else(|| conventional_executable(&self.path))
        }
    }

    fn cf_bundle(path: &Path) -> Option<Cf> {
        use std::os::unix::ffi::OsStrExt;
        let bytes = path.as_os_str().as_bytes();
        // SAFETY: plain CF object creation; every result is null-checked
        // and owned by a `Cf`.
        unsafe {
            let url = Cf::adopt(CFURLCreateFromFileSystemRepresentation(
                std::ptr::null(),
                bytes.as_ptr(),
                bytes.len() as isize,
                1,
            ))?;
            Cf::adopt(CFBundleCreate(std::ptr::null(), url.get()))
        }
    }

    fn executable_from_cf(bundle: CFTypeRef) -> Option<PathBuf> {
        // SAFETY: `bundle` is a live CFBundle; the URL is owned by a `Cf`.
        unsafe {
            let url = Cf::adopt(CFBundleCopyExecutableURL(bundle))?;
            let mut buf = vec![0u8; 4096];
            if CFURLGetFileSystemRepresentation(url.get(), 1, buf.as_mut_ptr(), buf.len() as isize) == 0
            {
                return None;
            }
            let s = CStr::from_bytes_until_nul(&buf).ok()?.to_str().ok()?;
            Some(PathBuf::from(s))
        }
    }

    fn conventional_executable(bundle: &Path) -> PathBuf {
        bundle.join("Contents").join("MacOS").join(super::stem(bundle))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TempDir;

    #[cfg(target_os = "macos")]
    fn make_bundle(root: &Path, name: &str, exe: &str, declare: bool) -> PathBuf {
        let b = root.join(format!("{name}.vst3"));
        std::fs::create_dir_all(b.join("Contents/MacOS")).unwrap();
        std::fs::write(b.join("Contents/MacOS").join(exe), b"").unwrap();
        if declare {
            let plist = format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>{exe}</string>
<key>CFBundleIdentifier</key><string>test.{name}</string>
<key>CFBundlePackageType</key><string>BNDL</string>
</dict></plist>"#
            );
            std::fs::write(b.join("Contents/Info.plist"), plist).unwrap();
        }
        b
    }

    #[test]
    fn a_file_is_its_own_executable() {
        let dir = TempDir::new("bundle-file");
        let lib = dir.path().join("libplug.so");
        std::fs::write(&lib, b"").unwrap();
        assert_eq!(Bundle::open(&lib).executable(), lib);
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn the_sdk_layout_names_the_binary() {
        let b = Path::new("/plugins/Tilt.vst3");
        let (os, ext) = if cfg!(windows) { ("win", ".vst3") } else { ("linux", ".so") };
        let arch = if cfg!(all(windows, target_arch = "aarch64")) { "arm64" } else { std::env::consts::ARCH };
        assert_eq!(
            Bundle::open(b).executable(),
            b.join("Contents").join(format!("{arch}-{os}")).join(format!("Tilt{ext}"))
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn executable_comes_from_info_plist() {
        let dir = TempDir::new("bundle-plist");
        let b = make_bundle(dir.path(), "Pretty Name", "real_binary", true);
        let bundle = Bundle::open(&b);
        assert!(!bundle.cf_ref().is_null());
        assert_eq!(bundle.executable().file_name().unwrap(), "real_binary");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn falls_back_to_the_naming_convention() {
        let dir = TempDir::new("bundle-bare");
        let b = make_bundle(dir.path(), "Bare", "Bare", false);
        assert_eq!(Bundle::open(&b).executable(), b.join("Contents/MacOS/Bare"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn missing_directory_still_yields_a_path() {
        let b = Path::new("/nonexistent/Ghost.vst3");
        let bundle = Bundle::open(b);
        assert_eq!(bundle.executable(), b.join("Contents/MacOS/Ghost"));
    }
}
