//! The game shaders' constant buffers.
//!
//! The engine keeps one object per named cbuffer (camera_VS_PS, lighting_VS_PS, fog_VS_PS...) shared by every shader that declares
//! it. [`ConstantBuffers`] mirrors that: values are set by variable name, uploaded when dirty, and bound to whatever slot each
//! shader's reflection asks for.

use std::collections::BTreeMap;

use wce_d3d11::{D3D11_BIND_CONSTANT_BUFFER, D3D11_BUFFER_DESC, D3D11_USAGE_DEFAULT, ID3D11Buffer, ID3D11Device, ID3D11DeviceContext};

/// A variable of a buffer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Var {
    pub offset: u32,
    pub size: u32,
}

/// A named buffer: its CPU copy, its variables and its GPU buffer.
#[derive(Clone, Debug, Default)]
pub struct Buffer {
    pub name: String,
    pub cpu: Vec<u8>,
    pub vars: BTreeMap<String, Var>,
    pub gpu: Option<ID3D11Buffer>,
    pub dirty: bool,
}

#[derive(Clone, Debug, Default)]
pub struct ConstantBuffers {
    buffers: BTreeMap<String, Buffer>,
    missing: Vec<String>,
}

impl ConstantBuffers {

    /// Declares (or widens) a buffer from a shader's reflection. Variables already declared keep their first layout.
    pub fn declare(&mut self, name: &str, size: u32, vars: &BTreeMap<String, Var>) {
        let b = self.buffers.entry(name.to_owned()).or_insert_with(|| Buffer { name: name.to_owned(), dirty: true, ..Default::default() });
        let aligned = ((size + 15) & !15) as usize;
        if b.cpu.len() < aligned {
            b.cpu.resize(aligned, 0);
            b.gpu = None;
            b.dirty = true;
        }

        for (k, v) in vars {
            b.vars.entry(k.clone()).or_insert(*v);
        }
    }

    pub fn has(&self, cb: &str, var: &str) -> bool {
        self.buffers.get(cb).is_some_and(|b| b.vars.contains_key(var))
    }

    /// Writes bytes into a variable, clamped to its size. Unknown names are ignored (and reported once through [`Self::missing`]),
    /// so a pass can set everything it knows about.
    pub fn set(&mut self, cb: &str, var: &str, data: &[u8]) {
        let Some((b, v)) = self.buffers.get_mut(cb).and_then(|b| b.vars.get(var).copied().map(|v| (b, v))) else {
            let key = format!("{cb}.{var}");
            if !self.missing.contains(&key) {
                self.missing.push(key);
            }

            return;
        };

        let n = data.len().min(v.size as usize);
        let dst = &mut b.cpu[v.offset as usize..v.offset as usize + n];
        if dst != &data[..n] {
            dst.copy_from_slice(&data[..n]);
            b.dirty = true;
        }
    }

    pub fn set_f(&mut self, cb: &str, var: &str, v: &[f32]) {
        self.set(cb, var, &v.iter().flat_map(|f| f.to_le_bytes()).collect::<Vec<_>>());
    }

    pub fn set_i(&mut self, cb: &str, var: &str, v: i32) {
        self.set(cb, var, &v.to_le_bytes());
    }

    /// A matrix, row-major as it is.
    pub fn set_m(&mut self, cb: &str, var: &str, m: &[f32; 16]) {
        self.set_f(cb, var, m);
    }

    pub fn find(&self, cb: &str) -> Option<&Buffer> {
        self.buffers.get(cb)
    }

    pub fn find_mut(&mut self, cb: &str) -> Option<&mut Buffer> {
        self.buffers.get_mut(cb)
    }

    /// Creates the GPU buffers that don't exist yet and uploads the dirty ones.
    ///
    /// # Errors
    ///
    /// Fails if a buffer can't be created.
    pub fn upload(&mut self, dev: &ID3D11Device, ctx: &ID3D11DeviceContext) -> windows_core::Result<()> {
        for b in self.buffers.values_mut() {
            if b.gpu.is_none() {
                let desc = D3D11_BUFFER_DESC {
                    ByteWidth: b.cpu.len() as u32,
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: D3D11_BIND_CONSTANT_BUFFER as u32,
                    CPUAccessFlags: 0,
                    MiscFlags: 0,
                    StructureByteStride: 0,
                };

                // SAFETY: the description is valid and the output is a local.
                unsafe { dev.CreateBuffer(&desc, None, Some(&mut b.gpu)).ok()? };
                b.dirty = true;
            }

            if b.dirty
                && let Some(gpu) = &b.gpu
            {
                // SAFETY: the CPU copy has the buffer's size.
                unsafe { ctx.UpdateSubresource(gpu, 0, None, b.cpu.as_ptr().cast(), 0, 0) };
                b.dirty = false;
            }
        }

        Ok(())
    }

    /// The `buffer.variable` names set that no shader declared.
    pub fn missing(&self) -> &[String] {
        &self.missing
    }

    pub fn all(&self) -> &BTreeMap<String, Buffer> {
        &self.buffers
    }
}
