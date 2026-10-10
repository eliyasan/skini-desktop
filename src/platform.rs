// Small OS-specific helpers so the rest of the code can stay platform neutral.
use std::path::PathBuf;

/// Per-user data folder: %LOCALAPPDATA% on Windows, ~/Library/Application Support on macOS.
pub fn app_data_dir() -> PathBuf {
    #[cfg(windows)]
    {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Users\Default\AppData\Local"))
    }
    #[cfg(target_os = "macos")]
    {
        home_dir().join("Library").join("Application Support")
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        home_dir().join(".local").join("share")
    }
}

#[cfg(not(windows))]
pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
}

/// Receive timeout on a raw socket handed out by MobileDevice, so a read cannot block forever.
pub fn set_recv_timeout(socket: i32, ms: u32) {
    if socket <= 0 {
        return;
    }
    #[cfg(windows)]
    unsafe {
        unsafe extern "system" {
            fn setsockopt(s: usize, level: i32, optname: i32, optval: *const i8, optlen: i32) -> i32;
        }
        const SOL_SOCKET: i32 = 0xffff;
        const SO_RCVTIMEO: i32 = 0x1006;
        setsockopt(
            socket as usize,
            SOL_SOCKET,
            SO_RCVTIMEO,
            &ms as *const u32 as *const i8,
            std::mem::size_of::<u32>() as i32,
        );
    }
    #[cfg(unix)]
    unsafe {
        #[repr(C)]
        struct Timeval {
            tv_sec: i64,
            tv_usec: i32,
        }
        unsafe extern "C" {
            fn setsockopt(s: i32, level: i32, optname: i32, optval: *const std::ffi::c_void, optlen: u32) -> i32;
        }
        // Darwin values (Linux differs, but this build only targets Windows and macOS).
        const SOL_SOCKET: i32 = 0xffff;
        const SO_RCVTIMEO: i32 = 0x1006;
        let tv = Timeval { tv_sec: (ms / 1000) as i64, tv_usec: ((ms % 1000) * 1000) as i32 };
        setsockopt(
            socket,
            SOL_SOCKET,
            SO_RCVTIMEO,
            &tv as *const Timeval as *const std::ffi::c_void,
            std::mem::size_of::<Timeval>() as u32,
        );
    }
}
