//! A campaign map's lighting as the game evaluates it (docs/plans/campaign_environment.md, "Engine"): `environment_collection.xml`'s
//! global lighting and spheres, each sphere's file resolved for the season and climate stage, and the spheres blended at the camera.
//!
//! - The collection (parser fns 0x116c5a50 / 0x116c5530 / 0x116bd080): `ENVIRONMENT_COLLECTION@global_lighting`,
//!   `ENVIRONMENT_SPHERES/SPHERE@lighting x y z inner_radius outer_radius`, in the `.terry`'s units (the campaign world's, a cell
//!   0.33391 on Attila's map). Its loader (fn 0x11760cf0) multiplies each sphere's z by `CAMPAIGN_Z_SCALE` (2 / sqrt 3): the game's
//!   campaign world is the hex grid's, its z that much longer (`map_data.esf`: 556.14 / 1437 rows against 678.5 / 2032 columns).
//! - A file's name per season (fn 0x118f6360): `<stem>_<season><n>.lighting`, else `<stem>_<season>.lighting`, else the name as
//!   written; the seasons `sp su au wi`, `n` the climate stage's (the same number as `snow_mask_<season><n>.dds`). ASSUMPTION: the
//!   order of the three. A name that already ends in a season (Attila's `europe_sp.lighting`) finds no `europe_sp_sp*`: it is spring
//!   all year, as written.
//! - The blend (fns 0x1173d760, 0x117609c0): a sphere's weight `1 - clamp((d² - inner²) / (outer² - inner²), 0, 1)`; the weighted
//!   sum of the spheres' lightings, divided by the weights' sum when that is above 1, else topped up with the global lighting's
//!   `1 - sum`. Every frame, at the camera (ASSUMPTION: its eye, not the point it looks at). No fade when the season changes.

use wce_formats::xml_doc::XmlDoc;

use crate::environment::{EnvironmentParams, blend_environments};

/// `CAMPAIGN_Z_SCALE` (tweakable, registered at 0x100b8290): the sphere's z into the game's campaign world.
pub const CAMPAIGN_Z_SCALE: f64 = 1.154_700_538_379_251_5;

/// The season suffixes, season 0 to 3 (fn 0x118f6360).
pub const SEASONS: [&str; 4] = ["sp", "su", "au", "wi"];

/// One `SPHERE`: its lighting file (as written) and where it is (`.terry` units, z as the file has it).
#[derive(Clone, Debug, PartialEq)]
pub struct LightingSphere {
    pub lighting: String,
    pub centre: [f64; 3],
    pub inner: f64,
    pub outer: f64,
}

/// `environment_collection.xml`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EnvironmentCollection {
    pub global: String,
    pub spheres: Vec<LightingSphere>,
}

/// Reads an `environment_collection.xml` (None: no `ENVIRONMENT_COLLECTION`). Paths are lower-cased with forward slashes, as the packs
/// key them.
pub fn read_collection(doc: &XmlDoc) -> Option<EnvironmentCollection> {
    let els = doc.elements();
    let root = els.iter().position(|e| e.name == "ENVIRONMENT_COLLECTION")?;
    let path = |s: String| s.replace('\\', "/").to_lowercase();
    let num = |e: usize, n: &str| doc.get(e, n).and_then(|v| v.trim().parse::<f64>().ok()).unwrap_or(0.0);
    let spheres = (0..els.len())
        .filter(|&e| els[e].name == "SPHERE")
        .filter_map(|e| {
            Some(LightingSphere {
                lighting: path(doc.get(e, "lighting")?),
                centre: [num(e, "x"), num(e, "y"), num(e, "z")],
                inner: num(e, "inner_radius"),
                outer: num(e, "outer_radius"),
            })
        })
        .collect();
    Some(EnvironmentCollection { global: doc.get(root, "global_lighting").map(path).unwrap_or_default(), spheres })
}

/// The files the engine tries for a lighting name in a season (`season`: 0 to 3) and climate stage number `n`, in order.
pub fn season_candidates(name: &str, season: usize, n: u32) -> [String; 3] {
    let s = SEASONS[season.min(3)];
    let stem = name.strip_suffix(".lighting").unwrap_or(name);
    [format!("{stem}_{s}{n}.lighting"), format!("{stem}_{s}.lighting"), name.to_owned()]
}

/// A sphere's weight at a squared distance (fn 0x1173d760).
pub fn sphere_weight(d2: f64, inner: f64, outer: f64) -> f64 {
    let (i2, o2) = (inner * inner, outer * outer);
    if o2 <= i2 {
        return if d2 <= i2 { 1.0 } else { 0.0 };
    }

    1.0 - ((d2 - i2) / (o2 - i2)).clamp(0.0, 1.0)
}

impl EnvironmentCollection {
    /// Each sphere's weight at a point (`.terry` units), and the global lighting's (`1 - sum` when the sum is below 1, else 0; the
    /// spheres' then divided by the sum).
    pub fn weights(&self, at: [f64; 3]) -> (Vec<f64>, f64) {
        let mut w: Vec<f64> = self
            .spheres
            .iter()
            .map(|s| {
                let d = [at[0] - s.centre[0], at[1] - s.centre[1], (at[2] - s.centre[2]) * CAMPAIGN_Z_SCALE];
                sphere_weight(d[0] * d[0] + d[1] * d[1] + d[2] * d[2], s.inner, s.outer)
            })
            .collect();
        let sum: f64 = w.iter().sum();
        if sum > 1.0 {
            w.iter_mut().for_each(|x| *x /= sum);
            (w, 0.0)
        } else {
            (w, 1.0 - sum)
        }
    }
}

/// The weighted sum of environments (weights summing to 1; those of 0 left out), as a chain of the cross-fade: each next one blended in
/// by its share of the weight so far, which for the numeric fields is the weighted sum. ASSUMPTION: the sun's azimuth goes the shorter
/// way round (the cross-fade's), and switches and names are the heaviest's. The sun colour is blended linear (each file's to the power
/// 2.2, the sum back to 1 / 2.2: RenderDoc, camp_attila over Britain, england_sp 0.958 + europe_sp 0.042).
pub fn blend_weighted(items: &[(f64, &EnvironmentParams)]) -> Option<EnvironmentParams> {
    let mut items: Vec<(f64, &EnvironmentParams)> = items.iter().copied().filter(|(w, _)| *w > 1e-6).collect();
    items.sort_by(|a, b| b.0.total_cmp(&a.0));
    let gamma = |c: wce_foundation::math::Float3, k: f32| {
        wce_foundation::math::Float3::new(c.x.max(0.0).powf(k), c.y.max(0.0).powf(k), c.z.max(0.0).powf(k))
    };
    let linear: Vec<(f64, EnvironmentParams)> = items
        .iter()
        .map(|&(w, p)| {
            let mut p = p.clone();
            p.sun_colour = gamma(p.sun_colour, 2.2);
            (w, p)
        })
        .collect();
    let (first, rest) = linear.split_first()?;
    let mut acc = first.1.clone();
    let mut total = first.0;
    for (w, p) in rest {
        total += w;
        let t = (w / total) as f32;
        // The heaviest's switches and names: blend_environments takes the nearer side's, and t stays below a half here.
        acc = blend_environments(&acc, p, t);
    }

    acc.sun_colour = gamma(acc.sun_colour, 1.0 / 2.2);
    Some(acc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_per_season() {
        let c = season_candidates("terrain/campaigns/m/lighting/europe.lighting", 3, 2);
        assert_eq!(c[0], "terrain/campaigns/m/lighting/europe_wi2.lighting");
        assert_eq!(c[1], "terrain/campaigns/m/lighting/europe_wi.lighting");
        assert_eq!(c[2], "terrain/campaigns/m/lighting/europe.lighting");
        // Attila's names already carry a season: never found with another, so spring all year.
        assert_eq!(season_candidates("l/europe_sp.lighting", 1, 1)[0], "l/europe_sp_su1.lighting");
    }

    #[test]
    fn weights_inside_between_outside() {
        assert_eq!(sphere_weight(0.0, 100.0, 150.0), 1.0);
        assert_eq!(sphere_weight(200.0 * 200.0, 100.0, 150.0), 0.0);
        let mid = (100.0f64 * 100.0 + 150.0 * 150.0) / 2.0;
        assert!((sphere_weight(mid, 100.0, 150.0) - 0.5).abs() < 1e-12);

        let c = EnvironmentCollection {
            global: "g".into(),
            spheres: vec![
                LightingSphere { lighting: "a".into(), centre: [0.0; 3], inner: 10.0, outer: 20.0 },
                LightingSphere { lighting: "b".into(), centre: [1.0, 0.0, 0.0], inner: 10.0, outer: 20.0 },
            ],
        };
        let (w, g) = c.weights([0.5, 0.0, 0.0]);
        assert!((w[0] - 0.5).abs() < 1e-12 && (w[1] - 0.5).abs() < 1e-12 && g == 0.0);
        let (w, g) = c.weights([0.0, 0.0, 100.0]);
        assert!(w.iter().all(|&x| x == 0.0) && g == 1.0);
        // z counts 2 / sqrt 3 times: 15 along z is 17.3 away.
        let one = EnvironmentCollection { global: "g".into(), spheres: vec![c.spheres[0].clone()] };
        assert!(one.weights([0.0, 0.0, 15.0]).0[0] < one.weights([15.0, 0.0, 0.0]).0[0]);
    }

    #[test]
    fn weighted_sum() {
        let mut a = EnvironmentParams::default();
        let mut b = EnvironmentParams::default();
        let mut c = EnvironmentParams::default();
        a.sun_colour_scale = 1.0;
        b.sun_colour_scale = 2.0;
        c.sun_colour_scale = 4.0;
        let r = blend_weighted(&[(0.5, &a), (0.25, &b), (0.25, &c)]).unwrap();
        assert!((r.sun_colour_scale - 2.0).abs() < 1e-5);
    }
}
