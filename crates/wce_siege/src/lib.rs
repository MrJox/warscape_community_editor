//! A settlement's siege AI layout generated in one go (docs/plans/siege_ai.md; what the game reads: docs/re/siege_ai.md): the street
//! graph (area, intersection and entry nodes; street and area connection edges), the wall edges, the fort perimeter, the approach lines
//! and both alliances' deployment zones. Pure geometry on the ground plane (x, z), scene units; the document side gathers the
//! [`Input`] and writes the [`Layout`].
//!
//! - [`raster`]: the grid and what is done on it;
//! - [`generate`]: the input to the layout;
//! - [`check`]: what the game needs of a layout, checked (also on CA's);
//! - [`svg`]: a layout drawn.

pub mod check;
pub mod generate;
pub mod palisade;
pub mod ports;
pub mod raster;
pub mod svg;

/// A point on the ground (x, z).
pub type P = [f64; 2];

/// The ground's height (metres, the sea at 0) on a grid: cell (i, j) covers x from x0 + i cell, z from z0 + j cell. Given for tiles with
/// ground under the sea: the sea, the coast and its beaches come from it.
#[derive(Clone, Debug, Default)]
pub struct Sea {
    pub x0: f64,
    pub z0: f64,
    pub cell: f64,
    pub w: usize,
    pub h: usize,
    pub height: Vec<f32>,
}

impl Sea {
    /// Ground lower than this is under water (the sea at 0 m; the quays stand just above it).
    pub const WATER: f64 = -0.5;

    /// The height at a point (the nearest sample; 0 off the grid).
    pub fn height(&self, p: P) -> f64 {
        let (i, j) = (((p[0] - self.x0) / self.cell).floor(), ((p[1] - self.z0) / self.cell).floor());
        if i >= 0.0 && j >= 0.0 && (i as usize) < self.w && (j as usize) < self.h { f64::from(self.height[j as usize * self.w + i as usize]) } else { 0.0 }
    }

    pub fn at(&self, p: P) -> bool {
        self.height(p) < Self::WATER
    }
}

/// A piece of an encampment's palisade: its place on the palisade's line (its origin).
#[derive(Clone, Copy, Debug, Default)]
pub struct PalisadePiece {
    pub at: P,
}

/// What the generator reads from a tile.
#[derive(Clone, Debug, Default)]
pub struct Input {
    /// The map's bounds (lo, hi).
    pub bounds: (P, P),

    /// The no-go regions (closed).
    pub nogo: Vec<Vec<P>>,

    /// What blocks of the buildings that are not fort pieces: their hard outlines (closed).
    pub obstacles: Vec<Vec<P>>,

    pub walls: Vec<WallPiece>,
    pub captures: Vec<Capture>,

    /// The pieces of an encampment's palisade (they are also in `walls`, for what they block).
    pub palisade: Vec<PalisadePiece>,

    /// The ground's heights where some are under the sea (the sea: under 0 m, joined to the map's edge), `None` without any.
    pub sea: Option<Sea>,
}

/// A piece of a fort: a wall, a gate, a tower.
#[derive(Clone, Debug, Default)]
pub struct WallPiece {
    /// What it stands on and what blocks (closed rings).
    pub footprint: Vec<Vec<P>>,

    /// The surfaces up on it (closed rings): the walkways, the towers' floors. The wall edges run along the middle of them all.
    pub platforms: Vec<Vec<P>>,

    /// A walkway given as its middle line instead (a procedural wall's), start to end.
    pub walkway: Vec<P>,

    /// A gate: the attacker's way in, an area node on the city side.
    pub gate: bool,

    /// Its ends along the wall (where its frame's x extent meets the middle of its walkway).
    pub ends: [P; 2],
}

#[derive(Clone, Copy, Debug)]
pub struct Capture {
    pub at: P,
    pub major: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NodeKind {
    Area,
    Intersection,
    Entry,
}

impl NodeKind {
    /// TEd's `type` (`SIEGE_NODE_TYPE`).
    pub fn ted(self) -> &'static str {
        match self {
            NodeKind::Area => "SANT_AREA",
            NodeKind::Intersection => "SANT_INTERSECTION",
            NodeKind::Entry => "SANT_ENTRY",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EdgeKind {
    Street,
    AreaConnection,
    Wall,
    Palisade,
}

impl EdgeKind {
    /// TEd's `type` (`SIEGE_EDGE_TYPE`).
    pub fn ted(self) -> &'static str {
        match self {
            EdgeKind::Street => "SAET_STREET",
            EdgeKind::AreaConnection => "SAET_AREA_CONNECTION",
            EdgeKind::Wall => "SAET_WALL",
            EdgeKind::Palisade => "SAET_PALISADE",
        }
    }
}

/// A node: its rectangle (the game's "Bounding Box": 4 corners in order; the occupiable area and what edges attach to) and its
/// Boundary (the walkable outline, closed).
#[derive(Clone, Debug)]
pub struct Node {
    pub kind: NodeKind,
    pub centre: P,
    pub bbox: [P; 4],
    pub boundary: Vec<P>,
}

/// An edge: its line ("Street": its first and last points in the two nodes' rectangles) and, for a street, the corridor's two sides.
#[derive(Clone, Debug)]
pub struct Edge {
    pub kind: EdgeKind,
    pub line: Vec<P>,
    pub sides: Vec<Vec<P>>,

    /// The nodes it joins (indices into [`Layout::nodes`]; none for a wall edge).
    pub nodes: Option<(usize, usize)>,
}

/// TEd's `deployment_polygon_type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoneKind {
    Additive,
    Subtractive,
    GuerrillaAdditive,
    GuerrillaSubtractive,
}

impl ZoneKind {
    pub fn ted(self) -> &'static str {
        match self {
            ZoneKind::Additive => "DZPT_ADDITIVE",
            // CA's spelling.
            ZoneKind::Subtractive => "DZPT_SUBSTRACTIVE",
            ZoneKind::GuerrillaAdditive => "DZPT_GUERRILLA_EXCLUSION_ADDITIVE",
            ZoneKind::GuerrillaSubtractive => "DZPT_GUERRILLA_EXCLUSION_SUBTRACTIVE",
        }
    }
}

/// An alliance's deployment zone: its polygons.
#[derive(Clone, Debug, Default)]
pub struct Deployment {
    pub polygons: Vec<(ZoneKind, Vec<P>)>,
}

/// What the generator makes.
#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,

    /// `AIH_FORT_PERIMETER` (closed).
    pub perimeter: Vec<P>,

    /// `AIH_FORT_APPROACH` lines.
    pub approaches: Vec<[P; 2]>,

    /// `PALT_FORT` AI separators (open lines).
    pub separators: Vec<Vec<P>>,

    /// A port's `AIH_DEFAULT_DEPLOYMENT_LINE`s (at sea, in the defender's zone).
    pub deployment_lines: Vec<[P; 2]>,

    /// A port's `AIH_BEACH_LANDING` lines (along the shore) and `AIH_BEACH_LANDING_FORT_APPROACH` lines (just inland of them).
    pub beaches: Vec<Vec<P>>,
    pub beach_approaches: Vec<[P; 2]>,

    /// alliance 1.
    pub defender: Deployment,

    /// alliance 0.
    pub attacker: Deployment,

    /// What the user should look at.
    pub warnings: Vec<String>,
}

impl Layout {
    /// One line: what was made.
    pub fn summary(&self) -> String {
        let count = |k: NodeKind| self.nodes.iter().filter(|n| n.kind == k).count();
        let edges = |k: EdgeKind| self.edges.iter().filter(|e| e.kind == k).count();
        let port = if self.deployment_lines.is_empty() && self.beaches.is_empty() {
            String::new()
        } else {
            format!("; {} deployment lines, {} beaches", self.deployment_lines.len(), self.beaches.len())
        };
        format!(
            "{} areas, {} intersections, {} entries; {} streets, {} area connections, {} wall edges, {} palisade edges; {} separators; {} approach lines{port}; {} warning(s)",
            count(NodeKind::Area), count(NodeKind::Intersection), count(NodeKind::Entry), edges(EdgeKind::Street), edges(EdgeKind::AreaConnection),
            edges(EdgeKind::Wall), edges(EdgeKind::Palisade), self.separators.len(), self.approaches.len(), self.warnings.len(),
        )
    }
}
