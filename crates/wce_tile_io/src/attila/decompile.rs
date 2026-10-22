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
