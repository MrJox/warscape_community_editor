//! An encampment's palisade (docs/plans/siege_ai.md, "Encampments"): CA's camps ring themselves with `siege_barricade_wall_1_encampment`
//! pieces 10 units apart, in runs; the openings between runs (about 30 units) are the ways in. Each run gets a palisade edge through the
//! pieces, each opening an entry node outside it.

use crate::raster::dist;
use crate::{P, PalisadePiece};

/// Pieces at most this far apart are in the same run (CA's step: 10).
const STEP: f64 = 15.0;

/// Two runs' ends at most this far apart (piece to piece) have an opening between them (CA's: ~40).
const MAX_OPENING: f64 = 70.0;

/// How far the palisade goes past a run's end piece (half a piece).
pub const END: f64 = 5.0;

/// A run of pieces, in order along the palisade.
#[derive(Clone, Debug)]
pub struct Run {
    pub pieces: Vec<P>,
}

impl Run {
    /// Its line: through the pieces, from half a piece before the first to half a piece past the last.
    pub fn line(&self) -> Vec<P> {
        let n = self.pieces.len();
        let mut out = self.pieces.clone();
        if n >= 2 {
            out.insert(0, beyond(self.pieces[1], self.pieces[0], END));
            out.push(beyond(self.pieces[n - 2], self.pieces[n - 1], END));
        }

        out
    }
}

/// An opening: its two sides (the runs' ends), its middle, the outward direction (unit) and its width.
#[derive(Clone, Copy, Debug)]
pub struct Opening {
    pub a: P,
    pub b: P,
    pub mid: P,
    pub out: P,
    pub width: f64,
}

/// `d` past `to` on the line from `from`.
fn beyond(from: P, to: P, d: f64) -> P {
    let l = dist(from, to).max(1e-9);
    [to[0] + (to[0] - from[0]) / l * d, to[1] + (to[1] - from[1]) / l * d]
}

fn unit(v: P) -> P {
    let l = v[0].hypot(v[1]).max(1e-9);
    [v[0] / l, v[1] / l]
}

/// The pieces chained into runs (of 2 pieces or more).
pub fn runs(pieces: &[PalisadePiece]) -> Vec<Run> {
    let n = pieces.len();
    let near = |i: usize| -> Vec<usize> { (0..n).filter(|&j| j != i && dist(pieces[i].at, pieces[j].at) <= STEP).collect() };
    let mut used = vec![false; n];
    let mut out = vec![];

    // From an end (one neighbour) first, then around what is left (a closed ring).
    let starts: Vec<usize> = (0..n).filter(|&i| near(i).len() <= 1).chain(0..n).collect();
    for s in starts {
        if used[s] {
            continue;
        }

        let mut chain = vec![s];
        used[s] = true;
        let mut at = s;
        while let Some(next) = near(at).into_iter().filter(|&j| !used[j]).min_by(|&a, &b| dist(pieces[at].at, pieces[a].at).total_cmp(&dist(pieces[at].at, pieces[b].at))) {
            used[next] = true;
            chain.push(next);
            at = next;
        }

        if chain.len() >= 2 {
            out.push(Run { pieces: chain.iter().map(|&i| pieces[i].at).collect() });
        }
    }

    out
}

/// The openings between runs: each run's end paired with the nearest end of another run ahead of it (closest pairs first).
pub fn openings(runs: &[Run]) -> Vec<Opening> {
    // End e of run r: (r, e), its last piece and the direction out of the run.
    let ends: Vec<(usize, P, P)> = runs.iter().enumerate().filter(|(_, r)| r.pieces.len() >= 2).flat_map(|(i, r)| {
        let n = r.pieces.len();
        [(i, r.pieces[0], unit([r.pieces[0][0] - r.pieces[1][0], r.pieces[0][1] - r.pieces[1][1]])), (i, r.pieces[n - 1], unit([r.pieces[n - 1][0] - r.pieces[n - 2][0], r.pieces[n - 1][1] - r.pieces[n - 2][1]]))]
    }).collect();
    let mut pairs = vec![];
    for x in 0..ends.len() {
        for y in x + 1..ends.len() {
            let ((ri, pi, di), (rj, pj, dj)) = (ends[x], ends[y]);
            let d = dist(pi, pj);
            if ri == rj || d > MAX_OPENING || d < 1.0 {
                continue;
            }

            let g = [(pj[0] - pi[0]) / d, (pj[1] - pi[1]) / d];
            if di[0] * g[0] + di[1] * g[1] > 0.5 && -(dj[0] * g[0] + dj[1] * g[1]) > 0.5 {
                pairs.push((d, x, y));
            }
        }
    }

    pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
    let all: Vec<P> = runs.iter().flat_map(|r| r.pieces.iter().copied()).collect();
    let n = all.len().max(1) as f64;
    let centre = [all.iter().map(|p| p[0]).sum::<f64>() / n, all.iter().map(|p| p[1]).sum::<f64>() / n];
    let mut taken = vec![false; ends.len()];
    let mut out = vec![];
    for (_, x, y) in pairs {
        if taken[x] || taken[y] {
            continue;
        }

        (taken[x], taken[y]) = (true, true);
        let ((_, pi, di), (_, pj, dj)) = (ends[x], ends[y]);
        let (a, b) = ([pi[0] + di[0] * END, pi[1] + di[1] * END], [pj[0] + dj[0] * END, pj[1] + dj[1] * END]);
        // Outward: across the opening, away from the camp's middle (the pieces' +x is not always the camp's side).
        let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
        let across = unit([-(b[1] - a[1]), b[0] - a[0]]);
        let away = (mid[0] - centre[0]) * across[0] + (mid[1] - centre[1]) * across[1];
        let out_dir = if away >= 0.0 { across } else { [-across[0], -across[1]] };
        out.push(Opening { a, b, mid, out: out_dir, width: dist(a, b) });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_camp_with_four_openings() {
        // A square of side 200 centred on (500, 500), pieces every 10, an opening of 30 in the middle of each side.
        let mut pieces = vec![];
        for side in 0..4 {
            let (c, s) = (std::f64::consts::FRAC_PI_2 * f64::from(side)).sin_cos();
            let along = [-c, s];
            for k in -9..=9 {
                if (-1..=1).contains(&k) {
                    continue;
                }

                let t = f64::from(k) * 10.0;
                let at = [500.0 + s * 100.0 + along[0] * t, 500.0 + c * 100.0 + along[1] * t];
                pieces.push(PalisadePiece { at });
            }
        }

        let rs = runs(&pieces);
        let os = openings(&rs);
        assert_eq!(os.len(), 4, "{} runs", rs.len());
        for o in &os {
            assert!((o.width - 30.0).abs() < 1.0, "{}", o.width);
            // Outward: away from the camp's centre.
            assert!((o.mid[0] - 500.0) * o.out[0] + (o.mid[1] - 500.0) * o.out[1] > 0.0);
        }
    }
}
