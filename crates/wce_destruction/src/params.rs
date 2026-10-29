//! What the artist sets: per damage level a preset and its volumes, the fragments, the collapse's motion; for the building the interior,
//! the rubble, the materials and the effects. [`Params::defaults`] fills them from the building's size and kind.

use crate::geom::{Aabb, V3, v3};

/// How a level breaks, as a starting point for its volumes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Preset {
    /// A gap through the middle, from the top down, wider at each level.
    #[default]
    Breach,
    /// The top falls in, over the whole footprint.
    TopCollapse,
    /// One corner falls.
    Corner,
    /// Everything but a stub.
    Full,
    /// Volumes placed by hand.
    Custom,
}

impl Preset {
    pub const ALL: [Self; 5] = [Self::Breach, Self::TopCollapse, Self::Corner, Self::Full, Self::Custom];

    pub fn label(self) -> &'static str {
        match self {
            Self::Breach => "Breach",
            Self::TopCollapse => "Top collapse",
            Self::Corner => "Corner",
            Self::Full => "Full collapse",
            Self::Custom => "Custom",
        }
    }
}

/// A damage volume in the building's frame: a box or an ellipsoid, turned about the vertical.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Volume {
    pub centre: [f32; 3],
    pub half: [f32; 3],
    pub yaw: f32,
    pub ellipsoid: bool,
}

impl Volume {
    fn local(&self, p: &V3) -> V3 {
        let d = p - v3(self.centre);
        let (s, c) = self.yaw.sin_cos();
        V3::new(c * d.x - s * d.z, d.y, s * d.x + c * d.z)
    }

    /// A signed distance (negative inside; exact for a box, scaled for an ellipsoid).
    pub fn sdf(&self, p: &V3) -> f32 {
        let q = self.local(p);
        let h = v3(self.half).map(|x| x.max(1e-3));
        if self.ellipsoid {
            let k = V3::new(q.x / h.x, q.y / h.y, q.z / h.z).norm();
            (k - 1.0) * h.min()
        } else {
            let d = q.abs() - h;
            d.sup(&V3::zeros()).norm() + d.max().min(0.0)
        }
    }

    pub fn contains(&self, p: &V3) -> bool {
        self.sdf(p) <= 0.0
    }

    /// The box around it.
    pub fn bounds(&self) -> Aabb {
        let (s, c) = self.yaw.sin_cos();
        let h = v3(self.half);
        let ex = (c * h.x).abs() + (s * h.z).abs();
        let ez = (s * h.x).abs() + (c * h.z).abs();
        let ctr = v3(self.centre);
        Aabb { min: ctr - V3::new(ex, h.y, ez), max: ctr + V3::new(ex, h.y, ez) }
    }

    pub fn volume(&self) -> f32 {
        let v = 8.0 * self.half[0] * self.half[1] * self.half[2];
        if self.ellipsoid { v * std::f32::consts::PI / 6.0 } else { v }
    }
}

/// A damage level (a level after the intact one): what breaks, and how its collapse moves.
#[derive(Clone, Debug, PartialEq)]
pub struct LevelParams {
    pub preset: Preset,
    pub volumes: Vec<Volume>,

    /// The typical fragment's size (m); [`Self::max_fragments`] can make them larger.
    pub fragment_size: f32,
    pub max_fragments: usize,

    /// How ragged the edge of the damage is (0: the volume's shape, 1: about a fragment either way).
    pub roughness: f32,

    /// The collapse's direction (the building's frame; it need not be unit), its speed (m/s), the spread of the directions (0..1),
    /// how long the fragments take to all let go (s), and how much they spin.
    pub direction: [f32; 3],
    pub force: f32,
    pub spread: f32,
    pub stagger: f32,
    pub spin: f32,
}

/// What breaks inside: thick parts as solid, shells (houses) with a wall thickness.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Interior {
    #[default]
    Auto,
    Solid,
    Hollow,
}

/// What the materials of the building are taken for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MaterialClass {
    #[default]
    Stone,
    Wood,
    Roof,
    Plaster,
    Other,
}

impl MaterialClass {
    pub const ALL: [Self; 5] = [Self::Stone, Self::Wood, Self::Roof, Self::Plaster, Self::Other];

    pub fn label(self) -> &'static str {
        match self {
            Self::Stone => "Stone",
            Self::Wood => "Wood",
            Self::Roof => "Roof",
            Self::Plaster => "Plaster",
            Self::Other => "Other",
        }
    }

    /// Density (t/m^3), friction, bounce.
    pub fn physics(self) -> (f32, f32, f32) {
        match self {
            Self::Stone | Self::Plaster => (2.3, 0.9, 0.05),
            Self::Wood => (0.6, 0.7, 0.15),
            Self::Roof => (1.6, 0.8, 0.1),
            Self::Other => (1.0, 0.8, 0.1),
        }
    }
}

/// An override of a material: by its diffuse texture's path, its class and the damage texture set (the base name: `roman_damage_bricks_all`)
/// its broken faces take.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MaterialOverride {
    pub diffuse: String,
    pub class: Option<MaterialClass>,
    pub damage: Option<String>,
}

/// The building's parameters.
#[derive(Clone, Debug, PartialEq)]
pub struct Params {
    /// The new building's key.
    pub key: String,
    pub seed: u64,

    /// The damage levels, 1 to 3 (the building then has 2 to 4 levels).
    pub levels: Vec<LevelParams>,
    pub interior: Interior,

    /// The wall thickness of shells (m).
    pub thickness: f32,

    /// The destroyed level's rubble: walkable (a breach), or a footprint units go around; `None`: by the building's category.
    pub walkable: Option<bool>,

    /// The culture of the damage textures (`roman`, `greek`, ...); `None`: from the key and the textures.
    pub culture: Option<String>,
    pub materials: Vec<MaterialOverride>,

    /// How much rubble piles up, relative to what falls (0: none).
    pub rubble: f32,

    /// The collapses' frame rate and their longest duration (s).
    pub fps: f32,
    pub max_duration: f32,

    /// Per damage level, the collapse's effects as edited (`None`: generated).
    pub effects: Vec<Option<Vec<wce_formats::anim_meta::MetaEntry>>>,
}

/// The ground of a building's frame: y = 0 (buildings stand on it, their foundations below).
pub const GROUND: f32 = 0.0;

impl Params {
    /// Defaults for a building of these bounds and category: 2 damage levels (3 levels in all) breaching it, fragments by its size.
    pub fn defaults(key: &str, bounds: &Aabb, category: &str) -> Self {
        let mut p = Self {
            key: key.to_owned(),
            seed: 1,
            levels: vec![],
            interior: Interior::Auto,
            thickness: 0.4,
            walkable: None,
            culture: None,
            materials: vec![],
            rubble: 1.0,
            fps: 20.0,
            max_duration: 10.0,
            effects: vec![],
        };
        p.set_level_count(2, bounds, category);
        p
    }

    /// Sets how many damage levels there are, giving the new ones the defaults (their presets' volumes for the new count).
    pub fn set_level_count(&mut self, n: usize, bounds: &Aabb, category: &str) {
        let n = n.clamp(1, 3);
        let preset = if is_fort(category) { Preset::Breach } else { Preset::TopCollapse };
        let size = bounds.size();
        let frag = (size.max() / 7.0).clamp(0.8, 3.0);
        while self.levels.len() < n {
            self.levels.push(LevelParams {
                preset,
                volumes: vec![],
                fragment_size: frag,
                max_fragments: 48,
                roughness: 0.6,
                direction: outward(bounds, category),
                force: 2.0,
                spread: 0.5,
                stagger: 1.2,
                spin: 1.0,
            });
        }

        self.levels.truncate(n);
        self.effects.resize(n, None);
        for k in 0..n {
            if self.levels[k].preset != Preset::Custom {
                self.levels[k].volumes = preset_volumes(self.levels[k].preset, bounds, k, n);
            }
        }
    }

    /// The new preset of a level, its volumes made for it.
    pub fn set_preset(&mut self, level: usize, preset: Preset, bounds: &Aabb) {
        let n = self.levels.len();
        if let Some(l) = self.levels.get_mut(level) {
            l.preset = preset;
            if preset != Preset::Custom {
                l.volumes = preset_volumes(preset, bounds, level, n);
            }
        }
    }
}

/// Fort pieces (walls, towers, gates): thick, solid, their rubble a breach.
pub fn is_fort(category: &str) -> bool {
    let c = category.to_lowercase();
    ["fort", "wall", "tower", "gate", "bastion"].iter().any(|k| c.contains(k))
}

/// Whether a building breaks as a solid (by its category and key).
pub fn solid_by_kind(category: &str, key: &str) -> bool {
    let k = key.to_lowercase();
    is_fort(category) || ["fort", "wall", "tower", "gate", "bastion", "rock", "cliff", "column", "statue", "monument", "obelisk", "pillar"].iter().any(|w| k.contains(w))
}

/// The default direction of a collapse: out of a wall's outer face (-z in the game's wall frames), else out along the longer side.
fn outward(bounds: &Aabb, category: &str) -> [f32; 3] {
    let s = bounds.size();
    if is_fort(category) || s.x >= s.z { [0.0, 0.0, -1.0] } else { [-1.0, 0.0, 0.0] }
}

/// The volumes of a preset for damage level `k` of `n` (0 the first damage level), in the building's frame: larger at each level; the
/// last level of a breach or a full collapse reaches the ground.
pub fn preset_volumes(preset: Preset, bounds: &Aabb, k: usize, n: usize) -> Vec<Volume> {
    if bounds.is_empty() {
        return vec![];
    }

    let f = (k + 1) as f32 / n.max(1) as f32;
    let (lo, hi) = (bounds.min, bounds.max);
    let ground = GROUND.max(lo.y);
    let top = hi.y + 0.5;
    let size = bounds.size();
    let c = bounds.centre();
    let along_x = size.x >= size.z;
    let (long, short) = if along_x { (size.x, size.z) } else { (size.z, size.x) };
    let depth = |from_top: f32| {
        let bottom = (top - from_top).max(ground + 0.3);
        ((top + bottom) * 0.5, (top - bottom) * 0.5)
    };

    match preset {
        Preset::Breach => {
            let (cy, hy) = depth((top - ground) * (0.35 + 0.65 * f));
            let w = long * (0.18 + 0.22 * f);
            let half = if along_x { [w, hy, short] } else { [short, hy, w] };
            vec![Volume { centre: [c.x, cy, c.z], half, yaw: 0.0, ellipsoid: false }]
        }

        Preset::TopCollapse => {
            let (cy, hy) = depth((top - ground) * (0.3 + 0.5 * f));
            vec![Volume { centre: [c.x, cy, c.z], half: [size.x * 0.6, hy, size.z * 0.6], yaw: 0.0, ellipsoid: false }]
        }

        Preset::Corner => {
            let (cy, hy) = depth((top - ground) * (0.4 + 0.6 * f));
            let r = 0.3 + 0.35 * f;
            vec![Volume { centre: [hi.x, cy, hi.z], half: [size.x * r, hy, size.z * r], yaw: 0.0, ellipsoid: false }]
        }

        Preset::Full => {
            let (cy, hy) = depth((top - ground) * (0.5 + 0.5 * f));
            vec![Volume { centre: [c.x, cy, c.z], half: [size.x * 0.6, hy, size.z * 0.6], yaw: 0.0, ellipsoid: false }]
        }

        Preset::Custom => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_grow_and_reach_down() {
        let b = Aabb { min: V3::new(-10.0, -2.0, -2.0), max: V3::new(10.0, 16.8, 10.5) };
        let p = Params::defaults("x", &b, "fort_wall");
        assert_eq!(p.levels.len(), 2);
        let (a, z) = (&p.levels[0].volumes[0], &p.levels[1].volumes[0]);
        assert!(z.volume() > a.volume());
        assert!(z.bounds().min.y < 1.0, "the last breach reaches near the ground: {:?}", z.bounds());
        assert!(a.contains(&V3::new(0.0, 15.0, 4.0)));
        assert!(!a.contains(&V3::new(9.5, 15.0, 4.0)));
        let e = Volume { centre: [0.0; 3], half: [2.0, 1.0, 1.0], yaw: 0.0, ellipsoid: true };
        assert!(e.contains(&V3::new(1.9, 0.0, 0.0)) && !e.contains(&V3::new(1.5, 0.9, 0.0)));
    }
}
