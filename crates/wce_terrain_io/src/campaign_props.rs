//! Native campaign props retain their own marker representation, beside the Terry adapter.
use crate::{Result, file_error};
use wce_terrain::{Item, MapKind, MapModel, campaign_props::CampaignProps};
use wce_tile_model::Blob;

pub(crate) fn load(m: &mut MapModel) -> Result<()> {
    if m.tile_list.as_ref().is_some_and(|t| t.get().is_campaign) {
        m.kind = MapKind::Campaign;
    }
    if m.kind != MapKind::Campaign {
        return Ok(());
    }
    let mut matching = m.carried.iter().filter(|(k, _)| k.eq_ignore_ascii_case("prop_marker.markers"));
    if let Some((path, blob)) = matching.next() {
        // Unknown profiles and ambiguous keys remain opaque/view-only; never guess their editing rules.
        if matching.next().is_none()
            && let Ok(props) = CampaignProps::parse(blob.bytes()?)
        {
            m.native_props = Some(Item::new(path.clone(), props, blob.clone()));
        }
    }
    Ok(())
}

pub(crate) fn write(m: &MapModel, folder: &mut std::collections::BTreeMap<String, Blob>) -> Result<()> {
    if let Some(props) = &m.native_props {
        if !m.built || m.kind != MapKind::Campaign || m.carried.get(&props.path) != Some(&props.origin) {
            return Err(file_error(&props.path, "native prop source association was replaced"));
        }
        folder.insert(props.path.clone(), Blob::Bytes(props.get().write()));
    }
    Ok(())
}
