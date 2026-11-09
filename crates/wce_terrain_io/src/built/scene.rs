//! Rome 2's battle locations (`locations.blm` version 7) and their deployments (`deployment/deployment_NNNN.xml`) as a `.terry` scene, so
//! the Terrain tool's entity editing (Phase 5) works on them, and back (docs/plans/terrain_editor.md, Phase 9). Only the campaign battle
//! maps have them (the seven `main_*`).
//!
//! - A location is a battle catchment area at its centre cell's corner, the origin of its deployment file's points (the cells are
//!   `w / 2 + trunc(x / 256)`: a corner gives its cell back), its `Catchment Region` a spatial child placed and sized so that its
//!   rectangle gives the record's back (the middles of the intervals that truncate to its cells). Its type is the record's.
//! - A location's id is `0xffff0000 + n`, n its deployment file's number: the BCAs with one come first, by n (`locations::records` and
//!   `deployment::build` number them in entity order), then the others type by type, in the file's order.
//! - A file's areas are deployment zones of the BCA (alliance 0's, then 1's: `deployment::build` writes them so), a boundary a zone region
//!   at the BCA's position whose polyline is the boundary's points.
//!
//! Written back: the locations by `locations::records` in the file's type order (new types after), the file's other bytes (the per-cell
//! table after the types, kept: ASSUMPTION, it is not made again) as they were; the deployment files by `deployment::build` with CRLF line
//! endings, as Rome 2's.

use std::collections::BTreeMap;

use wce_terrain::terry::{EntityKind, TerryProject, number_text};
use wce_tile::battle_map::locations::{BattleLocations, Location};
use wce_tile::vegetation::ca_xml::Element;

const CELL: f64 = 256.0;

/// The middle of the values `trunc(v / 256)` maps to `k` (0 for 0: the interval is -256 .. 256).
fn middle(k: i32) -> f64 {
    f64::from(k) * CELL + f64::from(k.signum()) * CELL / 2.0
}

/// The first element named `name`, this one or below.
fn find<'a>(e: &'a Element, name: &str) -> Option<&'a Element> {
    if e.name == name {
        return Some(e);
    }

    e.children.iter().find_map(|c| find(c, name))
}

fn attr<'a>(e: &'a Element, name: &str) -> &'a str {
    e.attribute(name).unwrap_or_default()
}

/// The scene of a map's locations and deployment files (by number). Errors: the ids are not in each type's file order (the order
/// could not be kept).
pub fn scene(blm: &BattleLocations, deployments: &BTreeMap<u32, Element>) -> Result<TerryProject, String> {
    let (cx, cy) = ((blm.width / 2) as i32, (blm.height / 2) as i32);
    // (type, index in the type, deployment number)
    let mut with: Vec<(usize, usize, u32)> = vec![];
    let mut without: Vec<(usize, usize)> = vec![];
    for (t, (_, locs)) in blm.types.iter().enumerate() {
        let mut last = None;
        for (i, l) in locs.iter().enumerate() {
            if l.id == -1 {
                without.push((t, i));
                continue;
            }

            let n = (l.id as u32).wrapping_sub(0xffff_0000);
            if last.is_some_and(|p| p > n) {
                return Err(format!("{}'s deployment ids are not in its file order", blm.types[t].0));
            }

            last = Some(n);
            with.push((t, i, n));
        }
    }

    with.sort_by_key(|w| w.2);
    if with.iter().enumerate().any(|(k, w)| w.2 != k as u32) {
        return Err("the deployment ids are not 0 .. n".to_owned());
    }

    let mut next = 0x1000_0000_u64;
    let mut id = || {
        next += 1;
        next
    };

    let n = |v: f64| number_text(v);
    let mut xml = String::from("<project version=\"3\">\n    <scene version=\"10\">\n");
    // By deployment number, each preceded by its type's locations without one that come before it in the file (so each type keeps its
    // file order), then the rest of those.
    let mut order: Vec<(usize, usize, Option<u32>)> = vec![];
    let mut done = vec![false; without.len()];
    for &(t, i, n) in &with {
        for (k, &(tt, ii)) in without.iter().enumerate() {
            if !done[k] && tt == t && ii < i {
                done[k] = true;
                order.push((tt, ii, None));
            }
        }

        order.push((t, i, Some(n)));
    }

    order.extend(without.iter().enumerate().filter(|(k, _)| !done[*k]).map(|(_, &(t, i))| (t, i, None)));
    for (t, i, dep) in order {
        let (kind, locs) = &blm.types[t];
        let l = &locs[i];
        // The centre cell's corner: the deployment files' origin (their river zones' points lie on the river tiles there: 99.6 % of
        // `main_rome_map`'s, `deployment_fit`; the cell's middle, 73 %), and `trunc` gives the cell back from it.
        let (bx, bz) = (f64::from(l.centre[0] - cx) * CELL, -f64::from(l.centre[1] - cy) * CELL);
        let (m0, m1) = (middle(l.rect[0] - cx), middle(l.rect[2] - cx));
        let (n0, n1) = (middle(l.rect[1] - cy), middle(l.rect[3] - cy));
        let (rx, rw) = ((m0 + m1) / 2.0, m1 - m0);
        let (rz, rh) = (-(n0 + n1) / 2.0, n1 - n0);
        let (bca, region) = (id(), id());
        let deployment = if dep.is_some() { " deployment=\"true\"" } else { "" };

        // The zones and their regions (alliance 0's first).
        let mut zones = String::new();
        let mut zone_ids = vec![];
        if let Some(d) = dep.and_then(|k| deployments.get(&k)) {
            let areas = find(d, "BATTLE_DEPLOYMENT_AREAS");
            for a in areas.iter().flat_map(|x| x.children.iter()).filter(|x| x.name == "ALLIANCE") {
                let alliance = attr(a, "id");
                for area in a.children.iter().filter(|x| x.name == "deployment_area") {
                    let zid = id();
                    zone_ids.push(zid);
                    let category = attr(area, "category");
                    let category = format!("DZC_{}", category.strip_prefix("DAC_").unwrap_or(category));
                    let orientation = area.children.iter().find(|x| x.name == "orientation")
                        .map_or(String::new(), |o| format!(" orientation=\"{}\"", attr(o, "radians")));
                    let mut regions = String::new();
                    let mut region_ids = vec![];
                    for b in area.children.iter().filter(|x| x.name == "boundary") {
                        let rid = id();
                        region_ids.push(rid);
                        let kind = if attr(b, "type").contains("subtractive") { "DZRT_SUBTRACTIVE" } else { "DZRT_ADDITIVE" };
                        let points: String = b.children.iter().filter(|x| x.name == "position")
                            .map(|q| format!("{{i}}{{i}}{{i}}{{i}}<point x=\"{}\" y=\"{}\"/>\n", attr(q, "x").parse::<f32>().map_or_else(|_| "0".to_owned(), |v| n(f64::from(v))), attr(q, "y").parse::<f32>().map_or_else(|_| "0".to_owned(), |v| n(f64::from(v)))))
                            .collect();
                        regions += &format!(
                            "<entity id=\"{rid:x}\" name=\"DZR {}\">\n{{i}}<components>\n{{i}}{{i}}<ECDeploymentZoneRegion active=\"true\" region_type=\"{kind}\"/>\n\
{{i}}{{i}}<ECPolyline active=\"true\">\n{{i}}{{i}}{{i}}<polyline closed=\"true\">\n{points}{{i}}{{i}}{{i}}</polyline>\n{{i}}{{i}}</ECPolyline>\n\
{{i}}{{i}}<ECTransform rotation=\"0 0 0\" scale=\"1 1 1\" active=\"true\" position=\"{} 0 {}\"/>\n{{i}}</components>\n{{i}}<children/>\n</entity>\n",
                            region_ids.len() - 1, n(bx), n(bz)
                        );
                    }

                    let kids: String = region_ids.iter().map(|r| format!("{{i}}{{i}}<child id=\"{r:x}\"/>\n")).collect();
                    zones += &format!(
                        "<entity id=\"{zid:x}\" name=\"DZ {}/{alliance}\">\n{{i}}<components>\n\
{{i}}{{i}}<ECDeploymentZone deployment_category=\"{category}\" deployment_type=\"DZT_1V1\" active=\"true\" facing_direction=\"0\" deployment_zone_id=\"{}\" alliance_id=\"{alliance}\" configuration=\"0\"{orientation}/>\n\
{{i}}</components>\n{{i}}<children>\n{kids}{{i}}</children>\n</entity>\n{regions}",
                        attr(area, "id"), attr(area, "id")
                    );
                }
            }
        }

        let kids: String = std::iter::once(region).chain(zone_ids.iter().copied()).map(|c| format!("{{i}}{{i}}<child id=\"{c:x}\"/>\n")).collect();
        xml += &format!(
            "<entity id=\"{bca:x}\" name=\"{kind} {i}\">\n{{i}}<components>\n{{i}}{{i}}<ECRectangle width=\"{}\" height=\"{}\" active=\"true\"/>\n\
{{i}}{{i}}<ECTransform rotation=\"0 0 0\" scale=\"1 1 1\" active=\"true\" position=\"{} 0 {}\">\n{{i}}{{i}}{{i}}<children>\n{{i}}{{i}}{{i}}{{i}}<child id=\"{region:x}\"/>\n{{i}}{{i}}{{i}}</children>\n{{i}}{{i}}</ECTransform>\n\
{{i}}{{i}}<ECLandscapeAbove active=\"true\"/>\n{{i}}{{i}}<ECBattleCatchmentArea catchment_type=\"{kind}\" active=\"true\"{deployment}/>\n{{i}}</components>\n{{i}}<children>\n{kids}{{i}}</children>\n</entity>\n\
<entity id=\"{region:x}\" name=\"Catchment Region\">\n{{i}}<components>\n{{i}}{{i}}<ECRectangle width=\"{}\" height=\"{}\" active=\"true\"/>\n\
{{i}}{{i}}<ECTransform rotation=\"0 0 0\" scale=\"1 1 1\" active=\"true\" position=\"{} 0 {}\"/>\n{{i}}{{i}}<ECLandscapeAbove active=\"true\"/>\n{{i}}</components>\n{{i}}<children/>\n</entity>\n{zones}",
            n((rw - CELL).max(0.0)), n((rh - CELL).max(0.0)), n(bx), n(bz), n(rw), n(rh), n(rx - bx), n(rz - bz)
        );
    }

    xml += "    </scene>\n</project>\n";
    let xml = indent(&xml);
    TerryProject::parse(xml.into_bytes()).map_err(|e| e.to_string())
}

/// The entities' `{i}` marks as the kit's indent (four spaces a level), each entity two levels in.
fn indent(xml: &str) -> String {
    xml.lines().map(|l| {
        if l.starts_with("<project") || l.starts_with("</project") || l.trim_start().starts_with("<scene") || l.trim_start().starts_with("</scene") {
            return format!("{l}\n");
        }

        let depth = l.matches("{i}").count();
        format!("        {}{}\n", "    ".repeat(depth), l.replace("{i}", ""))
    }).collect()
}

/// Whether a BCA has a deployment file: zones, or the mark of a location that had one (Rome 2's files with no areas).
fn has_deployment(p: &TerryProject, e: &wce_terrain::terry::TerryEntity) -> bool {
    let zones = e.children.iter().filter_map(|&c| p.entity(c)).any(|z| matches!(p.kind(z), EntityKind::DeploymentZone));
    zones || p.component(e, "ECBattleCatchmentArea").and_then(|c| p.doc.get(c, "deployment")).as_deref() == Some("true")
}

fn cell(m: f64, centre: i32) -> i32 {
    centre + (m / CELL).trunc() as i32
}

/// The locations of a scene's BCAs (`locations::records` with Rome 2's numbering), by type in entity order.
fn records(p: &TerryProject, w: u32, h: u32) -> Vec<(String, Vec<Location>)> {
    let (cx, cy) = ((w / 2) as i32, (h / 2) as i32);
    let mut types: Vec<(String, Vec<Location>)> = vec![];
    let mut deployment = 0u32;
    for e in &p.entities {
        let EntityKind::Bca(kind) = p.kind(e) else { continue };
        let id = if has_deployment(p, e) {
            deployment += 1;
            (0xffff_0000u32 + deployment - 1) as i32
        } else {
            -1
        };

        let pos = p.world_position(e.id);
        let region = e.children.iter().filter_map(|&c| p.entity(c)).find(|c| matches!(p.kind(c), EntityKind::CatchmentRegion)).unwrap_or(e);
        let size = p.rectangle(region).unwrap_or([0.0, 0.0]);
        let rp = p.world_position(region.id);
        let rect = [cell(rp[0] - size[0] / 2.0, cx), cell(-(rp[2] + size[1] / 2.0), cy), cell(rp[0] + size[0] / 2.0, cx), cell(-(rp[2] - size[1] / 2.0), cy)];
        let loc = Location { rect, centre: [cell(pos[0], cx), cell(-pos[2], cy)], id, extra: [0, 0] };
        match types.iter_mut().find(|t| t.0 == kind) {
            Some(t) => t.1.push(loc),
            None => types.push((kind, vec![loc])),
        }
    }

    types
}

/// The deployment files (Rome 2's: CRLF, an area's `orientation` after its boundaries when it had one; a BCA with a file but no zones
/// gets one with its alliances empty), numbered in entity order. A point is its region's offset from the BCA plus the polyline's point
/// (in f64, then f32: a point read comes back as it was).
fn deployments(p: &TerryProject) -> Vec<(String, Vec<u8>)> {
    let f = wce_terrain::deployment::printf_f;
    let mut out = vec![];
    for e in &p.entities {
        if !matches!(p.kind(e), EntityKind::Bca(_)) || !has_deployment(p, e) {
            continue;
        }

        let zones: Vec<_> = e.children.iter().filter_map(|&c| p.entity(c)).filter(|z| matches!(p.kind(z), EntityKind::DeploymentZone)).collect();
        let origin = p.world_position(e.id);
        let mut x = String::from("<BATTLE_DEPLOYMENT_AREA_HASH_TABLE>\n\t<BATTLE_DEPLOYMENT_AREAS>\n");
        for alliance in 0..2 {
            x += &format!("\t\t<ALLIANCE id='{alliance}'>\n");
            for z in &zones {
                let Some(c) = p.component(z, "ECDeploymentZone") else { continue };
                let get = |a: &str| p.doc.get(c, a).unwrap_or_default();
                if get("alliance_id").parse::<i32>().ok() != Some(alliance) {
                    continue;
                }

                let category = get("deployment_category");
                let category = category.strip_prefix("DZC_").unwrap_or(&category);
                x += &format!("\t\t\t<deployment_area id='{}' category='DAC_{category}'>\n", get("deployment_zone_id"));
                for r in z.children.iter().filter_map(|&c| p.entity(c)) {
                    let (Some(rc), Some((pts, _))) = (p.component(r, "ECDeploymentZoneRegion"), p.polyline(r)) else { continue };
                    let kind = if p.doc.get(rc, "region_type").as_deref() == Some("DZRT_SUBTRACTIVE") { "subtractive" } else { "additive" };
                    let w = p.world_position(r.id);
                    let (ox, oz) = (w[0] - origin[0], w[2] - origin[2]);
                    x += &format!("\t\t\t\t<boundary type='standard {kind}'>\n");
                    for q in &pts {
                        let (px, pz) = ((q[0] + ox) as f32, (q[1] + oz) as f32);
                        x += &format!("\t\t\t\t\t<position x='{}' y='{}'/>\n", f(f64::from(px)), f(f64::from(pz)));
                    }

                    x += "\t\t\t\t</boundary>\n";
                }

                if let Some(o) = p.doc.get(c, "orientation") {
                    x += &format!("\t\t\t\t<orientation radians='{o}'/>\n");
                }

                x += "\t\t\t</deployment_area>\n";
            }

            x += "\t\t</ALLIANCE>\n";
        }

        x += "\t</BATTLE_DEPLOYMENT_AREAS>\n</BATTLE_DEPLOYMENT_AREA_HASH_TABLE>\n";
        out.push((format!("deployment/deployment_{:04}.xml", out.len()), x.replace('\n', "\r\n").into_bytes()));
    }

    out
}

/// The locations and deployment files of a scene: (`locations.blm`, `deployment/deployment_NNNN.xml` files).
///
/// # Errors
///
/// A candidate BLM name or count cannot be represented in the native format.
pub fn write(p: &TerryProject, original: &BattleLocations) -> crate::Result<(Vec<u8>, Vec<(String, Vec<u8>)>)> {
    let records = records(p, original.width, original.height);
    let mut types: Vec<(String, Vec<Location>)> = original.types.iter().filter_map(|(t, _)| records.iter().find(|r| &r.0 == t).cloned()).collect();
    types.extend(records.iter().filter(|r| !original.types.iter().any(|(t, _)| t == &r.0)).cloned());
    let blm = BattleLocations { version: original.version, width: original.width, height: original.height, types, tail: original.tail.clone() };
    let bytes = blm.write().map_err(|e| crate::file_error("locations.blm", e))?;
    Ok((bytes, deployments(p)))
}
