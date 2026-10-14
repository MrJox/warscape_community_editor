//! The surface the renderer draws a view into: RGBA8, bindable as a render target and as a shader resource (ImGui shows it), any
//! size. Offscreen capture is rendering into a render target with no window involved and reading it back.

use std::path::Path;

use wce_d3d11::*;

use crate::image_io::{read_rgba8, save_texture_png};

#[derive(Clone, Debug, Default)]
pub struct RenderTarget {
    w: u32,
    h: u32,
    tex: Option<ID3D11Texture2D>,
    rtv: Option<ID3D11RenderTargetView>,
    srv: Option<ID3D11ShaderResourceView>,
}

impl RenderTarget {

    /// A target of a size; invalid when the device refuses it.
    pub fn new(dev: &ID3D11Device, width: u32, height: u32) -> Self {
        let mut target = Self::default();
        target.create(dev, width, height);
        target
    }

    /// (Re)creates the surface; false when the device refuses it.
    pub fn create(&mut self, dev: &ID3D11Device, width: u32, height: u32) -> bool {
        *self = Self::default();
        if width < 1 || height < 1 {
            return false;
        }

        let td = D3D11_TEXTURE2D_DESC {
            Width: width, Height: height, MipLevels: 1, ArraySize: 1, Format: DXGI_FORMAT_R8G8B8A8_UNORM, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT, BindFlags: (D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE) as u32, CPUAccessFlags: 0, MiscFlags: 0,
        };

        let mut target = Self { w: width, h: height, ..Default::default() };
        // SAFETY: plain creation calls with valid descriptions.
        let ok = unsafe {
            dev.CreateTexture2D(&td, None, Some(&mut target.tex)).is_ok()
                && target.tex.as_ref().is_some_and(|tex| dev.CreateRenderTargetView(tex, None, Some(&mut target.rtv)).is_ok())
                && target.tex.as_ref().is_some_and(|tex| dev.CreateShaderResourceView(tex, None, Some(&mut target.srv)).is_ok())
        };

        if ok {
            *self = target;
        }

        ok
    }

    /// Reallocates when the size differs; true if the surface is usable afterwards.
    pub fn resize(&mut self, dev: &ID3D11Device, width: u32, height: u32) -> bool {
        (width == self.w && height == self.h && self.tex.is_some()) || self.create(dev, width, height)
    }

    pub fn valid(&self) -> bool {
        self.tex.is_some()
    }

    pub fn width(&self) -> u32 {
        self.w
    }

    pub fn height(&self) -> u32 {
        self.h
    }

    pub fn texture(&self) -> Option<&ID3D11Texture2D> {
        self.tex.as_ref()
    }

    pub fn rtv(&self) -> Option<&ID3D11RenderTargetView> {
        self.rtv.as_ref()
    }

    pub fn srv(&self) -> Option<&ID3D11ShaderResourceView> {
        self.srv.as_ref()
    }

    /// The pixels as tightly packed RGBA8 rows, top row first. Waits for the GPU.
    pub fn read_rgba8(&self, dev: &ID3D11Device, ctx: &ID3D11DeviceContext) -> Option<Vec<u8>> {
        read_rgba8(dev, ctx, self.tex.as_ref()?)
    }

    /// Saves the pixels as an RGB PNG written with the kit's encoder.
    pub fn save_png(&self, dev: &ID3D11Device, ctx: &ID3D11DeviceContext, path: &Path) -> bool {
        self.tex.as_ref().is_some_and(|tex| save_texture_png(dev, ctx, tex, path))
    }
}
