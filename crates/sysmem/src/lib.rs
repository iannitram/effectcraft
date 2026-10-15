//! Ours (Z13 perf work, 2026-10-15): how much memory the machine has free, and how much this
//! process holds now and at its peak, read straight from the operating system (no child process,
//! microseconds a call), for sizing an export's frames in flight.
//!
//! - Linux: `/proc/meminfo` (`MemAvailable`, `MemTotal`) and `/proc/self/status` (`VmRSS`,
//!   `VmHWM`); the peak can be reset (`/proc/self/clear_refs`).
//! - Windows: `GlobalMemoryStatusEx` (`ullAvailPhys`, `ullTotalPhys`) and `GetProcessMemoryInfo`
//!   (`WorkingSetSize`, `PeakWorkingSetSize`); the peak cannot be reset.
//! - macOS: `host_statistics64` (free + inactive pages), `hw.memsize`, and `task_info`
//!   (`MACH_TASK_BASIC_INFO`: `resident_size`, `resident_size_max`); the peak cannot be reset.
//!
//! Elsewhere [`read`] is `None`.

mod os;

/// One set of readings, in bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Readings {
    /// What the system can still give without swapping.
    pub available: u64,
    /// Physical memory.
    pub total: u64,
    /// This process's resident memory now.
    pub resident: u64,
    /// This process's peak resident memory: since [`reset_peak`] where `peak_resets`, else
    /// since it started.
    pub peak: u64,
    /// Whether [`reset_peak`] works here (Linux).
    pub peak_resets: bool,
}

/// This machine's readings now, or `None` where they can't be read.
pub fn read() -> Option<Readings> {
    os::read()
}

/// Start a new peak from now, where the system allows it ([`Readings::peak_resets`]).
pub fn reset_peak() {
    os::reset_peak();
}

/// Linux: the readings from the text of `/proc/meminfo` and `/proc/self/status`.
pub fn parse_linux(meminfo: &str, status: &str) -> Option<Readings> {
    let kib = |text: &str, key: &str| -> Option<u64> {
        let line = text.lines().find(|l| l.starts_with(key))?;
        line[key.len()..].trim().trim_end_matches("kB").trim().parse::<u64>().ok().map(|k| k * 1024)
    };
    Some(Readings {
        available: kib(meminfo, "MemAvailable:")?,
        total: kib(meminfo, "MemTotal:")?,
        resident: kib(status, "VmRSS:")?,
        peak: kib(status, "VmHWM:")?,
        peak_resets: true,
    })
}

/// macOS: available memory from `host_statistics64`'s page counts.
pub fn mac_available(free_pages: u64, inactive_pages: u64, page_size: u64) -> u64 {
    free_pages.saturating_add(inactive_pages).saturating_mul(page_size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_readings_come_from_meminfo_and_status() {
        let meminfo = "MemTotal:       64934972 kB\nMemFree:         1000000 kB\nMemAvailable:   57272808 kB\n";
        let status = "Name:\teffectcraft\nVmHWM:\t 3600000 kB\nVmRSS:\t 1200000 kB\n";
        let r = parse_linux(meminfo, status).unwrap();
        assert_eq!(r, Readings { available: 57272808 * 1024, total: 64934972 * 1024, resident: 1200000 * 1024, peak: 3600000 * 1024, peak_resets: true });
        assert_eq!(parse_linux("MemTotal: 1 kB\n", status), None, "no MemAvailable");
        assert_eq!(parse_linux(meminfo, "VmRSS: x kB\n"), None, "unreadable");
    }

    #[test]
    fn mac_available_is_free_and_inactive_pages() {
        assert_eq!(mac_available(100, 50, 16384), 150 * 16384);
        assert_eq!(mac_available(u64::MAX, 1, 2), u64::MAX);
    }

    #[test]
    fn this_machine_reads_or_says_none() {
        if let Some(r) = read() {
            assert!(r.total > 0 && r.available <= r.total && r.resident > 0 && r.peak >= r.resident / 2, "{r:?}");
            reset_peak();
            let t = std::time::Instant::now();
            for _ in 0..100 {
                read();
            }
            assert!(t.elapsed() < std::time::Duration::from_secs(1), "100 readings took {:?}", t.elapsed());
        }
    }
}
