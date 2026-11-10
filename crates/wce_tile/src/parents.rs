//! The types that live only inside another (their holder) and what the holders hold, as in every tile of the ATTILA kit (917 `tile.ted`,
//! 2026-11-10): a deployment zone's regions, a siege AI node's or edge's shapes, a capture location's boundary. A holder holds nothing
//! else, and these never stand anywhere else. TEd tells a holder's shapes apart by their names (`TileDocument::siege_problems`).

/// A shape a holder has: its name, whether it is closed, and how many it has at most (0: any number).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Role {
    pub name: &'static str,
    pub closed: bool,
    pub most: usize,
}

const fn role(name: &'static str, closed: bool, most: usize) -> Role {
    Role { name, closed, most }
}

const CAPTURE: &[Role] = &[role("Boundary", true, 1)];
const DEPLOYMENT: &[Role] = &[role("Deployment Zone Polygon", true, 0)];
const NODE: &[Role] = &[role("Bounding Box", true, 1), role("Boundary", true, 1)];
const STREET_EDGE: &[Role] = &[role("Street", false, 1), role("Street boundary", false, 2)];
const OTHER_EDGE: &[Role] = &[role("Street", false, 1)];

/// The holders a type goes into; empty: a type that goes into layers and groups.
pub fn holders(kind: &str) -> &'static [&'static str] {
    match kind {
        "DEPLOYMENT_ZONE_POLYGON_INSTANCE" => &["DeploymentZonePolygonInstanceGroup"],
        "SIEGE_AI_POLYGON_INSTANCE" => &["SiegeAINodeInstance", "SiegeAIEdgeInstance"],
        "CaptureLocationPolygonInstance" => &["CaptureLocationInstance"],
        _ => &[],
    }
}

/// What a holder holds (`None`: not a holder).
pub fn child_kind(holder: &str) -> Option<&'static str> {
    match holder {
        "DeploymentZonePolygonInstanceGroup" => Some("DEPLOYMENT_ZONE_POLYGON_INSTANCE"),
        "SiegeAINodeInstance" | "SiegeAIEdgeInstance" => Some("SIEGE_AI_POLYGON_INSTANCE"),
        "CaptureLocationInstance" => Some("CaptureLocationPolygonInstance"),
        _ => None,
    }
}

/// The shapes a holder has, by its own `type` (a street edge has two street boundaries, the other edges a street only).
pub fn roles(holder: &str, holder_type: &str) -> &'static [Role] {
    match holder {
        "CaptureLocationInstance" => CAPTURE,
        "DeploymentZonePolygonInstanceGroup" => DEPLOYMENT,
        "SiegeAINodeInstance" => NODE,
        "SiegeAIEdgeInstance" if holder_type.is_empty() || holder_type == "SAET_STREET" => STREET_EDGE,
        "SiegeAIEdgeInstance" => OTHER_EDGE,
        _ => &[],
    }
}

/// A type's name for messages.
pub fn label(kind: &str) -> &'static str {
    match kind {
        "DEPLOYMENT_ZONE_POLYGON_INSTANCE" => "deployment region",
        "DeploymentZonePolygonInstanceGroup" => "deployment zone",
        "SIEGE_AI_POLYGON_INSTANCE" => "siege AI shape",
        "SiegeAINodeInstance" => "siege AI node",
        "SiegeAIEdgeInstance" => "siege AI edge",
        "CaptureLocationPolygonInstance" => "capture location boundary",
        "CaptureLocationInstance" => "capture location",
        _ => "item",
    }
}

/// Where a type's holders are, for messages ("a deployment zone", "a siege AI node or edge").
pub fn holders_label(kind: &str) -> String {
    match holders(kind) {
        [] => String::new(),
        [one] => format!("a {}", label(one)),
        ["SiegeAINodeInstance", "SiegeAIEdgeInstance"] => "a siege AI node or edge".to_owned(),
        many => many.iter().map(|h| format!("a {}", label(h))).collect::<Vec<_>>().join(" or "),
    }
}

/// Whether an element of type `kind` may go into one of type `into`.
///
/// # Errors
///
/// It may not: says where it goes.
pub fn fits(kind: &str, into: &str) -> Result<(), String> {
    let hs = holders(kind);
    if !hs.is_empty() {
        return if hs.contains(&into) { Ok(()) } else { Err(format!("a {} goes only inside {}", label(kind), holders_label(kind))) };
    }

    match child_kind(into) {
        Some(child) => Err(format!("a {} holds only its {}s", label(into), label(child))),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holders_and_children_go_together() {
        assert!(fits("DEPLOYMENT_ZONE_POLYGON_INSTANCE", "DeploymentZonePolygonInstanceGroup").is_ok());
        assert!(fits("DEPLOYMENT_ZONE_POLYGON_INSTANCE", "LayerInstance").is_err());
        assert!(fits("SIEGE_AI_POLYGON_INSTANCE", "SiegeAIEdgeInstance").is_ok());
        assert!(fits("SIEGE_AI_POLYGON_INSTANCE", "DeploymentZonePolygonInstanceGroup").is_err());
        assert!(fits("BUILDING_INSTANCE", "CaptureLocationInstance").is_err());
        assert!(fits("BUILDING_INSTANCE", "LayerInstance").is_ok());
        assert!(fits("DeploymentZonePolygonInstanceGroup", "UserInstanceGroup").is_ok());
        assert_eq!(fits("SIEGE_AI_POLYGON_INSTANCE", "LayerInstance").unwrap_err(), "a siege AI shape goes only inside a siege AI node or edge");
        assert_eq!(roles("SiegeAIEdgeInstance", "SAET_WALL").len(), 1);
        assert_eq!(roles("SiegeAIEdgeInstance", "SAET_STREET")[1].most, 2);
    }
}
