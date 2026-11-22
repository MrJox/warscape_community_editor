//! The area brush: any closed polygon painted instead of clicked (the biome brush's way, for every closed type). A stroke is circles of a
//! radius along the mouse's way: started on an area it grows it (or cuts it, taking away), started on none it makes a new one of what is
//! being made. What a cut leaves in more than one piece becomes as many polygons, copies of the first (the same type, values and holder).

use std::collections::BTreeMap;

use wce_foundation::math::Float3;

use super::{Create, FILLED_TYPES, Tile, TileDocument};

/// What a stroke started on no area makes: a polygon of `kind` with `key` and `attrs`, in `into` (a layer, or the holder it needs).
pub struct NewArea<'a> {
    pub kind: &'a str,
    pub into: usize,
    pub key: &'a str,
    pub attrs: &'a BTreeMap<String, String>,
}

/// Whether a polygon type (with its `name` and `polygon_shape`) can be painted: a closed area of a free shape. Not a siege AI node's
/// Bounding Box (a rectangle) nor its open shapes, nor a capture location's circle or square.
#[must_use]
pub fn paintable_kind(kind: &str, name: Option<&str>, shape: Option<&str>) -> bool {
    match kind {
        "SIEGE_AI_POLYGON_INSTANCE" => name == Some("Boundary"),
        "CaptureLocationPolygonInstance" => shape.is_none_or(|s| s == "PS_POLYGON"),
        super::LAKE => true,
        k => FILLED_TYPES.contains(&k),
    }
}

/// The grid a stroke's outline is made on, metres: a quarter of the radius, 0.5 to 4.
#[must_use]
pub fn stroke_cell(radius: f32) -> f32 {
    (radius / 4.0).clamp(0.5, 4.0)
}

impl Tile {
    /// Whether a polygon of the scene can be painted ([`paintable_kind`]).
    #[must_use]
    pub fn paintable(&self, elem: usize) -> bool {
        if self.is_lake(elem) {
            return true;
        }

        let sc = self.tscene();
        paintable_kind(sc.type_of(elem), sc.get(elem, "name").as_deref(), sc.get(elem, "polygon_shape").as_deref())
    }

    /// The polygons a stroke painting `kind` works on (shown ones; with `wce_biome` when `biome`, without it otherwise: a biome's brush
    /// does not grow a plain exclusion polygon; inside `within` when given, the holder new ones go into) and their outlines (world metres,
    /// x and z).
    #[must_use]
    pub fn paint_targets(&self, kind: &str, biome: bool, within: Option<usize>) -> Vec<(usize, Vec<(f32, f32)>)> {
        let sc = self.tscene();
        // The lakes: their water planes, painted by their outlines (`lakes.rs`).
        if kind == super::LAKE {
            return sc
                .find_all("SIMPLE_MESH_INSTANCE")
                .into_iter()
                .filter(|&e| self.is_lake(e) && self.shown(e))
                .map(|e| (e, self.area_outline(e)))
                .collect();
        }

        sc.find_all(kind)
            .into_iter()
            .filter(|&e| self.shown(e) && self.paintable(e))
            .filter(|&e| sc.get(e, wce_tile::biomes::BIOME_ATTR).is_some_and(|b| !b.is_empty()) == biome)
            .filter(|&e| within.is_none_or(|h| sc.is_descendant(e, h)))
            .map(|e| (e, self.polygon_world_points(e).into_iter().map(|p| (p.x, p.z)).collect()))
            .collect()
    }
}

impl TileDocument {
    /// A stroke of the area brush (circles of `radius` at `dabs`, world metres): `area`'s outline grown by them (or cut, `erase`), or (no
    /// area) a new polygon as `new` says. What is left of an area in more than one piece becomes as many polygons, copies of it (a value
    /// whose name ends in `seed` told apart); an area cut away entirely goes. One undo entry; the polygons after it.
    ///
    /// # Errors
    ///
    /// The area can't be painted, nothing to make (a stroke that makes no area, or one that takes away from none), or the scene's edit
    /// failed.
    pub fn area_stroke(
        &mut self,
        area: Option<usize>,
        new: Option<NewArea<'_>>,
        dabs: &[(f32, f32)],
        radius: f32,
        erase: bool,
    ) -> Result<Vec<usize>, String> {
        let cell = stroke_cell(radius);
        let Some(e) = area else {
            if erase {
                return Err("nothing to take away from".to_owned());
            }

            let new = new.ok_or("nothing to make")?;
            let outline = wce_tile::biomes::stroke_outline(&[], dabs, radius, false, cell).into_iter().next();
            let outline = outline.filter(|o| o.len() >= 3).ok_or("the stroke makes no area")?;
            let t = self.tile();
            // A lake: its water plane over the painted outline (rounded off), its water at its shore.
            if new.kind == super::LAKE {
                let round = super::lakes::smooth_outline(&outline);
                let points = round.iter().map(|&(x, z)| Float3::new(x, t.terrain_height(x, z), z)).collect::<Vec<_>>();
                return self.create_lake(new.into, &points, None).map(|e| vec![e]);
            }

            let points = outline.iter().map(|&(x, z)| Float3::new(x, t.terrain_height(x, z), z)).collect::<Vec<_>>();

            return self.create_with(new.kind, new.into, &Create::Line(points), new.key, new.attrs).map(|e| vec![e]);
        };

        let t = self.tile();
        if !t.paintable(e) {
            return Err(format!("a {} of this shape can't be painted", t.tscene().type_of(e)));
        }

        let lake = t.is_lake(e);
        let existing = t.area_outline(e);
        let mut outlines = wce_tile::biomes::stroke_outline(&existing, dabs, radius, erase, cell);
        if lake {
            outlines = outlines.iter().map(|o| super::lakes::smooth_outline(o)).collect();
        }

        let worlds = outlines
            .iter()
            .map(|o| o.iter().map(|&(x, z)| Float3::new(x, t.terrain_height(x, z), z)).collect::<Vec<_>>())
            .collect::<Vec<_>>();
        let mark = self.begin()?;
        let mut out = vec![];
        let result = (|| -> Result<(), String> {
            let mut pieces = worlds.iter();

            // The area takes the largest piece; none: it went.
            let Some(first) = pieces.next() else {
                self.tscene_mut().remove(e)?;
                return Ok(());
            };

            if lake {
                self.set_lake_outline(e, first)?;
            } else {
                self.set_area_points(e, first)?;
            }

            out.push(e);

            // The other pieces: copies of it, each with its own seeds (a lake: its own id, for its ground's layer).
            for (k, world) in pieces.enumerate() {
                let n = self.tscene_mut().duplicate(e)?;
                if lake {
                    self.set_lake_outline(n, world)?;
                    let id = self.tile().next_river_id();
                    self.tscene_mut().set_attr(n, super::river_links::RIVER_ID_ATTR, &id.to_string());
                } else {
                    self.set_area_points(n, world)?;
                }

                let sc = self.tscene_mut();
                let seeds = sc.doc().elements()[n]
                    .attrs
                    .iter()
                    .filter(|a| a.name.ends_with("seed"))
                    .map(|a| (a.name.clone(), sc.get(n, &a.name).unwrap_or_default()))
                    .collect::<Vec<_>>();
                for (name, v) in seeds {
                    sc.set_attr(n, &name, &format!("{v}{k}"));
                }

                out.push(n);
            }

            Ok(())
        })();

        if let Err(err) = result {
            return self.rolled_back(mark, err);
        }

        let sel = out.iter().map(|&e| super::item_of(e)).collect::<Vec<_>>();
        self.push_splice(if erase { "Area brush: take away" } else { "Area brush" }.to_owned(), mark, vec![], sel);
        Ok(out)
    }

    /// A polygon's points set to `world` (metres): moved, the missing ones added, the extra ones taken away.
    fn set_area_points(&mut self, e: usize, world: &[Float3]) -> Result<(), String> {
        let local = world.iter().map(|&p| self.tile().to_local(e, p)).collect::<Vec<_>>();
        let sc = self.tscene_mut();
        let had = sc.polygon_points(e).len();
        for (i, &p) in local.iter().enumerate().take(had) {
            sc.set_polygon_point(e, i, p)?;
        }

        for (i, &p) in local.iter().enumerate().skip(had) {
            sc.insert_polygon_point(e, i - 1, p)?;
        }

        while sc.polygon_points(e).len() > local.len() {
            let last = sc.polygon_points(e).len() - 1;
            if !sc.remove_polygon_point(e, last)? {
                break;
            }
        }

        Ok(())
    }
}
