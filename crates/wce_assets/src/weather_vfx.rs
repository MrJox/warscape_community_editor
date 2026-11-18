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

    /// Its library's or the effect's `affected_by_wind`: the emitter moves with the VFX wind (RenderDoc, camp_attila: the storm and
    /// rain clouds' `library_campaign_rainclouds_base` moved by `wind_velocity` x dt a frame, fn 0x11896b80).
    pub affected_by_wind: bool,

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

    /// The rigid emitters whose model is not a light: debris, pebbles, shards (drawn as models by the particle system).
    pub rigids: Vec<EffectLight>,

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
    let Some(i) =
        (0..doc.elements().len()).find(|&i| doc.elements()[i].name == "WEATHER_DEFINITION" && doc.get(i, "name").as_deref() == Some(name))
    else {
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
    lib.stacks
        .iter()
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
        out.notes
            .extend(effect.rigids.iter().map(|r| format!("{}/{}: rigid model '{}' not drawn in weather", r.instance, r.name, r.model)));
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
        inst_name: name.to_owned(),
        library: name.to_owned(),
        scale_min: 1.0,
        scale_max: 1.0,
        pos: [0.0; 3],
        start_time: 0.0,
        infinite_life: true,
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
                        instance: vfx_ref.inst_name.clone(),
                        name: em.id.clone(),
                        model: format!("rigidmodels/{model}"),
                        def: em.clone(),
                        tint_min,
                        tint_max,
                        scale_range: [vfx_ref.scale_min, vfx_ref.scale_max],
                        pos: vfx_ref.pos,
                    });
                } else {
                    out.rigids.push(EffectLight {
                        instance: vfx_ref.inst_name.clone(),
                        name: em.id.clone(),
                        model: format!("rigidmodels/{model}"),
                        def: em.clone(),
                        tint_min,
                        tint_max,
                        scale_range: [vfx_ref.scale_min, vfx_ref.scale_max],
                        pos: vfx_ref.pos,
                    });
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
                affected_by_wind: lib.affected_by_wind() || top.affected_by_wind(),
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

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;
    use crate::vfx_constants::VFX_MAIN_VIEW_ID;

    const DEFINITIONS: &str = r#"<root>
  <WEATHER_DEFINITION name="none"/>
  <WEATHER_DEFINITION name="storm" type="WT_SNOW" vfx1="top" vfx2="own" severity="-1.9" wind_size_min="0.5" wind_gust_probability="x"/>
  <WEATHER_DEFINITION name="storm" type="WT_RAIN"/>
  <WEATHER_DEFINITION name="bad" vfx1="broken" vfx2="missing"/>
  <OTHER name="other"/>
</root>"#;

    const TOP: &str = r#"<vfx camera_following="true"/>
<vfx_references>
  <vfx inst_name="i1" vfx_ref="lib" scale_min="1" scale_max="3" pos_x="2">
    <emitter name="drops" tint_r="0.5" particle_lifetime_bias_min="0.1"/>
    <emitter name="off" enable="false"/>
  </vfx>
  <vfx inst_name="i2" vfx_ref="lib" infinite_life="0"/>
</vfx_references>"#;

    const LIB: &str = r#"<vfx>
  <emitters>
    <emitter id="drops" type="continuous_mesh"/>
    <emitter id="off" type="continuous_mesh"/>
    <emitter id="disabled" type="continuous_mesh"><property name="Enable" value="false"/></emitter>
    <emitter id="light" type="burst_rigid"><property name="RigidModelFile" value="Point_Lights\Torch.rigid_model_v2"/></emitter>
    <emitter id="debris" type="continuous_rigid"><property name="RigidModelFile" value="rocks/rock.rigid_model_v2"/></emitter>
    <emitter id="sound" type="continuous_mesh"/>
    <emitter id="lonely" type="continuous_mesh"/>
    <emitter id="burst" type="burst_mesh"/>
    <emitter id="haze" type="continuous_mesh"/>
    <emitter id="overlay" type="continuous_mesh"/>
  </emitters>
  <behaviour_chains>
    <behaviour_chain id="logic"><modifier fragment="vs_rotation"/></behaviour_chain>
    <behaviour_chain id="render"><modifier fragment="ps_texture"><texture_property name="diffuse" path="drop.dds"/></modifier></behaviour_chain>
  </behaviour_chains>
  <effect_render_stacks>
    <effect_render_stack id="sound_only"><emitters_selected><e id="sound"/></emitters_selected>
      <behaviour_chains_selected><c id="logic"/><c id="missing_chain"/></behaviour_chains_selected></effect_render_stack>
    <effect_render_stack id="main"><emitters_selected><e id="drops"/><e id="off"/><e id="burst"/></emitters_selected>
      <behaviour_chains_selected><c id="logic"/><c id="render"/></behaviour_chains_selected></effect_render_stack>
    <effect_render_stack id="distort"><emitters_selected><e id="haze"/></emitters_selected>
      <behaviour_chains_selected><c id="render"/></behaviour_chains_selected><render_params scene_view_type="1"/></effect_render_stack>
    <effect_render_stack id="ui"><emitters_selected><e id="overlay"/></emitters_selected>
      <behaviour_chains_selected><c id="render"/></behaviour_chains_selected><render_params scene_view_type="2"/></effect_render_stack>
  </effect_render_stacks>
</vfx>"#;

    const OWN: &str = r#"<vfx camera_following="false">
  <emitters><emitter id="self" type="continuous_mesh"/></emitters>
  <behaviour_chains><behaviour_chain id="r"><modifier fragment="ps_texture"/></behaviour_chain></behaviour_chains>
  <effect_render_stacks><effect_render_stack id="s"><emitters_selected><e id="self"/></emitters_selected>
    <behaviour_chains_selected><c id="r"/></behaviour_chains_selected></effect_render_stack></effect_render_stacks>
</vfx>"#;

    /// A reader of the given files that logs the paths it is asked for.
    struct Files {
        files: BTreeMap<&'static str, Vec<u8>>,
        reads: RefCell<Vec<String>>,
    }

    impl Files {
        fn new(files: &[(&'static str, &str)]) -> Self {
            Self { files: files.iter().map(|(path, text)| (*path, text.as_bytes().to_vec())).collect(), reads: RefCell::default() }
        }

        fn read(&self, path: &str) -> Result<Vec<u8>, String> {
            self.reads.borrow_mut().push(path.to_owned());
            self.files.get(path).cloned().ok_or_else(|| format!("missing {path}"))
        }
    }

    fn standard() -> Files {
        Files::new(&[
            ("weather/weather_definitions.xml", DEFINITIONS),
            ("vfx/top.xml", TOP),
            ("vfx/lib.xml", LIB),
            ("vfx/own.xml", OWN),
            ("vfx/broken.xml", "<vfx><emitters></nope>"),
        ])
    }

    fn utf16(text: &str) -> Vec<u8> {
        let mut out = vec![0xff, 0xfe];
        out.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        out
    }

    #[test]
    fn definition_defaults_and_types() {
        let d = WeatherDefinition::default();
        assert_eq!((d.severity, d.wind_size_min, d.wind_size_max, d.wind_gust_curve, d.wind_gust_probability), (0, 0.0, 1.0, 4.0, 1.0));
        for (kind, index) in [("WT_RAIN", 1), ("WT_SNOW", 2), ("WT_DUST", 3), ("WT_NONE", 0), ("", 0), ("wt_rain", 0)] {
            assert_eq!(WeatherDefinition { kind: kind.into(), ..Default::default() }.type_index(), index, "{kind}");
        }
    }

    #[test]
    fn a_weather_plays_both_effects_with_their_overrides() {
        let files = standard();
        let w = load_weather_vfx(&|path| files.read(path), "storm").unwrap();

        // The first definition of a name wins; numbers read as atof.
        let d = &w.definition;
        assert_eq!((d.name.as_str(), d.type_index(), d.vfx1.as_str(), d.vfx2.as_str()), ("storm", 2, "top", "own"));
        assert_eq!((d.severity, d.wind_size_min, d.wind_size_max, d.wind_gust_probability), (-1, 0.5, 1.0, 0.0));

        let names: Vec<String> = w.emitters.iter().map(|e| format!("{}/{}", e.instance, e.name)).collect();
        assert_eq!(names, ["i1/drops", "i1/haze", "i2/drops", "i2/off", "i2/haze", "own/self"]);
        let lights: Vec<&str> = w.lights.iter().map(|l| l.model.as_str()).collect();
        assert_eq!(lights, ["rigidmodels/point_lights/torch.rigid_model_v2"; 2]);
        assert_eq!(
            w.notes,
            [
                "i1/debris: rigid model 'rigidmodels/rocks/rock.rigid_model_v2' not drawn in weather",
                "i2/debris: rigid model 'rigidmodels/rocks/rock.rigid_model_v2' not drawn in weather",
                "i1/burst: emitter type burst_mesh not supported",
                "i1/overlay: scene_view_type 2 not supported",
                "i2/burst: emitter type burst_mesh not supported",
                "i2/overlay: scene_view_type 2 not supported",
            ]
        );

        // The overrides of the reference, the reference's placement, and the library read once.
        let drops = &w.emitters[0];
        assert_eq!((drops.library.as_str(), drops.tint_min, drops.tint_max), ("lib", [0.5, 1.0, 1.0, 1.0], [0.5, 1.0, 1.0, 1.0]));
        assert_eq!(
            (drops.particle_lifetime_bias, drops.scale, drops.scale_range, drops.pos),
            ([0.1, 0.0], 2.0, [1.0, 3.0], [2.0, 0.0, 0.0])
        );
        assert!(drops.camera_following && drops.infinite_life);
        assert_eq!((drops.chain.id.as_str(), drops.k.diffuse.as_str()), ("render", "vfx/textures/drop.dds"));
        assert_eq!((drops.k.behaviour_mask[3], w.emitters[1].k.behaviour_mask[3]), (VFX_MAIN_VIEW_ID, VFX_DISTORTION_VIEW_ID));
        assert_eq!((w.emitters[2].tint_min, w.emitters[2].infinite_life), ([1.0; 4], false));
        let own = &w.emitters[5];
        assert_eq!((own.library.as_str(), own.scale, own.camera_following, own.infinite_life), ("own", 1.0, false, true));
        assert_eq!(*files.reads.borrow(), ["weather/weather_definitions.xml", "vfx/top.xml", "vfx/lib.xml", "vfx/own.xml"]);
    }

    #[test]
    fn missing_and_broken_files_become_notes() {
        let files = standard();
        let read = |path: &str| files.read(path);
        let w = load_weather_vfx(&read, "bad").unwrap();
        assert!(w.emitters.is_empty() && w.lights.is_empty());
        assert_eq!(w.notes.len(), 2);
        assert!(w.notes[0].starts_with("vfx/broken.xml: "), "{}", w.notes[0]);
        assert_eq!(w.notes[1], "vfx/missing.xml: missing vfx/missing.xml");

        // A reference to a library that is missing or broken is skipped; the next one still draws.
        let files = Files::new(&[
            (
                "vfx/top.xml",
                "<vfx_references><vfx inst_name=\"a\" vfx_ref=\"gone\"/><vfx inst_name=\"b\" vfx_ref=\"broken\"/><vfx inst_name=\"c\" vfx_ref=\"own\"/></vfx_references>",
            ),
            ("vfx/broken.xml", "<vfx"),
            ("vfx/own.xml", OWN),
        ]);
        let e = load_effect(&|path| files.read(path), "top");
        assert_eq!((e.name.as_str(), e.emitters.len(), e.emitters[0].instance.as_str()), ("top", 1, "c"));
        assert_eq!(e.notes.len(), 2);
        assert_eq!(e.notes[0], "vfx/gone.xml: missing vfx/gone.xml");
        assert!(e.notes[1].starts_with("vfx/broken.xml: "));

        let e = load_effect(&|path| files.read(path), "nothing");
        assert_eq!((e.emitters.len(), e.notes.as_slice()), (0, ["vfx/nothing.xml: missing vfx/nothing.xml".to_owned()].as_slice()));
    }

    #[test]
    fn unknown_and_empty_definition_names() {
        let files = standard();
        let read = |path: &str| files.read(path);
        let w = load_weather_vfx(&read, "nope").unwrap();
        assert_eq!(w.notes, ["weather definition 'nope' not in weather/weather_definitions.xml"]);
        assert_eq!(w.definition, WeatherDefinition::default());

        // An empty name is the engine's default, "none", which has no effects.
        let w = load_weather_vfx(&read, "").unwrap();
        assert_eq!((w.definition.name.as_str(), w.emitters.len(), w.notes.len()), ("none", 0, 0));

        // Only WEATHER_DEFINITION elements are definitions.
        assert_eq!(load_weather_vfx(&read, "other").unwrap().notes.len(), 1);
    }

    #[test]
    fn unreadable_definitions_are_an_error() {
        assert_eq!(load_weather_vfx(&|_: &str| Err("no packs".to_owned()), "storm").unwrap_err(), "no packs");
        for text in ["<root><WEATHER_DEFINITION name=\"x\"/></nope>", "<root", "</root>", "<a x=\"1></a>"] {
            let files = Files::new(&[("weather/weather_definitions.xml", text)]);
            assert!(load_weather_vfx(&|path| files.read(path), "x").is_err(), "{text}");
        }
        let odd = |_: &str| Ok(vec![0xff, 0xfe, b'<']);
        assert!(load_weather_vfx(&odd, "x").is_err());
    }

    #[test]
    fn utf16_files_read_as_utf8_ones() {
        let read = |path: &str| match path {
            "vfx/own.xml" => Ok(utf16(OWN)),
            "vfx/odd.xml" => Ok(vec![0xff, 0xfe, b'<', 0, b'v']),
            _ => Err(format!("missing {path}")),
        };
        let plain = Files::new(&[("vfx/own.xml", OWN)]);
        let from_utf16 = load_effect(&read, "own");
        assert_eq!(from_utf16, load_effect(&|path| plain.read(path), "own"));
        assert_eq!(from_utf16.emitters.len(), 1);

        let e = load_effect(&read, "odd");
        assert!(e.emitters.is_empty() && e.notes.len() == 1 && e.notes[0].starts_with("vfx/odd.xml: "));
    }

    #[test]
    fn effects_keep_their_rigids_and_lights() {
        let files = standard();
        let e = load_effect(&|path| files.read(path), "top");
        assert_eq!((e.lights.len(), e.rigids.len(), e.emitters.len()), (2, 2, 5));
        let light = &e.lights[0];
        assert_eq!(
            (light.instance.as_str(), light.name.as_str(), light.scale_range, light.pos),
            ("i1", "light", [1.0, 3.0], [2.0, 0.0, 0.0])
        );
        assert_eq!(e.rigids[1].model, "rigidmodels/rocks/rock.rigid_model_v2");
        assert_eq!(e.notes.len(), 4);

        // The library cache is shared between calls.
        let mut libs = BTreeMap::new();
        load_effect_with(&|path| files.read(path), "top", &mut libs);
        load_effect_with(&|path| files.read(path), "top", &mut libs);
        assert_eq!(files.reads.borrow().iter().filter(|p| *p == "vfx/lib.xml").count(), 2);
        assert!(libs.contains_key("lib") && !libs.contains_key("top"));
    }
}
