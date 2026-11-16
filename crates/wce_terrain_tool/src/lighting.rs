//! A campaign map's lighting in the view as the game evaluates it (`wce_scene::campaign_lighting`): `environment_collection.xml`'s
//! spheres and global lighting, each resolved for the view's season and climate stage, blended at the camera every frame.

use wce_editor::editor::Editor;
use wce_formats::xml_doc::XmlDoc;
use wce_scene::campaign_lighting::{EnvironmentCollection, read_collection, season_candidates};
use wce_scene::environment::{EnvironmentParams, read_environment};
use wce_terrain_editor::document::TerryFrame;
use wce_vfs::files::Files;

/// The spheres' lightings read, and the weights last applied.
pub(crate) struct CampaignLighting {
    /// What the editor's preview is named (no file: no document follows it).
    key: String,
    collection: EnvironmentCollection,
    frame: TerryFrame,

    /// The files read (resolved path, parameters); each sphere's and the global's index into it (none: no file found).
    files: Vec<(String, EnvironmentParams)>,
    spheres: Vec<Option<usize>>,
    global: Option<usize>,
    last: Vec<f64>,
}

impl CampaignLighting {
    /// The map folder's collection (`dir`: `terrain/campaigns/<map>`) for season `season` (0 to 3) and the climate stage's number `n`
    /// (the snow masks' `n`: the stage + 1). None: the folder has no collection.
    pub fn load(files: &dyn Files, dir: &str, season: usize, n: u32, frame: TerryFrame) -> Option<Self> {
        let doc = XmlDoc::parse(files.read(&format!("{dir}/environment_collection.xml")).ok()?).ok()?;
        let collection = read_collection(&doc)?;
        let mut me = Self {
            key: format!("{dir}/environment_collection.xml"),
            collection,
            frame,
            files: vec![],
            spheres: vec![],
            global: None,
            last: vec![],
        };
        let mut resolve = |name: &str| -> Option<usize> {
            let path = season_candidates(name, season, n).into_iter().find(|f| files.exists(f))?;
            if let Some(k) = me.files.iter().position(|(p, _)| *p == path) {
                return Some(k);
            }

            let doc = XmlDoc::parse(files.read(&path).ok()?).ok()?;
            me.files.push((path, read_environment(&doc)));
            Some(me.files.len() - 1)
        };
        let names: Vec<String> = me.collection.spheres.iter().map(|s| s.lighting.clone()).collect();
        let global = me.collection.global.clone();
        let spheres: Vec<Option<usize>> = names.iter().map(|n| resolve(n)).collect();
        let global = resolve(&global);
        me.spheres = spheres;
        me.global = global;
        (me.global.is_some() || me.spheres.iter().any(Option::is_some)).then_some(me)
    }

    /// The files the spheres and the global lighting resolved to, for the log.
    pub fn summary(&self) -> String {
        let missing = self.spheres.iter().filter(|s| s.is_none()).count() + usize::from(self.global.is_none());
        let names: Vec<&str> = self.files.iter().map(|(p, _)| p.rsplit('/').next().unwrap_or(p)).collect();
        format!(
            "{} spheres, global {}; files {}{}",
            self.collection.spheres.len(),
            self.global.map_or("(missing)", |k| self.files[k].0.rsplit('/').next().unwrap_or("")),
            names.join(", "),
            if missing > 0 { format!("; {missing} not found") } else { String::new() }
        )
    }

    /// The weights at the camera's eye (`.terry` units: x and z through the frame, y by the frame's unit, ASSUMPTION) and the global's.
    pub fn weights(&self, e: &Editor) -> (Vec<f64>, f64) {
        let eye = e.viewport.camera().eye;
        let (x, z) = self.frame.from_frame(f64::from(eye.x), f64::from(eye.z));
        self.collection.weights([x, f64::from(eye.y) * self.frame.unit, z])
    }

    /// Lights the view at the camera, when the weights changed.
    pub fn update(&mut self, e: &mut Editor) {
        let (mut w, g) = self.weights(e);
        w.push(g);
        if w.len() == self.last.len() && w.iter().zip(&self.last).all(|(a, b)| (a - b).abs() < 1e-4) {
            return;
        }

        // A sphere without a file counts as the global lighting (ASSUMPTION; every shipped sphere's file exists).
        let mut items: Vec<(f64, &EnvironmentParams)> = vec![];
        let mut to_global = g;
        for (k, s) in self.spheres.iter().enumerate() {
            match s {
                Some(f) => items.push((w[k], &self.files[*f].1)),
                None => to_global += w[k],
            }
        }

        if let Some(f) = self.global {
            items.push((to_global, &self.files[f].1));
        }

        if let Some(p) = wce_scene::campaign_lighting::blend_weighted(&items) {
            e.set_weather_params(&self.key, p);
        }

        self.last = w;
    }

    /// The spheres' files and weights at the camera, heaviest first (the panel's readout).
    pub fn readout(&self, e: &Editor) -> Vec<(String, f64)> {
        let (w, g) = self.weights(e);
        let mut v: Vec<(String, f64)> = vec![];
        let mut add = |name: String, x: f64| match v.iter_mut().find(|(n, _)| *n == name) {
            Some(it) => it.1 += x,
            None => v.push((name, x)),
        };
        for (k, s) in self.spheres.iter().enumerate() {
            add(s.map_or("(missing)".to_owned(), |f| self.files[f].0.rsplit('/').next().unwrap_or("").to_owned()), w[k]);
        }

        add(self.global.map_or("(missing)".to_owned(), |f| self.files[f].0.rsplit('/').next().unwrap_or("").to_owned()) + " (global)", g);
        v.retain(|(_, x)| *x > 1e-4);
        v.sort_by(|a, b| b.1.total_cmp(&a.1));
        v
    }
}
