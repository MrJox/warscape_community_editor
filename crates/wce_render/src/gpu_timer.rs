//! GPU time per named section of a frame, from D3D11 timestamp queries. [`GpuTimer::begin_frame`] and [`GpuTimer::end_frame`] bracket a
//! frame, [`GpuTimer::mark`] starts a section (and ends the previous one). Results arrive a few frames later (a ring of queries):
//! [`GpuTimer::last`] holds the newest complete frame, [`GpuTimer::report`] a mean per section since the last reset.

use std::collections::BTreeMap;

use wce_d3d11::*;

const RING: usize = 6;

#[derive(Clone, Debug)]
struct Section {
    name: String,
    start: Option<ID3D11Query>,
    end: Option<ID3D11Query>,
}

#[derive(Clone, Debug, Default)]
struct Frame {
    disjoint: Option<ID3D11Query>,
    begin: Option<ID3D11Query>,
    finish: Option<ID3D11Query>,
    sections: Vec<Section>,
    pending: bool,
}

#[derive(Clone, Debug)]
pub struct GpuTimer {
    dev: ID3D11Device,
    ctx: ID3D11DeviceContext,
    ring: [Frame; RING],
    cur: usize,
    open: bool,
    in_frame: bool,
    last: Vec<(String, f64)>,
    last_total: f64,
    sum: BTreeMap<String, f64>,
    sum_total: f64,
    frames: u32,
}

/// Starts a query, if it exists.
///
/// # Safety
///
/// The query must belong to the context's device.
unsafe fn begin(ctx: &ID3D11DeviceContext, q: &Option<ID3D11Query>) {
    if let Some(q) = q {
        // SAFETY: as the caller ensures.
        unsafe { ctx.Begin(q) };
    }
}

/// Ends a query, if it exists.
///
/// # Safety
///
/// The query must belong to the context's device.
unsafe fn end(ctx: &ID3D11DeviceContext, q: &Option<ID3D11Query>) {
    if let Some(q) = q {
        // SAFETY: as the caller ensures.
        unsafe { ctx.End(q) };
    }
}

fn new_query(dev: &ID3D11Device, kind: D3D11_QUERY, q: &mut Option<ID3D11Query>) -> Option<ID3D11Query> {
    if q.is_none() {
        let d = D3D11_QUERY_DESC { Query: kind, MiscFlags: 0 };
        // SAFETY: a plain creation call.
        let _ = unsafe { dev.CreateQuery(&d, Some(q)) };
    }

    q.clone()
}

impl GpuTimer {

    pub fn new(dev: &ID3D11Device, ctx: &ID3D11DeviceContext) -> Self {
        Self {
            dev: dev.clone(),
            ctx: ctx.clone(),
            ring: Default::default(),
            cur: 0,
            open: false,
            in_frame: false,
            last: vec![],
            last_total: 0.0,
            sum: BTreeMap::new(),
            sum_total: 0.0,
            frames: 0,
        }
    }

    pub fn begin_frame(&mut self) {
        self.collect();
        let f = &mut self.ring[self.cur];

        // The GPU is more than a ring behind: this frame isn't timed.
        if f.pending {
            self.in_frame = false;
            return;
        }

        f.sections.clear();
        // SAFETY: queries of this device.
        unsafe {
            begin(&self.ctx, &new_query(&self.dev, D3D11_QUERY_TIMESTAMP_DISJOINT, &mut f.disjoint));
            end(&self.ctx, &new_query(&self.dev, D3D11_QUERY_TIMESTAMP, &mut f.begin));
        }

        self.in_frame = true;
        self.open = false;
    }

    /// Ends the open section and starts `name`.
    pub fn mark(&mut self, name: &str) {
        if !self.in_frame {
            return;
        }

        let f = &mut self.ring[self.cur];
        // SAFETY: queries of this device.
        unsafe {
            if self.open
                && let Some(section) = f.sections.last()
            {
                end(&self.ctx, &section.end);
            }

            let mut section = Section { name: name.to_owned(), start: None, end: None };
            new_query(&self.dev, D3D11_QUERY_TIMESTAMP, &mut section.start);
            new_query(&self.dev, D3D11_QUERY_TIMESTAMP, &mut section.end);
            end(&self.ctx, &section.start);
            f.sections.push(section);
        }

        self.open = true;
    }

    /// Ends the open section and closes the frame.
    pub fn end_frame(&mut self) {
        if !self.in_frame {
            return;
        }

        let f = &mut self.ring[self.cur];
        // SAFETY: queries of this device.
        unsafe {
            if self.open
                && let Some(section) = f.sections.last()
            {
                end(&self.ctx, &section.end);
            }

            end(&self.ctx, &new_query(&self.dev, D3D11_QUERY_TIMESTAMP, &mut f.finish));
            end(&self.ctx, &f.disjoint);
        }

        self.open = false;
        f.pending = true;
        self.in_frame = false;
        self.cur = (self.cur + 1) % RING;
    }

    fn read(&self, q: &Option<ID3D11Query>) -> Option<u64> {
        let mut t = 0u64;
        // SAFETY: the output is a local of the query's data size.
        let hr = unsafe { self.ctx.GetData(q.as_ref()?, Some((&raw mut t).cast()), 8, 0) };
        (hr.0 == 0).then_some(t)
    }

    fn collect(&mut self) {
        for k in 0..RING {
            let index = (self.cur + k) % RING;
            if !self.ring[index].pending {
                continue;
            }

            let mut dj = D3D11_QUERY_DATA_TIMESTAMP_DISJOINT::default();
            let Some(disjoint) = self.ring[index].disjoint.clone() else { continue };
            // SAFETY: the output is a local of the query's data size.
            let hr = unsafe {
                self.ctx.GetData(&disjoint, Some((&raw mut dj).cast()), size_of::<D3D11_QUERY_DATA_TIMESTAMP_DISJOINT>() as u32,
                    D3D11_ASYNC_GETDATA_DONOTFLUSH as u32)
            };

            if hr.0 != 0 {
                break;
            }

            self.ring[index].pending = false;
            if dj.Disjoint.as_bool() {
                continue;
            }

            let frame = self.ring[index].clone();
            let (Some(t0), Some(t1)) = (self.read(&frame.begin), self.read(&frame.finish)) else { continue };
            let to_ms = 1000.0 / dj.Frequency as f64;
            self.last.clear();
            self.last_total = (t1 - t0) as f64 * to_ms;
            for s in &frame.sections {
                let (Some(a), Some(b)) = (self.read(&s.start), self.read(&s.end)) else { continue };
                let ms = (b - a) as f64 * to_ms;
                self.last.push((s.name.clone(), ms));
                *self.sum.entry(s.name.clone()).or_default() += ms;
            }

            self.sum_total += self.last_total;
            self.frames += 1;
        }
    }

    pub fn frame_ms(&self) -> f64 {
        self.last_total
    }

    pub fn last(&self) -> &[(String, f64)] {
        &self.last
    }

    pub fn reset(&mut self) {
        self.sum.clear();
        self.frames = 0;
    }

    /// The mean over the frames since the reset, sorted by time, and the last frame in order.
    pub fn report(&self) -> String {
        if self.frames == 0 {
            return "  (no GPU timing yet)\n".to_owned();
        }

        let frames = f64::from(self.frames);
        let mut v = self.sum.iter().collect::<Vec<_>>();
        v.sort_by(|a, b| b.1.total_cmp(a.1));
        let mut out = format!("  GPU frame (mean of {}): {:.2} ms\n", self.frames, self.sum_total / frames);
        for (name, ms) in v {
            out.push_str(&format!("    {name:<30} {:8.3} ms\n", ms / frames));
        }

        out.push_str(&format!("  last frame in order ({:.2} ms in all):", self.last_total));
        for (name, ms) in &self.last {
            out.push_str(&format!(" {name} {ms:.2} |"));
        }

        out.push('\n');
        out
    }
}
