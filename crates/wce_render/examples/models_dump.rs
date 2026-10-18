//! Differential dump of the model cache, for comparing with the C++ code on the same device: every model's GPU buffers read back, its
//! LODs, bounds and textures, and the terrain patches of synthetic grids (plain, with LODs, skinned, edited).
//!
//!   models_dump <file with one model path per line> <folder the paths are under>

use std::path::PathBuf;
use std::sync::Arc;

use wce_d3d11::*;
use wce_foundation::math::Float3;
use wce_render::model_cache::{GpuMesh, GpuModel, ModelCache, SharedReader, SharedTexture, TEX_NORMAL, TextureCache};
use wce_scene::render_scene::{Terrain, TerrainSkin};

fn b(f: f32) -> String {
    format!("{:08x}", f.to_bits())
}

fn v3(v: Float3) -> String {
    format!("{},{},{}", b(v.x), b(v.y), b(v.z))
}

struct Gpu {
    dev: ID3D11Device,
    ctx: ID3D11DeviceContext,
    textures: Arc<TextureCache>,
}

impl Gpu {
    fn buf(&self, buffer: Option<&ID3D11Buffer>) -> String {
        let Some(buffer) = buffer else { return "none".to_owned() };
        // SAFETY: a staging copy of a buffer of this device, mapped and unmapped here.
        unsafe {
            let mut d = D3D11_BUFFER_DESC::default();
            buffer.GetDesc(&mut d);
            let s = D3D11_BUFFER_DESC { ByteWidth: d.ByteWidth, Usage: D3D11_USAGE_STAGING, BindFlags: 0, CPUAccessFlags: D3D11_CPU_ACCESS_READ as u32, MiscFlags: 0, StructureByteStride: 0 };
            let mut st = None;
            if self.dev.CreateBuffer(&s, None, Some(&mut st)).is_err() {
                return "nostaging".to_owned();
            }

            let st = st.unwrap();
            self.ctx.CopyResource(&st, buffer);
            let mut ms = D3D11_MAPPED_SUBRESOURCE::default();
            if self.ctx.Map(&st, 0, D3D11_MAP_READ, 0, Some(&mut ms)).is_err() {
                return "nomap".to_owned();
            }

            let bytes = std::slice::from_raw_parts(ms.pData.cast::<u8>(), d.ByteWidth as usize);
            let h = bytes.iter().fold(0xCBF2_9CE4_8422_2325u64, |h, &c| (h ^ u64::from(c)).wrapping_mul(0x0100_0000_01B3));
            self.ctx.Unmap(&st, 0);
            format!("{}/{h:016x}", d.ByteWidth)
        }
    }

    fn tex(&self, t: &SharedTexture) -> String {
        if Arc::ptr_eq(t, &self.textures.white()) {
            return "W".to_owned();
        }

        if Arc::ptr_eq(t, &self.textures.black()) {
            return "B".to_owned();
        }

        if Arc::ptr_eq(t, &self.textures.flat_normal()) {
            return "N".to_owned();
        }

        let t = t.read().unwrap();
        format!("{}x{}m{}f{}", t.width, t.height, t.mips, t.format)
    }

    fn mesh(&self, m: &GpuMesh) {
        // The standard material's five slots: the C++ cache has no others.
        let tex = (0..=TEX_NORMAL).map(|k| format!("{},", self.tex(&m.tex[k]))).collect::<String>();
        println!("    mesh n{} a{} off={}/{} b={}..{} vb={} ib={} tex={tex}", m.index_count, m.alpha_mode, v3(m.offset), i32::from(m.has_offset), v3(m.bmin), v3(m.bmax),
            self.buf(m.vb.as_ref()), self.buf(m.ib.as_ref()));
    }

    fn model(&self, g: &GpuModel) {
        println!("  model {} d={} b={}..{} lods={}", g.path, b(g.distance), v3(g.bmin), v3(g.bmax), g.lods.len());
        for m in &g.meshes {
            self.mesh(m);
        }

        for l in &g.lods {
            println!("   lod d={}", b(l.distance));
            for m in &l.meshes {
                self.mesh(m);
            }
        }
    }
}

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    // SAFETY: no other thread exists yet.
    unsafe { init_platform() };
    let (mut dev, mut ctx) = (None, None);
    // SAFETY: device creation with valid arguments.
    unsafe {
        D3D11CreateDevice(None, D3D_DRIVER_TYPE_HARDWARE, HMODULE::default(), 0, None, D3D11_SDK_VERSION as u32, Some(&mut dev), None, Some(&mut ctx)).unwrap();
    }

    let (dev, ctx) = (dev.unwrap(), ctx.unwrap());
    let root = PathBuf::from(&args[2]);
    let read: SharedReader = Arc::new(move |path: &str| {
        let p = root.join(wce_vfs::normalize(path));
        if p.exists() { std::fs::read(&p).map_err(|error| error.to_string()) } else { Err(format!("missing {path}")) }
    });

    let textures = Arc::new(TextureCache::new(&dev, read.clone()));
    let gpu = Gpu { dev: dev.clone(), ctx, textures: textures.clone() };
    let mut models = ModelCache::new(&dev, read, textures.clone());
    for path in std::fs::read_to_string(&args[1]).unwrap().lines() {
        println!("== {path}");
        match models.model(path) {
            Some(g) => gpu.model(&g),
            None => println!("  none"),
        }
    }

    for k in 0..3u32 {
        let mut t = Terrain { nx: 200, nz: 150, origin_x: -100.0, origin_z: -75.0, step: 1.5, lod: k > 0, version: 1, ..Default::default() };
        t.diffuse = "terrain/textures/battle/default/grass/0_su_diffuse.dds".to_owned();
        t.heights = (0..t.nz).flat_map(|j| (0..t.nx).map(move |i| (i as f32 * 0.1).sin() * (j as f32 * 0.07).cos() * 20.0 + i as f32 * 0.01)).collect();
        if k == 2 {
            let rgba = (0..160 * 120 * 4).map(|i: usize| (i * 7 + i / 640) as u8).collect();
            t.skin = Some(Arc::new(TerrainSkin { x0: -110.0, z0: -80.0, w_m: 330.0, h_m: 240.0, w: 160, h: 120, rgba, id: 7 }));
        }

        let tg = models.terrain(10 + k, &t).unwrap();
        println!("== terrain {k} {}x{}", tg.patches_x, tg.patches_z);
        for p in &tg.patches {
            gpu.model(p);
        }

        if k == 1 {
            let nx = t.nx as usize;
            for i in 70..90 {
                t.heights[40 * nx + i] += 3.0;
            }

            t.version = 2;
            let tg = models.terrain(10 + k, &t).unwrap();
            println!("== terrain edited");
            for p in &tg.patches {
                gpu.model(p);
            }
        }
    }

    for e in models.errors() {
        println!("model error {e}");
    }

    for e in textures.errors() {
        println!("texture error {e}");
    }

    println!("loaded {}", textures.loaded());
}
