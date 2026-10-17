//! Frame times of the last frames (the app loop records them): the menu bar shows the mean, the `perf` command prints mean / p95 / worst.

const KEEP: usize = 600;

#[derive(Clone, Debug)]
pub struct PerfStats {
    frame: [f32; KEEP],
    cpu: [f32; KEEP],
    n: u64,
}

impl Default for PerfStats {
    fn default() -> Self {
        Self { frame: [0.0; KEEP], cpu: [0.0; KEEP], n: 0 }
    }
}

impl PerfStats {
    pub fn record(&mut self, frame_ms: f64, cpu_ms: f64) {
        let i = (self.n % KEEP as u64) as usize;
        self.frame[i] = frame_ms as f32;
        self.cpu[i] = cpu_ms as f32;
        self.n += 1;
    }

    pub fn reset(&mut self) {
        self.n = 0;
    }

    pub fn count(&self) -> usize {
        (self.n as usize).min(KEEP)
    }

    fn mean(&self, v: &[f32; KEEP], last: usize) -> f64 {
        let n = self.count().min(last);
        let s = (0..n).map(|i| f64::from(v[((self.n - 1 - i as u64) % KEEP as u64) as usize])).sum::<f64>();
        if n != 0 { s / n as f64 } else { 0.0 }
    }

    pub fn mean_frame_ms(&self, last: usize) -> f64 {
        self.mean(&self.frame, last)
    }

    pub fn mean_cpu_ms(&self, last: usize) -> f64 {
        self.mean(&self.cpu, last)
    }

    /// p in 0..1.
    pub fn percentile_frame_ms(&self, p: f64) -> f64 {
        let n = self.count();
        if n == 0 {
            return 0.0;
        }

        let mut v = self.frame[..n].to_vec();
        v.sort_by(f32::total_cmp);
        f64::from(v[((p * n as f64) as usize).min(n - 1)])
    }

    pub fn worst_frame_ms(&self) -> f64 {
        f64::from(self.frame[..self.count()].iter().fold(0.0f32, |w, &f| w.max(f)))
    }

    pub fn report(&self) -> String {
        let m = self.mean_frame_ms(KEEP);
        format!(
            "  frames: {}  mean {m:.2} ms ({:.1} fps)  p95 {:.2} ms  p99 {:.2} ms  worst {:.2} ms  | CPU mean {:.2} ms\n", self.count(), if m > 0.0 { 1000.0 / m } else { 0.0 },
            self.percentile_frame_ms(0.95), self.percentile_frame_ms(0.99), self.worst_frame_ms(), self.mean_cpu_ms(KEEP)
        )
    }
}
