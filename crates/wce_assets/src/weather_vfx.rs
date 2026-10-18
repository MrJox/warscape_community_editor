//! The precipitation and weather particles of one weather definition, and the effects placed in the world:
//!
//! `.environment` WEATHER `definition_name` -> `weather/weather_definitions.xml` WEATHER_DEFINITION (vfx1, wind) -> `vfx/<vfx1>.xml`
//! -> vfx_references -> library emitters (enable and tint overrides) + behaviour chains. A tile's VFXInstance names its `vfx/<name>.xml`
//! the same way ([`load_effect`]).

use std::collections::BTreeMap;

use wce_formats::xml_doc::XmlDoc;
use wce_vfs::Vfs;

use crate::vfx_constants::{EmitterConstants, VFX_DISTORTION_VIEW_ID, build_emitter_constants};
use crate::vfx_def::{VfxChain, VfxEmitterDef, VfxFile, VfxRef, VfxStack, num, parse_vfx};

/// An emitter that draws.
#[derive(Clone, Debug, PartialEq)]
pub struct ActiveEmitter {
    /// The library file, the reference's inst_name and the emitter id.
    pub library: String,
    pub instance: String,
    pub name: String,
    pub def: VfxEmitterDef,
    pub chain: VfxChain,
    pub k: EmitterConstants,
    pub tint_min: [f32; 4],
    pub tint_max: [f32; 4],
    pub particle_lifetime_bias: [f32; 2],
    pub emitter_lifetime_bias: [f32; 2],

    /// The middle of the reference's scale range (the weather uses it); a placed instance draws its own in `scale_range`.
    pub scale: f32,
    pub scale_range: [f32; 2],
    pub pos: [f32; 3],
    pub camera_following: bool,

    /// The reference's infinite_life (the emitter keeps emitting past its LifeTime).
    pub infinite_life: bool,
}

/// A point light an effect attaches: a rigid emitter (`burst_rigid` / `continuous_rigid`) whose RigidModelFile is one of
/// `rigidmodels/point_lights/` (a StaticPointLight mesh naming a `lighting/lights.xml` definition).
#[derive(Clone, Debug, PartialEq)]
pub struct EffectLight {
    pub instance: String,
    pub name: String,

    /// The rigid model (a VFS path under `rigidmodels/`).
    pub model: String,
    pub def: VfxEmitterDef,
    pub tint_min: [f32; 4],
    pub tint_max: [f32; 4],
    pub scale_range: [f32; 2],
    pub pos: [f32; 3],
}

/// The emitters and lights of one effect.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Effect {
    pub name: String,
    pub emitters: Vec<ActiveEmitter>,
    pub lights: Vec<EffectLight>,

    /// Skipped emitters and missing files.
    pub notes: Vec<String>,
}

/// `weather/weather_definitions.xml` WEATHER_DEFINITION. The defaults are the engine's constructor's; the comments give the field
/// offsets of the engine struct. The gust names of old .environment files (wind_gust_prob, _rampup, _max, _size_variety...) are
/// not read by the game.
#[derive(Clone, Debug, PartialEq)]
pub struct WeatherDefinition {
    pub name: String,

    /// +0x1c: WT_NONE 0, WT_RAIN 1, WT_SNOW 2 (-> weather_shared.g_weather_type).
    pub kind: String,
    pub vfx1: String,
    pub vfx2: String,

    /// +0x20.
    pub severity: i32,

    /// +0x3c, +0x40.
    pub wind_size_min: f32,
    pub wind_size_max: f32,

    /// +0x44.
    pub wind_gust_size_variation: f32,

    /// +0x48, +0x4c.
    pub wind_gust_duration: f32,
    pub wind_gust_duration_variation: f32,

    /// +0x50, +0x54.
    pub wind_gust_interval: f32,
    pub wind_gust_probability: f32,

    /// +0x58, +0x5c.
    pub wind_gust_curve: f32,
    pub wind_vegetation_multiplier: f32,
}

impl Default for WeatherDefinition {
    fn default() -> Self {
        Self {
            name: String::new(),
            kind: String::new(),
            vfx1: String::new(),
            vfx2: String::new(),
            severity: 0,
            wind_size_min: 0.0,
            wind_size_max: 1.0,
            wind_gust_size_variation: 0.5,
            wind_gust_duration: 2.0,
            wind_gust_duration_variation: 0.5,
            wind_gust_interval: 3.0,
            wind_gust_probability: 1.0,
            wind_gust_curve: 4.0,
            wind_vegetation_multiplier: 0.0,
        }
    }
}

impl WeatherDefinition {
    pub fn type_index(&self) -> i32 {
        match self.kind.as_str() {
            "WT_RAIN" => 1,
            "WT_SNOW" => 2,
            "WT_DUST" => 3,
            _ => 0,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WeatherVfx {
    pub definition: WeatherDefinition,
    pub emitters: Vec<ActiveEmitter>,

    /// Point lights of rigid emitters (the storms' lightning), camera-following like the rest.
    pub lights: Vec<EffectLight>,

    /// Skipped emitters and missing files, for the UI.
    pub notes: Vec<String>,
}

/// The bytes of an in-pack file. The editor passes one that prefers open (edited) documents.
pub type VfxReader<'a> = dyn Fn(&str) -> Result<Vec<u8>, String> + 'a;

fn find_definition(read: &VfxReader, name: &str) -> Result<Option<WeatherDefinition>, String> {
    let doc = XmlDoc::parse(read("weather/weather_definitions.xml")?).map_err(|error| error.to_string())?;
    let Some(i) = (0..doc.elements().len()).find(|&i| doc.elements()[i].name == "WEATHER_DEFINITION" && doc.get(i, "name").as_deref() == Some(name)) else {
        return Ok(None);
    };

    let d = WeatherDefinition::default();
    let n = |attr: &str, default: f32| num(doc.get(i, attr), default);
    let text = |attr: &str| doc.get(i, attr).unwrap_or_default();
    Ok(Some(WeatherDefinition {
        name: name.to_owned(),
        kind: text("type"),
        vfx1: text("vfx1"),
        vfx2: text("vfx2"),
        severity: n("severity", 0.0) as i32,
        wind_size_min: n("wind_size_min", d.wind_size_min),
        wind_size_max: n("wind_size_max", d.wind_size_max),
        wind_gust_size_variation: n("wind_gust_size_variation", d.wind_gust_size_variation),
        wind_gust_duration: n("wind_gust_duration", d.wind_gust_duration),
        wind_gust_duration_variation: n("wind_gust_duration_variation", d.wind_gust_duration_variation),
        wind_gust_interval: n("wind_gust_interval", d.wind_gust_interval),
        wind_gust_probability: n("wind_gust_probability", d.wind_gust_probability),
        wind_gust_curve: n("wind_gust_curve", d.wind_gust_curve),
        wind_vegetation_multiplier: n("wind_vegetation_multiplier", d.wind_vegetation_multiplier),
    }))
}

fn load_vfx(read: &VfxReader, name: &str) -> Result<VfxFile, String> {
    let doc = XmlDoc::parse(read(&format!("vfx/{name}.xml"))?).map_err(|error| error.to_string())?;
    Ok(parse_vfx(&doc))
}

/// The chain an emitter draws with: some render stack pairs it with a chain that has a texture.
fn render_chain<'a>(lib: &'a VfxFile, emitter: &str) -> Option<(&'a VfxStack, &'a VfxChain)> {
    lib.stacks.iter()
        .filter(|stack| stack.emitters.iter().any(|e| e == emitter))
        .flat_map(|stack| stack.chains.iter().filter_map(|c| lib.chain(c)).map(move |chain| (stack, chain)))
        .find(|(_, chain)| chain.find("ps_texture").is_some())
}

/// The effects of a weather definition (an empty name looks up `none`, like the engine's default). No vfx1 / vfx2 gives an
/// effect without emitters; files that can't be read become notes.
///
/// # Errors
///
/// Fails if `weather/weather_definitions.xml` can't be read.
pub fn load_weather_vfx(read: &VfxReader, definition_name: &str) -> Result<WeatherVfx, String> {
    let mut out = WeatherVfx::default();
    let Some(def) = find_definition(read, if definition_name.is_empty() { "none" } else { definition_name })? else {
        out.notes.push(format!("weather definition '{definition_name}' not in weather/weather_definitions.xml"));
        return Ok(out);
    };

    out.definition = def.clone();
    let mut libs = BTreeMap::<String, VfxFile>::new();

    // The engine plays both ("vfx 1", "vfx 2").
    for top_name in [&def.vfx1, &def.vfx2] {
        if top_name.is_empty() {
            continue;
        }

        let effect = load_effect_with(read, top_name, &mut libs);
        out.emitters.extend(effect.emitters);
        out.lights.extend(effect.lights);
        out.notes.extend(effect.notes);
    }

    Ok(out)
}

/// The emitters and lights of `vfx/<name>.xml`: its vfx_references' library emitters (enable, tint, lifetime overrides), or, for a file
/// without references, its own emitters.
pub fn load_effect(read: &VfxReader, name: &str) -> Effect {
    load_effect_with(read, name, &mut BTreeMap::new())
}

/// [`load_effect`] with a cache of the library files read so far.
pub fn load_effect_with(read: &VfxReader, name: &str, libs: &mut BTreeMap<String, VfxFile>) -> Effect {
    let mut out = Effect { name: name.to_owned(), ..Default::default() };
    let top = match load_vfx(read, name) {
        Ok(top) => top,
        Err(error) => {
            out.notes.push(format!("vfx/{name}.xml: {error}"));
            return out;
        }
    };

    // A file without references draws its own emitters, as one reference of itself.
    let own = VfxRef {
        inst_name: name.to_owned(), library: name.to_owned(), scale_min: 1.0, scale_max: 1.0, pos: [0.0; 3], start_time: 0.0, infinite_life: true,
        emitters: vec![],
    };

    if top.refs.is_empty() {
        libs.entry(name.to_owned()).or_insert_with(|| top.clone());
    }

    let refs = if top.refs.is_empty() { std::slice::from_ref(&own) } else { &top.refs[..] };
    for vfx_ref in refs {
        if !libs.contains_key(&vfx_ref.library) {
            match load_vfx(read, &vfx_ref.library) {
                Ok(lib) => {
                    libs.insert(vfx_ref.library.clone(), lib);
                }

                Err(error) => {
                    out.notes.push(format!("vfx/{}.xml: {error}", vfx_ref.library));
                    continue;
                }
            }
        }

        let lib = &libs[&vfx_ref.library];
        for em in &lib.emitters {
            let ov = vfx_ref.find(&em.id);
            if ov.is_some_and(|ov| !ov.enable) || !em.b("Enable", true) {
                continue;
            }

            let tint_min = ov.map_or([1.0; 4], |ov| ov.tint_min);
            let tint_max = ov.map_or([1.0; 4], |ov| ov.tint_max);
            if em.kind == "burst_rigid" || em.kind == "continuous_rigid" {
                let model = em.text("RigidModelFile").replace('\\', "/").to_lowercase();
                if model.starts_with("point_lights/") {
                    out.lights.push(EffectLight {
                        instance: vfx_ref.inst_name.clone(), name: em.id.clone(), model: format!("rigidmodels/{model}"), def: em.clone(), tint_min, tint_max,
                        scale_range: [vfx_ref.scale_min, vfx_ref.scale_max], pos: vfx_ref.pos,
                    });
                } else {
                    out.notes.push(format!("{}/{}: emitter type {} not supported (rigid model '{model}')", vfx_ref.inst_name, em.id, em.kind));
                }

                continue;
            }

            if em.kind != "continuous_mesh" {
                out.notes.push(format!("{}/{}: emitter type {} not supported", vfx_ref.inst_name, em.id, em.kind));
                continue;
            }

            // Sound and logic-only emitters draw nothing.
            let Some((stack, chain)) = render_chain(lib, &em.id) else { continue };
            if stack.scene_view_type > 1 {
                out.notes.push(format!("{}/{}: scene_view_type {} not supported", vfx_ref.inst_name, em.id, stack.scene_view_type));
                continue;
            }

            let mut k = build_emitter_constants(em, Some(chain));

            // Dump: heat haze rows have w = 12.
            if stack.scene_view_type == 1 {
                k.behaviour_mask[3] = VFX_DISTORTION_VIEW_ID;
            }

            out.emitters.push(ActiveEmitter {
                library: vfx_ref.library.clone(),
                instance: vfx_ref.inst_name.clone(),
                name: em.id.clone(),
                def: em.clone(),
                chain: chain.clone(),
                k,
                tint_min,
                tint_max,
                particle_lifetime_bias: ov.map_or([0.0; 2], |ov| ov.particle_lifetime_bias),
                emitter_lifetime_bias: ov.map_or([0.0; 2], |ov| ov.emitter_lifetime_bias),
                scale: 0.5 * (vfx_ref.scale_min + vfx_ref.scale_max),
                scale_range: [vfx_ref.scale_min, vfx_ref.scale_max],
                pos: vfx_ref.pos,
                camera_following: lib.camera_following() || top.camera_following(),
                infinite_life: vfx_ref.infinite_life,
            });
        }
    }

    out
}

/// [`load_weather_vfx`] reading from the packs.
///
/// # Errors
///
/// Fails if `weather/weather_definitions.xml` can't be read.
pub fn load_weather_vfx_from_vfs(vfs: &Vfs, definition_name: &str) -> Result<WeatherVfx, String> {
    load_weather_vfx(&|path| vfs.read(path).map_err(|error| error.to_string()), definition_name)
}
