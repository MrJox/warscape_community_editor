//! Exact land-LF PNG transfer at the document boundary. All validation precedes a single page-history command.

use std::io::{Read, Write};
use std::path::Path;
use std::sync::Arc;

use wce_formats::lf_region_png;
use wce_terrain::LfRole;
use wce_terrain::lf_edit::LfRecord;
use wce_terrain::lf_region::LfRegion;
use wce_terrain::paged::PagedU16;

use crate::MapDocument;
use crate::lf::{LfMirror, LfStrokeCommand};

impl MapDocument {
    /// Native/built documents are export-only, including native battle maps. Refuse before accessing the input file or history.
    pub fn check_lf_region_import(&self, region: LfRegion) -> Result<(), String> {
        let model = self.model().ok_or("LF region import requires a raw/kit map")?;
        if model.built || !matches!(&self.source, crate::MapSource::Kit(_)) {
            return Err("Native LF region import is unsupported; open a raw/kit map. PNG export is read-only and does not certify native campaign-height export.".into());
        }
        region.validate(self.land_region_field()?)?;
        Ok(())
    }

    fn land_region_field(&self) -> Result<&PagedU16, String> {
        self.lf_field(LfRole::Land).ok_or_else(|| "This document has no land LF samples".into())
    }

    pub fn export_lf_region_png(&self, region: LfRegion) -> Result<Vec<u8>, String> {
        let samples = region.read(self.land_region_field()?)?;
        lf_region_png::encode(region.width, region.height, &samples).map_err(|e| e.0)
    }

    /// True only when one edit was recorded. A no-op leaves Item::edited, dirty state and any redo tail unchanged.
    pub fn import_lf_region_png(&mut self, region: LfRegion, bytes: &[u8], mirror: Option<Arc<LfMirror>>) -> Result<bool, String> {
        self.check_lf_region_import(region)?;
        let image = lf_region_png::decode(bytes).map_err(|e| e.0)?;
        if (image.width, image.height) != (region.width, region.height) {
            return Err("PNG dimensions must exactly match the LF region; no resampling is performed".into());
        }
        let field = self.land_region_field()?;
        if mirror.as_ref().is_some_and(|m| (m.w, m.h) != (field.width(), field.height())) {
            return Err("LF view mirror does not match the document".into());
        }
        let Some(record) = LfRecord::replacement(field, region, &image.samples)? else {
            return Ok(false);
        };
        self.push_stroke(LfStrokeCommand { role: LfRole::Land, record: Arc::new(record), mirror });
        Ok(true)
    }

    /// None is file-dialog cancellation. Read a bounded input, then use the same document path as commands and the UI.
    pub fn import_lf_region_file(&mut self, region: LfRegion, path: Option<&Path>, mirror: Option<Arc<LfMirror>>) -> Result<bool, String> {
        let Some(path) = path else { return Ok(false) };
        self.check_lf_region_import(region)?;
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        file.take(lf_region_png::MAX_FILE_BYTES as u64 + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        self.import_lf_region_png(region, &bytes, mirror)
    }

    /// Create a new PNG only: existing paths (including map resources and aliases to them) are never overwritten.
    /// None is cancellation. An output I/O error never changes the document.
    pub fn export_lf_region_file(&self, region: LfRegion, path: Option<&Path>) -> Result<bool, String> {
        let Some(path) = path else { return Ok(false) };
        if !path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("png")) {
            return Err("Choose a new .png file for LF region export".into());
        }
        let bytes = self.export_lf_region_png(region)?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|e| format!("{}: {e}; choose a new PNG path", path.display()))?;
        if let Err(err) = file.write_all(&bytes) {
            drop(file);
            let _ = std::fs::remove_file(path);
            return Err(err.to_string());
        }
        Ok(true)
    }
}
