//! VFX XML files (`vfx/*.xml`, UTF-16): emitters, behaviour chains, effect render stacks and vfx_references.
//!
//! A weather effect file (weather_definitions.xml `vfx1`, e.g. `weather_vfx_rain_heavy_daytime`) is usually an empty `<vfx>` plus
//! `<vfx_references>`: each reference instantiates a library effect (`library_vfx_weather_rain`) and overrides per emitter the
//! enable flag, the tint and the lifetime biases.
//!
//! Numbers are read as C's `atof` / `atoi` read them (the longest numeric prefix, 0 for none), as the C++ editor did.

use std::collections::BTreeMap;
use std::ops::Deref;

use wce_formats::c_num::{strtod, strtol};
use wce_formats::xml_doc::{XmlDoc, decode_entities};

/// C's `atof`, to float.
pub(crate) fn atof(s: &str) -> f32 {
    strtod(s).unwrap_or(0.0) as f32
}

/// C's `atoi`.
pub(crate) fn atoi(s: &str) -> i32 {
    strtol(s).unwrap_or(0) as i32
}

/// A number attribute, or its default when missing.
pub(crate) fn num(value: Option<String>, default: f32) -> f32 {
    value.map_or(default, |value| atof(&value))
}

fn truthy(s: &str) -> bool {
    s == "true" || s == "1"
}

/// Splits like repeated `std::getline` with a separator: no piece after a trailing separator, none for an empty text.
pub(crate) fn getline_split(s: &str, separator: char) -> Vec<&str> {
    let mut pieces = s.split(separator).collect::<Vec<_>>();
    if pieces.last() == Some(&"") {
        pieces.pop();
    }

    pieces
}

/// A curve: linear between keys, clamped outside them.
///
/// The game bakes every curve into 128 entries and reads it with a linear lookup; how it interpolates the keys while baking is not decoded.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VfxCurve {
    /// (t, value), t in 0..1.
    pub keys: Vec<(f32, f32)>,
}

impl VfxCurve {
    pub fn eval(&self, t: f32) -> f32 {
        let Some(first) = self.keys.first() else { return 0.0 };
        if t <= first.0 {
            return first.1;
        }

        for pair in self.keys.windows(2) {
            let ((t0, v0), (t1, v1)) = (pair[0], pair[1]);
            if t <= t1 {
                let a = if t1 > t0 { (t - t0) / (t1 - t0) } else { 1.0 };
                return v0 + (v1 - v0) * a;
            }
        }

        self.keys.last().map_or(0.0, |last| last.1)
    }
}

/// `alpha_min,16711680;0.0,0.000212;0.204703,0.801828|alpha_max,65280;...` -> one curve per name.
pub fn parse_curves(text: &str) -> BTreeMap<String, VfxCurve> {
    let mut out = BTreeMap::new();
    for curve in getline_split(text, '|') {
        let parts = getline_split(curve, ';');
        let Some(head) = parts.first() else { continue };
        let name = head.split(',').next().unwrap_or_default().to_owned();
        let mut keys = parts[1..].iter()
            .filter_map(|key| key.split_once(','))
            .map(|(t, value)| (atof(t), atof(value)))
            .collect::<Vec<_>>();

        keys.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        out.insert(name, VfxCurve { keys });
    }

    out
}

/// One `<property>`, `<ranged_property>` or `<texture_property>`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VfxProp {
    /// `value="..."`, or the texture path.
    pub value: String,

    /// x y z w.
    pub v: [f32; 4],

    /// The number of x / y / z / w attributes present.
    pub components: usize,

    /// Ranged properties (numbers); colour ranges keep their raw text in `min_text` / `max_text`.
    pub min: f32,
    pub max: f32,
    pub ranged: bool,
    pub min_text: String,
    pub max_text: String,
    pub curves: BTreeMap<String, VfxCurve>,
}

/// `float4(1.0,0.9,0.8,1.0)`: up to four numbers after the parenthesis.
fn parse_float4(s: &str, out: &mut [f32; 4]) {
    let body = s.split_once('(').map_or(s, |(_, body)| body);
    for (slot, token) in out.iter_mut().zip(getline_split(body, ',')) {
        *slot = atof(token);
    }
}

/// The properties of an emitter or modifier, by name.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VfxProps {
    pub props: BTreeMap<String, VfxProp>,
}

impl VfxProps {

    pub fn find(&self, name: &str) -> Option<&VfxProp> {
        self.props.get(name)
    }

    /// A non-empty value of a property.
    fn value(&self, name: &str) -> Option<&str> {
        self.find(name).map(|p| p.value.as_str()).filter(|value| !value.is_empty())
    }

    pub fn b(&self, name: &str, default: bool) -> bool {
        self.value(name).map_or(default, truthy)
    }

    pub fn f(&self, name: &str, default: f32) -> f32 {
        self.value(name).map_or(default, atof)
    }

    pub fn i(&self, name: &str, default: i32) -> i32 {
        match self.value(name) {
            None => default,
            Some("true") => 1,
            Some("false") => 0,
            Some(value) => atoi(value),
        }
    }

    /// The (min, max) of a ranged property, or the defaults.
    pub fn range(&self, name: &str, default_lo: f32, default_hi: f32) -> (f32, f32) {
        match self.find(name) {
            Some(p) if p.ranged => (p.min, p.max),
            _ => (default_lo, default_hi),
        }
    }

    /// A vector property (x, y[, z, w]); missing components keep what `out` has.
    pub fn vec(&self, name: &str, out: &mut [f32]) {
        if let Some(p) = self.find(name) {
            for (slot, value) in out.iter_mut().zip(&p.v).take(p.components) {
                *slot = *value;
            }
        }
    }

    /// A colour range `float4(r,g,b,a)`: the min and max colours. Missing ones keep what the outputs have.
    pub fn colour(&self, name: &str, out_min: &mut [f32; 4], out_max: &mut [f32; 4]) {
        if let Some(p) = self.find(name).filter(|p| p.ranged) {
            parse_float4(&p.min_text, out_min);
            parse_float4(&p.max_text, out_max);
        }
    }

    /// `value="float2(1.0,2.0)"` style vectors; missing components keep what `out` has.
    pub fn float_n(&self, name: &str, out: &mut [f32]) {
        let Some(value) = self.value(name) else { return };
        let mut v = [0.0; 4];
        for (slot, value) in v.iter_mut().zip(out.iter()) {
            *slot = *value;
        }

        parse_float4(value, &mut v);
        for (slot, value) in out.iter_mut().zip(v) {
            *slot = value;
        }
    }

    pub fn curve(&self, name: &str, sub: &str) -> Option<&VfxCurve> {
        self.find(name)?.curves.get(sub)
    }

    /// The value of a property; empty if missing.
    pub fn text(&self, name: &str) -> String {
        self.find(name).map(|p| p.value.clone()).unwrap_or_default()
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VfxEmitterDef {
    pub id: String,

    /// `continuous_mesh`, `burst_rigid`...
    pub kind: String,
    pub props: VfxProps,
}

impl Deref for VfxEmitterDef {
    type Target = VfxProps;
    fn deref(&self) -> &VfxProps {
        &self.props
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VfxModifier {
    /// `vs_translation`, `vs_colour`, `ps_texture`...
    pub fragment: String,
    pub props: VfxProps,
}

impl Deref for VfxModifier {
    type Target = VfxProps;
    fn deref(&self) -> &VfxProps {
        &self.props
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VfxChain {
    pub id: String,
    pub modifiers: Vec<VfxModifier>,
}

impl VfxChain {
    pub fn find(&self, fragment: &str) -> Option<&VfxModifier> {
        self.modifiers.iter().find(|m| m.fragment == fragment)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VfxStack {
    pub id: String,
    pub emitters: Vec<String>,
    pub chains: Vec<String>,
    pub scene_view_type: i32,
    pub cast_shadows: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VfxRefEmitter {
    pub name: String,
    pub enable: bool,
    pub tint_min: [f32; 4],
    pub tint_max: [f32; 4],
    pub particle_lifetime_bias: [f32; 2],
    pub emitter_lifetime_bias: [f32; 2],
}

#[derive(Clone, Debug, PartialEq)]
pub struct VfxRef {
    pub inst_name: String,
    pub library: String,
    pub scale_min: f32,
    pub scale_max: f32,
    pub pos: [f32; 3],
    pub start_time: f32,
    pub infinite_life: bool,
    pub emitters: Vec<VfxRefEmitter>,
}

impl VfxRef {
    pub fn find(&self, emitter: &str) -> Option<&VfxRefEmitter> {
        self.emitters.iter().find(|e| e.name == emitter)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VfxFile {
    /// `camera_following`, `affected_by_wind`, `construction_type`...
    pub vfx_attrs: BTreeMap<String, String>,
    pub emitters: Vec<VfxEmitterDef>,
    pub chains: Vec<VfxChain>,
    pub stacks: Vec<VfxStack>,
    pub refs: Vec<VfxRef>,
}

impl VfxFile {

    pub fn camera_following(&self) -> bool {
        self.vfx_attrs.get("camera_following").is_some_and(|value| truthy(value))
    }

    pub fn emitter(&self, id: &str) -> Option<&VfxEmitterDef> {
        self.emitters.iter().find(|e| e.id == id)
    }

    pub fn chain(&self, id: &str) -> Option<&VfxChain> {
        self.chains.iter().find(|c| c.id == id)
    }
}

/// The text between an element's start and end tags (none for elements with children).
fn inner_text(doc: &XmlDoc, e: usize) -> String {
    let el = &doc.elements()[e];
    if el.self_closing || !el.children.is_empty() {
        return String::new();
    }

    let t = doc.original_text();
    let window = &t[..(el.end_off + 2).min(t.len())];
    match window.windows(2).rposition(|pair| pair == b"</") {
        Some(end) if end >= el.start_end => String::from_utf8_lossy(&decode_entities(&t[el.start_end..end])).into_owned(),
        _ => String::new(),
    }
}

fn read_prop(doc: &XmlDoc, e: usize) -> VfxProp {
    let mut p = VfxProp::default();
    if let Some(value) = doc.get(e, "value") {
        p.value = value;
    }

    if let Some(path) = doc.get(e, "path") {
        p.value = path;
    }

    for (i, component) in ["x", "y", "z", "w"].iter().enumerate() {
        if let Some(value) = doc.get(e, component) {
            p.v[i] = atof(&value);
            p.components = i + 1;
        }
    }

    if let Some(lo) = doc.get(e, "min") {
        p.ranged = true;
        p.min = atof(&lo);
        p.min_text = lo;
    }

    if let Some(hi) = doc.get(e, "max") {
        p.ranged = true;
        p.max = atof(&hi);
        p.max_text = hi;
    }

    let text = inner_text(doc, e);
    if !text.is_empty() {
        p.curves = parse_curves(&text);
    }

    p
}

/// The properties among an element's children.
///
/// Emitter properties repeat per graphics quality: quality 0 is the base value, higher levels are lower-quality overrides (fewer
/// particles, shorter culling). The editor keeps the base level, which is what gfx_effects_quality 3 (the maximum, the default)
/// selects; an override is only kept until a base value appears.
fn read_props(doc: &XmlDoc, e: usize) -> VfxProps {
    let mut out = VfxProps::default();
    for &c in &doc.elements()[e].children {
        if !matches!(doc.elements()[c].name.as_str(), "property" | "ranged_property" | "texture_property") {
            continue;
        }

        let Some(name) = doc.get(c, "name") else { continue };
        let quality = atoi(&doc.get(c, "quality").unwrap_or_else(|| "0".to_owned()));
        if quality != 0 && out.props.contains_key(&name) {
            continue;
        }

        out.props.insert(name, read_prop(doc, c));
    }

    out
}

fn child(doc: &XmlDoc, e: usize, name: &str) -> Option<usize> {
    doc.elements()[e].children.iter().copied().find(|&c| doc.elements()[c].name == name)
}

/// Reads the vfx definitions of a document.
pub fn parse_vfx(doc: &XmlDoc) -> VfxFile {
    let mut f = VfxFile::default();
    let els = doc.elements();
    let text = |e: usize, name: &str| doc.get(e, name).unwrap_or_default();
    for &r in doc.roots() {
        match els[r].name.as_str() {
            "vfx" => {
                for (a, at) in els[r].attrs.iter().enumerate() {
                    f.vfx_attrs.insert(at.name.clone(), String::from_utf8_lossy(&decode_entities(doc.attr_raw(r, a))).into_owned());
                }

                if let Some(list) = child(doc, r, "emitters") {
                    f.emitters.extend(els[list].children.iter().filter(|&&e| els[e].name == "emitter").map(|&e| VfxEmitterDef {
                        id: text(e, "id"),
                        kind: text(e, "type"),
                        props: read_props(doc, e),
                    }));
                }

                if let Some(list) = child(doc, r, "behaviour_chains") {
                    f.chains.extend(els[list].children.iter().filter(|&&c| els[c].name == "behaviour_chain").map(|&c| VfxChain {
                        id: text(c, "id"),
                        modifiers: els[c].children.iter().filter(|&&m| els[m].name == "modifier").map(|&m| VfxModifier {
                            fragment: text(m, "fragment"),
                            props: read_props(doc, m),
                        }).collect(),
                    }));
                }

                if let Some(list) = child(doc, r, "effect_render_stacks") {
                    for &s in els[list].children.iter().filter(|&&s| els[s].name == "effect_render_stack") {
                        let ids = |name: &str| child(doc, s, name).map(|list| els[list].children.iter().map(|&e| text(e, "id")).collect()).unwrap_or_default();
                        let mut stack = VfxStack { id: text(s, "id"), emitters: ids("emitters_selected"), chains: ids("behaviour_chains_selected"), ..Default::default() };
                        if let Some(rp) = child(doc, s, "render_params") {
                            stack.scene_view_type = num(doc.get(rp, "scene_view_type"), 0.0) as i32;
                            stack.cast_shadows = truthy(&doc.get(rp, "cast_shadows").unwrap_or_else(|| "false".to_owned()));
                        }

                        f.stacks.push(stack);
                    }
                }
            }

            "vfx_references" => {
                for &v in els[r].children.iter().filter(|&&v| els[v].name == "vfx") {
                    let n = |name: &str, default: f32| num(doc.get(v, name), default);
                    f.refs.push(VfxRef {
                        inst_name: text(v, "inst_name"),
                        library: text(v, "vfx_ref"),
                        // Rome 2 writes one `scale`, and `tint_r`... without a range.
                        scale_min: n("scale_min", n("scale", 1.0)),
                        scale_max: n("scale_max", n("scale", 1.0)),
                        pos: [n("pos_x", 0.0), n("pos_y", 0.0), n("pos_z", 0.0)],
                        start_time: n("start_time", 0.0),
                        infinite_life: truthy(&doc.get(v, "infinite_life").unwrap_or_else(|| "true".to_owned())),
                        emitters: els[v].children.iter().filter(|&&e| els[e].name == "emitter").map(|&e| {
                            let n = |name: &str, default: f32| num(doc.get(e, name), default);
                            VfxRefEmitter {
                                name: text(e, "name"),
                                enable: truthy(&doc.get(e, "enable").unwrap_or_else(|| "true".to_owned())),
                                tint_min: std::array::from_fn(|k| n(&format!("tint_min_{}", ['r', 'g', 'b', 'a'][k]), n(&format!("tint_{}", ['r', 'g', 'b', 'a'][k]), 1.0))),
                                tint_max: std::array::from_fn(|k| n(&format!("tint_max_{}", ['r', 'g', 'b', 'a'][k]), n(&format!("tint_{}", ['r', 'g', 'b', 'a'][k]), 1.0))),
                                particle_lifetime_bias: [n("particle_lifetime_bias_min", 0.0), n("particle_lifetime_bias_max", 0.0)],
                                emitter_lifetime_bias: [n("emitter_lifetime_bias_min", 0.0), n("emitter_lifetime_bias_max", 0.0)],
                            }
                        }).collect(),
                    });
                }
            }

            _ => {}
        }
    }

    f
}
