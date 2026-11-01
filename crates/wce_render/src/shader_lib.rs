//! Game shaders, loaded from the packs' fxc/ folder and bound by their reflected names.

use std::collections::BTreeMap;
use std::ffi::{CStr, c_void};
use std::io::Cursor;
use std::rc::Rc;

use rpfm_lib::files::{Decodeable, fxc::Fxc};
use wce_d3d11::*;
use windows_core::{Interface, PCSTR};

use crate::RenderError;
use crate::cbuffers::{ConstantBuffers, Var};

/// The bytes of an in-pack file.
pub type FileReader<'a> = dyn Fn(&str) -> Result<Vec<u8>, String> + 'a;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Stage {
    Vs,
    #[default]
    Ps,
    Gs,
    Cs,
    Hs,
    Ds,
}

/// A created shader of the game, with the slots its reflection gives each resource name.
#[derive(Clone, Debug, Default)]
pub struct GameShader {
    /// The fxc file name without extension.
    pub name: String,
    pub stage: Stage,
    pub bytecode: Vec<u8>,
    pub vs: Option<ID3D11VertexShader>,
    pub ps: Option<ID3D11PixelShader>,
    pub gs: Option<ID3D11GeometryShader>,
    pub cs: Option<ID3D11ComputeShader>,
    pub hs: Option<ID3D11HullShader>,
    pub ds: Option<ID3D11DomainShader>,
    pub cbuffers: BTreeMap<String, u32>,
    pub textures: BTreeMap<String, u32>,
    pub samplers: BTreeMap<String, u32>,
    pub uavs: BTreeMap<String, u32>,
}

/// D3D11_SHVER_GET_TYPE.
fn shader_version_type(version: u32) -> u32 {
    (version >> 16) & 0xFFFF
}

fn c_name(name: PCSTR) -> String {
    if name.is_null() {
        return String::new();
    }

    // SAFETY: reflection names are NUL-terminated strings owned by the reflection object, alive while it is.
    unsafe { CStr::from_ptr(name.0.cast()).to_string_lossy().into_owned() }
}

/// The game's shaders, created on first use and kept.
#[derive(Debug)]
pub struct ShaderLibrary {
    dev: ID3D11Device,
    cache: BTreeMap<String, Rc<GameShader>>,
    layouts: BTreeMap<String, ID3D11InputLayout>,
}

impl ShaderLibrary {

    pub fn new(dev: &ID3D11Device) -> Self {
        Self { dev: dev.clone(), cache: BTreeMap::new(), layouts: BTreeMap::new() }
    }

    /// A shader by fxc file name without folder and extension (`dx11_sm_3_0_skydome.hlsl_ps_ps30_skygen`), declaring its constant
    /// buffers in `cbs`.
    ///
    /// # Errors
    ///
    /// Fails if the file can't be read or decoded, reflected, or created.
    pub fn get(&mut self, read: &FileReader, cbs: &mut ConstantBuffers, name: &str) -> Result<Rc<GameShader>, RenderError> {
        if let Some(shader) = self.cache.get(name) {
            return Ok(shader.clone());
        }

        let bytes = read(&format!("fxc/{name}.fxc")).map_err(RenderError)?;
        let fxc = Fxc::decode(&mut Cursor::new(&bytes), &None).map_err(|error| RenderError(format!("{name}: {error}")))?;
        let mut s = GameShader { name: name.to_owned(), bytecode: fxc.shader().clone(), ..Default::default() };

        // SAFETY: the reflection reads the bytecode, alive during the block; descriptions are locals.
        unsafe {
            let mut raw: *mut c_void = std::ptr::null_mut();
            D3DReflect(s.bytecode.as_ptr().cast(), s.bytecode.len(), &ID3D11ShaderReflection::IID, &mut raw)
                .ok()
                .map_err(|_| RenderError(format!("reflect failed: {name}")))?;
            let refl = ID3D11ShaderReflection::from_raw(raw);
            let mut sd = D3D11_SHADER_DESC::default();
            refl.GetDesc(&mut sd).ok()?;

            let code = s.bytecode.as_slice();
            let created = match shader_version_type(sd.Version) {
                1 => self.dev.create_vertex_shader(code).map(|vs| {
                    s.vs = Some(vs);
                    Stage::Vs
                }),

                0 => self.dev.create_pixel_shader(code).map(|ps| {
                    s.ps = Some(ps);
                    Stage::Ps
                }),

                2 => self.dev.create_geometry_shader(code).map(|gs| {
                    s.gs = Some(gs);
                    Stage::Gs
                }),

                5 => self.dev.create_compute_shader(code).map(|cs| {
                    s.cs = Some(cs);
                    Stage::Cs
                }),

                3 => self.dev.create_hull_shader(code).map(|hs| {
                    s.hs = Some(hs);
                    Stage::Hs
                }),

                4 => self.dev.create_domain_shader(code).map(|ds| {
                    s.ds = Some(ds);
                    Stage::Ds
                }),

                _ => return Err(RenderError(format!("unsupported shader stage: {name}"))),
            };

            s.stage = created.map_err(|_| RenderError(format!("create failed: {name}")))?;

            for r in 0..sd.BoundResources {
                let mut bd = D3D11_SHADER_INPUT_BIND_DESC::default();
                refl.GetResourceBindingDesc(r, &mut bd).ok()?;
                let target = match bd.Type {
                    D3D_SIT_CBUFFER => &mut s.cbuffers,
                    D3D_SIT_TEXTURE | D3D_SIT_STRUCTURED | D3D_SIT_BYTEADDRESS => &mut s.textures,
                    D3D_SIT_SAMPLER => &mut s.samplers,
                    _ => &mut s.uavs,
                };

                target.insert(c_name(bd.Name), bd.BindPoint);
            }

            for c in 0..sd.ConstantBuffers {
                let Some(cb) = refl.GetConstantBufferByIndex(c) else { continue };
                let mut bd = D3D11_SHADER_BUFFER_DESC::default();
                cb.GetDesc(&mut bd).ok()?;
                if bd.Type != D3D_CT_CBUFFER {
                    continue;
                }

                let mut vars = BTreeMap::new();
                for v in 0..bd.Variables {
                    let Some(variable) = cb.GetVariableByIndex(v) else { continue };
                    let mut vd = D3D11_SHADER_VARIABLE_DESC::default();
                    variable.GetDesc(&mut vd).ok()?;
                    vars.insert(c_name(vd.Name), Var { offset: vd.StartOffset, size: vd.Size });
                }

                cbs.declare(&c_name(bd.Name), bd.Size, &vars);
            }
        }

        let shader = Rc::new(s);
        self.cache.insert(name.to_owned(), shader.clone());
        Ok(shader)
    }

    /// The input layout of a vertex shader, cached per shader and layout id.
    ///
    /// # Errors
    ///
    /// Fails if the device refuses the layout.
    pub fn layout(&mut self, vs: &GameShader, id: &str, elems: &[InputElement]) -> Result<ID3D11InputLayout, RenderError> {
        let key = format!("{}|{id}", vs.name);
        if let Some(layout) = self.layouts.get(&key) {
            return Ok(layout.clone());
        }

        let layout = self.dev.create_input_layout(elems, &vs.bytecode).map_err(|_| RenderError(format!("input layout failed: {key}")))?;
        self.layouts.insert(key, layout.clone());
        Ok(layout)
    }
}

/// Binds a shader and its constant buffers. Textures and samplers are bound by name by the caller.
pub fn bind_shader(ctx: &ID3D11DeviceContext, s: &GameShader, cbs: &ConstantBuffers) {
    match s.stage {
        Stage::Vs => ctx.vs_set_shader(s.vs.as_ref()),
        Stage::Ps => ctx.ps_set_shader(s.ps.as_ref()),
        Stage::Gs => ctx.gs_set_shader(s.gs.as_ref()),
        Stage::Cs => ctx.cs_set_shader(s.cs.as_ref()),
        Stage::Hs => ctx.hs_set_shader(s.hs.as_ref()),
        Stage::Ds => ctx.ds_set_shader(s.ds.as_ref()),
    }

    for (name, &slot) in &s.cbuffers {
        let Some(buffer) = cbs.find(name).and_then(|b| b.gpu.clone()) else { continue };
        let buffers = [Some(buffer)];
        match s.stage {
            Stage::Vs => ctx.vs_set_constant_buffers(slot, &buffers),
            Stage::Ps => ctx.ps_set_constant_buffers(slot, &buffers),
            Stage::Gs => ctx.gs_set_constant_buffers(slot, &buffers),
            Stage::Cs => ctx.cs_set_constant_buffers(slot, &buffers),
            Stage::Hs => ctx.hs_set_constant_buffers(slot, &buffers),
            Stage::Ds => ctx.ds_set_constant_buffers(slot, &buffers),
        }
    }
}

/// Binds a texture to the slot a shader gives its name; nothing if the shader doesn't use it.
pub fn bind_srv(ctx: &ID3D11DeviceContext, s: &GameShader, name: &str, srv: Option<&ID3D11ShaderResourceView>) {
    let Some(&slot) = s.textures.get(name) else { return };
    let views = [srv.cloned()];
    match s.stage {
        Stage::Vs => ctx.vs_set_shader_resources(slot, &views),
        Stage::Ps => ctx.ps_set_shader_resources(slot, &views),
        Stage::Gs => ctx.gs_set_shader_resources(slot, &views),
        Stage::Cs => ctx.cs_set_shader_resources(slot, &views),
        Stage::Hs => ctx.hs_set_shader_resources(slot, &views),
        Stage::Ds => ctx.ds_set_shader_resources(slot, &views),
    }
}

/// Binds a sampler to the slot a shader gives its name; nothing if the shader doesn't use it.
pub fn bind_sampler(ctx: &ID3D11DeviceContext, s: &GameShader, name: &str, smp: Option<&ID3D11SamplerState>) {
    let Some(&slot) = s.samplers.get(name) else { return };
    let samplers = [smp.cloned()];
    match s.stage {
        Stage::Vs => ctx.vs_set_samplers(slot, &samplers),
        Stage::Ps => ctx.ps_set_samplers(slot, &samplers),
        Stage::Gs => ctx.gs_set_samplers(slot, &samplers),
        Stage::Cs => ctx.cs_set_samplers(slot, &samplers),
        Stage::Hs => ctx.hs_set_samplers(slot, &samplers),
        Stage::Ds => ctx.ds_set_samplers(slot, &samplers),
    }
}
