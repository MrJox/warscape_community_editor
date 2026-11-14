//! The Terrain tool's checks of a kit map (docs/plans/terrain_editor.md, Phase 4, "Validation"): what BOB's fill would not take or the
//! game would not find. Each finding says where (a cell, for the camera to go to) when it is a place.
//!
//! Not checked yet: painted settlement sizes against `tile_upgrades.xml` (docs/plans/tile.md item 3; its rules to be settled), BCAs whose
//! infield holds restricted tiles (Phase 6).

use std::collections::BTreeMap;
use std::path::PathBuf;

use wce_terrain::LayerRole;
use wce_terrain_editor::MapDocument;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// The fill or the game will not do what the map says.
    Error,

    /// Works, but likely not as meant.
    Warning,
}

#[derive(Clone, Debug)]
pub struct Finding {
    pub severity: Severity,
    pub text: String,
    pub cell: Option<(i32, i32)>,
}

fn eq(k: [f32; 3], c: [u8; 4]) -> bool {
    k[0] == f32::from(c[0]) && k[1] == f32::from(c[1]) && k[2] == f32::from(c[2])
}

/// A layer's colours that `known` does not take: (colour, cells, first cell).
fn unknown_colours(l: &wce_terrain::cells::CellLayer, known: impl Fn([u8; 4]) -> bool) -> Vec<([u8; 4], usize, (i32, i32))> {
    let mut out = vec![];
    for (i, n) in l.histogram() {
        let c = l.palette()[usize::from(i)];
        if known(c) {
            continue;
        }

        let first = l.cells().iter().position(|&k| k == i).unwrap_or(0) as u32;
        out.push((c, n, ((first % l.width()) as i32, (first / l.width()) as i32)));
    }

    out
}

/// The checks of a document (`roots`: the kits a placement's tile is looked for in, in their order).
pub fn check(d: &MapDocument, roots: &[PathBuf]) -> Vec<Finding> {
    let mut out = vec![];
    let db = &d.db;
    let Some(m) = d.model() else {
        out.push(Finding {
            severity: Severity::Warning,
            text: "a map of the game's packs: only a kit map is checked".to_owned(),
            cell: None,
        });
        return out;
    };

    // Colours that name nothing: the fill leaves those cells empty.
    if let Some(l) = m.layer(&LayerRole::TileMap) {
        let known = |c: [u8; 4]| {
            c[..3] == [0, 0, 0]
                || db.sets.iter().any(|s| eq(s.colour, c))
                || db.tiles.iter().any(|t| eq(t.colour, c) || t.variations.iter().any(|v| eq(v.colour, c)))
        };

        for (c, n, at) in unknown_colours(l.get(), known) {
            out.push(Finding {
                severity: Severity::Error,
                cell: Some(at),
                text: format!(
                    "tile map: colour {}, {}, {} is no tile set, tile or variation of the database ({n} cells, the first at {}, {})",
                    c[0], c[1], c[2], at.0, at.1
                ),
            });
        }
    }

    if let Some(l) = m.layer(&LayerRole::Climate) {
        for (c, n, at) in unknown_colours(l.get(), |c| db.climates.iter().any(|k| eq(k.colour, c))) {
            out.push(Finding {
                severity: Severity::Warning,
                cell: Some(at),
                text: format!(
                    "climates: colour {}, {}, {} is no climate of the database ({n} cells, the first at {}, {}): its tiles take the first",
                    c[0], c[1], c[2], at.0, at.1
                ),
            });
        }
    }

    // Cells the fill found no tile for.
    for msg in &d.fill_messages {
        let at = msg.rsplit(' ').next().and_then(|p| p.split_once('x')).and_then(|(x, y)| Some((x.parse().ok()?, y.parse().ok()?)));
        out.push(Finding { severity: Severity::Error, cell: at, text: format!("the fill: {msg} (no tile of the painted set fits there)") });
    }

    // Explicit tiles the database does not know, or off the map; lines BOB does not read.
    if let Some(x) = &m.explicit {
        for n in x.get().bad_lines() {
            out.push(Finding {
                severity: Severity::Error,
                cell: None,
                text: format!("explicit_tiles.txt line {n} is no x,y,tile,rotation (an empty last line too): BOB's Tilemap fails on it"),
            });
        }

        for t in x.get().tiles() {
            match crate::explicit_tiles::tile_size(db, &t.path) {
                None => out.push(Finding {
                    severity: Severity::Error,
                    cell: Some((t.x, t.y)),
                    text: format!("explicit tile {} at {}, {}: not in the tile database (BOB skips it)", t.path, t.x, t.y),
                }),
                Some(s) => {
                    let r = crate::explicit_tiles::rect(t, s);
                    if r[0] < 0 || r[1] < 0 || r[2] > i64::from(d.placed.width) || r[3] > i64::from(d.placed.height) {
                        out.push(Finding {
                            severity: Severity::Error,
                            cell: Some((t.x, t.y)),
                            text: format!("explicit tile {} at {}, {}: reaches off the map", t.path, t.x, t.y),
                        });
                    }
                }
            }
        }
    }

    // Settlement and port battle catchment areas whose centre is on no settlement or port tile; deployment zones of no BCA.
    if let Some(p) = d.project() {
        for z in p.with_component("ECDeploymentZone") {
            let under_bca = p.parent_of(z.id).and_then(|b| p.entity(b)).is_some_and(|b| p.component(b, "ECBattleCatchmentArea").is_some());
            if !under_bca {
                out.push(Finding {
                    severity: Severity::Warning,
                    cell: None,
                    text: format!(
                        "deployment zone {} ({:x}) belongs to no battle catchment area: BOB writes no deployment of it",
                        z.name, z.id
                    ),
                });
            }
        }

        let (w, h) = (f64::from(d.placed.width / 2), f64::from(d.placed.height / 2));
        let cell = wce_terrain_editor::document::TERRY_CELL;
        for e in p.with_component("ECBattleCatchmentArea") {
            let wce_terrain::terry::EntityKind::Bca(t) = p.kind(e) else { continue };
            if !(t.starts_with("settlement") || t.contains("port")) {
                continue;
            }

            let pos = p.world_position(e.id);
            let (cx, cy) = ((pos[0] / cell + w).floor() as i32, (h - pos[2] / cell).floor() as i32);
            let on = d.placed.in_rect([cx, cy, cx + 1, cy + 1]).iter().any(|&i| {
                let l = d.placed.placements[i].location.to_lowercase();
                l.contains("settlement") || l.contains("port") || l.contains("city") || l.contains("town")
            });

            if !on {
                out.push(Finding {
                    severity: Severity::Warning,
                    cell: Some((cx, cy)),
                    text: format!("{} ({t}) at cell {cx}, {cy}: on no settlement or port tile", e.name),
                });
            }
        }
    }

    // Tiles in no kit: the game has them only if a pack does.
    let mut used: BTreeMap<(String, String), (usize, (i32, i32))> = BTreeMap::new();
    for p in &d.placed.placements {
        used.entry((p.location.clone(), p.variation.clone())).or_insert((0, (p.x, p.y))).0 += 1;
    }

    for ((loc, var), (n, at)) in used {
        if wce_tile::sources::find_tile(roots, &loc, &var).is_none() {
            out.push(Finding {
                severity: Severity::Warning, cell: Some(at),
                text: format!("{loc}\\{var} ({n} placements, the first at {}, {}): its folder is in no kit (the packs' copy is drawn, if a pack has it)", at.0, at.1),
            });
        }
    }

    out.sort_by_key(|f| f.severity);
    out
}
