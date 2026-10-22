//! An exported (binary) tile as a TEd project: the model of `attila.tile_binary` turned into the model `attila.ted_project` writes
//! (docs/plans/tile_roundtrip.md, Phase 2 step 3; docs/formats/tile_decompile.md).
//!
//! What the binary does not carry is reported as a loss ([`Losses`]): layer names and structure, the camera, the vegetation seeds, the
//! nogo / LF influence maps, the height map's border ring, TEd-only properties.

use rpfm_lib::files::tile_database::TileDatabase;
use wce_tile_model::{Entry, MapKind, Node, Raster, Samples, Sym, Tile, TileMap, Value};

use super::building_list::BattlefieldBuilding;
use super::ted_project;
use super::tile_binary::buildings;

/// A building key's number of destruction levels.
pub type LevelsOf<'a> = &'a (dyn Fn(&str) -> Option<u32> + Sync);

/// A building key's `battlefield_buildings` category.
pub type CategoryOf<'a> = &'a (dyn Fn(&str) -> Option<String> + Sync);

/// What the decompiler can be told besides the tile.
#[derive(Default)]
pub struct Context<'a> {
    /// The tile databases, searched in order for the tile's entry (a mod's before the game's).
    pub databases: &'a [TileDatabase],

    /// The number of destruction levels of a building key (its pieces' highest `destructNN`), to turn the binary's starting damage
    /// (level / (levels - 1)) back into TEd's destruction level; without it a damaged building gets level 1.
    pub building_levels: Option<LevelsOf<'a>>,

    /// The `battlefield_buildings` category of a building key: TEd writes `weak_wall`, `ai_breachable` and `dockable` for `fort_wall`,
    /// `dockable` for `unbreachable_fort_wall`, none for the others. Without it they are written when they differ from a plain building's.
    pub building_category: Option<CategoryOf<'a>>,

    /// The tree list whose trees become the "Decompiled vegetation" layer (default `outfield_tents.tree_list.bin`, then
    /// `default.tree_list.bin`; see docs/formats/tile_decompile.md).
    pub tree_list: Option<&'a str>,
}

/// What the conversion could not carry over, or had to make up.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Losses {
    pub notes: Vec<String>,
}

impl Losses {
    fn note(&mut self, what: impl Into<String>) {
        self.notes.push(what.into());
    }
}

/// `terrain/tiles/battle/settlement_western_cities/western_city_b/small` -> (`terrain\tiles\battle\settlement_western_cities\western_city_b`,
/// `small`).
pub fn split_location(folder: &str) -> (String, String) {
    let folder = folder.trim_end_matches('/').replace('\\', "/");
    let (tile, variation) = folder.rsplit_once('/').unwrap_or(("", &folder));
    (tile.replace('/', "\\"), variation.to_owned())
}

/// The entry's file name, as the kit names them: the location after `terrain\tiles\<kind>\`, `\` -> `_`, `.xml`.
pub fn entry_file_name(location: &str) -> String {
    let parts = location.split('\\').collect::<Vec<_>>();
    format!("{}.xml", parts.get(3..).map(|p| p.join("_")).unwrap_or_default())
}

/// The tile of a location in the databases, as (database, index): the first database that has it (list a mod's database before the
/// game's).
pub fn find_tile(databases: &[TileDatabase], location: &str) -> Option<(usize, usize)> {
    databases.iter().enumerate().find_map(|(d, db)| db.tiles().iter().position(|t| t.location().eq_ignore_ascii_case(location)).map(|i| (d, i)))
}

/// An entry number, as its `%.6f` text reads back.
fn f(v: f32) -> Value {
    Value::Float(crate::xml::values::Num::Fixed6.quantise(f64::from(v)))
}

fn b(v: bool) -> Value {
    Value::Bool(v)
}

fn s(v: &str) -> Value {
    Value::Str(v.to_owned())
}

fn i(v: impl Into<i64>) -> Value {
    Value::Int(v.into())
}

fn node(class: &str, props: Vec<(&str, Value)>) -> Node {
    let mut n = Node::new(class);
    for (k, v) in props {
        n.set(k, v);
    }

    n
}

/// Tile `index` of a database as its entry (`TILES/*.xml`), in the order and shape BOB writes them.
pub fn entry_node(db: &TileDatabase, index: usize) -> Node {
    let t = &db.tiles()[index];
    let mut root = node(
        "TILE",
        vec![
            ("serialise_version", i(*t.serialise_version())), ("location", s(t.location())), ("tile_set", s(t.tile_set())), ("mask", s(t.mask())),
            ("width", i(*t.width())), ("height", i(*t.height())), ("red", f(*t.red())), ("green", f(*t.green())), ("blue", f(*t.blue())),
            ("requires_infield_lodding", b(*t.requires_infield_lodding())), ("random_rotatable", b(*t.random_rotatable())),
            ("custom_alpha_blend_texture", s(t.custom_alpha_blend_texture())), ("scalable", b(*t.scalable())), ("encampable", b(*t.encampable())),
            ("custom_blend_tile", s(t.custom_blend_tile())),
        ],
    );

    root.children.push(node(
        "TEXTURE_GROUP",
        vec![("red", s(t.texture_red())), ("green", s(t.texture_green())), ("blue", s(t.texture_blue())), ("alpha", s(t.texture_alpha()))],
    ));

    let mut vars = Node::new("VARIATIONS");
    for v in t.variations() {
        vars.children.push(node(
            "VARIATION",
            vec![
                ("serialise_version", i(*v.serialise_version())), ("location", s(v.location())), ("min_height", f(*v.min_height())),
                ("scale", f(*v.scale())), ("normal_strength", f(*v.normal_strength())), ("overlap_border_size", f(*v.overlap_border_size())),
                ("raw_data_tri_density", i(*v.raw_data_tri_density())), ("blend_common", s(v.blend_common())), ("index_common", s(v.index_common())),
                ("normal_common", s(v.normal_common())), ("red", f(*v.red())), ("green", f(*v.green())), ("blue", f(*v.blue())),
                ("barbarian", b(*v.barbarian())),
            ],
        ));
    }

    // BOB writes an empty VARIATIONS as a pair on two lines.
    if vars.children.is_empty() {
        vars.trivia_mut().layout.push((crate::xml::layout::EMPTY_PAIR, b"\r\n\t".to_vec().into()));
    }

    root.children.push(vars);
    let mut targets = Node::new("TILE_LINK_TARGETS");
    for lt in t.link_targets() {
        targets.children.push(node("TILE_LINK_TARGET", vec![("serialise_version", i(*lt.serialise_version())), ("tile_set", s(lt.tile_set())), ("x", i(*lt.x())), ("y", i(*lt.y()))]));
    }

    root.children.push(targets);
    let mut links = Node::new("TILE_LINKS");
    for l in t.links() {
        let mut n = node(
            "TILE_LINK",
            vec![
                ("serialise_version", i(*l.serialise_version())), ("link_set", s(l.link_set())), ("x", i(*l.x())), ("y", i(*l.y())), ("base_x", i(*l.base_x())),
                ("base_y", i(*l.base_y())), ("is_entry", b(*l.is_entry())), ("blend_size", i(*l.blend_size())), ("no_offline_blend", b(*l.no_offline_blend())),
                ("test", s(l.test())),
            ],
        );

        n.children.push(Node::new("blend_quad"));
        links.children.push(n);
    }

    root.children.push(links);
    root
}

/// Tile `index` of a database as its entry's XML, as BOB writes it.
pub fn entry_xml(db: &TileDatabase, index: usize) -> Vec<u8> {
    crate::xml::write(&entry_node(db, index), &ted_project::entry_style(), &super::schema::EntrySchema)
}

/// The TEd height map from the binary's heights: the samples of the CHMF's first region inside a border ring of `border` pixels filled from
/// the edge. Returns the raster and the entry's min / scale that make its raw values the samples.
fn height_map(binary: &Tile, cells: (u32, u32), border_cells: u32, losses: &mut Losses) -> Option<(Raster, f32, f32)> {
    let doc = binary.document("hf_height_map.data")?;
    let m = binary.map(MapKind::Height, 0)?;
    let Samples::U16(src) = &m.raster.samples else { return None };
    let (w, h) = (m.raster.width, m.raster.height);
    if doc.root.children.iter().filter(|c| c.class == "region").count() != 1 {
        losses.note("height map: more than one region, only the first is used");
    }

    // Pixels per cell from the samples (cells * ppc + 1 per axis).
    let ppc = (w - 1) / cells.0.max(1);
    if (h - 1) / cells.1.max(1) != ppc || (w - 1) % cells.0.max(1) != 0 {
        losses.note(format!("height map: {w} x {h} samples do not fit {} x {} cells", cells.0, cells.1));
    }

    let border = border_cells * ppc;
    let (ow, oh) = ((cells.0 + 2 * border_cells) * ppc, (cells.1 + 2 * border_cells) * ppc);
    let mut out = vec![0u16; (ow * oh) as usize];
    for y in 0..oh {
        for x in 0..ow {
            let sx = (x as i64 - i64::from(border)).clamp(0, i64::from(w) - 1) as u32;
            let sy = (y as i64 - i64::from(border)).clamp(0, i64::from(h) - 1) as u32;
            out[(y * ow + x) as usize] = src[(sy * w + sx) as usize];
        }
    }

    if border > 0 {
        losses.note(format!("height map: the border ring ({border_cells} cell, {border} px) is not in the binary: filled from the edge"));
    }

    let min = doc.root.get("min").and_then(Value::as_f64).unwrap_or(0.0) as f32;
    let range = doc.root.get("range").and_then(Value::as_f64).unwrap_or(1.0) as f32;
    Some((Raster { width: ow, height: oh, channels: 1, samples: Samples::U16(out) }, min, range))
}

// ---- the scene

fn flags(names: &[&str]) -> Value {
    Value::Flags(names.iter().map(|n| Sym::new(n)).collect())
}

/// A number as TEd's files hold it (float32 written `%.6f`).
fn r32(v: f64) -> f64 {
    crate::xml::values::Num::Ted.quantise(v)
}

/// The base properties of a TEd instance in v16's order for most classes (flags first, orientation last are TEd's; the middle is the
/// class's own).
fn instance(class: &str, name: &str, position: [f64; 3], euler: [f64; 3], own: Vec<(&str, Value)>) -> Node {
    let mut n = node(class, vec![("flags", flags(&["OIF_VISIBLE", "OIF_ACTIVE"]))]);
    for (k, v) in own {
        n.set(k, v);
    }

    n.set("lf_offset", Value::Float(0.0));
    n.set("position", Value::Vec3(position.map(r32)));
    n.set("name", s(name));
    n.set("surface_mode", s("MSM_FREEFORM"));
    n.set("orientation", Value::Euler(euler.map(r32)));
    n
}

/// A TEd layer.
fn layer(name: &str, export_type: &str) -> Node {
    node(
        "LayerInstance",
        vec![
            ("flags", flags(&["OIF_VISIBLE", "OIF_ACTIVE"])), ("export_type", s(export_type)), ("lf_offset", Value::Float(0.0)),
            ("position", Value::Vec3([0.0; 3])), ("name", s(name)), ("surface_mode", s("MSM_FREEFORM")), ("orientation", Value::Euler([0.0; 3])),
        ],
    )
}

/// tile.ted of version 16 without layers: modules, the root, a camera looking at the tile from its south edge.
fn project(extent: (f64, f64)) -> Node {
    let mut p = node("project", vec![("version", i(16))]);
    let mut modules = Node::new("modules");
    modules.children.push(node("Vegetation", vec![("generation_seed", i(0))]));
    modules.children.push(node("Grass", vec![("generation_seed", i(0))]));
    p.children.push(modules);
    let mut scene = Node::new("scene");
    scene.children.push(node(
        "SCENE_GRAPH",
        vec![
            ("flags", flags(&["OIF_VISIBLE", "OIF_ACTIVE"])), ("export_type", s("LayerExportType_Normal")), ("lf_offset", Value::Float(0.0)),
            ("position", Value::Vec3([0.0; 3])), ("name", s("root")), ("surface_mode", s("MSM_FREEFORM")), ("orientation", Value::Euler([0.0; 3])),
        ],
    ));
    p.children.push(scene);
    let (cx, cz) = (extent.0, extent.1 * 2.0);
    p.children.push(node(
        "camera",
        vec![
            ("vertical_fov", Value::Float(r32(std::f64::consts::FRAC_PI_3))), ("orthographic", b(false)), ("orthographic_height", Value::Float(1.0)), ("far_distance", Value::Float(12000.0)),
            ("smoothed", b(true)), ("target", Value::Vec3([cx, 0.0, cz * 0.5].map(r32))), ("ground_height", Value::Float(1.4)), ("DOF_blur_size", Value::Float(0.0)),
            ("terrain_collision", b(false)), ("head_bob", b(false)), ("aspect_ratio", Value::Float(1.687541)), ("focal_length", Value::Float(0.0)),
            ("position", Value::Vec3([cx, extent.0.max(extent.1), cz * 1.1].map(r32))), ("DOF_focus", s("variance(0,40)")), ("movement_mode", s("CM_LOOK")),
            ("roll", Value::Float(0.0)), ("near_distance", Value::Float(1.0)),
        ],
    ));
    p
}

/// The scene graph's root of a project made by [`project`].
fn graph(p: &mut Node) -> &mut Node {
    &mut p.child_mut("scene").expect("scene").children[0]
}

/// The euler angles of a quaternion (i, j, k, w), TEd's `Rz(c) * Ry(a) * Rx(-b)` (gimbal lock: c = 0).
pub fn quaternion_euler(q: [f64; 4]) -> [f64; 3] {
    let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt().max(1e-30);
    let (x, y, z, w) = (q[0] / n, q[1] / n, q[2] / n, q[3] / n);
    let (m00, m10, m11, m12, m20, m21, m22) = (
        1.0 - 2.0 * (y * y + z * z),
        2.0 * (x * y + z * w),
        1.0 - 2.0 * (x * x + z * z),
        2.0 * (y * z - x * w),
        2.0 * (x * z - y * w),
        2.0 * (y * z + x * w),
        1.0 - 2.0 * (x * x + y * y),
    );

    let sa = (-m20).clamp(-1.0, 1.0);
    let a = sa.asin();
    let (beta, c) = if sa.abs() < 1.0 - 1e-12 { (m21.atan2(m22), m10.atan2(m00)) } else { ((-m12).atan2(m11), 0.0) };
    [a, -beta, c]
}

/// A building of a binary list as TEd's instance, top level of its layer: scene x = 2 x, scene z = 2 (extent z - z), scene y = y (the
/// list keeps x / z in metres, y in scene units; docs/formats/tile_ted.md).
fn building_node(bl: &BattlefieldBuilding, extent_z: f64, ctx: &Context, losses: &mut BuildingLosses) -> Node {
    let p = bl.position.map(f64::from);
    let pos = [2.0 * p[0], p[1], 2.0 * (extent_z - p[2])];
    let euler = quaternion_euler(bl.orientation.map(f64::from));
    let mut own = vec![
        ("opacity", Value::Float(1.0)), ("building_tech_level", s("TL_TECH_LEVEL1")), ("bounds_colour", Value::Colour([1.0, 0.0, 0.0, 1.0])),
        ("indestructible", b(bl.indestructible)), ("glow_amount", Value::Float(1.0)), ("key", s(&bl.key)), ("dirt_level", Value::Float(1.0)),
        ("destruction_level", i(destruction_level(bl, ctx, losses))), ("toggleable", b(bl.toggleable.unwrap_or(false))), ("burn", b(bl.on_fire)),
        ("burn_amount", Value::Float(1.0)),
    ];

    // TEd keeps the wall flags only on walls (their category); without the category: when they differ from a plain building's.
    let (walls, docks) = match ctx.building_category.map(|f| f(&bl.key)) {
        Some(Some(c)) => (c == "fort_wall", c == "fort_wall" || c == "unbreachable_fort_wall"),
        Some(None) | None => {
            let odd = !bl.ai_breachable || !bl.dockable || bl.weak_point;
            (odd, odd)
        }
    };

    if walls {
        own.push(("weak_wall", b(bl.weak_point)));
        own.push(("ai_breachable", b(bl.ai_breachable)));
    }

    if docks {
        own.push(("dockable", b(bl.dockable)));
    }

    if bl.scale != [1.0; 3] {
        losses.scaled += 1;
    }

    if bl.start_disabled {
        losses.start_disabled += 1;
    }

    if bl.position_type != 2 {
        losses.position_type += 1;
    }

    instance("BUILDING_INSTANCE", &bl.key, pos, euler, own)
}

/// The buildings of a BMD document (`battlefield_building_list` > `buildings` > `item`), as the building list's.
fn bmd_buildings(bmd: &Node) -> Vec<BattlefieldBuilding> {
    let Some(items) = bmd.child("battlefield_building_list").and_then(|l| l.child("buildings")) else { return vec![] };
    let fl = |n: Option<&Node>, k: &str| n.and_then(|n| n.get(k)).and_then(Value::as_f64).unwrap_or(0.0) as f32;
    let bo = |n: Option<&Node>, k: &str| n.and_then(|n| n.get(k)).and_then(Value::as_bool).unwrap_or(false);
    items
        .children
        .iter()
        .map(|it| {
            let (pos, ori, scale, props) = (it.child("position"), it.child("orientation"), it.child("scale"), it.child("properties"));
            BattlefieldBuilding {
                key: it.get("building_key").and_then(Value::as_str).unwrap_or_default().to_owned(),
                position: [fl(pos, "x"), fl(pos, "y"), fl(pos, "z")],
                orientation: [fl(ori, "i"), fl(ori, "j"), fl(ori, "k"), fl(ori, "w")],
                scale: [fl(scale, "x"), fl(scale, "y"), fl(scale, "z")],
                starting_damage: fl(props, "starting_damage_unary"),
                on_fire: bo(props, "on_fire"),
                start_disabled: bo(props, "start_disabled"),
                weak_point: bo(props, "weak_point"),
                ai_breachable: bo(props, "ai_breachable"),
                indestructible: bo(props, "indestructible"),
                dockable: bo(props, "dockable"),
                position_type: match it.get("position_type").and_then(Value::as_str) {
                    Some("BBPT_RELATIVE") => 0,
                    Some("BBPT_ABSOLUTE") => 1,
                    _ => 2,
                },
                toggleable: Some(bo(props, "toggleable")),
                lite: Some(bo(props, "lite")),
            }
        })
        .collect()
}

/// TEd's destruction level of a building of the binary: its starting damage is level / (levels - 1).
fn destruction_level(bl: &BattlefieldBuilding, ctx: &Context, losses: &mut BuildingLosses) -> i64 {
    if bl.starting_damage <= 0.0 {
        return 0;
    }

    match ctx.building_levels.and_then(|f| f(&bl.key)) {
        Some(levels) if levels > 1 => (f64::from(bl.starting_damage) * f64::from(levels - 1)).round() as i64,
        _ => {
            losses.unknown_levels += 1;
            1
        }
    }
}

// ---- markers

/// (record, group key) of a markers document: the key is the group's first part (a string: the model, the effect, the light's name; a
/// number: the AI hint type).
fn marker_records(doc: &Node) -> Vec<(&Node, Option<&Value>)> {
    let recs = doc.children.iter().filter(|c| c.class == "record").collect::<Vec<_>>();
    let mut keys = vec![None; recs.len()];
    if let Some(gs) = doc.child("groups") {
        for g in &gs.children {
            let key = g.children.first().and_then(|k| k.get("value"));
            for &i in g.get("indices").and_then(Value::as_ints).unwrap_or_default() {
                if let Some(k) = keys.get_mut(i as usize) {
                    *k = key;
                }
            }
        }
    }

    recs.into_iter().zip(keys).collect()
}

fn num(n: &Node, k: &str) -> f64 {
    n.get(k).and_then(Value::as_f64).unwrap_or(0.0)
}

/// The world rotation of a marker's 3 x 3 (rows `a`, scale removed): the markers keep the rotation transposed and in the frame with z
/// mirrored, R = F a^T F with F = diag(1, 1, -1) (for a turn about y alone that is `a` itself).
fn marker_rotation(a: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let f = [1.0, 1.0, -1.0];
    std::array::from_fn(|i| std::array::from_fn(|j| f[i] * a[j][i] * f[j]))
}

/// The euler angles of a rotation given by its rows.
fn rows_euler(r: [[f64; 3]; 3]) -> [f64; 3] {
    let sa = (-r[2][0]).clamp(-1.0, 1.0);
    let a = sa.asin();
    let (beta, c) = if sa.abs() < 1.0 - 1e-12 { (r[2][1].atan2(r[2][2]), r[1][0].atan2(r[0][0])) } else { ((-r[1][2]).atan2(r[1][1]), 0.0) };
    [a, -beta, c]
}

/// TEd's seasons flags of a bit mask (bit 0 winter, 1 spring, 2 summer, 3 autumn).
fn seasons(mask: i64) -> Value {
    let names = ["SEASON_BF_WINTER", "SEASON_BF_SPRING", "SEASON_BF_SUMMER", "SEASON_BF_AUTUMN"];
    flags(&names.iter().enumerate().filter(|(i, _)| mask & (1 << i) != 0).map(|(_, n)| *n).collect::<Vec<_>>())
}

/// A prop record: a projected decal, or a model (SIMPLE_MESH_INSTANCE). The matrix rows hold the rotation ([`marker_rotation`]) times |scale| / 2;
/// the position is (x, y, z) halved and z mirrored: scene (2 x, 2 y, 2 (extent - z)). The decal's scale fields are TEd's scale halved.
fn prop_node(rec: &Node, key: &str, extent_z: f64) -> Node {
    let m = rec.get("transform").and_then(Value::as_floats).unwrap_or(&[0.0; 16]);
    let decal = rec.get("is_decal").and_then(Value::as_int) == Some(1);
    let half = if decal {
        [num(rec, "scale_x"), num(rec, "scale_y"), num(rec, "scale_z")]
    } else {
        std::array::from_fn(|k| (m[4 * k] * m[4 * k] + m[4 * k + 1] * m[4 * k + 1] + m[4 * k + 2] * m[4 * k + 2]).sqrt())
    };

    let rows = marker_rotation(std::array::from_fn(|k| {
        let n = half[k].abs().max(1e-12);
        [m[4 * k] / n, m[4 * k + 1] / n, m[4 * k + 2] / n]
    }));

    let pos = [2.0 * m[12], 2.0 * m[13], 2.0 * (extent_z - m[14])];
    let scale = Value::Vec3(half.map(|h| r32(2.0 * h)));
    if decal {
        instance(
            "ProjectedDecalInstance",
            "Projected Decal",
            pos,
            rows_euler(rows),
            vec![("scale", scale), ("key", s(key)), ("parallax_scale", Value::Float(r32(num(rec, "parallax_scale")))), ("tiled", b(num(rec, "tiled") != 0.0))],
        )
    } else {
        instance("SIMPLE_MESH_INSTANCE", "Simple Mesh", pos, rows_euler(rows), vec![("scale", scale), ("key", s(key)), ("seasons", seasons(num(rec, "seasons") as i64))])
    }
}

/// A particle emitter record as a VFXInstance: the matrix holds the rotation transposed (not mirrored, unlike a prop's: all 42 emitters of
/// western_city_b/small agree), the position is in the buildings' frame (scene (2 x, y, 2 (extent - z))), the scale is TEd's halved.
fn emitter_node(rec: &Node, key: &str, extent_z: f64) -> Node {
    let m = rec.get("transform").and_then(Value::as_floats).unwrap_or(&[0.0; 16]);
    let rows: [[f64; 3]; 3] = std::array::from_fn(|k| [m[k], m[4 + k], m[8 + k]]);
    instance(
        "VFXInstance",
        "VFX",
        [2.0 * m[12], m[13], 2.0 * (extent_z - m[14])],
        rows_euler(rows),
        vec![
            ("vfx", s(key)), ("use_in_outfield", b(num(rec, "use_in_outfield") != 0.0)), ("scale", Value::Float(r32(2.0 * num(rec, "scale")))),
            ("seasons", seasons(rec.get("seasons").and_then(Value::as_int).unwrap_or(15))), ("autoplay", b(num(rec, "autoplay") != 0.0)),
        ],
    )
}

/// A point light record (the buildings' frame, no rotation).
fn light_node(rec: &Node, key: &str, extent_z: f64) -> Node {
    instance(
        "POINT_LIGHT_INSTANCE",
        key,
        [2.0 * num(rec, "x"), num(rec, "y"), 2.0 * (extent_z - num(rec, "z"))],
        [0.0; 3],
        vec![
            ("random_offset", Value::Float(r32(num(rec, "random_offset")))), ("animation_type", rec.get("animation_type").cloned().unwrap_or_else(|| s("LAT_NONE"))),
            ("radius", Value::Float(r32(num(rec, "radius")))), ("falloff_type", rec.get("falloff_type").cloned().unwrap_or_else(|| s("WPLFT_SMOOTH"))),
            ("colour_min", Value::Float(r32(num(rec, "colour_min")))), ("colour_scale", Value::Float(r32(num(rec, "colour_scale")))),
            ("colour", Value::Colour([num(rec, "red"), num(rec, "green"), num(rec, "blue"), 1.0].map(r32))),
            ("animation_speed_scale", Value::Vec2([num(rec, "animation_speed_x"), num(rec, "animation_speed_y")].map(r32))),
        ],
    )
}

/// A BMD transform (`m00` .. `m33`) as 16 numbers, row-major.
fn bmd_matrix(n: Option<&Node>) -> Value {
    Value::Floats((0..16).map(|i| n.map_or(0.0, |n| num(n, &format!("m{}{}", i / 4, i % 4)))).collect())
}

/// A BMD prop (`prop_list` > `markers` > `item`) as a prop marker record and its key (prop_type 1: a decal).
fn bmd_prop_record(item: &Node) -> (Node, String) {
    let mut r = Node::new("record");
    r.set("transform", bmd_matrix(item.child("transform")));
    r.set("is_decal", i(i64::from(item.get("prop_type").and_then(Value::as_int) == Some(1))));
    let sc = item.child("decal_scale");
    for (k, a) in [("scale_x", "x"), ("scale_y", "y"), ("scale_z", "z")] {
        r.set(k, Value::Float(sc.map_or(0.0, |n| num(n, a))));
    }

    r.set("parallax_scale", Value::Float(num(item, "decal_parallax")));
    r.set("tiled", i(item.get("decal_tiled").and_then(Value::as_int).unwrap_or(0)));
    r.set("seasons", i(15));
    (r, item.get("key").and_then(Value::as_str).unwrap_or_default().to_owned())
}

/// A BMD particle emitter (`particle_emitter_list` > `markers` > `item`) as an emitter marker record and its key.
fn bmd_emitter_record(item: &Node) -> (Node, String) {
    let mut r = Node::new("record");
    r.set("transform", bmd_matrix(item.child("transform")));
    r.set("scale", Value::Float(num(item, "scale")));
    r.set("autoplay", i(i64::from(item.get("autoplay").and_then(Value::as_bool).unwrap_or(true))));
    r.set("use_in_outfield", i(i64::from(item.get("use_in_outfield").and_then(Value::as_bool).unwrap_or(false))));
    r.set("seasons", i(15));
    (r, item.get("key").and_then(Value::as_str).unwrap_or_default().to_owned())
}

/// TEd's vegetation key of a tree model path (`BattleTerrain/vegetation/trees\olive\olive_d.rigid_model_v2` -> `olive.olive_d`).
pub fn tree_key(model: &str) -> String {
    let parts = model.split(['/', '\\']).collect::<Vec<_>>();
    let variant = parts.last().copied().unwrap_or_default();
    let variant = variant.split('.').next().unwrap_or(variant);
    let species = parts.len().checked_sub(2).map_or("", |i| parts[i]);
    format!("{}.{}", species.to_lowercase(), variant.to_lowercase())
}

/// The trees of a FASTBIN tree list document as TEd's VEGETATION_INSTANCEs (the list's frame: x, z metres with z from the far edge, y in
/// scene units; scale as the list's).
fn tree_nodes(doc: &Node, extent_z: f64) -> Vec<Node> {
    let mut out = vec![];
    for t in doc.children.iter().filter(|c| c.class == "type") {
        let key = tree_key(t.get("key").and_then(Value::as_str).unwrap_or_default());
        let (pos, scale) = (t.get("position").and_then(Value::as_floats).unwrap_or_default(), t.get("scale").and_then(Value::as_floats).unwrap_or_default());
        for (i, p) in pos.chunks_exact(3).enumerate() {
            out.push(instance(
                "VEGETATION_INSTANCE",
                "tree",
                [2.0 * p[0], p[1], 2.0 * (extent_z - p[2])],
                [0.0; 3],
                vec![("key", s(&key)), ("scale", Value::Float(r32(scale.get(i).copied().unwrap_or(1.0))))],
            ));
        }
    }

    out
}

// ---- logic: polygons, deployment, AI hints, the siege graph

/// The binaries' 2D points (x, z from the far edge, metres) in the scene (2 x, 2 (extent - z)).
fn scene_xz(points: &[(f64, f64)], extent_z: f64) -> Vec<(f64, f64)> {
    points.iter().map(|&(x, z)| (2.0 * x, 2.0 * (extent_z - z))).collect()
}

/// A TEd polygon instance: at the centre of its points' bounding box (y 0: the binary has no heights; landscape polygons follow the
/// terrain), the points relative to it, the area of the outline. `parent` is the scene position of the instance it goes under.
fn polygon(class: &str, name: &str, pts: &[(f64, f64)], closed: bool, own: Vec<(&str, Value)>, parent: (f64, f64)) -> Node {
    // TEd gives an open polygon an area too (separators, siege streets), but for the AI hints (0 in the kit).
    let with_area = closed || class != "AI_HINT_POLYGON_INSTANCE";
    let (mut lo, mut hi) = ((f64::MAX, f64::MAX), (f64::MIN, f64::MIN));
    for &(x, z) in pts {
        lo = (lo.0.min(x), lo.1.min(z));
        hi = (hi.0.max(x), hi.1.max(z));
    }

    let c = if pts.is_empty() { (0.0, 0.0) } else { ((lo.0 + hi.0) / 2.0, (lo.1 + hi.1) / 2.0) };
    let area = if with_area {
        (0..pts.len()).map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            a.0 * b.1 - b.0 * a.1
        }).sum::<f64>().abs() / 2.0
    } else {
        0.0
    };

    let g = |v: f64| crate::xml::values::Num::G.quantise(v);
    let mut n = node(class, vec![("flags", flags(&["OIF_VISIBLE", "OIF_ACTIVE", "OIF_DRAGGABLE"])), ("recenter_polygon", i(0)), ("lf_offset", Value::Float(0.0))]);
    n.set("line_colour", Value::Colour([1.0, 1.0, 1.0, 1.0]));
    for (k, v) in own {
        n.set(k, v);
    }

    n.set("polygon_closed", i(i64::from(closed)));
    n.set("position", Value::Vec3([c.0 - parent.0, 0.0, c.1 - parent.1].map(r32)));
    n.set("selected_line_colour", Value::Colour([1.0, 0.0, 0.0, 1.0]));
    n.set("name", s(name));
    n.set("polygon_shape", s("PS_POLYGON"));
    n.set("area", Value::Float(r32(area)));
    n.set("surface_mode", s("MSM_LANDSCAPE"));
    n.set("orientation", Value::Euler([0.0; 3]));
    let points = n.wrapped_mut("points");
    for &(x, z) in pts {
        points.push(node("point", vec![("position", Value::Vec3([g(x - c.0), 0.0, g(z - c.1)]))]));
    }

    n
}

/// The `position` children's (x, y) of an XML element.
fn xml_points(e: &Node) -> Vec<(f64, f64)> {
    e.children
        .iter()
        .filter(|c| c.class == "position")
        .map(|c| (c.get("x").and_then(Value::as_str).and_then(|v| v.parse().ok()).unwrap_or(0.0), c.get("y").and_then(Value::as_str).and_then(|v| v.parse().ok()).unwrap_or(0.0)))
        .collect()
}

/// Every descendant of a class.
fn all<'a>(n: &'a Node, class: &str, out: &mut Vec<&'a Node>) {
    for c in &n.children {
        if c.class == class {
            out.push(c);
        }

        all(c, class, out);
    }
}

fn str_attr(n: &Node, k: &str) -> String {
    n.get(k).and_then(Value::as_str).unwrap_or_default().to_owned()
}

/// The polylines of a marker record (`points`, or `polylines` > `polyline` > `points`) as (x, z) pairs.
fn record_polylines(rec: &Node) -> Vec<Vec<(f64, f64)>> {
    let pts = |v: &[f64]| v.chunks_exact(2).map(|c| (c[0], c[1])).collect::<Vec<_>>();
    if let Some(p) = rec.get("points").and_then(Value::as_floats) {
        return vec![pts(p)];
    }

    rec.child("polylines").map_or(vec![], |l| l.children.iter().filter_map(|c| c.get("points").and_then(Value::as_floats)).map(pts).collect())
}

/// TEd's AI hint type of the markers' number (the index in TEd's table).
fn hint_type(n: i64) -> String {
    // TEd's table (wce_tile::property_enums::AI_HINT_TYPE, read from TEd.AssemblyKit.exe).
    const NAMES: [&str; 41] = [
        "AIH_DEFENSIVE_LINE", "AIH_DEFENSIVE_HILL", "AIH_DEAD_GROUND", "AIH_IMPASSIBLE", "AIH_TOWN_CLUSTER", "AIH_CHOKE_POINT",
        "AIH_GOOD_CAVALRY_GROUND", "AIH_COVER", "AIH_AMBUSH_FOREST", "AIH_AMBUSH_BUILDINGS", "AIH_REDOUBT",
        "AIH_SUITABLE_GROUND_BOMBARDMENT", "AIH_SUITABLE_DEPLOYABLE_GABIONADE", "AIH_SUITABLE_DEPLOYABLE_FOUGASSE",
        "AIH_SUITABLE_DEPLOYABLE_CHEVAUX_DE_FRISE", "AIH_SUITABLE_DEPLOYABLE_EARTHWORKS", "AIH_FORDING_POINT", "AIH_FORT_APPROACH",
        "AIH_BRIDGE_CROSSING", "AIH_BEACH_LANDING", "AIH_RIVER_EDGE", "AIH_SIEGE_AREA_NODE", "AIH_SIEGE_INTERSECTION_NODE",
        "AIH_SIEGE_STREET_EDGE", "AIH_CAPTURE_POINT_MAJOR", "AIH_CAPTURE_POINT_MINOR", "AIH_SIEGE_WALL_AREA_NODE",
        "AIH_SIEGE_AREA_CONNECTION_EDGE", "AIH_SIEGE_WALL_EDGE", "AIH_COVER_LINE", "AIH_SIEGE_ENTRY_NODE", "AIH_AMBUSH_MELEE_LINE",
        "AIH_AMBUSH_MISSILE_LINE", "AIH_AMBUSH_CAVALRY_LINE", "AIH_AMBUSH_DEFENDER_INITIAL_PATH", "AIH_FORT_PERIMETER",
        "AIH_BEACH_LANDING_FORT_APPROACH", "AIH_AMBUSH_ASSAULT_AREA", "AIH_AMBUSH_FIREBALL_LINE", "AIH_DEFAULT_DEPLOYMENT_LINE",
        "AIH_SIEGE_PALISADE_EDGE",
    ];

    usize::try_from(n).ok().and_then(|i| NAMES.get(i)).map_or_else(|| format!("{n}"), |s| (*s).to_owned())
}

/// The logic layer: outlines, zones, deployment areas, AI hints and separators, the siege graph.
fn logic_layer(binary: &Tile, extent_z: f64, losses: &mut Losses) -> Node {
    let mut l = layer("Logic", "LayerExportType_Normal");
    let mut items = vec![];
    let xml_outlines = |file: &str| binary.document(file).map_or(vec![], |d| d.root.children.iter().filter(|c| c.class == "OUTLINE").map(xml_points).collect::<Vec<_>>());
    for pts in xml_outlines("go_outlines.xml") {
        items.push(polygon("GO_REGION_POLYGON_INSTANCE", "Go region", &scene_xz(&pts, extent_z), true, vec![("line_colour", Value::Colour([0.0, 0.25, 0.0, 1.0]))], (0.0, 0.0)));
    }

    for pts in xml_outlines("non_terrain_outlines.xml") {
        items.push(polygon("NOGO_REGION_POLYGON_INSTANCE", "No-go region", &scene_xz(&pts, extent_z), true, vec![("line_colour", Value::Colour([0.65, 0.0, 0.0, 1.0]))], (0.0, 0.0)));
    }

    if let Some(z) = binary.document("zones.xml") {
        let mut zones = vec![];
        all(&z.root, "BATTLEFIELD_ZONE_TEMPLATE", &mut zones);
        for t in zones {
            items.push(polygon(
                "ZONE_POLYGON_INSTANCE",
                "Zone",
                &scene_xz(&xml_points(t), extent_z),
                true,
                vec![
                    ("purpose", s("EFP_LOW_WALL")), ("rank_count", i(0)), ("zone_skirt_distance", Value::Float(3.0)), ("locked", b(false)),
                    ("max_subdivisions", i(100)), ("rank_distance", Value::Float(3.0)), ("subdivision_length", Value::Float(5.0)),
                ],
                (0.0, 0.0),
            ));
        }

        if !items.is_empty() {
            losses.note("zones: purpose, ranks and skirt are not in zones.xml (TEd's defaults)");
        }
    }

    // Deployment: a group per (alliance, area), a polygon per boundary.
    if let Some(d) = binary.document("deployment_areas.xml") {
        let mut alliances = vec![];
        all(&d.root, "ALLIANCE", &mut alliances);
        for a in alliances {
            let alliance = str_attr(a, "id").parse::<i64>().unwrap_or(0);
            for area in a.children.iter().filter(|c| c.class == "deployment_area") {
                let facing = area.child("orientation").map_or(0.0, |o| str_attr(o, "radians").parse::<f64>().unwrap_or(0.0));
                let mut g = instance(
                    "DeploymentZonePolygonInstanceGroup",
                    "Deployment Zone",
                    [0.0; 3],
                    [0.0; 3],
                    vec![
                        ("deployment_category", s(&str_attr(area, "category"))), ("deployment_type", s("EDZT_1V1")), ("new_polygon_type", s("PDM_POLYGON")),
                        ("facing_direction", Value::Float(r32(facing))), ("deployment_zone_id", i(str_attr(area, "id").parse::<i64>().unwrap_or(0))),
                        ("add_polygon", i(0)), ("alliance_id", i(alliance)),
                    ],
                );

                g.set("flags", flags(&["OIF_VISIBLE", "OIF_ACTIVE", "OIF_DRAGGABLE"]));
                for bnd in area.children.iter().filter(|c| c.class == "boundary") {
                    let kind = match str_attr(bnd, "type").as_str() {
                        "standard additive" => "DZPT_ADDITIVE",
                        "standard subtractive" => "DZPT_SUBSTRACTIVE",
                        "guerrilla exclusion additive" => "DZPT_GUERRILLA_EXCLUSION_ADDITIVE",
                        _ => "DZPT_GUERRILLA_EXCLUSION_SUBTRACTIVE",
                    };

                    g.wrapped_mut("children").push(polygon(
                        "DEPLOYMENT_ZONE_POLYGON_INSTANCE",
                        "Deployment Zone Polygon",
                        &scene_xz(&xml_points(bnd), extent_z),
                        true,
                        vec![
                            ("line_colour", Value::Colour([0.0, 0.0, 1.0, 0.7])), ("subdivision_length", Value::Float(5.0)), ("max_subdivisions", i(100)),
                            ("deployment_polygon_type", s(kind)),
                        ],
                        (0.0, 0.0),
                    ));
                }

                items.push(g);
            }
        }

        losses.note("deployment: the deployment type (1v1 ...) is not in deployment_areas.xml (EDZT_1V1)");
    }

    // AI hints and separators.
    if let Some(d) = binary.document("ai_hint_polyline_marker.markers") {
        for (rec, key) in marker_records(&d.root) {
            let ty = hint_type(key.and_then(Value::as_int).unwrap_or(0));
            for mut pts in record_polylines(rec) {
                // A closed hint (the fort perimeter) repeats its first point at the end.
                let closed = pts.len() > 3 && pts.first().zip(pts.last()).is_some_and(|(a, b)| (a.0 - b.0).abs() < 1e-4 && (a.1 - b.1).abs() < 1e-4);
                if closed {
                    pts.pop();
                }

                items.push(polygon(
                    "AI_HINT_POLYGON_INSTANCE",
                    "AI hint polygon",
                    &scene_xz(&pts, extent_z),
                    closed,
                    vec![
                        ("subdivision_length", Value::Float(5.0)), ("max_subdivisions", i(100)), ("hint_type", s(&ty)), ("is_generated", b(false)),
                        ("generated", b(false)),
                    ],
                    (0.0, 0.0),
                ));
            }
        }
    }

    if let Some(d) = binary.document("ai_separator_marker.markers") {
        for (rec, key) in marker_records(&d.root) {
            let ty = key.and_then(Value::as_str).unwrap_or("PALT_NONE").to_owned();
            for pts in record_polylines(rec) {
                items.push(polygon(
                    "AISeparatorInstance",
                    "AI Separator",
                    &scene_xz(&pts, extent_z),
                    false,
                    vec![("subdivision_length", Value::Float(5.0)), ("max_subdivisions", i(100)), ("type", s(&ty))],
                    (0.0, 0.0),
                ));
            }
        }
    }

    // The siege graph: a node (area, intersection, wall area, entry: its boundary and bounding box) or an edge (street, connection, wall:
    // the street and its boundaries) per polyline list.
    if let Some(d) = binary.document("ai_hint_polyline_list_marker.markers") {
        for (rec, key) in marker_records(&d.root) {
            let lines = record_polylines(rec);
            let k = key.and_then(Value::as_int).unwrap_or(0);
            let (class, ty, names) = match k {
                21 => ("SiegeAINodeInstance", "SANT_AREA", ["Bounding Box", "Boundary"]),
                22 => ("SiegeAINodeInstance", "SANT_INTERSECTION", ["Bounding Box", "Boundary"]),
                26 => ("SiegeAINodeInstance", "SANT_WALL_AREA", ["Bounding Box", "Boundary"]),
                30 => ("SiegeAINodeInstance", "SANT_ENTRY", ["Bounding Box", "Boundary"]),
                23 => ("SiegeAIEdgeInstance", "SAET_STREET", ["Street", "Street boundary"]),
                27 => ("SiegeAIEdgeInstance", "SAET_AREA_CONNECTION", ["Street", "Street boundary"]),
                _ => ("SiegeAIEdgeInstance", "SAET_WALL", ["Street", "Street boundary"]),
            };

            let node_pos = if class == "SiegeAINodeInstance" {
                lines.first().map_or((0.0, 0.0), |p| {
                    let p = scene_xz(p, extent_z);
                    let (mut lo, mut hi) = ((f64::MAX, f64::MAX), (f64::MIN, f64::MIN));
                    for &(x, z) in &p {
                        lo = (lo.0.min(x), lo.1.min(z));
                        hi = (hi.0.max(x), hi.1.max(z));
                    }

                    ((lo.0 + hi.0) / 2.0, (lo.1 + hi.1) / 2.0)
                })
            } else {
                (0.0, 0.0)
            };

            let mut n = instance(class, if class == "SiegeAINodeInstance" { "Siege AI Node" } else { "Siege AI Edge" }, [node_pos.0, 0.0, node_pos.1], [0.0; 3], vec![("add_boundary", i(0)), ("type", s(ty))]);
            n.set("flags", flags(&["OIF_VISIBLE", "OIF_ACTIVE", "OIF_DRAGGABLE"]));
            for (j, pts) in lines.iter().enumerate() {
                let name = names[j.min(1)];
                // A node's bounding box and boundary are closed, an edge's street and boundaries open.
                let closed = class == "SiegeAINodeInstance";
                n.wrapped_mut("children").push(polygon("SIEGE_AI_POLYGON_INSTANCE", name, &scene_xz(pts, extent_z), closed, vec![], node_pos));
            }

            items.push(n);
        }
    }

    // Civilian deployment zones (civilian_deployment.bin): a polygon each with its kind and size.
    if let Some(d) = binary.document("civilian_deployment.bin") {
        let mut list = vec![];
        all(&d.root, "civilian_deployment_list", &mut list);
        for zone in list.iter().flat_map(|l| l.children.iter().filter(|c| c.class == "item")) {
            let mut pts = vec![];
            if let Some(points) = zone.child("area").and_then(|a| a.child("points")) {
                for p in &points.children {
                    pts.push((num(p, "x"), num(p, "y")));
                }
            }

            items.push(polygon(
                "CivilianDeploymentZone",
                "Civilian Deployment Zone",
                &scene_xz(&pts, extent_z),
                true,
                vec![("line_colour", Value::Colour([0.294, 0.0, 0.51, 1.0])), ("civilian_type", s(&str_attr(zone, "kind"))), ("civilian_amount", s(&str_attr(zone, "size")))],
                (0.0, 0.0),
            ));
        }
    }

    if binary.document("civilian_shelter.bin").is_some() {
        losses.note("civilian shelters: not decompiled (BOB generates them for tiles whose tile.ted has none)");
    }

    *l.wrapped_mut("children") = items;
    l
}

/// The capture locations of a BMD document (`capture_location_set` > `capture_location_sets` > `item` > `capture_locations` > `item`) as
/// TEd's CaptureLocationInstance (at the flag: the binary's `location`) with its boundary: a circle of the binary's radius doubled around
/// the one location point, or a polygon through the location points.
fn capture_nodes(bmd: &Node, extent_z: f64) -> Vec<Node> {
    let mut sets = vec![];
    all(bmd, "capture_locations", &mut sets);
    let mut out = vec![];
    for loc in sets.iter().flat_map(|s| s.children.iter().filter(|c| c.class == "item")) {
        let l = loc.child("location");
        let (lx, lz) = (2.0 * l.map_or(0.0, |l| num(l, "x")), 2.0 * (extent_z - l.map_or(0.0, |l| num(l, "y"))));
        let f = loc.child("flag_facing");
        // The binary's facing vector (x, y) is TEd's angle atan2(-y, x) (the z mirror).
        let facing = (-f.map_or(0.0, |f| num(f, "y"))).atan2(f.map_or(1.0, |f| num(f, "x"))).rem_euclid(std::f64::consts::TAU);
        let ty = if str_attr(loc, "capture_point_type") == "CAPTURE_LOCATION_MAJOR" { "CLT_MAJOR" } else { "CLT_MINOR" };
        let mut n = instance(
            "CaptureLocationInstance",
            "capture_point",
            [lx, 0.0, lz],
            [0.0; 3],
            vec![
                ("flag_facing", Value::Float(r32(facing))), ("database_type", s(&str_attr(loc, "database_key"))),
                ("min_num_players", i(loc.get("valid_for_min_num_players").and_then(Value::as_int).unwrap_or(2))), ("type", s(ty)),
                ("max_num_players", i(loc.get("valid_for_max_num_players").and_then(Value::as_int).unwrap_or(8))), ("flag_position", Value::Vec3([0.0; 3])),
            ],
        );

        n.set("flags", flags(&["OIF_VISIBLE", "OIF_ACTIVE", "OIF_DRAGGABLE"]));
        n.set("surface_mode", s("MSM_LANDSCAPE"));
        let pts = loc.child("location_points").map_or(vec![], |p| p.children.iter().map(|c| (num(c, "x"), num(c, "y"))).collect::<Vec<_>>());
        let radius = num(loc, "radius");
        let boundary = if radius > 0.0 && pts.len() <= 1 {
            // TEd's circle: 20 points from +z towards +x, (r sin a, r cos a) for a = 0, 18, ... 342 degrees.
            let c = pts.first().map_or((lx, lz), |&(x, z)| (2.0 * x, 2.0 * (extent_z - z)));
            let r = 2.0 * radius;
            let ring = (0..20).map(|k| {
                let a = f64::from(k) * 18f64.to_radians();
                (c.0 + r * a.sin(), c.1 + r * a.cos())
            }).collect::<Vec<_>>();

            let mut b = polygon("CaptureLocationPolygonInstance", "Boundary", &ring, true, vec![("radius", Value::Float(r32(r)))], (lx, lz));
            b.set("polygon_shape", s("PS_CIRCLE"));
            b
        } else {
            polygon("CaptureLocationPolygonInstance", "Boundary", &scene_xz(&pts, extent_z), true, vec![], (lx, lz))
        };

        n.wrapped_mut("children").push(boundary);
        out.push(n);
    }

    out
}

#[derive(Default)]
struct BuildingLosses {
    unknown_levels: usize,
    scaled: usize,
    start_disabled: usize,
    position_type: usize,
}

/// The TEd project of an exported tile. `folder` is the tile's folder in the pack (`terrain/tiles/battle/<set>/<tile>/<variation>`).
///
/// # Errors
///
/// The databases do not have the tile.
pub fn decompile(binary: &Tile, folder: &str, ctx: &Context) -> Result<(Tile, Losses), String> {
    let databases = ctx.databases;
    let (location, variation) = split_location(folder);
    let (d, index) = find_tile(databases, &location).ok_or_else(|| format!("no tile database has {location}"))?;
    let row = &databases[d].tiles()[index];
    let mut losses = Losses::default();
    let mut out = Tile { source_format: None, variation: Some(variation.clone()), ..Default::default() };
    let mut entry = entry_node(&databases[d], index);
    let vi = row.variations().iter().position(|v| v.location().eq_ignore_ascii_case(&variation));
    if vi.is_none() {
        losses.note(format!("the entry has no variation {variation}"));
    }

    let border_cells = vi.map_or(1.0, |v| *row.variations()[v].overlap_border_size()) as u32;
    if let Some((raster, min, range)) = height_map(binary, (*row.width(), *row.height()), border_cells, &mut losses) {
        out.maps.push(TileMap { kind: MapKind::Height, index: 0, raster });

        // The variation's numbers become the binary's, so the raw values are its samples.
        if let Some(v) = vi
            && let Some(vn) = entry.child_mut("VARIATIONS").and_then(|vs| vs.children.get_mut(v))
        {
            let (old_min, old_scale) = (vn.get("min_height").and_then(Value::as_f64), vn.get("scale").and_then(Value::as_f64));
            if old_min != f(min).as_f64() || old_scale != f(range).as_f64() {
                losses.note(format!("entry: {variation} min_height / scale {old_min:?} / {old_scale:?} -> {min} / {range} (the binary's heights)"));
            }

            vn.set("min_height", f(min));
            vn.set("scale", f(range));
        }
    } else {
        losses.note("no hf_height_map.data: no height map");
    }

    // The scene.
    let extent = (f64::from(*row.width()) * 128.0, f64::from(*row.height()) * 128.0);
    let mut p = project(extent);
    losses.note("tile.ted: the camera, the vegetation seeds and the layers' names and structure are not in the binary (made up)");
    let mut bl = BuildingLosses::default();
    for (file, layer_name) in [("building_list", "Buildings"), ("far_building_list", "Far buildings"), ("building_list_sprawl", "Sprawl")] {
        let Some(doc) = binary.document(file) else { continue };
        let list = buildings(doc)?;
        if list.is_empty() {
            continue;
        }

        let mut l = layer(layer_name, "LayerExportType_Normal");
        for b in &list {
            l.wrapped_mut("children").push(building_node(b, extent.1, ctx, &mut bl));
        }

        graph(&mut p).wrapped_mut("children").push(l);
    }

    // The escalation layers (TEd's LayerExportType_Escalation<n>, exported into bmd_data_escalation<n>).
    for n in 1..=3 {
        let Some(doc) = binary.document(&format!("bmd_data_escalation{n}.bin")) else { continue };
        let items = |list: &str| doc.root.child(list).and_then(|l| l.child("markers")).map_or(&[][..], |m| &m.children[..]);
        let mut l = layer(&format!("Escalation {n}"), &format!("LayerExportType_Escalation{n}"));
        for b in &bmd_buildings(&doc.root) {
            l.wrapped_mut("children").push(building_node(b, extent.1, ctx, &mut bl));
        }

        for it in items("prop_list") {
            let (rec, key) = bmd_prop_record(it);
            l.wrapped_mut("children").push(prop_node(&rec, &key, extent.1));
        }

        for it in items("particle_emitter_list") {
            let (rec, key) = bmd_emitter_record(it);
            l.wrapped_mut("children").push(emitter_node(&rec, &key, extent.1));
        }

        l.wrapped_mut("children").extend(capture_nodes(&doc.root, extent.1));

        if !l.wrapped("children").is_empty() {
            graph(&mut p).wrapped_mut("children").push(l);
        }
    }

    // The markers of the normal layers.
    let key_str = |k: Option<&Value>| k.and_then(Value::as_str).unwrap_or_default().to_owned();
    for (file, layer_name) in [("prop_marker.markers", "Props"), ("particle_emitter_marker.markers", "VFX"), ("point_light_marker.markers", "Point lights")] {
        let Some(doc) = binary.document(file) else { continue };
        let recs = marker_records(&doc.root);
        if recs.is_empty() {
            continue;
        }

        let mut l = layer(layer_name, "LayerExportType_Normal");
        for (rec, key) in recs {
            let key = key_str(key);
            l.wrapped_mut("children").push(match file {
                "prop_marker.markers" => prop_node(rec, &key, extent.1),
                "particle_emitter_marker.markers" => emitter_node(rec, &key, extent.1),
                _ => light_node(rec, &key, extent.1),
            });
        }

        graph(&mut p).wrapped_mut("children").push(l);
    }

    let logic = logic_layer(binary, extent.1, &mut losses);
    if !logic.wrapped("children").is_empty() {
        losses.note("logic: the polygons' heights are not in the binary (landscape polygons at height 0)");
        graph(&mut p).wrapped_mut("children").push(logic);
    }

    // The trees: one list, as explicit instances (the seeds that would regenerate them are not in the binary).
    let lists = ctx.tree_list.map_or(vec!["outfield_tents.tree_list.bin", "default.tree_list.bin"], |l| vec![l]);
    if let Some(doc) = lists.iter().find_map(|l| binary.document(l)) {
        let trees = tree_nodes(&doc.root, extent.1);
        if !trees.is_empty() {
            losses.note(format!("vegetation: {} trees of {} as explicit instances (the generation seeds are not in the binary)", trees.len(), doc.name));
            let mut l = layer("Decompiled vegetation", "LayerExportType_Normal");
            *l.wrapped_mut("children") = trees;
            graph(&mut p).wrapped_mut("children").push(l);
        }
    }

    if bl.unknown_levels > 0 {
        losses.note(format!("buildings: {} damaged buildings without a known number of destruction levels: level 1", bl.unknown_levels));
    }

    if bl.scaled + bl.start_disabled + bl.position_type > 0 {
        losses.note(format!(
            "buildings: TEd has no scale, start_disabled or other position type: {} scaled, {} disabled, {} not lf_relative",
            bl.scaled, bl.start_disabled, bl.position_type
        ));
    }

    losses.note("buildings: surface_mode (landscape / freeform) is not in the binary: freeform, at the binary's heights");
    out.scene = Some(p);
    out.entry = Some(Entry { file_name: entry_file_name(&location), root: entry });
    Ok((out, losses))
}
