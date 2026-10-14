//! The element index, the building models of the tile, the marker lists, and what the document answers about an element.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use rayon::prelude::*;
use wce_formats::c_num::strtod_prefix;
use wce_formats::tile_climate::ClimateList;
use wce_foundation::math::{Float3, Float4x4};
use wce_foundation::{log, wce_prof};
use wce_scene::render_scene::overlay_colour;
use wce_tile::ted_scene::{TedScene, Vec3, parse_vec3};
use wce_tile::world_transform::{WorldTransform, euler_matrix, mat_mul, rotate_vec, world_transform};

use super::{BuildingModel, INSTANCE_BIT, Index, LAYER_BIT, Markers, POLYGON_BIT, PieceState, PieceStatus, Placement, TileTypeStyle, Tile};

pub fn style_of(t: &str) -> TileTypeStyle {
    let s = |r: f32, g: f32, b: f32, size: f32, cross: bool| TileTypeStyle { color: overlay_colour(r, g, b, 1.0), size, cross };
    match t {
        "BUILDING_INSTANCE" => s(0.85, 0.45, 0.2, 5.0, false),
        "VEGETATION_INSTANCE" => s(0.2, 0.7, 0.25, 1.5, true),
        "ProjectedDecalInstance" => s(0.55, 0.45, 0.35, 1.5, true),
        "VFXInstance" => s(0.75, 0.35, 0.9, 2.0, false),
        "POINT_LIGHT_INSTANCE" => s(1.0, 0.95, 0.4, 1.5, true),
        "SIMPLE_MESH_INSTANCE" => s(0.6, 0.6, 0.75, 4.0, false),
        "CaptureLocationInstance" => s(0.2, 0.6, 1.0, 3.0, false),
        "SiegeAINodeInstance" | "SiegeAIEdgeInstance" => s(0.95, 0.2, 0.2, 2.0, false),
        _ if t.contains("POLYGON") || t.contains("Polygon") || t.contains("Zone") || t == "AISeparatorInstance" => s(0.3, 0.9, 0.9, 1.5, true),
        _ => s(0.8, 0.8, 0.8, 2.0, false),
    }
}

/// A number the way `std::from_chars` reads one: no leading whitespace or '+', otherwise what `strtod` reads.
fn from_chars(s: &str) -> Option<(f64, usize)> {
    match s.as_bytes().first() {
        None | Some(b'+' | b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C) => None,
        Some(_) => strtod_prefix(s),
    }
}

/// "vector(1,2,3)" / "euler(..)" (prefix) or "x,y,z" (empty prefix); spaces are allowed around the numbers. `None` when the text is anything
/// else. Numbers `from_chars` does not take ("+1", " 1") go through `parse_vec3`.
pub(crate) fn parse_triple(s: &str, prefix: &str) -> Option<[f64; 3]> {
    let mut s = s;
    if !prefix.is_empty() {
        if s.len() < prefix.len() + 2 || !s.starts_with(prefix) || s.as_bytes()[prefix.len()] != b'(' || !s.ends_with(')') {
            return None;
        }

        s = &s[prefix.len() + 1..s.len() - 1];
    }

    let mut out = [0.0; 3];
    let mut p = s;
    for (i, slot) in out.iter_mut().enumerate() {
        p = p.trim_start_matches(' ');
        let Some((v, used)) = from_chars(p) else { return parse_vec3(s, "") };
        *slot = v;
        p = &p[used..];
        if i < 2 {
            p = p.trim_start_matches(' ');
            p = p.strip_prefix(',')?;
        }
    }

    Some(out)
}

pub(crate) fn matrix_of(w: &WorldTransform, unit: f64, scale: Option<Vec3>) -> Float4x4 {
    let mut m = Float4x4::IDENTITY;
    let s = scale.unwrap_or([1.0; 3]);
    for (r, row) in m.m.iter_mut().take(3).enumerate() {
        for (c, v) in row.iter_mut().take(3).enumerate() {
            *v = (w.rotation[r * 3 + c] * s[c]) as f32;
        }
    }

    m.m[0][3] = (w.position[0] / unit) as f32;
    m.m[1][3] = w.position[1] as f32;
    m.m[2][3] = (w.position[2] / unit) as f32;
    m
}

fn text(raw: &[u8]) -> std::borrow::Cow<'_, str> {
    String::from_utf8_lossy(raw)
}

/// The parsed local transform and flags of one element.
#[derive(Clone, Copy, Default)]
struct Local {
    pos: Option<[f64; 3]>,
    eul: Option<[f64; 3]>,

    /// 1: has a flags attribute, 2: OIF_VISIBLE, 4: OIF_ACTIVE.
    flagbits: u8,
    kind: u8,
    mode: u8,
    scale: Option<[f32; 3]>,
}

impl Tile {
    pub(crate) fn unit(&self) -> f64 {
        self.project.unit_scale
    }

    pub fn tscene(&self) -> &TedScene {
        &self.project.scene
    }

    pub fn title(&self) -> String {
        let d = &self.project.dir;
        let name = |p: &std::path::Path| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        format!("{}  ({})", name(d), d.parent().map(name).unwrap_or_default())
    }

    pub fn extent_x(&self) -> f64 {
        self.project.extent_x()
    }

    pub fn extent_z(&self) -> f64 {
        self.project.extent_z()
    }

    /// Metres: the terrain height under (x, z).
    pub fn terrain_height(&self, x: f32, z: f32) -> f32 {
        self.project.height_at(f64::from(x), f64::from(z)) as f32
    }

    pub fn landscape_y(&self, _elem: usize, x: f32, z: f32) -> f32 {
        self.terrain_height(x, z)
    }

    fn elements(&self) -> &[wce_formats::xml_doc::XmlElement] {
        self.tscene().doc().elements()
    }

    pub fn is_polygon(&self, elem: usize) -> bool {
        if self.idx.valid && elem < self.idx.kind.len() {
            return self.idx.kind[elem] & POLYGON_BIT != 0;
        }

        elem < self.elements().len() && !self.tscene().polygon_points(elem).is_empty()
    }

    /// The element's own attributes, not the index (which is stale right after a structural edit).
    pub(crate) fn is_instance_slow(&self, elem: usize) -> bool {
        let Some(el) = self.elements().get(elem) else { return false };
        if matches!(el.name.as_str(), "point" | "points" | "children" | "camera") || self.tscene().is_layer(elem) {
            return false;
        }

        self.tscene().position(elem).is_some()
    }

    /// An instance with a position (not a layer, not a polygon point).
    pub fn is_instance(&self, elem: usize) -> bool {
        if self.idx.valid && elem < self.idx.kind.len() {
            return self.idx.kind[elem] & INSTANCE_BIT != 0;
        }

        self.is_instance_slow(elem)
    }

    /// The nearest layer at or above `elem` (the scene graph for loose ones).
    pub fn layer_of(&self, elem: usize) -> Option<usize> {
        let mut p = Some(elem);
        while let Some(q) = p {
            if self.tscene().is_layer(q) {
                return Some(q);
            }

            p = self.elements()[q].parent;
        }

        self.tscene().scene_graph()
    }

    fn metres(&self, w: &WorldTransform) -> Float3 {
        Float3::new((w.position[0] / self.unit()) as f32, w.position[1] as f32, (w.position[2] / self.unit()) as f32)
    }

    pub fn world_position(&self, elem: usize) -> Float3 {
        if self.idx.valid && elem < self.idx.slot.len() {
            return self.metres(&self.idx.world[self.idx.slot[elem] as usize]);
        }

        self.metres(&world_transform(self.tscene(), elem))
    }

    pub fn world_matrix(&self, elem: usize) -> Float4x4 {
        if self.idx.valid && elem < self.idx.slot.len() {
            let s = self.idx.scale[elem];
            return matrix_of(&self.idx.world[self.idx.slot[elem] as usize], self.unit(), Some([f64::from(s[0]), f64::from(s[1]), f64::from(s[2])]));
        }

        matrix_of(&world_transform(self.tscene(), elem), self.unit(), self.tscene().scale(elem))
    }

    pub fn building_model(&self, key: &str) -> Option<&BuildingModel> {
        self.models.get(key)
    }

    /// Of a BUILDING_INSTANCE element.
    pub fn model_of(&self, elem: usize) -> Option<&BuildingModel> {
        if self.idx.valid && elem < self.idx.key.len() {
            return self.idx.key[elem].and_then(|k| self.models.get(&self.idx.keys[k as usize]));
        }

        self.building_model(&self.tscene().doc().get(elem, "key").unwrap_or_default())
    }

    /// What a click ray tests against.
    pub fn marker_radius(&self, elem: usize) -> f32 {
        if let Some(m) = self.model_of(elem).filter(|m| m.ok) {
            let e = m.bmax - m.bmin;
            return (0.5 * e.x.max(e.y).max(e.z)).clamp(2.0, 80.0);
        }

        if self.idx.valid && elem < self.idx.type_.len() {
            return self.idx.style[self.idx.type_[elem] as usize].size * 0.75;
        }

        style_of(self.tscene().type_of(elem)).size * 0.75
    }

    pub fn type_counts(&self) -> &BTreeMap<String, i32> {
        &self.type_counts
    }

    pub fn instances(&self) -> &[usize] {
        &self.instances
    }

    pub fn model_fallbacks(&self) -> &[String] {
        &self.fallbacks
    }

    pub fn structure_version(&self) -> u64 {
        self.structure_version
    }

    pub fn stroke_open(&self) -> bool {
        self.stroke_open
    }

    /// The kit's _settings.xml, read on first use (empty outside a kit).
    pub fn climates(&self) -> &ClimateList {
        self.climates.get_or_init(|| {
            let Some(entry) = &self.project.entry else { return ClimateList::default() };
            let Some(settings) = entry.file().parent().and_then(|p| p.parent()).map(|p| p.join("_settings.xml")) else { return ClimateList::default() };
            ClimateList::load(&settings).unwrap_or_else(|e| {
                log::warn(format!("climates: {e}"));
                ClimateList::default()
            })
        })
    }

    // ---- placement

    /// The element's own surface_mode.
    pub fn placement(&self, elem: usize) -> Placement {
        if self.idx.valid && elem < self.idx.mode.len() {
            return match self.idx.mode[elem] {
                1 => Placement::Landscape,
                2 => Placement::Freeform,
                _ => Placement::None,
            };
        }

        self.placement_of_attr(elem)
    }

    /// `placement` from the text, for elements the index does not know yet (a copy made in this edit).
    pub(crate) fn placement_of_attr(&self, elem: usize) -> Placement {
        if elem >= self.elements().len() {
            return Placement::None;
        }

        match self.tscene().doc().get(elem, "surface_mode").as_deref() {
            Some("MSM_LANDSCAPE") => Placement::Landscape,
            Some("MSM_FREEFORM") => Placement::Freeform,
            _ => Placement::None,
        }
    }

    /// A landscape instance stands on the terrain whatever y its file has: TEd re-snaps them against the tile's current height map and entry,
    /// while the file's y comes from the height map of the day (5 m off in tiles whose entry min_height / scale changed later). The element
    /// index holds the snapped height of leaf instances (buildings, vegetation, decals, ...; polygons and splines carry their own heights); the
    /// file keeps the y it had until the instance is edited (TEd shows the same on opening it).
    pub(crate) fn snap_landscape_world(&self, elem: usize, w: &mut WorldTransform) {
        snap(self, &self.idx.mode, &self.idx.kind, elem, w);
    }

    // ---- layer flags

    /// This element's own flags attribute (true when it has none).
    pub fn own_flag(&self, elem: usize, flag: &str) -> bool {
        self.tscene().doc().get(elem, "flags").is_none_or(|f| f.contains(flag))
    }

    /// Visible itself and in every ancestor.
    pub fn shown(&self, elem: usize) -> bool {
        self.state_bit(elem, 1, "OIF_VISIBLE")
    }

    /// Active itself and in every ancestor.
    pub fn selectable(&self, elem: usize) -> bool {
        self.state_bit(elem, 2, "OIF_ACTIVE")
    }

    fn state_bit(&self, elem: usize, bit: u8, flag: &str) -> bool {
        if self.idx.valid && elem < self.idx.state.len() {
            return self.idx.state[elem] & bit != 0;
        }

        let mut p = Some(elem);
        while let Some(q) = p {
            if !self.own_flag(q, flag) {
                return false;
            }

            p = self.elements()[q].parent;
        }

        true
    }

    // ---- the element index

    pub(crate) fn build_index(&mut self) {
        wce_prof!("tile.index");
        let doc = self.project.scene.doc();
        let els = doc.elements();
        let n = els.len();
        let mut ix = Index { slot: vec![0; n], key: vec![None; n], ..Default::default() };

        // Element type names -> small ids.
        let mut ids = HashMap::<&str, u16>::new();
        ix.type_ = els.iter().map(|el| {
            *ids.entry(el.name.as_str()).or_insert_with(|| {
                ix.type_names.push(el.name.clone());
                (ix.type_names.len() - 1) as u16
            })
        }).collect();

        ix.style = ix.type_names.iter().map(|t| style_of(t)).collect();

        // Which type ids are layers / never instances.
        let layer_type = ix.type_names.iter().map(|s| matches!(s.as_str(), "LayerInstance" | "UserInstanceGroup" | "INSTANCE_GROUP" | "SCENE_GRAPH")).collect::<Vec<_>>();
        let plain_type = ix.type_names.iter().map(|s| matches!(s.as_str(), "point" | "points" | "children" | "camera")).collect::<Vec<_>>();
        let key_type = ix.type_names.iter().position(|s| s == "BUILDING_INSTANCE");

        // Parse the local transforms in parallel (each element reads only its own attributes).
        let types = &ix.type_;
        let local = (0..n).into_par_iter().map(|i| {
            let mut l = Local::default();
            let t = types[i] as usize;
            let mut kind = if layer_type[t] { LAYER_BIT } else { 0 };

            // Points, wrappers: nothing to parse (a point's position is a plain triple, not a vector).
            if plain_type[t] && kind == 0 {
                return l;
            }

            let raw = |name: &str| doc.find_attr(i, name).map(|k| doc.attr_raw(i, k));
            if let Some(fv) = raw("flags") {
                let fv = text(fv);
                l.flagbits = 1 | if fv.contains("OIF_VISIBLE") { 2 } else { 0 } | if fv.contains("OIF_ACTIVE") { 4 } else { 0 };
            }

            l.pos = raw("position").and_then(|r| parse_triple(&text(r), "vector"));
            l.eul = raw("orientation").and_then(|r| parse_triple(&text(r), "euler"));
            if kind == 0 && l.pos.is_some() {
                kind |= INSTANCE_BIT;
                if let Some(m) = raw("surface_mode") {
                    l.mode = match m {
                        b"MSM_LANDSCAPE" => 1,
                        b"MSM_FREEFORM" => 2,
                        _ => 0,
                    };
                }

                if let Some(r) = raw("scale") {
                    let r = text(r);
                    if let Some(s3) = parse_triple(&r, "vector") {
                        l.scale = Some([s3[0] as f32, s3[1] as f32, s3[2] as f32]);
                    } else if let Some((d, _)) = strtod_prefix(&r) {
                        l.scale = Some([d as f32; 3]);
                    }
                }

                // A polygon: an instance with a <points> child holding <point>s.
                if let Some(&c) = els[i].children.iter().find(|&&c| els[c].name == "points")
                    && els[c].children.iter().any(|&p| els[p].name == "point")
                {
                    kind |= POLYGON_BIT;
                }
            }

            l.kind = kind;
            l
        }).collect::<Vec<_>>();

        ix.kind = local.iter().map(|l| l.kind).collect();
        ix.mode = local.iter().map(|l| l.mode).collect();
        ix.scale = local.iter().map(|l| l.scale.unwrap_or([1.0; 3])).collect();
        ix.state = vec![3; n];

        // Compose the parent chains: elements are in document order, so a parent always comes before its children.
        ix.world = vec![WorldTransform::default()];
        let mut key_ids = HashMap::<String, u32>::new();
        for i in 0..n {
            let p = els[i].parent;

            // Visibility and selectability are inherited from the ancestors.
            let ps = p.map_or(3, |p| ix.state[p]);
            let fb = local[i].flagbits;
            ix.state[i] = if fb != 0 { ps & (u8::from(fb & 2 != 0) | if fb & 4 != 0 { 2 } else { 0 }) } else { ps };

            let mut slot = p.map_or(0, |p| ix.slot[p]);
            let l = &local[i];
            if l.pos.is_some() || l.eul.is_some() {
                let mut w = ix.world[slot as usize];
                if let Some(pos) = l.pos {
                    let r = rotate_vec(&w.rotation, pos);
                    for (a, b) in w.position.iter_mut().zip(r) {
                        *a += b;
                    }
                }

                if let Some(e) = l.eul {
                    w.rotation = mat_mul(&w.rotation, &euler_matrix(e));
                }

                slot = ix.world.len() as u32;

                // ix.mode / ix.kind of this element are set; the children (none for a leaf) compose from the file's transform.
                snap(self, &ix.mode, &ix.kind, i, &mut w);
                ix.world.push(w);
            }

            ix.slot[i] = slot;
            if Some(ix.type_[i] as usize) == key_type && ix.kind[i] & INSTANCE_BIT != 0
                && let Some(k) = doc.find_attr(i, "key")
            {
                let key = text(doc.attr_raw(i, k)).into_owned();
                if !key.is_empty() {
                    let next = key_ids.len() as u32;
                    let id = *key_ids.entry(key.clone()).or_insert_with(|| {
                        ix.keys.push(key);
                        next
                    });

                    ix.key[i] = Some(id);
                }
            }
        }

        ix.valid = true;
        self.idx = ix;
        self.grid_dirty.set(true);
    }

    /// The position / orientation of one element changed.
    pub(crate) fn index_edited(&mut self, elem: usize) {
        if !self.idx.valid || elem >= self.idx.slot.len() {
            return;
        }

        let parent = self.elements()[elem].parent;
        let own = self.idx.slot[elem];
        if own != 0 && parent.is_some_and(|p| own != self.idx.slot[p]) && self.idx.kind[elem] & LAYER_BIT == 0 {
            // An instance owns its slot: only it changes.
            let mut w = world_transform(self.tscene(), elem);
            self.snap_landscape_world(elem, &mut w);
            self.idx.world[own as usize] = w;
        } else {
            // A layer (or an element without a transform of its own): everything below it moved.
            self.build_index();
        }

        self.grid_dirty.set(true);
    }

    // ---- buildings

    pub(crate) fn log_fallbacks(&self) {
        if self.fallbacks.is_empty() {
            return;
        }

        let mut list = self.fallbacks.iter().take(8).cloned().collect::<Vec<_>>().join(", ");
        if self.fallbacks.len() > 8 {
            list += ", ...";
        }

        log::warn(format!("tile {}: {} of {} building keys have no drawable model and show as boxes: {list}", self.title(), self.fallbacks.len(), self.idx.keys.len()));
    }

    /// The pieces of every building key: the intact state of every piece in rigidmodels/buildings/<key>/. A key nobody has looked at yet is
    /// assumed drawable until its pieces have loaded (`poll_models`). The C++ code also extracted the pieces and their textures in the
    /// background the first time a tile was opened; the packs are read in-process now, so there is nothing to extract.
    pub(crate) fn resolve_buildings(&mut self) {
        wce_prof!("tile.resolve_buildings");
        if self.idx.keys.is_empty() {
            return;
        }

        // Keys no one has looked at yet.
        let fresh = self.idx.keys.iter().filter(|k| !self.models.contains_key(*k)).cloned().collect::<BTreeSet<_>>();
        let mut folders = BTreeMap::<String, Vec<String>>::new();
        if let Some(files) = self.files.as_ref().filter(|_| !fresh.is_empty()) {
            let prefix = "rigidmodels/buildings/";
            for p in files.list(prefix, ".rigid_model_v2") {
                let slash = p.rfind('/').unwrap_or(0);
                let Some(folder) = p.get(prefix.len()..slash) else { continue };
                if !fresh.contains(folder) {
                    continue;
                }

                let file = &p[slash + 1..];
                if file.contains("destruct") && !file.contains("destruct01") {
                    continue;
                }

                folders.entry(folder.to_owned()).or_default().push(p.clone());
            }
        }

        let mut wanted = 0;
        for key in self.idx.keys.clone() {
            if self.models.contains_key(&key) {
                continue;
            }

            let pieces = folders.remove(&key).unwrap_or_default();
            let ok = !pieces.is_empty();
            wanted += pieces.len();
            if !ok {
                self.fallbacks.push(key.clone());
            }

            self.models.insert(key, BuildingModel { pieces, ok, resolved: !ok, ..Default::default() });
        }

        self.unresolved = (0..self.idx.keys.len()).filter(|&k| self.models.get(&self.idx.keys[k]).is_some_and(|m| m.ok && !m.resolved)).collect();
        if self.unresolved.is_empty() && (wanted != 0 || fresh.len() == self.idx.keys.len()) {
            self.log_fallbacks();
        }
    }

    /// Per frame: looks at the building pieces that were still loading; true when a building turned out to have no drawable model (it is a box
    /// now: the markers were rebuilt).
    pub fn poll_models(&mut self, peek: &dyn Fn(&str) -> PieceState) -> bool {
        if self.unresolved.is_empty() {
            return false;
        }

        let (mut changed, mut any) = (false, false);
        let mut still = vec![];
        for k in std::mem::take(&mut self.unresolved) {
            let key = self.idx.keys[k].clone();
            let m = self.models.get_mut(&key).expect("an unresolved key has a model");
            let (mut loading, mut ready) = (0, 0);
            let (mut lo, mut hi) = (Float3::new(1e30, 1e30, 1e30), Float3::new(-1e30, -1e30, -1e30));
            for piece in &m.pieces {
                let s = peek(piece);
                match s.status {
                    PieceStatus::Loading => loading += 1,
                    PieceStatus::Ready => {
                        ready += 1;
                        lo = Float3::new(lo.x.min(s.bmin.x), lo.y.min(s.bmin.y), lo.z.min(s.bmin.z));
                        hi = Float3::new(hi.x.max(s.bmax.x), hi.y.max(s.bmax.y), hi.z.max(s.bmax.z));
                    }

                    PieceStatus::Failed => {}
                }
            }

            if loading != 0 {
                still.push(k);
                continue;
            }

            m.ok = ready > 0;
            m.resolved = true;
            if m.ok {
                m.bmin = lo;
                m.bmax = hi;
            } else {
                self.fallbacks.push(key);
                changed = true;
            }

            any = true;
        }

        self.unresolved = still;
        if any {
            self.grid_dirty.set(true);
        }

        if self.unresolved.is_empty() && any {
            self.log_fallbacks();
        }

        // The buildings without a model become markers.
        if changed {
            self.rebuild_items(false);
        }

        changed
    }

    /// The instances, type counts and markers (from the scene and the index).
    pub(crate) fn rebuild_items(&mut self, reindex: bool) {
        wce_prof!("tile.rebuild_items");
        if reindex || !self.idx.valid {
            self.build_index();

            // New key ids; a building with a key no one has seen yet is looked up.
            if !self.models.is_empty() || !self.idx.keys.is_empty() {
                self.resolve_buildings();
            }
        }

        let ix = &self.idx;
        let (n, types) = (ix.type_.len(), ix.type_names.len());
        self.instances.clear();
        self.type_counts.clear();
        let (mut counts, mut marker_counts) = (vec![0u32; types], vec![0u32; types]);
        let mut is_marker = vec![false; n];
        for i in 0..n {
            if ix.kind[i] & INSTANCE_BIT == 0 {
                continue;
            }

            counts[ix.type_[i] as usize] += 1;
            self.instances.push(i);

            // Hidden by its layer: no marker, no model entity.
            if ix.state[i] & 1 == 0 {
                continue;
            }

            // Drawn as a model.
            if ix.key[i].is_some_and(|k| self.models.get(&ix.keys[k as usize]).is_some_and(|m| m.ok)) {
                continue;
            }

            is_marker[i] = true;
            marker_counts[ix.type_[i] as usize] += 1;
        }

        for (t, &c) in counts.iter().enumerate() {
            if c != 0 {
                self.type_counts.insert(ix.type_names[t].clone(), c as i32);
            }
        }

        // Markers grouped by type (counting sort, stable: the element order inside a group).
        let unit = self.project.unit_scale;
        let mk = &mut self.markers;
        mk.group_begin = std::iter::once(0).chain(marker_counts.iter().scan(0, |sum, &c| {
            *sum += c;
            Some(*sum)
        })).collect();

        let total = mk.group_begin[types] as usize;
        *mk = Markers { elem: vec![0; total], x: vec![0.0; total], y: vec![0.0; total], z: vec![0.0; total], group_begin: std::mem::take(&mut mk.group_begin), slot: vec![None; n] };
        let mut cursor = mk.group_begin[..types].to_vec();
        for i in (0..n).filter(|&i| is_marker[i]) {
            let at = cursor[ix.type_[i] as usize];
            cursor[ix.type_[i] as usize] += 1;
            let w = &ix.world[ix.slot[i] as usize];
            let a = at as usize;
            mk.elem[a] = i;
            mk.x[a] = (w.position[0] / unit) as f32;
            mk.y[a] = w.position[1] as f32;
            mk.z[a] = (w.position[2] / unit) as f32;
            mk.slot[i] = Some(at);
        }

        self.dirty_type = vec![0; types];
        self.markers_dirty = true;
        self.polygons_dirty = true;
        self.grid_dirty.set(true);
    }

    pub(crate) fn lod_distance(&self, kind: &str) -> f32 {
        match kind {
            "VEGETATION_INSTANCE" => self.display.veg_distance,
            "ProjectedDecalInstance" => self.display.decal_distance,
            _ => self.display.other_distance,
        }
    }
}

/// `Tile::snap_landscape_world` on index arrays that may be under construction.
fn snap(t: &Tile, mode: &[u8], kind: &[u8], elem: usize, w: &mut WorldTransform) {
    if t.project.height.is_none() || elem >= mode.len() || mode[elem] != 1 {
        return;
    }

    let k = kind[elem];
    if k & INSTANCE_BIT == 0 || k & (POLYGON_BIT | LAYER_BIT) != 0 || !t.tscene().doc().elements()[elem].children.is_empty() {
        return;
    }

    w.position[1] = t.project.height_at(w.position[0] / t.unit(), w.position[2] / t.unit());
}
