use std::fmt::Write as _;

/// Returns the current process's memory usage in bytes, or `None` if unavailable on this platform.
pub fn current_process_memory_bytes() -> Option<u64> {
    current_process_memory_bytes_impl()
}

/// Releases unused allocator pages back to the operating system where supported.
pub fn trim_process_memory() {
    trim_process_memory_impl();
}

/// Formats a byte count into a human-readable memory size string using binary units.
pub fn format_memory_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

    let mut out = String::with_capacity(16);
    let value = bytes as f64;
    if value >= GIB {
        let _ = write!(out, "{:.2} GB", value / GIB);
    } else if value >= MIB {
        let _ = write!(out, "{:.1} MB", value / MIB);
    } else if value >= KIB {
        let _ = write!(out, "{:.0} KB", value / KIB);
    } else {
        let _ = write!(out, "{bytes} B");
    }
    out
}

#[cfg(target_os = "macos")]
fn current_process_memory_bytes_impl() -> Option<u64> {
    use std::ffi::c_void;

    const RUSAGE_INFO_V0: i32 = 0;

    #[repr(C)]
    #[derive(Default)]
    struct RusageInfoV0 {
        ri_uuid: [u8; 16],
        ri_user_time: u64,
        ri_system_time: u64,
        ri_pkg_idle_wkups: u64,
        ri_interrupt_wkups: u64,
        ri_pageins: u64,
        ri_wired_size: u64,
        ri_resident_size: u64,
        ri_phys_footprint: u64,
        ri_proc_start_abstime: u64,
        ri_proc_exit_abstime: u64,
    }

    const _: () = assert!(std::mem::size_of::<RusageInfoV0>() == 96);

    unsafe extern "C" {
        fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut c_void) -> i32;
    }

    let mut info = RusageInfoV0::default();
    let pid = i32::try_from(std::process::id()).ok()?;
    let ret = unsafe {
        proc_pid_rusage(
            pid,
            RUSAGE_INFO_V0,
            (&mut info as *mut RusageInfoV0).cast::<c_void>(),
        )
    };
    if ret != 0 {
        return None;
    }

    if info.ri_phys_footprint > 0 {
        Some(info.ri_phys_footprint)
    } else if info.ri_resident_size > 0 {
        Some(info.ri_resident_size)
    } else {
        None
    }
}

#[cfg(target_os = "linux")]
fn current_process_memory_bytes_impl() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    parse_proc_status_vm_rss_bytes(&status)
}

#[cfg(target_os = "windows")]
fn current_process_memory_bytes_impl() -> Option<u64> {
    use std::ffi::c_void;

    #[repr(C)]
    #[derive(Default)]
    struct ProcessMemoryCounters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut c_void;
        fn K32GetProcessMemoryInfo(
            process: *mut c_void,
            ppsmem_counters: *mut ProcessMemoryCounters,
            cb: u32,
        ) -> i32;
    }

    let mut counters = ProcessMemoryCounters {
        cb: std::mem::size_of::<ProcessMemoryCounters>() as u32,
        ..ProcessMemoryCounters::default()
    };
    let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb) };
    if ok == 0 || counters.working_set_size == 0 {
        return None;
    }
    Some(counters.working_set_size as u64)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn current_process_memory_bytes_impl() -> Option<u64> {
    None
}

#[cfg(target_os = "macos")]
fn trim_process_memory_impl() {
    use std::ffi::c_void;

    unsafe extern "C" {
        fn malloc_zone_pressure_relief(zone: *mut c_void, goal: usize) -> usize;
    }

    unsafe {
        let _ = malloc_zone_pressure_relief(std::ptr::null_mut(), 0);
    }
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn trim_process_memory_impl() {
    unsafe extern "C" {
        fn malloc_trim(pad: usize) -> i32;
    }

    unsafe {
        let _ = malloc_trim(0);
    }
}

#[cfg(not(any(target_os = "macos", all(target_os = "linux", target_env = "gnu"))))]
fn trim_process_memory_impl() {}

#[cfg(any(target_os = "linux", test))]
fn parse_proc_status_vm_rss_bytes(status: &str) -> Option<u64> {
    for line in status.lines() {
        let Some(rest) = line.strip_prefix("VmRSS:") else {
            continue;
        };
        let kib = rest.split_whitespace().next()?.parse::<u64>().ok()?;
        return kib.checked_mul(1024);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_memory_units() {
        assert_eq!(format_memory_bytes(512), "512 B");
        assert_eq!(format_memory_bytes(2048), "2 KB");
        assert_eq!(
            format_memory_bytes(48 * 1024 * 1024 + 512 * 1024),
            "48.5 MB"
        );
        assert_eq!(
            format_memory_bytes(1024 * 1024 * 1024 + 256 * 1024 * 1024),
            "1.25 GB"
        );
    }

    #[test]
    fn parses_linux_proc_status_vm_rss() {
        let sample = "Name:\tproj-panel\nVmSize:\t  123456 kB\nVmRSS:\t   49152 kB\n";
        assert_eq!(parse_proc_status_vm_rss_bytes(sample), Some(49152 * 1024));
        assert_eq!(parse_proc_status_vm_rss_bytes("Name:\tproj-panel\n"), None);
    }

    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    #[test]
    fn reads_current_process_memory() {
        let bytes =
            current_process_memory_bytes().expect("current process memory should be readable");
        assert!(bytes > 0);
    }

    #[test]
    fn trims_process_memory_safely() {
        let scratch = vec![0u8; 256 * 1024];
        drop(scratch);
        trim_process_memory();
    }
}
