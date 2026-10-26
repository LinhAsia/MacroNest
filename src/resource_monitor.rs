use std::time::Instant;

#[derive(Clone, Debug, Default)]
pub struct ProcessResourceStats {
    pub cpu_usage_pct: f32,
    pub ram_working_set_mb: f64,
    pub ram_commit_mb: f64,
    pub thread_count: u32,
    pub active_macros_count: usize,
    pub macros_master_enabled: bool,
    pub uptime_secs: u64,
    pub num_cpus: usize,
    pub pid: u32,
}

#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct PROCESS_MEMORY_COUNTERS_EX {
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
    private_usage: usize,
}

#[cfg(windows)]
#[repr(C)]
#[derive(Default, Copy, Clone)]
struct FILETIME {
    dw_low_date_time: u32,
    dw_high_date_time: u32,
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn K32GetProcessMemoryInfo(
        hprocess: isize,
        ppsmemcounters: *mut PROCESS_MEMORY_COUNTERS_EX,
        cb: u32,
    ) -> i32;
    fn GetProcessTimes(
        hprocess: isize,
        lpcreationtime: *mut FILETIME,
        lpexittime: *mut FILETIME,
        lpkerneltime: *mut FILETIME,
        lpusertime: *mut FILETIME,
    ) -> i32;
    fn GetCurrentProcess() -> isize;
    fn GetCurrentProcessId() -> u32;
}

#[cfg(windows)]
fn get_process_thread_count(pid: u32) -> u32 {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
    };
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return 1;
        }
        let mut te: THREADENTRY32 = std::mem::zeroed();
        te.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
        let mut count = 0;
        if Thread32First(snapshot, &mut te) != 0 {
            loop {
                if te.th32OwnerProcessID == pid {
                    count += 1;
                }
                if Thread32Next(snapshot, &mut te) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snapshot);
        count.max(1)
    }
}

pub struct ResourceMonitorTracker {
    start_time: Instant,
    last_sample_time: Instant,
    #[cfg(windows)]
    last_kernel_time: u64,
    #[cfg(windows)]
    last_user_time: u64,
    num_cpus: usize,
    cached_stats: ProcessResourceStats,
}

impl Default for ResourceMonitorTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl ResourceMonitorTracker {
    pub fn new() -> Self {
        let now = Instant::now();
        let num_cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
            .max(1);
        let mut tracker = Self {
            start_time: now,
            last_sample_time: now,
            #[cfg(windows)]
            last_kernel_time: 0,
            #[cfg(windows)]
            last_user_time: 0,
            num_cpus,
            cached_stats: ProcessResourceStats {
                num_cpus,
                ..Default::default()
            },
        };
        #[cfg(windows)]
        {
            let (k, u) = tracker.sample_times();
            tracker.last_kernel_time = k;
            tracker.last_user_time = u;
        }
        tracker
    }

    #[cfg(windows)]
    fn sample_times(&self) -> (u64, u64) {
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        unsafe {
            let proc = GetCurrentProcess();
            if GetProcessTimes(proc, &mut creation, &mut exit, &mut kernel, &mut user) != 0 {
                let k = ((kernel.dw_high_date_time as u64) << 32) | (kernel.dw_low_date_time as u64);
                let u = ((user.dw_high_date_time as u64) << 32) | (user.dw_low_date_time as u64);
                return (k, u);
            }
        }
        (0, 0)
    }

    pub fn sample(&mut self, active_macros_count: usize, macros_master_enabled: bool) -> ProcessResourceStats {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_sample_time);

        // Update stats if at least 250ms have passed since last sample, or on first run
        if elapsed.as_millis() >= 250 || self.cached_stats.uptime_secs == 0 {
            #[cfg(windows)]
            {
                let (k, u) = self.sample_times();
                if self.last_kernel_time > 0 || self.last_user_time > 0 {
                    let delta_cpu_100ns = (k.saturating_sub(self.last_kernel_time))
                        + (u.saturating_sub(self.last_user_time));
                    let elapsed_100ns = (elapsed.as_nanos() / 100) as u64;
                    if elapsed_100ns > 0 {
                        let total_system_time = elapsed_100ns * (self.num_cpus as u64);
                        let cpu_pct = ((delta_cpu_100ns as f64) / (total_system_time as f64) * 100.0) as f32;
                        self.cached_stats.cpu_usage_pct = cpu_pct.clamp(0.0, 100.0);
                    }
                }
                self.last_kernel_time = k;
                self.last_user_time = u;
                self.last_sample_time = now;

                // Memory
                let mut mem = PROCESS_MEMORY_COUNTERS_EX::default();
                mem.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
                unsafe {
                    let proc = GetCurrentProcess();
                    if K32GetProcessMemoryInfo(proc, &mut mem, mem.cb) != 0 {
                        self.cached_stats.ram_working_set_mb = (mem.working_set_size as f64) / (1024.0 * 1024.0);
                        self.cached_stats.ram_commit_mb = (mem.private_usage as f64) / (1024.0 * 1024.0);
                    }
                }

                // Threads
                let pid = unsafe { GetCurrentProcessId() };
                self.cached_stats.pid = pid;
                self.cached_stats.thread_count = get_process_thread_count(pid);
            }
        }

        self.cached_stats.num_cpus = self.num_cpus;
        self.cached_stats.uptime_secs = self.start_time.elapsed().as_secs();
        self.cached_stats.active_macros_count = active_macros_count;
        self.cached_stats.macros_master_enabled = macros_master_enabled;
        self.cached_stats.clone()
    }

    pub fn last_stats(&self) -> &ProcessResourceStats {
        &self.cached_stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resource_monitor_sample() {
        let mut tracker = ResourceMonitorTracker::new();
        let stats = tracker.sample(0, true);
        assert!(stats.num_cpus >= 1);
        #[cfg(windows)]
        {
            assert!(stats.ram_working_set_mb > 0.0);
            assert!(stats.thread_count >= 1);
        }
    }
}
