//! The binary building list of an exported tile (`building_list`, `far_building_list`, `building_list_sprawl`, `building_list_veg`): a CAAB
//! ESF file, `BATTLEFIELD_BUILDING_LIST` (record version 12; 11 in three sprawl lists, without `toggleable` and `lite`) >
//! `BATTLEFIELD_BUILDING_LIST_BLOCK`, one block per building. The XML twin
//! (`building_list.xml`, TEd's stage-A export) names the fields; docs/formats/tile_binary.md, "Building lists".

use rpfm_lib::files::esf::{ESF, NodeType};

/// One building of the list, the XML's attribute names.
#[derive(Clone, Debug, PartialEq)]
pub struct BattlefieldBuilding {
    pub key: String,
    pub position: [f32; 3],

    /// A quaternion (i, j, k, w).
    pub orientation: [f32; 4],
    pub scale: [f32; 3],
    pub starting_damage: f32,
    pub on_fire: bool,
    pub start_disabled: bool,
    pub weak_point: bool,
    pub ai_breachable: bool,
    pub indestructible: bool,
    pub dockable: bool,

    /// 0 relative, 1 absolute, 2 `lf_relative` (every shipped building).
    pub position_type: u32,

    /// `None` in a version 11 list (the fields were added in 12).
    pub toggleable: Option<bool>,
    pub lite: Option<bool>,
}

fn record<'a>(node: &'a NodeType, name: &str) -> Result<&'a rpfm_lib::files::esf::RecordNode, String> {
    match node {
        NodeType::Record(r) if r.name() == name => Ok(r),
        NodeType::Record(r) => Err(format!("{} where {name} was expected", r.name())),
        _ => Err(format!("a value where {name} was expected")),
    }
}

/// The buildings of a list.
///
/// # Errors
///
/// Not a building list, or a block of another shape.
pub fn read(esf: &ESF) -> Result<Vec<BattlefieldBuilding>, String> {
    let list = record(esf.root_node(), "BATTLEFIELD_BUILDING_LIST")?;
    let [children] = list.children().as_slice() else { return Err("the list has more than one block".to_owned()) };
    let [block] = children.as_slice() else { return Err("the list holds more than its block record".to_owned()) };
    let blocks = record(block, "BATTLEFIELD_BUILDING_LIST_BLOCK")?;
    if !(11..=12).contains(list.version()) {
        return Err(format!("list version {}", list.version()));
    }

    blocks.children().iter().enumerate().map(|(i, b)| building(b, *list.version()).map_err(|e| format!("building {i}: {e}"))).collect()
}

fn building(values: &[NodeType], version: u8) -> Result<BattlefieldBuilding, String> {
    use NodeType as N;
    let f = |n: &N| match n {
        N::F32(v) => Ok(*v.value()),
        other => Err(format!("{other:?} where an f32 was expected")),
    };

    let b = |n: &N| match n {
        N::Bool(v) => Ok(*v.value()),
        other => Err(format!("{other:?} where a bool was expected")),
    };

    let v3 = |n: &N| match n {
        N::Coord3d(c) => Ok([*c.x(), *c.y(), *c.z()]),
        other => Err(format!("{other:?} where a vector was expected")),
    };

    let expected = if version >= 12 { 17 } else { 15 };
    if values.len() != expected {
        return Err(format!("{} values ({expected} expected)", values.len()));
    }

    let [key, pos, oi, oj, ok, ow, scale, damage, fire, disabled, weak, breach, indestructible, dock, ptype] = &values[..15] else { unreachable!() };
    let (toggle, lite) = (values.get(15), values.get(16));

    Ok(BattlefieldBuilding {
        key: match key {
            N::Ascii(s) => s.clone(),
            other => return Err(format!("{other:?} where the key was expected")),
        },
        position: v3(pos)?,
        orientation: [f(oi)?, f(oj)?, f(ok)?, f(ow)?],
        scale: v3(scale)?,
        starting_damage: f(damage)?,
        on_fire: b(fire)?,
        start_disabled: b(disabled)?,
        weak_point: b(weak)?,
        ai_breachable: b(breach)?,
        indestructible: b(indestructible)?,
        dockable: b(dock)?,
        position_type: match ptype {
            N::U32(v) => *v.value(),
            other => return Err(format!("{other:?} where the position type was expected")),
        },
        toggleable: toggle.map(b).transpose()?,
        lite: lite.map(b).transpose()?,
    })
}
