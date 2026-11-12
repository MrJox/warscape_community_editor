//! The Terrain panel and command registry share one exact land-LF region transfer path.

use super::*;
use std::path::Path;
use wce_terrain::lf_region::LfRegion;

pub(super) struct Panel {
    samples: [i32; 4],
    path: PathBuf,
}

impl Default for Panel {
    fn default() -> Self {
        Self { samples: [0, 0, 64, 64], path: PathBuf::from("land_lf_region.png") }
    }
}

fn region(values: &[String]) -> Result<(LfRegion, PathBuf), String> {
    if values.len() != 5 {
        return Err("usage: terrain_lf_region_<export|import> <x> <y> <width> <height> <png>".into());
    }
    let n: Result<Vec<u32>, _> = values[..4].iter().map(|s| s.parse::<u32>()).collect();
    let n = n.map_err(|_| "Region coordinates and dimensions must be unsigned whole sample counts")?;
    Ok((LfRegion { x: n[0], y: n[1], width: n[2], height: n[3] }, PathBuf::from(&values[4])))
}

impl TerrainTool {
    fn transfer_lf_region(&mut self, e: &mut Editor, region: LfRegion, path: Option<&Path>, import: bool) -> Result<String, String> {
        if self.painting.is_some() || self.rx.is_some() || self.refill.busy() || self.export.is_some() {
            return Err("Finish the current Terrain operation before transferring an LF region".into());
        }
        let d = e.docs.find_as_mut::<MapDocument>(self.shown).ok_or("No map shown")?;
        let changed =
            if import { d.import_lf_region_file(region, path, self.mirror.clone())? } else { d.export_lf_region_file(region, path)? };
        Ok(if path.is_none() {
            "LF region transfer cancelled".into()
        } else if import && !changed {
            "Land LF region already matches; history unchanged".into()
        } else {
            format!(
                "{} land LF region at ({}, {}), {} x {} samples, top row first",
                if import { "Imported" } else { "Exported" },
                region.x,
                region.y,
                region.width,
                region.height
            )
        })
    }

    pub(super) fn draw_lf_region_section(&mut self, e: &mut Editor) {
        if !gui::collapsing_header("Land LF region PNG", 0) {
            return;
        }
        let Some(d) = e.docs.find_as::<MapDocument>(self.shown) else {
            return;
        };
        let Some(field) = d.lf_field(LfRole::Land) else {
            gui::text_disabled("This document has no land LF samples.");
            return;
        };
        gui::text_wrapped(&format!(
            "Land LF: {} x {} samples. File coordinates: x right, y down; top row first. Exact 16-bit grayscale, at most 1,048,576 samples. No normalization or resampling.",
            field.width(),
            field.height()
        ));
        for (label, value) in ["sample x", "sample y", "width in samples", "height in samples"].into_iter().zip(&mut self.lf_region.samples)
        {
            gui::set_next_item_width(120.0);
            gui::input_int(label, value);
        }
        let n = self.lf_region.samples.map(u32::try_from);
        let selected = match n {
            [Ok(x), Ok(y), Ok(width), Ok(height)] => Some(LfRegion { x, y, width, height }),
            _ => None,
        };
        let error = selected.map_or_else(|| Some("Use non-negative file sample coordinates".into()), |r| r.validate(field).err());
        let can_import = selected.is_some_and(|r| d.check_lf_region_import(r).is_ok());
        let native = d.model().is_some_and(|m| m.built);
        if let Some(error) = &error {
            gui::text_wrapped(error);
        }
        if native {
            gui::text_wrapped(
                "Native map: PNG export is read-only; region import is unsupported. This does not make native campaign-height export safe.",
            );
        }
        let busy = self.painting.is_some() || self.rx.is_some() || self.refill.busy() || self.export.is_some();
        gui::begin_disabled(error.is_some() || busy);
        let export = gui::button("Export region PNG...");
        gui::begin_disabled(!can_import);
        let import = gui::button("Replace region from PNG...");
        gui::end_disabled();
        gui::end_disabled();
        gui::text_wrapped("Export creates a new PNG file; choose a path that does not exist.");
        if (export || import)
            && let Some(region) = selected
        {
            let filters = [("16-bit grayscale PNG".into(), "*.png".into())];
            let path = wce_editor::steam_detect::file_dialog(export, &self.lf_region.path, &filters);
            match self.transfer_lf_region(e, region, path.as_deref(), import) {
                Ok(message) => e.status(&message, false),
                Err(error) => e.status(&error, true),
            }
            if let Some(path) = path {
                self.lf_region.path = path;
            }
        }
    }
}

pub(super) fn register(c: &mut CommandRegistry<Editor>) {
    for (name, import) in [("terrain_lf_region_export", false), ("terrain_lf_region_import", true)] {
        c.add(
            name,
            &format!("{name} <x> <y> <width> <height> <png>"),
            "Exact 16-bit land LF region PNG, file samples top row first; import requires raw/kit, export creates a new file",
            move |e, args| {
                let (region, path) = match region(args) {
                    Ok(value) => value,
                    Err(error) => return CommandResult::fail(error),
                };
                with_terrain(e, |t, e| match t.transfer_lf_region(e, region, Some(&path), import) {
                    Ok(message) => CommandResult::success(message),
                    Err(error) => CommandResult::fail(error),
                })
            },
        );
    }
}
