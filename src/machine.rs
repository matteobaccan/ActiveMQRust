// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! What the machine offers. The processor count lives in [`crate::cpu`]; here is the
//! physical memory, read once at startup to decide how much of it the broker's messages may
//! take when `broker.max_memory_mb` says nothing.
//!
//! Reading it is one call per system, each behind its own gate; the arithmetic that turns it
//! into a limit is cross-platform, so the tests cover it everywhere.

/// Default limit when nothing is configured and the machine has plenty of memory.
pub const DEFAULT_LIMIT_MB: u64 = 2048;
/// Smallest default limit, so a small machine still holds a useful backlog.
pub const MIN_LIMIT_MB: u64 = 256;

/// Physical memory of the machine in MB, or `None` when it cannot be read.
pub fn physical_memory_mb() -> Option<u64> {
    physical_memory_bytes().map(|b| b / (1024 * 1024))
}

/// How much memory the broker's messages may take when `broker.max_memory_mb` is absent:
/// half of the machine, capped at [`DEFAULT_LIMIT_MB`] and floored at [`MIN_LIMIT_MB`].
///
/// Half, because the broker is one process on a machine that runs others, and the accounted
/// size of the messages is not the whole footprint of the process. Capped, because a broker
/// meant to be small has no business holding 128 GB of messages on a 256 GB server by
/// default. Floored, because a limit of a few MB would refuse messages a 512 MB machine can
/// perfectly well hold. With no reading of the machine the cap is used, not the floor: a
/// large machine refusing messages for a reason nobody can see is the worse surprise.
pub fn default_limit_mb(physical_mb: Option<u64>) -> u64 {
    match physical_mb {
        Some(mb) => (mb / 2).clamp(MIN_LIMIT_MB, DEFAULT_LIMIT_MB),
        None => DEFAULT_LIMIT_MB,
    }
}

#[cfg(windows)]
fn physical_memory_bytes() -> Option<u64> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    // SAFETY: the buffer is a zeroed MEMORYSTATUSEX whose dwLength says how long it is.
    let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
    (ok != 0 && status.ullTotalPhys > 0).then_some(status.ullTotalPhys)
}

#[cfg(target_os = "macos")]
fn physical_memory_bytes() -> Option<u64> {
    let mut size: u64 = 0;
    let mut len = std::mem::size_of::<u64>();
    let name = c"hw.memsize";
    // SAFETY: name is a NUL-terminated string, and the buffer is one u64 as len says.
    let rc = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            &mut size as *mut u64 as *mut libc::c_void,
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    (rc == 0 && size > 0).then_some(size)
}

#[cfg(not(any(windows, target_os = "macos")))]
fn physical_memory_bytes() -> Option<u64> {
    // `MemTotal:   16327456 kB`, the first line of /proc/meminfo on Linux.
    let meminfo = std::fs::read_to_string("/proc/meminfo").ok()?;
    mem_total_kb(&meminfo).map(|kb| kb * 1024)
}

/// Reads `MemTotal` out of the text of /proc/meminfo, in kB.
#[cfg(not(any(windows, target_os = "macos")))]
fn mem_total_kb(meminfo: &str) -> Option<u64> {
    let line = meminfo.lines().find(|l| l.starts_with("MemTotal:"))?;
    line.split_whitespace().nth(1)?.parse().ok().filter(|kb| *kb > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_limit_is_half_the_machine_within_its_bounds() {
        assert_eq!(default_limit_mb(Some(16 * 1024)), 2048, "16 GB: the cap");
        assert_eq!(default_limit_mb(Some(4 * 1024)), 2048, "4 GB: exactly the cap");
        assert_eq!(default_limit_mb(Some(2 * 1024)), 1024, "2 GB: half of it");
        assert_eq!(default_limit_mb(Some(1024)), 512, "1 GB: half of it");
        assert_eq!(default_limit_mb(Some(256)), 256, "256 MB: the floor, not 128");
        assert_eq!(default_limit_mb(Some(1)), 256, "a machine smaller than the floor");
        assert_eq!(default_limit_mb(Some(0)), 256, "a reading of zero");
        assert_eq!(default_limit_mb(None), 2048, "unknown: the cap, not the floor");
    }

    #[test]
    fn the_machine_reports_its_memory() {
        // Linux, macOS and Windows all have a way to read it; the test says which platforms
        // the broker can size itself on, and does not guess how much memory CI runners have.
        match physical_memory_mb() {
            Some(mb) => assert!(mb >= 64, "implausible physical memory: {mb} MB"),
            None => panic!("the physical memory of this machine could not be read"),
        }
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    #[test]
    fn mem_total_is_read_from_the_text() {
        let meminfo = "MemTotal:       16327456 kB\nMemFree:         1234 kB\n";
        assert_eq!(mem_total_kb(meminfo), Some(16_327_456));
        assert_eq!(mem_total_kb("MemFree: 12 kB\n"), None, "no MemTotal line");
        assert_eq!(mem_total_kb("MemTotal:\n"), None, "no value");
        assert_eq!(mem_total_kb("MemTotal: nonsense kB\n"), None, "not a number");
        assert_eq!(mem_total_kb("MemTotal: 0 kB\n"), None, "zero is no answer");
    }
}
