//! Fixture bootstrap only. Production publication belongs to the document/export routes.
pub trait FixtureWrite {
    fn write_to(&self, dir: &std::path::Path) -> Result<(), String>;
}
impl FixtureWrite for wce_terrain_io::MapFiles {
    fn write_to(&self, dir: &std::path::Path) -> Result<(), String> {
        use wce_foundation::publication::{Output, Source, Staged};
        let outputs = self
            .folder
            .iter()
            .map(|(key, blob)| {
                let source = match blob {
                    wce_tile_model::Blob::File(p) => Source::File(p.clone()),
                    wce_tile_model::Blob::Bytes(b) => Source::Bytes(b.clone()),
                };
                Ok(Output::new(wce_foundation::publication::resource_path(dir, key)?, source))
            })
            .collect::<Result<Vec<_>, String>>()?;
        Staged::prepare(outputs).and_then(Staged::publish).map(|_| ()).map_err(|e| e.to_string())
    }
}
