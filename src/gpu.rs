use std::ffi::c_void;
use windows::core::{Interface, Result};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_POINT_2F, D2D_RECT_F,
    D2D_SIZE_U,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Bitmap1, ID2D1Device, ID2D1DeviceContext, ID2D1Factory1,
    ID2D1RenderTarget, D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_NONE,
    D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1, D2D1_DEVICE_CONTEXT_OPTIONS_NONE,
    D2D1_FACTORY_TYPE_SINGLE_THREADED,
};
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL_10_0,
    D3D_FEATURE_LEVEL_10_1, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION,
};
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice, IDCompositionDevice, IDCompositionTarget, IDCompositionVisual,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, DWRITE_FACTORY_TYPE_SHARED,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIAdapter1, IDXGIDevice, IDXGIFactory2, IDXGISurface, IDXGISwapChain1,
    DXGI_ADAPTER_FLAG_SOFTWARE, DXGI_PRESENT, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1,
    DXGI_SWAP_CHAIN_FLAG, DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL, DXGI_USAGE_RENDER_TARGET_OUTPUT,
};

pub struct GpuRenderer {
    pub d3d_device: ID3D11Device,
    pub d2d_factory: ID2D1Factory1,
    pub d2d_device: ID2D1Device,
    pub d2d_ctx: ID2D1DeviceContext,
    pub d2d_rt: ID2D1RenderTarget,
    pub dwrite_factory: IDWriteFactory,
    pub dxgi_factory: IDXGIFactory2,
    pub swap_chain: IDXGISwapChain1,
    pub dcomp_device: IDCompositionDevice,
    pub dcomp_target: IDCompositionTarget,
    pub dcomp_visual: IDCompositionVisual,
    pub width: u32,
    pub height: u32,
    pub dpi: f32,
}

impl GpuRenderer {
    pub unsafe fn new(hwnd: HWND, width: u32, height: u32, dpi: f32) -> Result<Self> {
        let dxgi_factory: IDXGIFactory2 = CreateDXGIFactory1()?;
        let adapter = pick_best_hardware_adapter(&dxgi_factory);

        let flags = D3D11_CREATE_DEVICE_BGRA_SUPPORT;
        let feature_levels = [
            D3D_FEATURE_LEVEL_11_1,
            D3D_FEATURE_LEVEL_11_0,
            D3D_FEATURE_LEVEL_10_1,
            D3D_FEATURE_LEVEL_10_0,
        ];
        let mut d3d_device: Option<ID3D11Device> = None;

        if let Some(ref adp) = adapter {
            let _ = D3D11CreateDevice(
                adp,
                D3D_DRIVER_TYPE_UNKNOWN,
                None,
                flags,
                Some(&feature_levels),
                D3D11_SDK_VERSION,
                Some(&mut d3d_device),
                None,
                None,
            );
        }

        let d3d_device = match d3d_device {
            Some(dev) => dev,
            None => {
                let mut dev = None;
                D3D11CreateDevice(
                    None,
                    D3D_DRIVER_TYPE_HARDWARE,
                    None,
                    flags,
                    Some(&feature_levels),
                    D3D11_SDK_VERSION,
                    Some(&mut dev),
                    None,
                    None,
                )?;
                dev.unwrap()
            }
        };

        let dxgi_device: IDXGIDevice = d3d_device.cast()?;
        let d2d_factory: ID2D1Factory1 =
            D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
        let d2d_device = d2d_factory.CreateDevice(&dxgi_device)?;
        let d2d_ctx = d2d_device.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
        d2d_ctx.SetDpi(dpi, dpi);
        let d2d_rt: ID2D1RenderTarget = d2d_ctx.cast()?;

        let dwrite_factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;

        let w = width.max(1);
        let h = height.max(1);
        let desc = DXGI_SWAP_CHAIN_DESC1 {
            Width: w,
            Height: h,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            Stereo: false.into(),
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            Scaling: DXGI_SCALING_STRETCH,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
            AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
            Flags: 0,
        };

        let swap_chain = dxgi_factory.CreateSwapChainForComposition(&dxgi_device, &desc, None)?;

        let dcomp_device: IDCompositionDevice = DCompositionCreateDevice(&dxgi_device)?;
        let dcomp_target = dcomp_device.CreateTargetForHwnd(hwnd, true)?;
        let dcomp_visual = dcomp_device.CreateVisual()?;
        dcomp_visual.SetContent(&swap_chain)?;
        dcomp_target.SetRoot(&dcomp_visual)?;
        dcomp_device.Commit()?;

        let mut renderer = Self {
            d3d_device,
            d2d_factory,
            d2d_device,
            d2d_ctx,
            d2d_rt,
            dwrite_factory,
            dxgi_factory,
            swap_chain,
            dcomp_device,
            dcomp_target,
            dcomp_visual,
            width: w,
            height: h,
            dpi,
        };
        renderer.bind_target_bitmap()?;
        Ok(renderer)
    }

    pub unsafe fn resize(&mut self, width: u32, height: u32, dpi: f32) -> Result<()> {
        let w = width.max(1);
        let h = height.max(1);
        self.dpi = dpi;
        self.d2d_ctx.SetDpi(dpi, dpi);

        if w == self.width && h == self.height {
            return Ok(());
        }

        self.d2d_ctx.SetTarget(None);
        self.swap_chain.ResizeBuffers(
            2,
            w,
            h,
            DXGI_FORMAT_B8G8R8A8_UNORM,
            DXGI_SWAP_CHAIN_FLAG(0),
        )?;
        self.width = w;
        self.height = h;
        self.bind_target_bitmap()?;
        self.dcomp_device.Commit()?;
        Ok(())
    }

    unsafe fn bind_target_bitmap(&mut self) -> Result<()> {
        let surface: IDXGISurface = self.swap_chain.GetBuffer(0)?;
        let props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: self.dpi,
            dpiY: self.dpi,
            bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
            ..Default::default()
        };
        let target = self
            .d2d_ctx
            .CreateBitmapFromDxgiSurface(&surface, Some(&props))?;
        self.d2d_ctx.SetTarget(&target);
        Ok(())
    }

    pub unsafe fn create_bitmap_from_bgra(
        &self,
        width: u32,
        height: u32,
        data: &[u8],
    ) -> Option<ID2D1Bitmap1> {
        if width == 0 || height == 0 || data.len() < (width * height * 4) as usize {
            return None;
        }
        let props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            bitmapOptions: D2D1_BITMAP_OPTIONS_NONE,
            ..Default::default()
        };
        let size = D2D_SIZE_U { width, height };
        self.d2d_ctx
            .CreateBitmap(size, Some(data.as_ptr() as *const c_void), width * 4, &props)
            .ok()
    }

    pub unsafe fn present(&self, _vsync: bool) -> Result<()> {
        // 使用无阻塞 Present(0, 0) 交给 DirectComposition 合成，避免 VSync 阻塞 Win32 消息循环导致鼠标移动掉帧
        self.swap_chain.Present(0, DXGI_PRESENT(0)).ok()?;
        Ok(())
    }
}

/// 显式选择物理硬件显卡，避开软件渲染器与虚拟显示驱动（如 OrayIddDriver）
unsafe fn pick_best_hardware_adapter(factory: &IDXGIFactory2) -> Option<IDXGIAdapter1> {
    let mut best: Option<(u64, IDXGIAdapter1)> = None;
    let mut i = 0u32;
    while let Ok(adapter) = factory.EnumAdapters1(i) {
        i += 1;
        if let Ok(desc) = adapter.GetDesc1() {
            if (desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0 {
                continue;
            }
            let name = String::from_utf16_lossy(
                &desc.Description[..desc
                    .Description
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(desc.Description.len())],
            );
            let lower = name.to_lowercase();
            if lower.contains("oray") || lower.contains("virtual") || lower.contains("basic render")
            {
                continue;
            }
            let vram = desc.DedicatedVideoMemory as u64;
            if best.as_ref().map_or(true, |(b_vram, _)| vram > *b_vram) {
                best = Some((vram, adapter));
            }
        }
    }
    best.map(|(_, adp)| adp)
}

#[inline]
pub fn rgba(r: u8, g: u8, b: u8, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a,
    }
}

#[inline]
pub fn rect_f(x: f32, y: f32, w: f32, h: f32) -> D2D_RECT_F {
    D2D_RECT_F {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    }
}

#[inline]
pub fn point_f(x: f32, y: f32) -> D2D_POINT_2F {
    D2D_POINT_2F { x, y }
}

pub fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
