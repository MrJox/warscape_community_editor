//! What the Tile tool is about to place (the Create panel's entry): a type with its key, the values the Inspector changed and a turn. Its text
//! is what the preview shows and what a click inserts, so the two are always the same thing.

use std::collections::BTreeMap;

use wce_editor_core::inspector::Inspector;
use wce_tile::properties::{Access, Kind, RuleContext, access, schema};
use wce_tile::ted_scene::TedScene;
use wce_foundation::math::{Double3, Double3x3};
use wce_tile::world_transform::{rotate_world_yaw, set_world_position, set_world_rotation, world_transform};

use super::edits::{edit_value, point_instance, scene_point};
use super::{TileDocument, item_of};

/// A thing to place: an instance of `kind` (`key`: its building, tree, decal, model or effect), these attributes over the type's defaults
/// (raw values), turned `yaw` degrees about the vertical.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Draft {
    pub kind: String,
    pub key: String,
    pub attrs: BTreeMap<String, String>,
    pub yaw: f32,
}

impl Draft {
    pub fn new(kind: &str, key: &str) -> Self {
        Self { kind: kind.to_owned(), key: key.to_owned(), ..Default::default() }
    }
}

/// `text` (an element) turned `yaw` degrees about the vertical through its position.
pub fn turned(text: &[u8], yaw: f32) -> Vec<u8> {
    if yaw.rem_euclid(360.0) == 0.0 {
        return text.to_vec();
    }

    let Ok(mut sc) = TedScene::parse_any(text.to_vec()) else { return text.to_vec() };
    let els = sc.doc().elements();
    let Some(root) = (0..els.len()).find(|&e| els[e].parent.is_none()) else { return text.to_vec() };
    rotate_world_yaw(&mut sc, root, f64::from(yaw).to_radians());
    sc.save()
}

/// The rotation that takes the up axis (+y) to `normal` (the shortest one: about the axis up x normal).
pub fn tilt_to(normal: Double3) -> Double3x3 {
    let len = (normal.x * normal.x + normal.y * normal.y + normal.z * normal.z).sqrt();
    if len < 1e-12 {
        return Double3x3::IDENTITY;
    }

    let n = normal * (1.0 / len);

    // up x n = (n.z, 0, -n.x); its length is the sine, n.y the cosine.
    let (ax, az) = (n.z, -n.x);
    let s = (ax * ax + az * az).sqrt();
    if s < 1e-12 {
        return if n.y > 0.0 { Double3x3::IDENTITY } else { Double3x3::rotation_x(std::f64::consts::PI) };
    }

    let (kx, kz, c) = (ax / s, az / s, n.y);
    let t = 1.0 - c;

    // Rodrigues about (kx, 0, kz).
    Double3x3 { m: [[t * kx * kx + c, -s * kz, t * kx * kz], [s * kz, c, -s * kx], [t * kx * kz, s * kx, t * kz * kz + c]] }
}

/// `text` (an element) tilted so that its up axis is `normal` (its turn about it kept).
pub fn tilted(text: &[u8], normal: Double3) -> Vec<u8> {
    let Ok(mut sc) = TedScene::parse_any(text.to_vec()) else { return text.to_vec() };
    let els = sc.doc().elements();
    let Some(root) = (0..els.len()).find(|&e| els[e].parent.is_none()) else { return text.to_vec() };
    let world = world_transform(&sc, root).rotation;
    set_world_rotation(&mut sc, root, &(tilt_to(normal) * world));
    sc.save()
}

/// `text` (an element) with an attribute of its start tag set (replacing the one it has).
pub fn with_root_attr(text: &[u8], name: &str, value: &str) -> Vec<u8> {
    let Some(end) = text.iter().position(|&c| c == b'>') else { return text.to_vec() };
    let end = if end > 0 && text[end - 1] == b'/' { end - 1 } else { end };
    let tag = String::from_utf8_lossy(&text[..end]).into_owned();
    let tag = match tag.find(&format!(" {name}=\"")) {
        Some(at) => {
            let close = tag[at + name.len() + 3..].find('"').map_or(tag.len(), |i| at + name.len() + 3 + i + 1);
            format!("{}{}", &tag[..at], &tag[close..])
        }

        None => tag,
    };

    let mut out = format!("{tag} {name}=\"{value}\"").into_bytes();
    out.extend_from_slice(&text[end..]);
    out
}

impl TileDocument {
    /// The text of a placed draft, at the origin (a click puts it where it goes).
    pub fn draft_text(&self, d: &Draft) -> Option<Vec<u8>> {
        let mut n = if d.kind == "BUILDING_INSTANCE" {
            if d.key.is_empty() {
                return None;
            }

            wce_tile::new_instances::NewInstance::new(&d.kind).attr("key", d.key.as_str()).attr("name", d.key.as_str())
        } else {
            point_instance(&d.kind, wce_foundation::math::Double3::ZERO, &d.key, self.tile().unit()).ok()?
        };

        n.attrs.extend(d.attrs.clone());
        let text = wce_tile::new_instances::element_text(&n).ok()?;
        Some(turned(&text, d.yaw))
    }

    /// The Inspector of a draft: the type's properties (TEd's Property Editor rows) with the draft's values, else the kit's defaults; an edit
    /// changes the draft (`turnable`: a placed type, with its turn). Not shown: what a click decides (position, flags) and the key (the Create panel's entry). Returns whether it changed.
    pub fn inspect_draft(&self, d: &mut Draft, turnable: bool, ins: &mut dyn Inspector) -> bool {
        ins.section(&format!("New {}", d.kind));
        if !d.key.is_empty() {
            ins.text("key", if d.kind == "VFXInstance" { "vfx" } else { "key" }, &d.key);
        }

        let Some(props) = schema(&d.kind) else { return false };
        // The values a new one gets without the draft's: its kind's own (an effect's scale) over the type's defaults.
        let template = wce_tile::instance_templates::template(&d.kind);
        let own = point_instance(&d.kind, Double3::ZERO, &d.key, self.tile().unit()).map(|n| n.attrs).unwrap_or_default();
        let default = |attr: &str| own.get(attr).cloned().or_else(|| template.and_then(|t| t.attrs.iter().find(|(k, _)| *k == attr)).map(|(_, v)| (*v).to_owned()));
        let before = d.yaw;
        let mut yaw = d.yaw;
        if turnable && ins.edit_float("yaw", "turn (deg; R / Shift+R)", &mut yaw, 1.0, -360.0, 360.0) {
            d.yaw = yaw;
        }

        let category = if d.kind == "BUILDING_INSTANCE" { self.tile().building_category(&d.key) } else { None };
        ins.section("Properties");
        let mut changed = d.yaw != before;
        for prop in props {
            if matches!(prop.attr, "key" | "vfx" | "name") || matches!(prop.kind, Kind::Flags | Kind::Position | Kind::Orientation | Kind::DestructionLevel) {
                continue;
            }

            let cur = d.attrs.get(prop.attr).cloned().or_else(|| default(prop.attr));
            let present = cur.is_some();
            let cur = cur.unwrap_or_default();

            // TEd's rules, as for a placed one (a building's category decides its wall properties; a draft has no file to go by).
            let ctx = RuleContext { category: category.as_deref(), shape: d.attrs.get("polygon_shape").map(String::as_str), present: false };
            match access(prop, &ctx) {
                Access::Hidden => continue,
                Access::ReadOnly => {
                    ins.text(prop.attr, prop.attr, &cur);
                    continue;
                }

                Access::Edit => {}
            }

            let new = match prop.kind {
                // While placing, the placement chips decide it.
                Kind::Placement if turnable => continue,
                Kind::Placement => {
                    let modes = ["Landscape: Above", "Freeform"].map(str::to_owned);
                    let mut idx = i32::from(cur == "MSM_FREEFORM");
                    ins.edit_choice("surface_mode", "surface_mode", &mut idx, &modes).then(|| if idx == 1 { "MSM_FREEFORM" } else { "MSM_LANDSCAPE" }.to_owned())
                }

                Kind::Text => {
                    let mut v = cur.clone();
                    ins.edit_string(prop.attr, prop.attr, &mut v).then_some(v)
                }

                _ => edit_value(prop, &cur, present, 1, ins),
            };

            if let Some(v) = new.filter(|v| *v != cur) {
                d.attrs.insert(prop.attr.to_owned(), v);
                changed = true;
            }
        }

        changed
    }

    /// Inserts an element's text (a draft's, a prefab's group) into `into` and puts it at `at` (world metres; on the terrain under it when
    /// `on_terrain`); `name` renames it. One undo entry named `what`; the selection afterwards is it.
    ///
    /// # Errors
    ///
    /// The text is not an element, or there is no such layer.
    pub fn place_text(&mut self, what: &str, text: &[u8], into: usize, at: wce_foundation::math::Float3, name: Option<&str>, on_terrain: bool) -> Result<usize, String> {
        let mark = self.begin()?;
        let result = (|| -> Result<usize, String> {
            let g = self.tscene_mut().insert_element_text(into, text)?;
            if let Some(name) = name {
                self.tscene_mut().set_attr(g, "name", name);
            }

            let t = self.tile();
            let y = if on_terrain { t.terrain_height(at.x, at.z) } else { at.y };
            let p = scene_point(wce_foundation::math::Float3::new(at.x, y, at.z), t.unit());
            set_world_position(self.tscene_mut(), g, p);
            Ok(g)
        })();

        match result {
            Ok(g) => {
                self.push_splice(what.to_owned(), mark, vec![], vec![item_of(g)]);
                Ok(g)
            }

            Err(e) => {
                let record = self.tscene_mut().end_edit(mark).unwrap_or_default();
                let _ = self.tscene_mut().unapply(&record);
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tilt_takes_up_to_the_normal() {
        for n in [Double3::new(0.3, 1.0, -0.2), Double3::new(-1.0, 0.1, 0.0), Double3::new(0.0, 1.0, 0.0), Double3::new(0.0, -1.0, 0.0), Double3::new(0.2, -0.5, 0.7)] {
            let len = (n.x * n.x + n.y * n.y + n.z * n.z).sqrt();
            let up = tilt_to(n) * Double3::new(0.0, 1.0, 0.0);
            assert!((up - n * (1.0 / len)).max_abs_diff(Double3::ZERO) < 1e-12, "{n:?} -> {up:?}");
        }

        let t = with_root_attr(b"<A x=\"1\" wce_surface=\"old\"/>", "wce_surface", "entity");
        assert_eq!(t, b"<A x=\"1\" wce_surface=\"entity\"/>");
    }
}
