//! GPU time per pass for F3, from timestamp queries: what the GPU spends

use ash::vk;
use std::cell::{Cell, RefCell};

/// Timestamps one frame may write.
const MAX_MARKS: u32 = 24;
/// How much each frame moves the average: about the last 30 frames count.
const SMOOTHING: f32 = 1.0 / 30.0;

pub(super) struct GpuTimer {
    /// One per frame in flight; empty when the queue can't time.
    pools: Vec<vk::QueryPool>,
    /// What each slot's pool holds from its last frame, in order.
    written: Vec<Vec<&'static str>>,
    /// The frame being recorded: its pool, command buffer and labels so far.
    pool: Cell<vk::QueryPool>,
    cmd: Cell<vk::CommandBuffer>,
    labels: RefCell<Vec<&'static str>>,
    /// Nanoseconds per timestamp tick.
    period_ns: f64,
    /// The bits a timestamp holds (`timestampValidBits`).
    valid_mask: u64,
    /// Smoothed milliseconds per label, in the order first seen.
    phases: Vec<(&'static str, f32)>,
    total_ms: f32,
    /// The newest frame read back, unsmoothed, for the benchmark.
    last: Vec<(&'static str, f32)>,
    last_total_ms: f32,
}

impl GpuTimer {
    /// `valid_bits` is the queue family's `timestampValidBits`; 0, or a
    pub(super) unsafe fn new(
        device: &ash::Device,
        limits: &vk::PhysicalDeviceLimits,
        valid_bits: u32,
        frames: usize,
    ) -> Self {
        let supported = valid_bits > 0 && limits.timestamp_compute_and_graphics == vk::TRUE;
        let info = vk::QueryPoolCreateInfo::default()
            .query_type(vk::QueryType::TIMESTAMP)
            .query_count(MAX_MARKS);
        let pools = if supported {
            (0..frames)
                .map_while(|_| unsafe { device.create_query_pool(&info, None) }.ok())
                .collect()
        } else {
            Vec::new()
        };
        Self {
            written: vec![Vec::new(); pools.len()],
            pools,
            pool: Cell::new(vk::QueryPool::null()),
            cmd: Cell::new(vk::CommandBuffer::null()),
            labels: RefCell::default(),
            period_ns: f64::from(limits.timestamp_period),
            valid_mask: if valid_bits >= 64 {
                u64::MAX
            } else {
                (1u64 << valid_bits) - 1
            },
            phases: Vec::new(),
            total_ms: 0.0,
            last: Vec::new(),
            last_total_ms: 0.0,
        }
    }

    pub(super) unsafe fn destroy(&mut self, device: &ash::Device) {
        for pool in self.pools.drain(..) {
            unsafe { device.destroy_query_pool(pool, None) };
        }
    }

    /// Starts frame slot `slot` on `cmd`, outside any render pass: takes in
    pub(super) unsafe fn begin(
        &mut self,
        device: &ash::Device,
        cmd: vk::CommandBuffer,
        slot: usize,
    ) {
        let Some(&pool) = self.pools.get(slot) else {
            return;
        };
        let written = std::mem::take(&mut self.written[slot]);
        if written.len() > 1 {
            let mut ticks = vec![0u64; written.len()];
            let read = unsafe {
                device.get_query_pool_results(pool, 0, &mut ticks, vk::QueryResultFlags::TYPE_64)
            };
            if read.is_ok() {
                self.take_in(&written, &ticks);
            }
        }
        unsafe { device.cmd_reset_query_pool(cmd, pool, 0, MAX_MARKS) };
        self.pool.set(pool);
        self.cmd.set(cmd);
        self.labels.borrow_mut().clear();
        unsafe { self.write(device, vk::PipelineStageFlags::TOP_OF_PIPE, "start") };
    }

    /// Ends the GPU work named `label`, which ran since the last mark.
    pub(super) unsafe fn mark(&self, device: &ash::Device, label: &'static str) {
        unsafe { self.write(device, vk::PipelineStageFlags::BOTTOM_OF_PIPE, label) };
    }

    /// Ends the frame's recording; its times are read when its slot comes
    pub(super) fn end(&mut self, slot: usize) {
        if let Some(written) = self.written.get_mut(slot) {
            *written = std::mem::take(&mut *self.labels.borrow_mut());
        }
        self.pool.set(vk::QueryPool::null());
    }

    unsafe fn write(
        &self,
        device: &ash::Device,
        stage: vk::PipelineStageFlags,
        label: &'static str,
    ) {
        let pool = self.pool.get();
        let mut labels = self.labels.borrow_mut();
        if pool == vk::QueryPool::null() || labels.len() >= MAX_MARKS as usize {
            return;
        }
        unsafe { device.cmd_write_timestamp(self.cmd.get(), stage, pool, labels.len() as u32) };
        labels.push(label);
    }

    /// Folds one frame's timestamps into the averages: each label gets the
    fn take_in(&mut self, labels: &[&'static str], ticks: &[u64]) {
        let ms = |from: u64, to: u64| {
            let delta = to.wrapping_sub(from) & self.valid_mask;
            (delta as f64 * self.period_ns / 1_000_000.0) as f32
        };
        let mut frame: Vec<(&'static str, f32)> = Vec::new();
        for (pair, &label) in ticks.windows(2).zip(&labels[1..]) {
            let spent = ms(pair[0], pair[1]);
            match frame.iter_mut().find(|(name, _)| *name == label) {
                Some((_, sum)) => *sum += spent,
                None => frame.push((label, spent)),
            }
        }
        for (name, average) in &mut self.phases {
            let spent = frame
                .iter()
                .find(|(n, _)| n == name)
                .map_or(0.0, |&(_, ms)| ms);
            *average += (spent - *average) * SMOOTHING;
        }
        for &(name, spent) in &frame {
            if !self.phases.iter().any(|(n, _)| *n == name) {
                self.phases.push((name, spent));
            }
        }
        let total = ms(ticks[0], ticks[ticks.len() - 1]);
        self.total_ms += (total - self.total_ms) * SMOOTHING;
        self.last = frame;
        self.last_total_ms = total;
    }

    /// The newest frame the GPU finished: its time and each pass's. It is
    pub(super) fn last_frame(&self) -> (f32, &[(&'static str, f32)]) {
        (self.last_total_ms, &self.last)
    }

    /// F3's lines: the frame's GPU time, then each pass with its share.
    pub(super) fn lines(&self) -> Vec<String> {
        if self.pools.is_empty() {
            return Vec::new();
        }
        let mut lines = vec![format!("Frame: {:.2} ms (GPU)", self.total_ms)];
        let total = self.total_ms.max(f32::EPSILON);
        lines.extend(
            self.phases
                .iter()
                .map(|(name, ms)| format!("  {name}: {ms:.2} ms ({:.0}%)", ms / total * 100.0)),
        );
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timer() -> GpuTimer {
        GpuTimer {
            pools: vec![vk::QueryPool::null()],
            written: vec![Vec::new()],
            pool: Cell::new(vk::QueryPool::null()),
            cmd: Cell::new(vk::CommandBuffer::null()),
            labels: RefCell::default(),
            period_ns: 1.0,
            valid_mask: u64::MAX,
            phases: Vec::new(),
            total_ms: 0.0,
            last: Vec::new(),
            last_total_ms: 0.0,
        }
    }

    #[test]
    fn each_label_gets_the_time_since_the_mark_before() {
        let mut timer = timer();
        let labels = ["start", "upload", "solid", "ui"];
        let ticks = [10_000_000, 11_000_000, 14_000_000, 16_000_000];
        timer.take_in(&labels, &ticks);
        assert_eq!(
            timer.phases,
            vec![("upload", 1.0), ("solid", 3.0), ("ui", 2.0)]
        );
        assert!((timer.total_ms - 6.0 * SMOOTHING).abs() < 1e-6);
    }

    #[test]
    fn a_wrapped_counter_still_measures_forward() {
        let mut timer = timer();
        timer.valid_mask = (1 << 36) - 1;
        timer.take_in(&["start", "solid"], &[(1 << 36) - 500, 1_500]);
        assert!((timer.phases[0].1 - 0.002).abs() < 1e-6);
    }

    #[test]
    fn a_pass_missing_this_frame_fades_out() {
        let mut timer = timer();
        timer.take_in(&["start", "solid"], &[0, 1_000_000]);
        timer.take_in(&["start", "ui"], &[0, 1_000_000]);
        assert!((timer.phases[0].1 - (1.0 - SMOOTHING)).abs() < 1e-6);
        assert_eq!(timer.phases[1], ("ui", 1.0));
    }
}
