//! Read one existing native prop resource in place; no assets are written or activated.
//! WCE_GAME_KEY=rome_2 cargo run --release -p wce_terrain_io --example campaign_prop_check -- <game> <map>
use wce_terrain::campaign_props::CampaignProps;
use wce_vfs::{Config, GameProfile, Vfs};
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    assert_eq!(args.len(), 2, "expected game directory and campaign map");
    let mut cfg = Config::new(&args[0], GameProfile::from_env().clone());
    cfg.mod_packs = Some(vec![]);
    let vfs = Vfs::mount(&cfg).expect("game sources");
    let path = format!("terrain/campaigns/{}/prop_marker.markers", args[1]);
    let bytes = vfs.read(&path).expect("native prop resource");
    let header = wce_formats::markers::MarkersFile::parse(&bytes).expect("marker codec");
    println!(
        "native {} v{}, {} records, first group {:?}",
        header.class_name,
        header.version,
        header.records.len(),
        header.groups.as_ref().and_then(|g| g.first()).map(|g| (g.key.len(), g.indices.len()))
    );
    let mut props = CampaignProps::parse(bytes.clone().into()).expect("supported native marker profile");
    assert_eq!(props.write().as_ref(), &bytes[..]);
    let original = props.position(0).expect("record 0");
    let mut moved = original;
    moved[0] += 0.125;
    moved[2] += 0.25;
    props.translate(0, moved).unwrap();
    let written = props.write();
    let mut a = wce_formats::markers::MarkersFile::parse(&bytes).unwrap();
    let b = wce_formats::markers::MarkersFile::parse(&written).unwrap();
    a.records[0][48..60].copy_from_slice(&b.records[0][48..60]);
    assert_eq!(a, b, "only record 0 translation may change");
    assert_eq!(CampaignProps::parse(written).unwrap().position(0).unwrap(), moved);
    props.translate(0, original).unwrap();
    assert_eq!(props.write().as_ref(), &bytes[..]);
    println!("{path}: {} records, no-op/position-only/reopen/revert exact; model {}", props.len(), props.model(0).unwrap());
}
