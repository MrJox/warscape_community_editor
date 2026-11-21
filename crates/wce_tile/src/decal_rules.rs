//! Decals by rules (docs/plans/auto_detail_2.md, E): puddles in hollows, dirt at building bases, ruts along roads, gravel where water
//! ran, forest floor under trees, laid by where the ground is as the texture rules lay textures.
//!
//! A rule's conditions are a texture rule's ([`Rule`]: height, slope, hollows, distances to water, roads and buildings, water flow,
//! deposit, breakup; its strength scales how many are laid), plus how far from a tree's trunk. Where they hold fully, `density` decals
//! per 100 m² are laid, fewer as the weight falls, none closer than `spacing` to another of the same rule; each picks one of the rule's
//! decals by weight, a size, and a direction (random, or along the roads, the buildings' sides, the water's edge, down the slope). The
//! same rules, ground and seed lay the same decals.

use std::fmt::Write as _;

use crate::texture_rules::{Distances, Feature, Fields, Range, Rule};
use wce_formats::xml_doc::XmlDoc;

/// The tile's decal rules, beside tile.wce.
pub const FILE: &str = "decal_rules.wce";

const ROOT: &str = "decal_rules";

/// The most decals all the rules lay together, and one rule.
pub const MOST: usize = 30_000;
pub const MOST_A_RULE: usize = 10_000;

/// Which way a decal's length runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Random,

    /// Down the slope (erosion runnels).
    Slope,

    /// Along the nearest road, the nearest building's side, the water's edge.
    Road,
    Building,
    Water,
}

impl Align {
    pub const ALL: [Self; 5] = [Self::Random, Self::Slope, Self::Road, Self::Building, Self::Water];

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Random => "random",
            Self::Slope => "slope",
            Self::Road => "road",
            Self::Building => "building",
            Self::Water => "water",
        }
    }

    fn named(s: &str) -> Self {
        Self::ALL.into_iter().find(|a| a.name().eq_ignore_ascii_case(s.trim())).unwrap_or(Self::Random)
    }
}

/// A decal rule.
#[derive(Clone, Debug, PartialEq)]
pub struct DecalRule {
    pub name: String,
    pub enabled: bool,

    /// The decals (a name under `rigidmodels\decals\battle`, or a whole key) and their weights.
    pub keys: Vec<(String, f32)>,

    /// Where (its conditions, its breakup and seed; its strength scales the density). Its texture group, variation and ground type are
    /// not read.
    pub place: Rule,

    /// Metres from the nearest tree's trunk.
    pub trees: Option<Range>,

    /// Decals per 100 m² where the conditions hold fully.
    pub density: f32,

    /// How wide a decal is (metres, the least and the most) and how many times longer than its own shape (1: as the kit draws it).
    pub size: (f32, f32),
    pub stretch: f32,

    /// The least metres between two of the rule's decals.
    pub spacing: f32,
    pub align: Align,
}

impl Default for DecalRule {
    fn default() -> Self {
        Self {
            name: String::new(),
            enabled: true,
            keys: vec![],
            place: Rule::default(),
            trees: None,
            density: 1.0,
            size: (2.0, 4.0),
            stretch: 1.0,
            spacing: 3.0,
            align: Align::Random,
        }
    }
}

/// A tile's decal rules and the preset they came from.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DecalRuleSet {
    pub preset: String,
    pub rules: Vec<DecalRule>,
}

/// A decal laid: its rule, its key (whole), where (metres from the map's corner, as the fields), which way its length runs (radians
/// about the up axis: its length along (sin, cos) in x and z) and its size (metres).
#[derive(Clone, Debug, PartialEq)]
pub struct Decal {
    pub rule: usize,
    pub key: String,
    pub x: f32,
    pub z: f32,
    pub yaw: f32,
    pub across: f32,
    pub length: f32,
}

/// A decal's whole key from a rule's name for it.
#[must_use]
pub fn key_of(name: &str) -> String {
    if name.contains('\\') || name.contains('/') {
        name.replace('/', "\\")
    } else {
        format!("rigidmodels\\decals\\battle\\{name}.rigid_model_v2")
    }
}

/// A key's short name: under `rigidmodels\decals\battle` without its extension, or the key as it is.
#[must_use]
pub fn name_of(key: &str) -> String {
    let k = key.replace('/', "\\");
    let lower = k.to_lowercase();
    match (lower.strip_prefix("rigidmodels\\decals\\battle\\"), lower.ends_with(".rigid_model_v2")) {
        (Some(_), true) => {
            let start = "rigidmodels\\decals\\battle\\".len();
            k[start..k.len() - ".rigid_model_v2".len()].to_owned()
        }
        _ => k,
    }
}

/// Whether any rule that lays something measures to a feature (the texture rules' [`Feature::used`] for decal rules).
#[must_use]
pub fn uses(rules: &[DecalRule], feature: Feature) -> bool {
    live(rules).any(|r| r.place.distance(feature).is_some() || (feature == Feature::Road && r.align == Align::Road))
        || (feature == Feature::Building && live(rules).any(|r| r.align == Align::Building))
        || (feature == Feature::Water && live(rules).any(|r| r.align == Align::Water))
}

/// Whether any rule that lays something measures to the trees.
#[must_use]
pub fn uses_trees(rules: &[DecalRule]) -> bool {
    live(rules).any(|r| r.trees.is_some())
}

/// Whether any rule that lays something reads where water runs.
#[must_use]
pub fn uses_runoff(rules: &[DecalRule]) -> bool {
    live(rules).any(|r| r.place.flow.is_some() || r.place.deposit.is_some())
}

fn live(rules: &[DecalRule]) -> impl Iterator<Item = &DecalRule> {
    rules.iter().filter(|r| r.enabled && r.density > 0.0 && r.place.strength > 0.0 && !r.keys.is_empty())
}

/// The decals the rules lay over a rectangle of the map (`rect`: metres from its corner, `[x0, z0, x1, z1]`) on `fields`; `trees`: the
/// distance to the trees' trunks on the fields' frame, when worked out. `cancel` is polled between rules and darts; None when it said
/// stop.
#[must_use]
pub fn lay(
    rules: &[DecalRule],
    fields: &Fields,
    trees: Option<&Distances>,
    rect: [f64; 4],
    seed: u64,
    cancel: &dyn Fn() -> bool,
) -> Option<Vec<Decal>> {
    let mut out: Vec<Decal> = vec![];
    let (w, h) = ((rect[2] - rect[0]).max(0.0), (rect[3] - rect[1]).max(0.0));
    for (n, r) in rules.iter().enumerate() {
        if !(r.enabled && r.density > 0.0 && r.place.strength > 0.0) || r.keys.iter().all(|k| !(k.1 > 0.0)) {
            continue;
        }

        let mut rng = Random::new(seed ^ (n as u64 + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ u64::from(r.place.seed));
        let darts = ((w * h / 100.0 * f64::from(r.density)) as usize).min(MOST_A_RULE * 8);
        let spacing = f64::from(r.spacing.max(0.0));
        let mut near = Buckets::new(spacing.max(1.0));
        let total: f32 = r.keys.iter().map(|k| k.1.max(0.0)).sum();
        let mut laid = 0;
        for d in 0..darts {
            if d % 2048 == 0 && cancel() {
                return None;
            }

            if laid >= MOST_A_RULE || out.len() >= MOST {
                break;
            }

            let (x, z) = (rect[0] + f64::from(rng.unit()) * w, rect[1] + f64::from(rng.unit()) * h);
            let keep = f64::from(rng.unit());
            let mut weight = r.place.weight(fields, x, z);
            if let Some(t) = &r.trees {
                weight *= t.weight(trees.map_or(f64::INFINITY, |d| d.at(x, z)));
            }

            if keep >= weight || (spacing > 0.0 && near.any_within(x, z, spacing)) {
                continue;
            }

            // The decal, its size and its way.
            let mut pick = rng.unit() * total;
            let key = r
                .keys
                .iter()
                .find(|k| {
                    pick -= k.1.max(0.0);
                    pick < 0.0
                })
                .unwrap_or(&r.keys[0]);
            let across = r.size.0 + (r.size.1 - r.size.0).max(0.0) * rng.unit();
            let jitter = (rng.unit() - 0.5) * 0.5;
            let yaw = match direction(r.align, fields, x, z) {
                Some((dx, dz)) => dx.atan2(dz) as f32 + jitter,
                None => rng.unit() * std::f32::consts::TAU,
            };
            near.add(x, z);
            laid += 1;
            // Stretched from its own shape (the kit's most common size of it; square when the kit has none).
            let key = key_of(&key.0);
            let shape = crate::decal_sizes::decal_size(&key).map_or(1.0, |[x, _, z]| if x > 0.0 { z / x } else { 1.0 });
            out.push(Decal { rule: n, key, x: x as f32, z: z as f32, yaw, across, length: across * shape * r.stretch.max(0.1) });
        }
    }

    Some(out)
}

/// Which way a rule's decal runs at a point: down the slope, or along the edge of what it is near (across the way to it), None for random
/// or where the ground gives no way (flat, or nothing near).
fn direction(align: Align, fields: &Fields, x: f64, z: f64) -> Option<(f64, f64)> {
    let gradient = |f: &dyn Fn(f64, f64) -> f64| {
        let e = 0.5;
        let (gx, gz) = ((f(x + e, z) - f(x - e, z)) / (2.0 * e), (f(x, z + e) - f(x, z - e)) / (2.0 * e));
        let len = gx.hypot(gz);
        (len > 1e-6 && len.is_finite()).then(|| (gx / len, gz / len))
    };
    let along = |feature: Feature| gradient(&|x, z| fields.distance(feature, x, z)).map(|(gx, gz)| (-gz, gx));
    match align {
        Align::Random => None,
        Align::Slope => gradient(&|x, z| fields.height(x, z)).map(|(gx, gz)| (-gx, -gz)),
        Align::Road => along(Feature::Road),
        Align::Building => along(Feature::Building),
        Align::Water => along(Feature::Water),
    }
}

/// Points in square buckets, to find those near a point quickly.
struct Buckets {
    size: f64,
    cells: std::collections::HashMap<(i64, i64), Vec<(f64, f64)>>,
}

impl Buckets {
    fn new(size: f64) -> Self {
        Self { size, cells: std::collections::HashMap::new() }
    }

    fn cell(&self, x: f64, z: f64) -> (i64, i64) {
        ((x / self.size).floor() as i64, (z / self.size).floor() as i64)
    }

    fn add(&mut self, x: f64, z: f64) {
        let c = self.cell(x, z);
        self.cells.entry(c).or_default().push((x, z));
    }

    fn any_within(&self, x: f64, z: f64, d: f64) -> bool {
        let (cx, cz) = self.cell(x, z);
        let r = (d / self.size).ceil() as i64;
        (-r..=r)
            .any(|i| (-r..=r).any(|j| self.cells.get(&(cx + i, cz + j)).is_some_and(|v| v.iter().any(|p| (p.0 - x).hypot(p.1 - z) < d))))
    }
}

/// A small random sequence (xorshift, seeded through splitmix), as the ground coverage's.
struct Random(u64);

impl Random {
    fn new(seed: u64) -> Self {
        let mut z = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        Self((z ^ (z >> 31)) | 1)
    }

    fn unit(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;")
}

/// A decal rule set's text: a `<decal_rule>` a rule, its conditions a `<rule>` inside it (as a texture rule's).
#[must_use]
pub fn text(set: &DecalRuleSet) -> Vec<u8> {
    let num = |v: f32| crate::ted_scene::format_number(f64::from(v));
    let mut out = format!("<{ROOT} {}=\"{}\"", crate::project_files::WCE_VERSION_ATTR, crate::project_files::WCE_VERSION);
    if !set.preset.is_empty() {
        let _ = write!(out, " preset=\"{}\"", escape(&set.preset));
    }

    out += ">\n";
    for r in &set.rules {
        let keys = r.keys.iter().map(|(k, w)| format!("{}:{}", k.replace(' ', "_"), num(*w))).collect::<Vec<_>>().join(" ");
        let _ = write!(
            out,
            "    <decal_rule name=\"{}\" keys=\"{}\" density=\"{}\" size=\"{} {}\" stretch=\"{}\" spacing=\"{}\" align=\"{}\"",
            escape(&r.name),
            escape(&keys),
            num(r.density),
            num(r.size.0),
            num(r.size.1),
            num(r.stretch),
            num(r.spacing),
            r.align.name()
        );
        if !r.enabled {
            out += " enabled=\"0\"";
        }

        if let Some(t) = &r.trees {
            let _ = write!(out, " trees=\"{}\" trees_fade=\"{}\"", t.text(), num(t.fade));
        }

        out += ">\n";
        out += &crate::texture_rules::rule_lines(std::slice::from_ref(&r.place), "        ");
        out += "    </decal_rule>\n";
    }

    out += &format!("</{ROOT}>\n");
    out.into_bytes()
}

/// A decal rule set from its text.
///
/// # Errors
///
/// It is not XML, or not a decal rule set.
pub fn parse(bytes: &[u8]) -> Result<DecalRuleSet, String> {
    let doc = XmlDoc::parse(bytes.to_vec()).map_err(|e| e.to_string())?;
    let els = doc.elements();
    if els.first().is_none_or(|e| e.name != ROOT) {
        return Err("not a decal rule set".to_owned());
    }

    let num = |e: usize, n: &str| doc.get(e, n).and_then(|v| v.trim().parse::<f32>().ok()).filter(|v| v.is_finite());
    let d = DecalRule::default();
    let rules = (0..els.len())
        .filter(|&e| els[e].name == "decal_rule")
        .map(|e| {
            // Its conditions: the first `<rule>` inside it.
            let place = (e + 1..els.len())
                .find(|&c| els[c].name == "rule" && els[c].parent == Some(e))
                .and_then(|c| crate::texture_rules::rules_in(&doc).into_iter().nth(rule_index(&doc, c)))
                .unwrap_or_default();
            let keys = doc
                .get(e, "keys")
                .unwrap_or_default()
                .split_whitespace()
                .map(|k| match k.rsplit_once(':') {
                    Some((name, w)) => (name.to_owned(), w.parse::<f32>().ok().filter(|w| w.is_finite()).unwrap_or(1.0)),
                    None => (k.to_owned(), 1.0),
                })
                .collect();
            let size = doc
                .get(e, "size")
                .and_then(|v| {
                    let mut p = v.split_whitespace().map(str::parse::<f32>);
                    Some((p.next()?.ok()?, p.next()?.ok()?))
                })
                .filter(|s| s.0.is_finite() && s.1.is_finite())
                .map_or(d.size, |(a, b)| (a.clamp(0.1, 200.0), b.clamp(0.1, 200.0)));
            DecalRule {
                name: doc.get(e, "name").unwrap_or_default(),
                enabled: doc.get(e, "enabled").is_none_or(|v| v.trim() != "0"),
                keys,
                place,
                trees: doc.get(e, "trees").and_then(|v| Range::parse(&v, num(e, "trees_fade"))),
                density: num(e, "density").map_or(d.density, |v| v.clamp(0.0, 100.0)),
                size,
                stretch: num(e, "stretch").map_or(d.stretch, |v| v.clamp(0.1, 20.0)),
                spacing: num(e, "spacing").map_or(d.spacing, |v| v.clamp(0.0, 100.0)),
                align: doc.get(e, "align").map_or(Align::Random, |v| Align::named(&v)),
            }
        })
        .collect();
    Ok(DecalRuleSet { preset: doc.get(0, "preset").unwrap_or_default(), rules })
}

/// Which `<rule>` of the document an element is, counting from the first.
fn rule_index(doc: &XmlDoc, elem: usize) -> usize {
    doc.elements()[..elem].iter().filter(|e| e.name == "rule").count()
}

/// The built-in preset's name.
pub const BUILTIN: &str = "Default";

fn rule(name: &str, keys: &[&str], place: Rule, density: f32, size: (f32, f32), stretch: f32, spacing: f32, align: Align) -> DecalRule {
    DecalRule {
        name: name.to_owned(),
        keys: keys.iter().map(|k| ((*k).to_owned(), 1.0)).collect(),
        place: Rule { name: name.to_owned(), ..place },
        density,
        size,
        stretch,
        spacing,
        align,
        ..DecalRule::default()
    }
}

/// The built-in rules: puddles in hollows, dirt at building bases, ruts along roads, runnels where water ran, stones where it laid
/// them, forest floor under trees. Of the kit's own decals.
#[must_use]
pub fn default_rules() -> Vec<DecalRule> {
    let below = |hi: f32, fade: f32| Some(Range { lo: None, hi: Some(hi), fade });
    let above = |lo: f32, fade: f32| Some(Range { lo: Some(lo), hi: None, fade });
    vec![
        rule(
            "Puddles in hollows",
            &["global_terrain_puddle_01", "global_terrain_puddle_02"],
            Rule { slope: below(8.0, 4.0), hollow: above(0.3, 0.2), hollow_radius: 8.0, seed: 11, ..Rule::default() },
            0.5,
            (1.5, 3.0),
            1.0,
            5.0,
            Align::Random,
        ),
        rule(
            "Dirt at building bases",
            &["dirt_1", "dirt_2", "global_terrain_mud_01"],
            Rule { slope: below(25.0, 8.0), building: below(1.5, 2.0), seed: 12, ..Rule::default() },
            3.0,
            (2.5, 5.0),
            1.0,
            2.5,
            Align::Building,
        ),
        rule(
            "Ruts along roads",
            &["global_terrain_erossion_09", "global_terrain_erossion_10", "global_terrain_erossion_12"],
            Rule { road: below(1.0, 1.5), seed: 13, ..Rule::default() },
            1.5,
            (1.5, 3.0),
            1.0,
            4.0,
            Align::Road,
        ),
        rule(
            "Runnels where water runs",
            &["global_terrain_erossion_03", "global_terrain_erossion_04", "global_terrain_erossion_08"],
            Rule { slope: Some(Range { lo: Some(8.0), hi: Some(40.0), fade: 4.0 }), flow: above(0.6, 0.1), seed: 14, ..Rule::default() },
            1.0,
            (2.0, 4.0),
            1.0,
            5.0,
            Align::Slope,
        ),
        rule(
            "Stones where water laid them",
            &["stones1", "mud_stone01"],
            Rule { slope: below(20.0, 6.0), deposit: above(0.15, 0.1), seed: 15, ..Rule::default() },
            1.0,
            (1.5, 3.0),
            1.0,
            3.0,
            Align::Random,
        ),
        DecalRule {
            trees: below(3.0, 2.0),
            ..rule(
                "Forest floor under trees",
                &["forest_0_su", "roots_01"],
                Rule { slope: below(30.0, 8.0), seed: 16, ..Rule::default() },
                1.0,
                (3.0, 6.0),
                1.0,
                4.0,
                Align::Random,
            )
        },
    ]
}

/// A preset of decal rules: its name (the file's), the folder below `decal_rule_sets` it is in, its file (none: built in) and its rules,
/// as the texture rules' presets.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Preset {
    pub name: String,
    pub folder: String,
    pub path: Option<std::path::PathBuf>,
    pub rules: Vec<DecalRule>,
}

impl Preset {
    /// The built-in preset.
    #[must_use]
    pub fn builtin() -> Self {
        Self { name: BUILTIN.to_owned(), rules: default_rules(), ..Self::default() }
    }

    #[must_use]
    pub fn is_builtin(&self) -> bool {
        self.path.is_none()
    }

    /// Writes it to its file (its folder made).
    ///
    /// # Errors
    ///
    /// The built-in preset (no file), or the file could not be written.
    pub fn save(&self) -> Result<(), String> {
        let path = self.path.as_ref().ok_or("the built-in preset has no file")?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }

        let set = DecalRuleSet { preset: self.name.clone(), rules: self.rules.clone() };
        std::fs::write(path, text(&set)).map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// The folder of the presets in a raw_data folder (the projects').
#[must_use]
pub fn presets_root(raw_data: &std::path::Path) -> std::path::PathBuf {
    raw_data.join("art").join("decal_rule_sets")
}

/// The presets: the built-in one, then the folder's files (`*.wce` below it, by name).
#[must_use]
pub fn scan_presets(root: &std::path::Path) -> Vec<Preset> {
    let mut out = vec![Preset::builtin()];
    let mut found = vec![];
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else { continue };
        for entry in read.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "wce")
                && let Some(set) = std::fs::read(&path).ok().and_then(|b| parse(&b).ok())
            {
                let folder = path.parent().and_then(|p| p.strip_prefix(root).ok()).map(|p| p.to_string_lossy().replace('\\', "/"));
                let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                found.push(Preset { name, folder: folder.unwrap_or_default(), path: Some(path), rules: set.rules });
            }
        }
    }

    found.sort_by_key(|p| (p.folder.to_lowercase(), p.name.to_lowercase()));
    out.extend(found);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn never() -> bool {
        false
    }

    /// 60 × 60 m at 1 m a sample, a slope down +x (1 m in 10) or flat, with a bowl at (40, 40).
    fn ground_with(slope: f32) -> Fields {
        let w = 60u32;
        let heights = (0..w * w)
            .map(|i| {
                let (x, z) = ((i % w) as f32, (i / w) as f32);
                let bowl = (1.5 - ((x - 40.0).hypot(z - 40.0) / 5.0).powi(2)).max(0.0);
                -x * slope - bowl
            })
            .collect();
        Fields::new(w, w, (1.0, 1.0), heights)
    }

    fn ground() -> Fields {
        ground_with(0.1)
    }

    fn puddles() -> DecalRule {
        rule(
            "puddles",
            &["global_terrain_puddle_01"],
            Rule { hollow: Some(Range { lo: Some(0.3), hi: None, fade: 0.1 }), hollow_radius: 8.0, ..Rule::default() },
            40.0,
            (1.0, 2.0),
            2.0,
            1.5,
            Align::Random,
        )
    }

    #[test]
    fn decals_go_where_their_conditions_hold_spaced_and_sized() {
        let f = ground_with(0.0);
        let laid = lay(&[puddles()], &f, None, [0.0, 0.0, 60.0, 60.0], 1, &never).unwrap();
        assert!(laid.len() > 5, "{}", laid.len());
        for d in &laid {
            assert!((d.x - 40.0).hypot(d.z - 40.0) < 7.0, "in the bowl: {d:?}");
            // Twice its own shape: the kit's puddle is half as long as it is wide.
            assert!((1.0..=2.0).contains(&d.across) && (d.length - d.across * 0.5 * 2.0).abs() < 1e-4);
            assert_eq!(d.key, "rigidmodels\\decals\\battle\\global_terrain_puddle_01.rigid_model_v2");
        }

        for (i, a) in laid.iter().enumerate() {
            for b in &laid[i + 1..] {
                assert!((a.x - b.x).hypot(a.z - b.z) >= 1.5 - 1e-4, "spaced");
            }
        }

        // The same seed, the same decals; another, others.
        assert_eq!(lay(&[puddles()], &f, None, [0.0, 0.0, 60.0, 60.0], 1, &never).unwrap(), laid);
        assert_ne!(lay(&[puddles()], &f, None, [0.0, 0.0, 60.0, 60.0], 2, &never).unwrap(), laid);
    }

    #[test]
    fn a_decal_runs_down_the_slope_or_along_a_road() {
        // Down the slope: the ground falls along +x, so the length runs along x (yaw about ±90°).
        let f = ground();
        let r = DecalRule {
            place: Rule::default(),
            keys: vec![("a".to_owned(), 1.0)],
            density: 5.0,
            align: Align::Slope,
            ..DecalRule::default()
        };
        let laid = lay(&[r], &f, None, [0.0, 0.0, 30.0, 30.0], 3, &never).unwrap();
        assert!(!laid.is_empty());
        for d in &laid {
            assert!(d.yaw.sin() > 0.9, "down the slope: {}", d.yaw.to_degrees());
        }

        // A road along z at x = 20: its decals run along z.
        let mask = (0..60 * 60).map(|i| i % 60 == 20).collect::<Vec<_>>();
        let f = ground().with_distances(Feature::Road, Distances::from_mask(60, 60, (1.0, 1.0), &mask));
        let r = DecalRule {
            place: Rule { road: Some(Range { lo: None, hi: Some(3.0), fade: 0.0 }), ..Rule::default() },
            keys: vec![("a".to_owned(), 1.0)],
            density: 20.0,
            align: Align::Road,
            ..DecalRule::default()
        };
        assert!(uses(std::slice::from_ref(&r), Feature::Road) && !uses(std::slice::from_ref(&r), Feature::Water));
        let laid = lay(&[r], &f, None, [0.0, 0.0, 60.0, 60.0], 3, &never).unwrap();
        assert!(!laid.is_empty());
        for d in laid.iter().filter(|d| (d.x - 20.5).abs() > 0.8) {
            assert!(d.yaw.cos().abs() > 0.9, "along the road: {} at {}", d.yaw.to_degrees(), d.x);
        }
    }

    #[test]
    fn under_trees_needs_the_trees() {
        let f = ground();
        let r = DecalRule {
            keys: vec![("forest_0_su".to_owned(), 1.0)],
            trees: Some(Range { lo: None, hi: Some(3.0), fade: 0.0 }),
            density: 20.0,
            spacing: 0.0,
            ..DecalRule::default()
        };
        assert!(lay(std::slice::from_ref(&r), &f, None, [0.0, 0.0, 60.0, 60.0], 1, &never).unwrap().is_empty());
        let mask = (0..60 * 60).map(|i| i == 10 * 60 + 10).collect::<Vec<_>>();
        let trees = Distances::from_mask(60, 60, (1.0, 1.0), &mask);
        let laid = lay(&[r], &f, Some(&trees), [0.0, 0.0, 60.0, 60.0], 1, &never).unwrap();
        assert!(!laid.is_empty());
        assert!(laid.iter().all(|d| (d.x - 10.5).hypot(d.z - 10.5) <= 3.0 + 1e-3));
    }

    #[test]
    fn rules_through_their_text_and_the_defaults_name_kit_decals() {
        let mut rules = default_rules();
        rules[0].enabled = false;
        rules[1].keys.push(("rigidmodels\\decals\\vfx\\stone_cracks.rigid_model_v2".to_owned(), 0.5));
        let set = DecalRuleSet { preset: "Mine".to_owned(), rules };
        assert_eq!(parse(&text(&set)).unwrap(), set);
        assert!(parse(b"<texture_rules/>").is_err());
        assert_eq!(name_of(&key_of("dirt_1")), "dirt_1");
        assert_eq!(
            name_of("rigidmodels\\decals\\vfx\\stone_cracks.rigid_model_v2"),
            "rigidmodels\\decals\\vfx\\stone_cracks.rigid_model_v2"
        );
        assert!(uses_trees(&default_rules()) && uses_runoff(&default_rules()));
    }
}
