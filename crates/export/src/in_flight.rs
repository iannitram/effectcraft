//! Ours (Z13 perf work, 2026-10-15): how many frames an export renders at once, following the
//! memory the machine has free.
//!
//! [`crate::batch_size`] is the ceiling: one frame per thread, at most 16. Each frame in flight
//! holds its own layer and effect buffers, measured at about 1.9 GB per 1080p frame of a
//! layer-heavy After Effects intro (Radeon 8060S / Ryzen AI Max+ 395, 46 GiB): 16 at once peaked at
//! 37.7 GB and came within 1.4 GB of the out-of-memory killer, while 8 at once took 9 % longer and
//! 10 GB less (the threads inside each frame keep the CPU busy).
//!
//! So [`Batch`] starts at what the free memory surely holds, measures what each frame in flight
//! really costs (the process's peak resident memory over a batch, above where it began, counted a
//! quarter over), and sizes
//! every later batch to fit the memory then free, between 1 and the ceiling: down when memory
//! runs short, back up when it frees. Output is unchanged: frames are rendered and handed out in
//! order whatever the batch size. Platforms without the readings keep the ceiling.

/// Memory left free for the rest of the machine: the larger of this and a tenth of RAM.
const RESERVE: u64 = 4 << 30;

/// The measured cost of a frame is counted this many quarters over: frames differ, and the
/// measurement is one batch behind (beside a 16 GB load, sizing on the bare cost left 3.3 GB free,
/// below the reserve).
const MARGIN_QUARTERS: u64 = 5;

/// The first batch's guess of one frame's cost, before anything is measured: generous, so the
/// first batch is small enough on any project (a 1080p frame of the intro above took 1.9 GB).
const FIRST_GUESS: u64 = 2 << 30;

/// The memory readings a [`Batch`] works from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Memory {
    /// Bytes the system can still give (Linux `MemAvailable`).
    pub available: u64,
    /// Total RAM.
    pub total: u64,
    /// This process's resident memory now.
    pub resident: u64,
    /// This process's peak resident memory since the last [`Probe::reset_peak`].
    pub peak: u64,
}

/// Where the readings come from (the system, or a test's script).
pub(crate) trait Probe {
    fn read(&mut self) -> Option<Memory>;
    /// Start a new peak from now.
    fn reset_peak(&mut self);
}

/// Frames to render at once, batch after batch (see the module docs).
pub(crate) struct Batch<P: Probe = System> {
    ceiling: u64,
    probe: P,
    /// The last batch's size and the resident memory it started from.
    last: Option<(u64, u64)>,
    /// Bytes one frame in flight costs, once measured (the largest seen).
    per_frame: Option<u64>,
}

impl Batch<System> {
    /// A batch sizer under today's ceiling ([`crate::batch_size`]) reading this machine.
    pub(crate) fn new() -> Self {
        Self::with_probe(crate::batch_size(), System)
    }
}

impl<P: Probe> Batch<P> {
    pub(crate) fn with_probe(ceiling: u64, probe: P) -> Self {
        Self { ceiling: ceiling.max(1), probe, last: None, per_frame: None }
    }

    /// The size of the next batch; call it once before each batch is rendered.
    pub(crate) fn next(&mut self) -> u64 {
        let Some(m) = self.probe.read() else { return self.ceiling };
        if let Some((n, start)) = self.last {
            let cost = m.peak.saturating_sub(start) / n.max(1);
            self.per_frame = Some(self.per_frame.map_or(cost, |c| c.max(cost)));
        }
        let size = fit(&m, self.per_frame.map_or(FIRST_GUESS, |c| c / 4 * MARGIN_QUARTERS), self.ceiling);
        if self.last.is_none_or(|(n, _)| n != size) && size < self.ceiling {
            log::info!(
                "decision: export: {size} frames at once, not {}: {} MB free, about {} MB per frame in flight{}",
                self.ceiling,
                m.available >> 20,
                self.per_frame.unwrap_or(FIRST_GUESS) >> 20,
                if self.per_frame.is_none() { " (a first guess, before measuring)" } else { "" }
            );
        }
        self.probe.reset_peak();
        self.last = Some((size, m.resident));
        size
    }
}

/// How many frames of `per_frame` bytes fit in the memory `m` has free, keeping the reserve:
/// between 1 and `ceiling`.
pub(crate) fn fit(m: &Memory, per_frame: u64, ceiling: u64) -> u64 {
    let reserve = RESERVE.max(m.total / 10);
    let spare = m.available.saturating_sub(reserve);
    (spare / per_frame.max(1)).clamp(1, ceiling.max(1))
}

/// This machine's readings: Linux `/proc` (elsewhere none, and the ceiling stands).
pub(crate) struct System;

impl Probe for System {
    #[cfg(target_os = "linux")]
    fn read(&mut self) -> Option<Memory> {
        let kib = |text: &str, key: &str| -> Option<u64> {
            let line = text.lines().find(|l| l.starts_with(key))?;
            line[key.len()..].trim().trim_end_matches("kB").trim().parse::<u64>().ok().map(|k| k * 1024)
        };
        let meminfo = std::fs::read_to_string("/proc/meminfo").ok()?;
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        Some(Memory {
            available: kib(&meminfo, "MemAvailable:")?,
            total: kib(&meminfo, "MemTotal:")?,
            resident: kib(&status, "VmRSS:")?,
            peak: kib(&status, "VmHWM:")?,
        })
    }

    #[cfg(not(target_os = "linux"))]
    fn read(&mut self) -> Option<Memory> {
        None
    }

    fn reset_peak(&mut self) {
        // "5" resets the peak resident size (VmHWM) to the current one (proc(5), clear_refs)
        #[cfg(target_os = "linux")]
        let _ = std::fs::write("/proc/self/clear_refs", "5");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1 << 30;

    /// Readings played back in order; the peak is reset to the resident size.
    struct Script(Vec<Memory>, usize);

    impl Probe for Script {
        fn read(&mut self) -> Option<Memory> {
            let m = self.0.get(self.1).copied();
            self.1 += 1;
            m
        }
        fn reset_peak(&mut self) {}
    }

    fn mem(available: u64, resident: u64, peak: u64) -> Memory {
        Memory { available: available * GB, total: 46 * GB, resident: resident * GB, peak: peak * GB }
    }

    #[test]
    fn what_fits_keeps_the_reserve_and_stays_between_one_and_the_ceiling() {
        // 46 GB total: the reserve is 4.6 GB
        assert_eq!(fit(&mem(40, 0, 0), 2 * GB, 16), 16);
        assert_eq!(fit(&mem(24, 0, 0), 2 * GB, 16), 9);
        assert_eq!(fit(&mem(5, 0, 0), 2 * GB, 16), 1, "never below one frame");
        assert_eq!(fit(&mem(0, 0, 0), 2 * GB, 16), 1);
        assert_eq!(fit(&mem(40, 0, 0), 1, 16), 16, "never above the ceiling");
        assert_eq!(fit(&mem(40, 0, 0), 0, 4), 4);
    }

    #[test]
    fn batches_follow_the_measured_cost_down_and_back_up_to_the_ceiling() {
        let mut b = Batch::with_probe(
            16,
            Script(
                vec![
                    // start: 40 GB free, first guess 2 GB a frame → 17 → 16 (ceiling)
                    mem(40, 1, 1),
                    // the 16 frames peaked 31 GB above the start (≈1.9 GB each, 2.4 with the
                    // margin); 30 GB free → 10
                    mem(30, 2, 32),
                    // another program took memory: 12 GB free → 3
                    mem(12, 2, 21),
                    // it let go: 42 GB free → back up (2.5 GB a frame with the margin → 14)
                    mem(42, 2, 8),
                    // more still: back to the ceiling, never above
                    mem(45, 2, 4),
                ],
                0,
            ),
        );
        assert_eq!(b.next(), 16);
        assert_eq!(b.next(), 10);
        assert_eq!(b.next(), 3);
        assert_eq!(b.next(), 14);
        assert_eq!(b.next(), 16);
        // no readings: the ceiling
        assert_eq!(b.next(), 16);
    }

    #[test]
    fn the_first_batch_is_what_free_memory_surely_holds() {
        // 14 GB free: (14 − 4.6) / 2 → 4 frames, not 16, before anything is measured
        let mut b = Batch::with_probe(16, Script(vec![mem(14, 1, 1)], 0));
        assert_eq!(b.next(), 4);
    }

    #[test]
    fn this_machine_gives_readings_or_none() {
        let mut s = System;
        if let Some(m) = s.read() {
            assert!(m.total > 0 && m.available <= m.total && m.resident > 0 && m.peak >= m.resident / 2, "{m:?}");
            s.reset_peak();
        }
    }
}
