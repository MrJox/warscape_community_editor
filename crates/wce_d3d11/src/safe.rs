//! A safe layer over the D3D11 device and immediate context: extension traits on the generated interfaces, so code that holds an
//! `ID3D11Device` / `ID3D11DeviceContext` calls `ctx.set_render_targets(..)` instead of an `unsafe` `OMSetRenderTargets`.
//!
//! What makes the generated methods unsafe is the raw pointers they take, so this layer accepts slices and references instead and
//! checks what the runtime would read through them:
//!
//! - initial data and [`ContextExt::update_subresource`] sources must cover the region the runtime copies (its size comes from the
//!   description and the format);
//! - input element semantics can only be static C strings ([`InputElement`]);
//! - a mapped subresource is a byte slice of the mapped size, unmapped when the [`Mapped`] guard drops;
//! - query results are read into types for which any bytes are a valid value ([`QueryData`]).
//!
//! The rest the runtime validates itself: descriptions, bytecode (signed DXBC), and binding objects of another device (a debug layer
//! error, not memory unsafety). Out-of-range GPU reads in draws are defined in D3D11 (they return zero).

use std::ffi::CStr;
use std::ops::{Deref, DerefMut};

use windows_core::{HRESULT, Interface, PCSTR};

use crate::bindings::*;

const E_INVALIDARG: HRESULT = HRESULT(0x8007_0057_u32 as i32);

fn invalid<T>() -> windows_core::Result<T> {
    Err(E_INVALIDARG.into())
}

/// Takes the output of a creation call: the object, or the call's error.
fn created<T>(hr: HRESULT, out: Option<T>) -> windows_core::Result<T> {
    hr.ok()?;
    out.ok_or_else(|| windows_core::Error::from_hresult(E_INVALIDARG))
}

// ------------------------------------------------------------------------------------------------ formats

/// How a format lays out texels: blocks of `block` x `block` texels taking `bytes` bytes each (1 x 1 for the uncompressed formats).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormatLayout {
    pub block: u32,
    pub bytes: u32,
}

/// The layout of the formats the editor creates or loads; `None` for the others (no initial data or mapping is allowed for them).
pub fn format_layout(f: DXGI_FORMAT) -> Option<FormatLayout> {
    let (block, bytes) = match f {
        DXGI_FORMAT_R32G32B32A32_FLOAT | DXGI_FORMAT_R32G32B32A32_UINT | DXGI_FORMAT_R32G32B32A32_TYPELESS => (1, 16),
        DXGI_FORMAT_R32G32B32_FLOAT => (1, 12),
        DXGI_FORMAT_R16G16B16A16_FLOAT | DXGI_FORMAT_R16G16B16A16_UNORM | DXGI_FORMAT_R16G16B16A16_TYPELESS | DXGI_FORMAT_R32G32_FLOAT => (1, 8),
        DXGI_FORMAT_R8G8B8A8_UNORM | DXGI_FORMAT_R8G8B8A8_UNORM_SRGB | DXGI_FORMAT_R8G8B8A8_TYPELESS | DXGI_FORMAT_B8G8R8A8_UNORM
        | DXGI_FORMAT_B8G8R8A8_UNORM_SRGB | DXGI_FORMAT_B8G8R8A8_TYPELESS | DXGI_FORMAT_B8G8R8X8_UNORM | DXGI_FORMAT_B8G8R8X8_UNORM_SRGB
        | DXGI_FORMAT_B8G8R8X8_TYPELESS | DXGI_FORMAT_R10G10B10A2_UNORM | DXGI_FORMAT_R16G16_UNORM | DXGI_FORMAT_R16G16_FLOAT
        | DXGI_FORMAT_R32_FLOAT | DXGI_FORMAT_R32_UINT | DXGI_FORMAT_D32_FLOAT | DXGI_FORMAT_D24_UNORM_S8_UINT => (1, 4),
        DXGI_FORMAT_R8G8_UNORM | DXGI_FORMAT_R16_FLOAT | DXGI_FORMAT_R16_UINT | DXGI_FORMAT_R16_UNORM | DXGI_FORMAT_B5G6R5_UNORM
        | DXGI_FORMAT_B5G5R5A1_UNORM => (1, 2),
        DXGI_FORMAT_R8_UNORM | DXGI_FORMAT_A8_UNORM => (1, 1),
        DXGI_FORMAT_BC1_TYPELESS | DXGI_FORMAT_BC1_UNORM | DXGI_FORMAT_BC1_UNORM_SRGB | DXGI_FORMAT_BC4_TYPELESS | DXGI_FORMAT_BC4_UNORM
        | DXGI_FORMAT_BC4_SNORM => (4, 8),
        DXGI_FORMAT_BC2_TYPELESS | DXGI_FORMAT_BC2_UNORM | DXGI_FORMAT_BC2_UNORM_SRGB | DXGI_FORMAT_BC3_TYPELESS | DXGI_FORMAT_BC3_UNORM
        | DXGI_FORMAT_BC3_UNORM_SRGB | DXGI_FORMAT_BC5_TYPELESS | DXGI_FORMAT_BC5_UNORM | DXGI_FORMAT_BC5_SNORM | DXGI_FORMAT_BC6H_TYPELESS
        | DXGI_FORMAT_BC6H_UF16 | DXGI_FORMAT_BC6H_SF16 | DXGI_FORMAT_BC7_TYPELESS | DXGI_FORMAT_BC7_UNORM | DXGI_FORMAT_BC7_UNORM_SRGB => (4, 16),
        _ => return None,
    };

    Some(FormatLayout { block, bytes })
}

/// The bytes the runtime reads or writes for a `w` x `h` x `d` region with these pitches: every row of every slice, the last row and
/// slice without their padding. `None` for a format without a known layout or pitches too small for a row / slice.
fn region_bytes(format: DXGI_FORMAT, w: u32, h: u32, d: u32, row_pitch: usize, slice_pitch: usize) -> Option<usize> {
    let l = format_layout(format)?;
    let row_bytes = w.div_ceil(l.block).max(1) as usize * l.bytes as usize;
    let rows = h.div_ceil(l.block).max(1) as usize;
    if row_pitch < row_bytes {
        return None;
    }

    let slice_bytes = row_pitch * (rows - 1) + row_bytes;
    if d > 1 && slice_pitch < slice_bytes {
        return None;
    }

    Some(slice_pitch * (d.max(1) as usize - 1) + slice_bytes)
}

fn mip_dim(size: u32, mip: u32) -> u32 {
    size.checked_shr(mip).unwrap_or(0).max(1)
}

fn full_chain(w: u32, h: u32, d: u32) -> u32 {
    32 - w.max(h).max(d).max(1).leading_zeros()
}

// ------------------------------------------------------------------------------------------------ descriptions

/// Initial data of one subresource: its bytes and the distance between rows and between slices (volume textures).
#[derive(Clone, Copy, Debug)]
pub struct InitData<'a> {
    pub data: &'a [u8],
    pub row_pitch: u32,
    pub slice_pitch: u32,
}

impl<'a> InitData<'a> {
    pub fn new(data: &'a [u8], row_pitch: u32) -> Self {
        Self { data, row_pitch, slice_pitch: 0 }
    }

    fn raw(&self) -> D3D11_SUBRESOURCE_DATA {
        D3D11_SUBRESOURCE_DATA { pSysMem: self.data.as_ptr().cast(), SysMemPitch: self.row_pitch, SysMemSlicePitch: self.slice_pitch }
    }
}

/// An input element. Its semantic is a static string, so a layout can't point at freed memory.
#[derive(Clone, Copy, Debug)]
#[repr(transparent)]
pub struct InputElement(D3D11_INPUT_ELEMENT_DESC);

impl InputElement {
    pub fn new(semantic: &'static CStr, index: u32, format: DXGI_FORMAT, slot: u32, offset: u32, instanced: bool) -> Self {
        Self(D3D11_INPUT_ELEMENT_DESC {
            SemanticName: PCSTR(semantic.as_ptr().cast()),
            SemanticIndex: index,
            Format: format,
            InputSlot: slot,
            AlignedByteOffset: offset,
            InputSlotClass: if instanced { D3D11_INPUT_PER_INSTANCE_DATA } else { D3D11_INPUT_PER_VERTEX_DATA },
            InstanceDataStepRate: u32::from(instanced),
        })
    }

    pub fn desc(&self) -> &D3D11_INPUT_ELEMENT_DESC {
        &self.0
    }
}

/// Plain data a query writes: any bytes are a valid value.
///
/// # Safety
///
/// Implement it only for types without padding, references or invalid bit patterns.
pub unsafe trait QueryData: Copy + Default {}

// SAFETY: integers.
unsafe impl QueryData for u64 {}
// SAFETY: integers.
unsafe impl QueryData for u32 {}
// SAFETY: a u64 and a 32-bit BOOL: no invalid bit patterns, and padding only at the end, which the runtime writes over.
unsafe impl QueryData for D3D11_QUERY_DATA_TIMESTAMP_DISJOINT {}

// ------------------------------------------------------------------------------------------------ descriptions of objects

/// The descriptions of resources.
pub trait DescExt {
    type Desc;
    fn desc(&self) -> Self::Desc;
}

macro_rules! desc_ext {
    ($($ty:ty => $desc:ty),* $(,)?) => {$(
        impl DescExt for $ty {
            type Desc = $desc;
            fn desc(&self) -> $desc {
                let mut d = <$desc>::default();
                // SAFETY: the output is a local of the description's type.
                unsafe { self.GetDesc(&mut d) };
                d
            }
        }
    )*};
}

desc_ext!(ID3D11Buffer => D3D11_BUFFER_DESC, ID3D11Texture2D => D3D11_TEXTURE2D_DESC, ID3D11Texture3D => D3D11_TEXTURE3D_DESC);

/// The resource behind a view.
pub trait ViewExt {
    fn resource(&self) -> windows_core::Result<ID3D11Resource>;
}

macro_rules! view_ext {
    ($($ty:ty),*) => {$(
        impl ViewExt for $ty {
            fn resource(&self) -> windows_core::Result<ID3D11Resource> {
                // SAFETY: a getter with no arguments.
                unsafe { self.GetResource() }
            }
        }
    )*};
}

view_ext!(ID3D11ShaderResourceView, ID3D11RenderTargetView, ID3D11DepthStencilView);

/// What a subresource is: its dimensions, format, or byte size for a buffer.
enum Extent {
    Buffer(u32),
    Texture { format: DXGI_FORMAT, w: u32, h: u32, d: u32 },
}

fn extent(resource: &ID3D11Resource, sub: u32) -> Option<Extent> {
    if let Ok(b) = resource.cast::<ID3D11Buffer>() {
        return (sub == 0).then(|| Extent::Buffer(b.desc().ByteWidth));
    }

    if let Ok(t) = resource.cast::<ID3D11Texture2D>() {
        let d = t.desc();
        let mips = if d.MipLevels == 0 { full_chain(d.Width, d.Height, 1) } else { d.MipLevels };
        if sub >= mips * d.ArraySize.max(1) {
            return None;
        }

        let mip = sub % mips;
        return Some(Extent::Texture { format: d.Format, w: mip_dim(d.Width, mip), h: mip_dim(d.Height, mip), d: 1 });
    }

    if let Ok(t) = resource.cast::<ID3D11Texture3D>() {
        let d = t.desc();
        let mips = if d.MipLevels == 0 { full_chain(d.Width, d.Height, d.Depth) } else { d.MipLevels };
        if sub >= mips {
            return None;
        }

        return Some(Extent::Texture { format: d.Format, w: mip_dim(d.Width, sub), h: mip_dim(d.Height, sub), d: mip_dim(d.Depth, sub) });
    }

    None
}

// ------------------------------------------------------------------------------------------------ device

/// Safe object creation. Every method returns the object or the runtime's error.
pub trait DeviceExt {
    /// A buffer; `init` must hold at least `ByteWidth` bytes.
    fn create_buffer(&self, desc: &D3D11_BUFFER_DESC, init: Option<&[u8]>) -> windows_core::Result<ID3D11Buffer>;

    /// A 2D texture (array); `init` must have one entry per subresource (mip-major within each slice) covering its mip.
    fn create_texture_2d(&self, desc: &D3D11_TEXTURE2D_DESC, init: Option<&[InitData]>) -> windows_core::Result<ID3D11Texture2D>;

    /// A volume texture; `init` as for [`Self::create_texture_2d`], one entry per mip.
    fn create_texture_3d(&self, desc: &D3D11_TEXTURE3D_DESC, init: Option<&[InitData]>) -> windows_core::Result<ID3D11Texture3D>;
    fn create_shader_resource_view(&self, resource: &ID3D11Resource, desc: Option<&D3D11_SHADER_RESOURCE_VIEW_DESC>) -> windows_core::Result<ID3D11ShaderResourceView>;
    fn create_render_target_view(&self, resource: &ID3D11Resource, desc: Option<&D3D11_RENDER_TARGET_VIEW_DESC>) -> windows_core::Result<ID3D11RenderTargetView>;
    fn create_depth_stencil_view(&self, resource: &ID3D11Resource, desc: Option<&D3D11_DEPTH_STENCIL_VIEW_DESC>) -> windows_core::Result<ID3D11DepthStencilView>;
    fn create_sampler_state(&self, desc: &D3D11_SAMPLER_DESC) -> windows_core::Result<ID3D11SamplerState>;
    fn create_blend_state(&self, desc: &D3D11_BLEND_DESC) -> windows_core::Result<ID3D11BlendState>;
    fn create_depth_stencil_state(&self, desc: &D3D11_DEPTH_STENCIL_DESC) -> windows_core::Result<ID3D11DepthStencilState>;
    fn create_rasterizer_state(&self, desc: &D3D11_RASTERIZER_DESC) -> windows_core::Result<ID3D11RasterizerState>;
    fn create_vertex_shader(&self, bytecode: &[u8]) -> windows_core::Result<ID3D11VertexShader>;
    fn create_pixel_shader(&self, bytecode: &[u8]) -> windows_core::Result<ID3D11PixelShader>;
    fn create_geometry_shader(&self, bytecode: &[u8]) -> windows_core::Result<ID3D11GeometryShader>;
    fn create_hull_shader(&self, bytecode: &[u8]) -> windows_core::Result<ID3D11HullShader>;
    fn create_domain_shader(&self, bytecode: &[u8]) -> windows_core::Result<ID3D11DomainShader>;
    fn create_compute_shader(&self, bytecode: &[u8]) -> windows_core::Result<ID3D11ComputeShader>;
    fn create_unordered_access_view(&self, resource: &ID3D11Resource, desc: Option<&D3D11_UNORDERED_ACCESS_VIEW_DESC>) -> windows_core::Result<ID3D11UnorderedAccessView>;
    fn create_input_layout(&self, elements: &[InputElement], bytecode: &[u8]) -> windows_core::Result<ID3D11InputLayout>;
    fn create_query(&self, kind: D3D11_QUERY) -> windows_core::Result<ID3D11Query>;
}

/// Checks texture initial data against a description: one entry per subresource, each covering its mip.
fn check_init(init: &[InitData], format: DXGI_FORMAT, (w, h, d): (u32, u32, u32), mips: u32, slices: u32) -> bool {
    if mips == 0 || init.len() != (mips * slices.max(1)) as usize {
        return false;
    }

    init.iter().enumerate().all(|(i, s)| {
        let mip = i as u32 % mips;
        region_bytes(format, mip_dim(w, mip), mip_dim(h, mip), mip_dim(d, mip), s.row_pitch as usize, s.slice_pitch as usize)
            .is_some_and(|need| s.data.len() >= need)
    })
}

impl DeviceExt for ID3D11Device {
    fn create_buffer(&self, desc: &D3D11_BUFFER_DESC, init: Option<&[u8]>) -> windows_core::Result<ID3D11Buffer> {
        if init.is_some_and(|data| data.len() < desc.ByteWidth as usize) {
            return invalid();
        }

        let init = init.map(|data| InitData::new(data, 0).raw());
        let mut out = None;
        // SAFETY: the initial data covers the buffer and lives during the call.
        let hr = unsafe { self.CreateBuffer(desc, init.as_ref().map(|i| i as *const _), Some(&mut out)) };
        created(hr, out)
    }

    fn create_texture_2d(&self, desc: &D3D11_TEXTURE2D_DESC, init: Option<&[InitData]>) -> windows_core::Result<ID3D11Texture2D> {
        if init.is_some_and(|init| !check_init(init, desc.Format, (desc.Width, desc.Height, 1), desc.MipLevels, desc.ArraySize)) {
            return invalid();
        }

        let init = init.map(|init| init.iter().map(InitData::raw).collect::<Vec<_>>());
        let mut out = None;
        // SAFETY: one entry per subresource, each covering its mip, alive during the call.
        let hr = unsafe { self.CreateTexture2D(desc, init.as_ref().map(|i| i.as_ptr()), Some(&mut out)) };
        created(hr, out)
    }

    fn create_texture_3d(&self, desc: &D3D11_TEXTURE3D_DESC, init: Option<&[InitData]>) -> windows_core::Result<ID3D11Texture3D> {
        if init.is_some_and(|init| !check_init(init, desc.Format, (desc.Width, desc.Height, desc.Depth), desc.MipLevels, 1)) {
            return invalid();
        }

        let init = init.map(|init| init.iter().map(InitData::raw).collect::<Vec<_>>());
        let mut out = None;
        // SAFETY: one entry per mip, each covering it, alive during the call.
        let hr = unsafe { self.CreateTexture3D(desc, init.as_ref().map(|i| i.as_ptr()), Some(&mut out)) };
        created(hr, out)
    }

    fn create_shader_resource_view(&self, resource: &ID3D11Resource, desc: Option<&D3D11_SHADER_RESOURCE_VIEW_DESC>) -> windows_core::Result<ID3D11ShaderResourceView> {
        let mut out = None;
        // SAFETY: a description by reference, or none.
        let hr = unsafe { self.CreateShaderResourceView(resource, desc.map(|d| d as *const _), Some(&mut out)) };
        created(hr, out)
    }

    fn create_render_target_view(&self, resource: &ID3D11Resource, desc: Option<&D3D11_RENDER_TARGET_VIEW_DESC>) -> windows_core::Result<ID3D11RenderTargetView> {
        let mut out = None;
        // SAFETY: a description by reference, or none.
        let hr = unsafe { self.CreateRenderTargetView(resource, desc.map(|d| d as *const _), Some(&mut out)) };
        created(hr, out)
    }

    fn create_depth_stencil_view(&self, resource: &ID3D11Resource, desc: Option<&D3D11_DEPTH_STENCIL_VIEW_DESC>) -> windows_core::Result<ID3D11DepthStencilView> {
        let mut out = None;
        // SAFETY: a description by reference, or none.
        let hr = unsafe { self.CreateDepthStencilView(resource, desc.map(|d| d as *const _), Some(&mut out)) };
        created(hr, out)
    }

    fn create_sampler_state(&self, desc: &D3D11_SAMPLER_DESC) -> windows_core::Result<ID3D11SamplerState> {
        let mut out = None;
        // SAFETY: a description by reference.
        let hr = unsafe { self.CreateSamplerState(desc, Some(&mut out)) };
        created(hr, out)
    }

    fn create_blend_state(&self, desc: &D3D11_BLEND_DESC) -> windows_core::Result<ID3D11BlendState> {
        let mut out = None;
        // SAFETY: a description by reference.
        let hr = unsafe { self.CreateBlendState(desc, Some(&mut out)) };
        created(hr, out)
    }

    fn create_depth_stencil_state(&self, desc: &D3D11_DEPTH_STENCIL_DESC) -> windows_core::Result<ID3D11DepthStencilState> {
        let mut out = None;
        // SAFETY: a description by reference.
        let hr = unsafe { self.CreateDepthStencilState(desc, Some(&mut out)) };
        created(hr, out)
    }

    fn create_rasterizer_state(&self, desc: &D3D11_RASTERIZER_DESC) -> windows_core::Result<ID3D11RasterizerState> {
        let mut out = None;
        // SAFETY: a description by reference.
        let hr = unsafe { self.CreateRasterizerState(desc, Some(&mut out)) };
        created(hr, out)
    }

    fn create_vertex_shader(&self, bytecode: &[u8]) -> windows_core::Result<ID3D11VertexShader> {
        let mut out = None;
        // SAFETY: the bytecode is a slice; the runtime validates it.
        let hr = unsafe { self.CreateVertexShader(bytecode, None, Some(&mut out)) };
        created(hr, out)
    }

    fn create_pixel_shader(&self, bytecode: &[u8]) -> windows_core::Result<ID3D11PixelShader> {
        let mut out = None;
        // SAFETY: the bytecode is a slice; the runtime validates it.
        let hr = unsafe { self.CreatePixelShader(bytecode, None, Some(&mut out)) };
        created(hr, out)
    }

    fn create_geometry_shader(&self, bytecode: &[u8]) -> windows_core::Result<ID3D11GeometryShader> {
        let mut out = None;
        // SAFETY: the bytecode is a slice; the runtime validates it.
        let hr = unsafe { self.CreateGeometryShader(bytecode, None, Some(&mut out)) };
        created(hr, out)
    }

    fn create_hull_shader(&self, bytecode: &[u8]) -> windows_core::Result<ID3D11HullShader> {
        let mut out = None;
        // SAFETY: the bytecode is a slice; the runtime validates it.
        let hr = unsafe { self.CreateHullShader(bytecode, None, Some(&mut out)) };
        created(hr, out)
    }

    fn create_domain_shader(&self, bytecode: &[u8]) -> windows_core::Result<ID3D11DomainShader> {
        let mut out = None;
        // SAFETY: the bytecode is a slice; the runtime validates it.
        let hr = unsafe { self.CreateDomainShader(bytecode, None, Some(&mut out)) };
        created(hr, out)
    }

    fn create_compute_shader(&self, bytecode: &[u8]) -> windows_core::Result<ID3D11ComputeShader> {
        let mut out = None;
        // SAFETY: the bytecode is a slice; the runtime validates it.
        let hr = unsafe { self.CreateComputeShader(bytecode, None, Some(&mut out)) };
        created(hr, out)
    }

    fn create_unordered_access_view(&self, resource: &ID3D11Resource, desc: Option<&D3D11_UNORDERED_ACCESS_VIEW_DESC>) -> windows_core::Result<ID3D11UnorderedAccessView> {
        let mut out = None;
        // SAFETY: a description by reference, or none.
        let hr = unsafe { self.CreateUnorderedAccessView(resource, desc.map(|d| d as *const _), Some(&mut out)) };
        created(hr, out)
    }

    fn create_input_layout(&self, elements: &[InputElement], bytecode: &[u8]) -> windows_core::Result<ID3D11InputLayout> {
        let descs = elements.iter().map(|e| e.0).collect::<Vec<_>>();
        let mut out = None;
        // SAFETY: the semantics are static strings and the bytecode is a slice.
        let hr = unsafe { self.CreateInputLayout(&descs, bytecode, Some(&mut out)) };
        created(hr, out)
    }

    fn create_query(&self, kind: D3D11_QUERY) -> windows_core::Result<ID3D11Query> {
        let mut out = None;
        // SAFETY: a description by reference.
        let hr = unsafe { self.CreateQuery(&D3D11_QUERY_DESC { Query: kind, MiscFlags: 0 }, Some(&mut out)) };
        created(hr, out)
    }
}

// ------------------------------------------------------------------------------------------------ mapping

/// A mapped subresource, unmapped on drop. Derefs to its bytes: the whole buffer, or the rows of a texture mip (the last row without
/// its padding), with [`Self::row_pitch`] between rows.
pub struct Mapped<'a> {
    ctx: &'a ID3D11DeviceContext,
    resource: ID3D11Resource,
    sub: u32,
    bytes: &'a mut [u8],
    row_pitch: usize,
    depth_pitch: usize,
}

impl Mapped<'_> {
    pub fn row_pitch(&self) -> usize {
        self.row_pitch
    }

    pub fn depth_pitch(&self) -> usize {
        self.depth_pitch
    }

    /// Row `y` of a texture mip, `len` bytes long; `None` past the end.
    pub fn row(&self, y: usize, len: usize) -> Option<&[u8]> {
        self.bytes.get(y * self.row_pitch..y * self.row_pitch + len)
    }

    /// Copies `data` to byte `offset`; false (nothing written) when it doesn't fit.
    pub fn write<T: bytemuck::NoUninit>(&mut self, offset: usize, data: &[T]) -> bool {
        let src = bytemuck::cast_slice::<T, u8>(data);
        match self.bytes.get_mut(offset..offset + src.len()) {
            Some(dst) => {
                dst.copy_from_slice(src);
                true
            }

            None => false,
        }
    }

    /// The first `size_of::<T>()` bytes as a `T`; `None` when the mapping is smaller.
    pub fn read<T: bytemuck::AnyBitPattern>(&self) -> Option<T> {
        self.bytes.get(..size_of::<T>()).map(bytemuck::pod_read_unaligned)
    }
}

impl Deref for Mapped<'_> {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        self.bytes
    }
}

impl DerefMut for Mapped<'_> {
    fn deref_mut(&mut self) -> &mut [u8] {
        self.bytes
    }
}

impl Drop for Mapped<'_> {
    fn drop(&mut self) {
        // SAFETY: the subresource was mapped by this guard, whose bytes can't outlive it.
        unsafe { self.ctx.Unmap(&self.resource, self.sub) };
    }
}

// ------------------------------------------------------------------------------------------------ context

/// Safe immediate context calls. Slices replace pointer + count pairs; `None` unbinds.
pub trait ContextExt {
    fn set_render_targets(&self, rtvs: &[Option<ID3D11RenderTargetView>], dsv: Option<&ID3D11DepthStencilView>);
    /// The render targets and the pixel shader's UAVs (slots from `rtvs.len()`); `counts` sets the append / counter buffers' counters
    /// (one per UAV; u32::MAX keeps a counter).
    fn set_render_targets_and_uavs(&self, rtvs: &[Option<ID3D11RenderTargetView>], dsv: Option<&ID3D11DepthStencilView>, uavs: &[Option<ID3D11UnorderedAccessView>], counts: &[u32]);
    fn clear_unordered_access_view_uint(&self, uav: &ID3D11UnorderedAccessView, values: [u32; 4]);
    fn set_viewports(&self, viewports: &[D3D11_VIEWPORT]);
    fn set_scissor_rects(&self, rects: &[D3D11_RECT]);

    /// A blend state with no blend factor and every sample.
    fn set_blend_state(&self, state: Option<&ID3D11BlendState>);
    fn set_blend_state_factor(&self, state: Option<&ID3D11BlendState>, factor: [f32; 4], sample_mask: u32);
    fn set_depth_stencil_state(&self, state: Option<&ID3D11DepthStencilState>, stencil_ref: u32);
    fn set_rasterizer_state(&self, state: Option<&ID3D11RasterizerState>);
    fn set_input_layout(&self, layout: Option<&ID3D11InputLayout>);
    fn set_topology(&self, topology: D3D_PRIMITIVE_TOPOLOGY);

    /// Vertex buffers from `start`, one stride and offset per buffer.
    ///
    /// # Panics
    ///
    /// If the three slices have different lengths.
    fn set_vertex_buffers(&self, start: u32, buffers: &[Option<ID3D11Buffer>], strides: &[u32], offsets: &[u32]);
    fn set_index_buffer(&self, buffer: Option<&ID3D11Buffer>, format: DXGI_FORMAT, offset: u32);

    fn vs_set_shader(&self, shader: Option<&ID3D11VertexShader>);
    fn ps_set_shader(&self, shader: Option<&ID3D11PixelShader>);
    fn gs_set_shader(&self, shader: Option<&ID3D11GeometryShader>);
    fn hs_set_shader(&self, shader: Option<&ID3D11HullShader>);
    fn ds_set_shader(&self, shader: Option<&ID3D11DomainShader>);
    fn cs_set_shader(&self, shader: Option<&ID3D11ComputeShader>);
    fn vs_set_constant_buffers(&self, start: u32, buffers: &[Option<ID3D11Buffer>]);
    fn ps_set_constant_buffers(&self, start: u32, buffers: &[Option<ID3D11Buffer>]);
    fn gs_set_constant_buffers(&self, start: u32, buffers: &[Option<ID3D11Buffer>]);
    fn hs_set_constant_buffers(&self, start: u32, buffers: &[Option<ID3D11Buffer>]);
    fn ds_set_constant_buffers(&self, start: u32, buffers: &[Option<ID3D11Buffer>]);
    fn cs_set_constant_buffers(&self, start: u32, buffers: &[Option<ID3D11Buffer>]);
    fn vs_set_shader_resources(&self, start: u32, views: &[Option<ID3D11ShaderResourceView>]);
    fn ps_set_shader_resources(&self, start: u32, views: &[Option<ID3D11ShaderResourceView>]);
    fn gs_set_shader_resources(&self, start: u32, views: &[Option<ID3D11ShaderResourceView>]);
    fn hs_set_shader_resources(&self, start: u32, views: &[Option<ID3D11ShaderResourceView>]);
    fn ds_set_shader_resources(&self, start: u32, views: &[Option<ID3D11ShaderResourceView>]);
    fn cs_set_shader_resources(&self, start: u32, views: &[Option<ID3D11ShaderResourceView>]);
    fn vs_set_samplers(&self, start: u32, samplers: &[Option<ID3D11SamplerState>]);
    fn ps_set_samplers(&self, start: u32, samplers: &[Option<ID3D11SamplerState>]);
    fn gs_set_samplers(&self, start: u32, samplers: &[Option<ID3D11SamplerState>]);
    fn hs_set_samplers(&self, start: u32, samplers: &[Option<ID3D11SamplerState>]);
    fn ds_set_samplers(&self, start: u32, samplers: &[Option<ID3D11SamplerState>]);
    fn cs_set_samplers(&self, start: u32, samplers: &[Option<ID3D11SamplerState>]);
    fn cs_set_unordered_access_views(&self, start: u32, views: &[Option<ID3D11UnorderedAccessView>]);
    fn dispatch(&self, x: u32, y: u32, z: u32);

    fn draw(&self, vertices: u32, first: u32);
    fn draw_indexed(&self, indices: u32, first: u32, base_vertex: i32);
    fn draw_instanced(&self, vertices: u32, instances: u32, first: u32, first_instance: u32);
    fn draw_indexed_instanced(&self, indices: u32, instances: u32, first: u32, base_vertex: i32, first_instance: u32);

    fn clear_render_target_view(&self, rtv: &ID3D11RenderTargetView, colour: [f32; 4]);
    fn clear_depth_stencil_view(&self, dsv: &ID3D11DepthStencilView, flags: D3D11_CLEAR_FLAG, depth: f32, stencil: u8);
    fn generate_mips(&self, srv: &ID3D11ShaderResourceView);

    fn copy_resource(&self, dst: &ID3D11Resource, src: &ID3D11Resource);
    fn copy_subresource_region(&self, dst: &ID3D11Resource, dst_sub: u32, dst_at: [u32; 3], src: &ID3D11Resource, src_sub: u32, src_box: Option<&D3D11_BOX>);

    /// Copies `data` into a subresource, or the `dst_box` region of it, with these pitches.
    ///
    /// # Errors
    ///
    /// Fails without copying when the subresource doesn't exist, has a format without a known layout, or `data` doesn't cover the region.
    fn update_subresource(&self, dst: &ID3D11Resource, sub: u32, dst_box: Option<&D3D11_BOX>, data: &[u8], row_pitch: u32, depth_pitch: u32) -> windows_core::Result<()>;

    /// Maps a subresource (`flags`: `D3D11_MAP_FLAG_DO_NOT_WAIT` or 0).
    ///
    /// # Errors
    ///
    /// The runtime's (`DXGI_ERROR_WAS_STILL_DRAWING` with `DO_NOT_WAIT`), or invalid for a subresource whose size isn't known.
    fn map(&self, resource: &ID3D11Resource, sub: u32, kind: D3D11_MAP, flags: u32) -> windows_core::Result<Mapped<'_>>;

    fn begin(&self, query: &ID3D11Query);
    fn end(&self, query: &ID3D11Query);

    /// A query's result, `None` while it isn't ready (`flags`: `D3D11_ASYNC_GETDATA_DONOTFLUSH` or 0).
    ///
    /// # Errors
    ///
    /// The runtime's, e.g. for a query that was never issued.
    fn get_data<T: QueryData>(&self, query: &ID3D11Query, flags: u32) -> windows_core::Result<Option<T>>;
}

impl ContextExt for ID3D11DeviceContext {
    fn set_render_targets(&self, rtvs: &[Option<ID3D11RenderTargetView>], dsv: Option<&ID3D11DepthStencilView>) {
        // SAFETY: a binding call with a slice.
        unsafe { self.OMSetRenderTargets((!rtvs.is_empty()).then_some(rtvs), dsv) };
    }

    fn set_render_targets_and_uavs(&self, rtvs: &[Option<ID3D11RenderTargetView>], dsv: Option<&ID3D11DepthStencilView>, uavs: &[Option<ID3D11UnorderedAccessView>], counts: &[u32]) {
        assert_eq!(uavs.len(), counts.len(), "one initial count per UAV");
        // SAFETY: a binding call with slices; the UAVs and their counts have the same length (checked above).
        unsafe {
            self.OMSetRenderTargetsAndUnorderedAccessViews(
                (!rtvs.is_empty()).then_some(rtvs), dsv, rtvs.len() as u32, uavs.len() as u32, Some(uavs.as_ptr()), Some(counts.as_ptr()),
            );
        };
    }

    fn clear_unordered_access_view_uint(&self, uav: &ID3D11UnorderedAccessView, values: [u32; 4]) {
        // SAFETY: a clear call with a view and four values.
        unsafe { self.ClearUnorderedAccessViewUint(uav, &values) };
    }

    fn set_viewports(&self, viewports: &[D3D11_VIEWPORT]) {
        // SAFETY: a state call with a slice.
        unsafe { self.RSSetViewports(Some(viewports)) };
    }

    fn set_scissor_rects(&self, rects: &[D3D11_RECT]) {
        // SAFETY: a state call with a slice.
        unsafe { self.RSSetScissorRects(Some(rects)) };
    }

    fn set_blend_state(&self, state: Option<&ID3D11BlendState>) {
        // SAFETY: a binding call.
        unsafe { self.OMSetBlendState(state, None, 0xFFFF_FFFF) };
    }

    fn set_blend_state_factor(&self, state: Option<&ID3D11BlendState>, factor: [f32; 4], sample_mask: u32) {
        // SAFETY: a binding call; the factor is a local.
        unsafe { self.OMSetBlendState(state, Some(&factor), sample_mask) };
    }

    fn set_depth_stencil_state(&self, state: Option<&ID3D11DepthStencilState>, stencil_ref: u32) {
        // SAFETY: a binding call.
        unsafe { self.OMSetDepthStencilState(state, stencil_ref) };
    }

    fn set_rasterizer_state(&self, state: Option<&ID3D11RasterizerState>) {
        // SAFETY: a binding call.
        unsafe { self.RSSetState(state) };
    }

    fn set_input_layout(&self, layout: Option<&ID3D11InputLayout>) {
        // SAFETY: a binding call.
        unsafe { self.IASetInputLayout(layout) };
    }

    fn set_topology(&self, topology: D3D_PRIMITIVE_TOPOLOGY) {
        // SAFETY: a state call.
        unsafe { self.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY(topology)) };
    }

    fn set_vertex_buffers(&self, start: u32, buffers: &[Option<ID3D11Buffer>], strides: &[u32], offsets: &[u32]) {
        assert!(buffers.len() == strides.len() && buffers.len() == offsets.len(), "one stride and offset per vertex buffer");
        // SAFETY: three slices of the length passed.
        unsafe { self.IASetVertexBuffers(start, buffers.len() as u32, Some(buffers.as_ptr()), Some(strides.as_ptr()), Some(offsets.as_ptr())) };
    }

    fn set_index_buffer(&self, buffer: Option<&ID3D11Buffer>, format: DXGI_FORMAT, offset: u32) {
        // SAFETY: a binding call.
        unsafe { self.IASetIndexBuffer(buffer, format, offset) };
    }

    fn vs_set_shader(&self, shader: Option<&ID3D11VertexShader>) {
        // SAFETY: a binding call without class instances.
        unsafe { self.VSSetShader(shader, None) };
    }

    fn ps_set_shader(&self, shader: Option<&ID3D11PixelShader>) {
        // SAFETY: a binding call without class instances.
        unsafe { self.PSSetShader(shader, None) };
    }

    fn gs_set_shader(&self, shader: Option<&ID3D11GeometryShader>) {
        // SAFETY: a binding call without class instances.
        unsafe { self.GSSetShader(shader, None) };
    }

    fn hs_set_shader(&self, shader: Option<&ID3D11HullShader>) {
        // SAFETY: a binding call without class instances.
        unsafe { self.HSSetShader(shader, None) };
    }

    fn ds_set_shader(&self, shader: Option<&ID3D11DomainShader>) {
        // SAFETY: a binding call without class instances.
        unsafe { self.DSSetShader(shader, None) };
    }

    fn cs_set_shader(&self, shader: Option<&ID3D11ComputeShader>) {
        // SAFETY: a binding call without class instances.
        unsafe { self.CSSetShader(shader, None) };
    }

    fn vs_set_constant_buffers(&self, start: u32, buffers: &[Option<ID3D11Buffer>]) {
        // SAFETY: a binding call with a slice.
        unsafe { self.VSSetConstantBuffers(start, Some(buffers)) };
    }

    fn ps_set_constant_buffers(&self, start: u32, buffers: &[Option<ID3D11Buffer>]) {
        // SAFETY: a binding call with a slice.
        unsafe { self.PSSetConstantBuffers(start, Some(buffers)) };
    }

    fn gs_set_constant_buffers(&self, start: u32, buffers: &[Option<ID3D11Buffer>]) {
        // SAFETY: a binding call with a slice.
        unsafe { self.GSSetConstantBuffers(start, Some(buffers)) };
    }

    fn hs_set_constant_buffers(&self, start: u32, buffers: &[Option<ID3D11Buffer>]) {
        // SAFETY: a binding call with a slice.
        unsafe { self.HSSetConstantBuffers(start, Some(buffers)) };
    }

    fn ds_set_constant_buffers(&self, start: u32, buffers: &[Option<ID3D11Buffer>]) {
        // SAFETY: a binding call with a slice.
        unsafe { self.DSSetConstantBuffers(start, Some(buffers)) };
    }

    fn cs_set_constant_buffers(&self, start: u32, buffers: &[Option<ID3D11Buffer>]) {
        // SAFETY: a binding call with a slice.
        unsafe { self.CSSetConstantBuffers(start, Some(buffers)) };
    }

    fn vs_set_shader_resources(&self, start: u32, views: &[Option<ID3D11ShaderResourceView>]) {
        // SAFETY: a binding call with a slice.
        unsafe { self.VSSetShaderResources(start, Some(views)) };
    }

    fn ps_set_shader_resources(&self, start: u32, views: &[Option<ID3D11ShaderResourceView>]) {
        // SAFETY: a binding call with a slice.
        unsafe { self.PSSetShaderResources(start, Some(views)) };
    }

    fn gs_set_shader_resources(&self, start: u32, views: &[Option<ID3D11ShaderResourceView>]) {
        // SAFETY: a binding call with a slice.
        unsafe { self.GSSetShaderResources(start, Some(views)) };
    }

    fn hs_set_shader_resources(&self, start: u32, views: &[Option<ID3D11ShaderResourceView>]) {
        // SAFETY: a binding call with a slice.
        unsafe { self.HSSetShaderResources(start, Some(views)) };
    }

    fn ds_set_shader_resources(&self, start: u32, views: &[Option<ID3D11ShaderResourceView>]) {
        // SAFETY: a binding call with a slice.
        unsafe { self.DSSetShaderResources(start, Some(views)) };
    }

    fn cs_set_shader_resources(&self, start: u32, views: &[Option<ID3D11ShaderResourceView>]) {
        // SAFETY: a binding call with a slice.
        unsafe { self.CSSetShaderResources(start, Some(views)) };
    }

    fn vs_set_samplers(&self, start: u32, samplers: &[Option<ID3D11SamplerState>]) {
        // SAFETY: a binding call with a slice.
        unsafe { self.VSSetSamplers(start, Some(samplers)) };
    }

    fn ps_set_samplers(&self, start: u32, samplers: &[Option<ID3D11SamplerState>]) {
        // SAFETY: a binding call with a slice.
        unsafe { self.PSSetSamplers(start, Some(samplers)) };
    }

    fn gs_set_samplers(&self, start: u32, samplers: &[Option<ID3D11SamplerState>]) {
        // SAFETY: a binding call with a slice.
        unsafe { self.GSSetSamplers(start, Some(samplers)) };
    }

    fn hs_set_samplers(&self, start: u32, samplers: &[Option<ID3D11SamplerState>]) {
        // SAFETY: a binding call with a slice.
        unsafe { self.HSSetSamplers(start, Some(samplers)) };
    }

    fn ds_set_samplers(&self, start: u32, samplers: &[Option<ID3D11SamplerState>]) {
        // SAFETY: a binding call with a slice.
        unsafe { self.DSSetSamplers(start, Some(samplers)) };
    }

    fn cs_set_samplers(&self, start: u32, samplers: &[Option<ID3D11SamplerState>]) {
        // SAFETY: a binding call with a slice.
        unsafe { self.CSSetSamplers(start, Some(samplers)) };
    }

    fn cs_set_unordered_access_views(&self, start: u32, views: &[Option<ID3D11UnorderedAccessView>]) {
        // SAFETY: a binding call with a slice (its length is the count); no initial counts (no append / consume buffers).
        unsafe { self.CSSetUnorderedAccessViews(start, views.len() as u32, Some(views.as_ptr()), None) };
    }

    fn dispatch(&self, x: u32, y: u32, z: u32) {
        // SAFETY: a dispatch call; out-of-range reads are defined.
        unsafe { self.Dispatch(x, y, z) };
    }

    fn draw(&self, vertices: u32, first: u32) {
        // SAFETY: a draw call; out-of-range reads are defined.
        unsafe { self.Draw(vertices, first) };
    }

    fn draw_indexed(&self, indices: u32, first: u32, base_vertex: i32) {
        // SAFETY: a draw call; out-of-range reads are defined.
        unsafe { self.DrawIndexed(indices, first, base_vertex) };
    }

    fn draw_instanced(&self, vertices: u32, instances: u32, first: u32, first_instance: u32) {
        // SAFETY: a draw call; out-of-range reads are defined.
        unsafe { self.DrawInstanced(vertices, instances, first, first_instance) };
    }

    fn draw_indexed_instanced(&self, indices: u32, instances: u32, first: u32, base_vertex: i32, first_instance: u32) {
        // SAFETY: a draw call; out-of-range reads are defined.
        unsafe { self.DrawIndexedInstanced(indices, instances, first, base_vertex, first_instance) };
    }

    fn clear_render_target_view(&self, rtv: &ID3D11RenderTargetView, colour: [f32; 4]) {
        // SAFETY: the colour is a local.
        unsafe { self.ClearRenderTargetView(rtv, &colour) };
    }

    fn clear_depth_stencil_view(&self, dsv: &ID3D11DepthStencilView, flags: D3D11_CLEAR_FLAG, depth: f32, stencil: u8) {
        // SAFETY: a plain call.
        unsafe { self.ClearDepthStencilView(dsv, flags as u32, depth, stencil) };
    }

    fn generate_mips(&self, srv: &ID3D11ShaderResourceView) {
        // SAFETY: a plain call.
        unsafe { self.GenerateMips(srv) };
    }

    fn copy_resource(&self, dst: &ID3D11Resource, src: &ID3D11Resource) {
        // SAFETY: a GPU copy; the runtime checks the resources match.
        unsafe { self.CopyResource(dst, src) };
    }

    fn copy_subresource_region(&self, dst: &ID3D11Resource, dst_sub: u32, [x, y, z]: [u32; 3], src: &ID3D11Resource, src_sub: u32, src_box: Option<&D3D11_BOX>) {
        // SAFETY: a GPU copy; the runtime checks the regions; the box is a reference or none.
        unsafe { self.CopySubresourceRegion(dst, dst_sub, x, y, z, src, src_sub, src_box.map(|b| b as *const _)) };
    }

    fn update_subresource(&self, dst: &ID3D11Resource, sub: u32, dst_box: Option<&D3D11_BOX>, data: &[u8], row_pitch: u32, depth_pitch: u32) -> windows_core::Result<()> {
        let need = match extent(dst, sub) {
            Some(Extent::Buffer(size)) => dst_box.map_or(size, |b| b.right.saturating_sub(b.left)) as usize,
            Some(Extent::Texture { format, w, h, d }) => {
                let (w, h, d) = dst_box.map_or((w, h, d), |b| (b.right.saturating_sub(b.left), b.bottom.saturating_sub(b.top), b.back.saturating_sub(b.front)));
                if w == 0 || h == 0 || d == 0 {
                    return Ok(());
                }

                let Some(need) = region_bytes(format, w, h, d, row_pitch as usize, depth_pitch as usize) else { return invalid() };
                need
            }

            None => return invalid(),
        };

        if data.len() < need {
            return invalid();
        }

        // SAFETY: the data covers the region the runtime reads with these pitches, and lives during the call.
        unsafe { self.UpdateSubresource(dst, sub, dst_box.map(|b| b as *const _), data.as_ptr().cast(), row_pitch, depth_pitch) };
        Ok(())
    }

    fn map(&self, resource: &ID3D11Resource, sub: u32, kind: D3D11_MAP, flags: u32) -> windows_core::Result<Mapped<'_>> {
        let Some(extent) = extent(resource, sub) else { return invalid() };
        let mut ms = D3D11_MAPPED_SUBRESOURCE::default();
        // SAFETY: the output is a local.
        unsafe { self.Map(resource, sub, kind, flags, Some(&mut ms)) }.ok()?;
        let (row_pitch, depth_pitch) = (ms.RowPitch as usize, ms.DepthPitch as usize);
        let len = match extent {
            Extent::Buffer(size) => Some(size as usize),
            Extent::Texture { format, w, h, d } => region_bytes(format, w, h, d, row_pitch, depth_pitch),
        };

        let guard = |bytes| Mapped { ctx: self, resource: resource.clone(), sub, bytes, row_pitch, depth_pitch };
        match len {
            Some(len) if !ms.pData.is_null() => {
                // SAFETY: the runtime maps at least the subresource's rows at these pitches; the slice lives in the guard, which unmaps it.
                Ok(guard(unsafe { std::slice::from_raw_parts_mut(ms.pData.cast::<u8>(), len) }))
            }

            _ => {
                drop(guard(&mut []));
                invalid()
            }
        }
    }

    fn begin(&self, query: &ID3D11Query) {
        // SAFETY: a plain call.
        unsafe { self.Begin(query) };
    }

    fn end(&self, query: &ID3D11Query) {
        // SAFETY: a plain call.
        unsafe { self.End(query) };
    }

    fn get_data<T: QueryData>(&self, query: &ID3D11Query, flags: u32) -> windows_core::Result<Option<T>> {
        let mut out = T::default();
        // SAFETY: the output is a local of the size passed, valid for any bytes.
        let hr = unsafe { self.GetData(query, Some((&raw mut out).cast()), size_of::<T>() as u32, flags) };
        hr.ok()?;
        Ok((hr.0 == 0).then_some(out))
    }
}

// ------------------------------------------------------------------------------------------------ compiler

/// Compiles HLSL source with D3DCompile.
///
/// # Errors
///
/// The compiler's messages.
pub fn compile(source: &str, name: &str, entry: &str, target: &str) -> Result<Vec<u8>, String> {
    let c = |s: &str| std::ffi::CString::new(s).map_err(|_| format!("{name}: a NUL in {s:?}"));
    let (name_c, entry_c, target_c) = (c(name)?, c(entry)?, c(target)?);
    let (mut code, mut errors) = (None, None);
    // SAFETY: the source, names and outputs are alive during the call.
    let hr = unsafe {
        D3DCompile(source.as_ptr().cast(), source.len(), PCSTR(name_c.as_ptr().cast()), None, None, PCSTR(entry_c.as_ptr().cast()),
            PCSTR(target_c.as_ptr().cast()), 0, 0, &raw mut code, &raw mut errors)
    };

    match (hr.is_ok(), code) {
        (true, Some(code)) => Ok(blob_bytes(&code)),
        _ => Err(errors.as_ref().map(|e| String::from_utf8_lossy(&blob_bytes(e)).trim_end_matches('\0').to_owned()).unwrap_or_else(|| entry.to_owned())),
    }
}

/// A blob's bytes.
pub fn blob_bytes(blob: &ID3D10Blob) -> Vec<u8> {
    // SAFETY: the blob's buffer is valid for its size while the blob lives.
    unsafe {
        let (ptr, len) = (blob.GetBufferPointer().cast::<u8>(), blob.GetBufferSize());
        if ptr.is_null() { vec![] } else { std::slice::from_raw_parts(ptr, len).to_vec() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_sizes() {
        // 4x4 RGBA8, tight and padded pitch: the last row has no padding.
        assert_eq!(region_bytes(DXGI_FORMAT_R8G8B8A8_UNORM, 4, 4, 1, 16, 0), Some(64));
        assert_eq!(region_bytes(DXGI_FORMAT_R8G8B8A8_UNORM, 4, 4, 1, 32, 0), Some(32 * 3 + 16));

        // BC1: 8 bytes per 4x4 block, a 1x1 mip is one block.
        assert_eq!(region_bytes(DXGI_FORMAT_BC1_UNORM, 8, 8, 1, 16, 0), Some(32));
        assert_eq!(region_bytes(DXGI_FORMAT_BC1_UNORM, 1, 1, 1, 8, 0), Some(8));

        // A pitch smaller than a row, or a slice pitch smaller than a slice, is refused.
        assert_eq!(region_bytes(DXGI_FORMAT_R32_FLOAT, 4, 1, 1, 8, 0), None);
        assert_eq!(region_bytes(DXGI_FORMAT_R32_FLOAT, 2, 2, 2, 8, 8), None);
        assert_eq!(region_bytes(DXGI_FORMAT_R32_FLOAT, 2, 2, 2, 8, 16), Some(32));

        // Unknown formats have no layout.
        assert_eq!(region_bytes(DXGI_FORMAT_UNKNOWN, 1, 1, 1, 4, 0), None);
    }

    #[test]
    fn init_data_checks() {
        let level0 = [0u8; 4 * 4 * 4];
        let level1 = [0u8; 2 * 2 * 4];
        let level2 = [0u8; 4];
        let full = [InitData::new(&level0, 16), InitData::new(&level1, 8), InitData::new(&level2, 4)];
        assert!(check_init(&full, DXGI_FORMAT_R8G8B8A8_UNORM, (4, 4, 1), 3, 1));

        // One entry short, or a mip too small.
        assert!(!check_init(&full[..2], DXGI_FORMAT_R8G8B8A8_UNORM, (4, 4, 1), 3, 1));
        let short = [InitData::new(&level0, 16), InitData::new(&level2, 8), InitData::new(&level2, 4)];
        assert!(!check_init(&short, DXGI_FORMAT_R8G8B8A8_UNORM, (4, 4, 1), 3, 1));

        // Two slices of one mip each.
        assert!(check_init(&[InitData::new(&level2, 4), InitData::new(&level2, 4)], DXGI_FORMAT_R8G8B8A8_UNORM, (1, 1, 1), 1, 2));
    }

    #[test]
    fn full_chains() {
        assert_eq!(full_chain(256, 256, 1), 9);
        assert_eq!(full_chain(1, 1, 1), 1);
        assert_eq!(full_chain(300, 2, 1), 9);
    }
}
