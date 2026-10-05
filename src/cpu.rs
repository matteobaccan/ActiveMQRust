// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! Processor count and runtime sizing, following the JVM.
//!
//! The count is computed like `Runtime.availableProcessors()`: on Windows it is the number of
//! processors in the process affinity mask, so a broker started with `start /affinity` or
//! confined by an administrator uses only the processors it was given. `broker.processors`
//! (or `--processors`) overrides it, like `-XX:ActiveProcessorCount`.
//!
//! The runtime then mirrors how ActiveMQ uses its threads: one worker per processor for
//! network I/O and dispatch, and CPU-heavy work (compression) on at most `processors - 1`
//! threads, like the `ForkJoinPool.commonPool()` parallelism, so it never takes every core
//! away from the connections.

/// Processors available to this process, like the JVM's `Runtime.availableProcessors()`.
pub fn available() -> usize {
    affinity_count()
        .or_else(|| std::thread::available_parallelism().ok().map(|n| n.get()))
        .unwrap_or(1)
}

#[cfg(windows)]
fn affinity_count() -> Option<usize> {
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessAffinityMask};
    let mut process = 0usize;
    let mut system = 0usize;
    // SAFETY: GetCurrentProcess returns a pseudo-handle; both pointers are valid for writes.
    let ok = unsafe { GetProcessAffinityMask(GetCurrentProcess(), &mut process, &mut system) };
    // With more than 64 processors the mask covers only one processor group: let the
    // standard library count every group instead, as the JVM does.
    if ok == 0 || process == 0 || system == usize::MAX {
        return None;
    }
    Some(process.count_ones() as usize)
}

#[cfg(not(windows))]
fn affinity_count() -> Option<usize> {
    None
}

/// Effective processor count: the configured value, or the available processors when 0.
pub fn effective(configured: usize) -> usize {
    if configured > 0 {
        configured
    } else {
        available()
    }
}

/// Threads for CPU-heavy work: one less than the processors, at least one.
pub fn heavy_threads(processors: usize) -> usize {
    processors.saturating_sub(1).max(1)
}

/// Builds the broker runtime for the configured processor count (0 = automatic).
pub fn runtime(configured: usize) -> std::io::Result<tokio::runtime::Runtime> {
    let processors = effective(configured);
    let heavy = heavy_threads(processors);
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(processors)
        .max_blocking_threads(heavy)
        .thread_name("mqrust-worker")
        .enable_all()
        .build()?;
    let source = if configured > 0 { "configured" } else { "available to the process" };
    tracing::info!("using {processors} processors ({source}): {processors} I/O workers, up to {heavy} compression threads");
    Ok(rt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn available_is_positive() {
        assert!(available() >= 1);
    }

    #[test]
    fn configured_value_wins() {
        assert_eq!(effective(3), 3);
        assert_eq!(effective(0), available());
    }

    #[test]
    fn heavy_work_leaves_one_processor_free() {
        assert_eq!(heavy_threads(1), 1);
        assert_eq!(heavy_threads(2), 1);
        assert_eq!(heavy_threads(8), 7);
    }

    #[test]
    fn runtime_honours_the_configured_count() {
        let rt = runtime(2).unwrap();
        assert_eq!(rt.metrics().num_workers(), 2);
    }
}
