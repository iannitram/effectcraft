//! The operating-system calls behind [`crate::read`] and [`crate::reset_peak`].

use crate::Readings;

#[cfg(target_os = "linux")]
pub fn read() -> Option<Readings> {
    let meminfo = std::fs::read_to_string("/proc/meminfo").ok()?;
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    crate::parse_linux(&meminfo, &status)
}

#[cfg(target_os = "linux")]
pub fn reset_peak() {
    // "5" resets the peak resident size (VmHWM) to the current one (proc(5), clear_refs)
    let _ = std::fs::write("/proc/self/clear_refs", "5");
}

#[cfg(windows)]
pub fn read() -> Option<Readings> {
    use windows_sys::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    // SAFETY: a zeroed MEMORYSTATUSEX with dwLength set is what GlobalMemoryStatusEx fills in.
    let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    // SAFETY: `status` is a live, correctly sized MEMORYSTATUSEX.
    if unsafe { GlobalMemoryStatusEx(&mut status) } == 0 {
        return None;
    }
    // SAFETY: a zeroed PROCESS_MEMORY_COUNTERS is a valid output buffer.
    let mut counters: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
    let cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
    // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no closing; `counters` is `cb` bytes.
    if unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, cb) } == 0 {
        return None;
    }
    Some(Readings {
        available: status.ullAvailPhys,
        total: status.ullTotalPhys,
        resident: counters.WorkingSetSize as u64,
        peak: counters.PeakWorkingSetSize as u64,
        peak_resets: false,
    })
}

#[cfg(target_os = "macos")]
// libc marks mach_host_self / mach_task_self deprecated in favour of the mach2 crate, which has no
// mach_host_self (0.4); they are the system's own calls and stay.
#[allow(deprecated)]
pub fn read() -> Option<Readings> {
    use libc::{
        HOST_VM_INFO64, HOST_VM_INFO64_COUNT, KERN_SUCCESS, MACH_TASK_BASIC_INFO, MACH_TASK_BASIC_INFO_COUNT, c_void, host_statistics64, mach_host_self,
        mach_task_basic_info, mach_task_self, sysctlbyname, task_info, vm_page_size, vm_statistics64,
    };
    // SAFETY: zeroed plain-data structs are valid outputs; counts are the sizes libc gives for them.
    unsafe {
        let mut vm: vm_statistics64 = std::mem::zeroed();
        let mut count = HOST_VM_INFO64_COUNT;
        if host_statistics64(mach_host_self(), HOST_VM_INFO64, (&mut vm as *mut vm_statistics64).cast(), &mut count) != KERN_SUCCESS {
            return None;
        }
        let mut total: u64 = 0;
        let mut len = std::mem::size_of::<u64>();
        if sysctlbyname(c"hw.memsize".as_ptr(), (&mut total as *mut u64).cast::<c_void>(), &mut len, std::ptr::null_mut(), 0) != 0 {
            return None;
        }
        let mut info: mach_task_basic_info = std::mem::zeroed();
        let mut icount = MACH_TASK_BASIC_INFO_COUNT;
        if task_info(mach_task_self(), MACH_TASK_BASIC_INFO, (&mut info as *mut mach_task_basic_info).cast(), &mut icount) != KERN_SUCCESS {
            return None;
        }
        Some(Readings {
            available: crate::mac_available(vm.free_count as u64, vm.inactive_count as u64, vm_page_size as u64),
            total,
            resident: info.resident_size,
            peak: info.resident_size_max,
            peak_resets: false,
        })
    }
}

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
pub fn read() -> Option<Readings> {
    None
}

#[cfg(not(target_os = "linux"))]
pub fn reset_peak() {}
