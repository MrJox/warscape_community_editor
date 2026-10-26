//! The binary building list of an exported tile (`building_list`, `far_building_list`, `building_list_sprawl`, `building_list_veg`): a CAAB
//! ESF file, `BATTLEFIELD_BUILDING_LIST` > `BATTLEFIELD_BUILDING_LIST_BLOCK`, one block per building. Attila writes record version 12 (11 in
//! three sprawl lists, without `toggleable` and `lite`); Rome 2 versions 10 (without `position_type`), 11 and 12, whose 12 adds
//! `burn_amount` instead (docs/formats/rome2_tiles.md). The XML twin (`building_list.xml`, TEd's stage-A export) names the fields;
//! docs/formats/tile_binary.md, "Building lists".

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

    /// 0 relative, 1 absolute, 2 `lf_relative` (every shipped building); `None` in a version 10 list (Rome 2's).
    pub position_type: Option<u32>,

    /// Attila's version 12 fields: `None` in a version 11 list and in Rome 2's.
    pub toggleable: Option<bool>,
    pub lite: Option<bool>,

    /// How much of the building burns, 0 to 1: Rome 2's version 12 field (EmpireUtility's XML writer names it).
    pub burn_amount: Option<f32>,
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
    if !(10..=12).contains(list.version()) {
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

    // Attila's 12: 17 values (two bools added); Rome 2's 12: 16 (an f32 added); 11: 15; Rome 2's 10: 14 (no position type).
    let expected: &[usize] = match version {
        12 => &[16, 17],
        11 => &[15],
        _ => &[14],
    };
    if !expected.contains(&values.len()) {
        return Err(format!("{} values ({expected:?} expected)", values.len()));
    }

    let [key, pos, oi, oj, ok, ow, scale, damage, fire, disabled, weak, breach, indestructible, dock] = &values[..14] else { unreachable!() };
    let ptype = values.get(14);
    let (toggle, lite) = if values.len() == 17 { (values.get(15), values.get(16)) } else { (None, None) };
    let burn = if values.len() == 16 { values.get(15) } else { None };

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
            Some(N::U32(v)) => Some(*v.value()),
            None => None,
            Some(other) => return Err(format!("{other:?} where the position type was expected")),
        },
        toggleable: toggle.map(b).transpose()?,
        lite: lite.map(b).transpose()?,
        burn_amount: burn.map(f).transpose()?,
    })
}
