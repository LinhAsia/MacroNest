use super::{GeometryRenderDraw, GeometryRenderShape};
use anyhow::{Context, Result, bail};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use windows::{
    Win32::{
        Foundation::{HMODULE, HWND},
        Graphics::{
            Direct2D::{
                Common::{
                    D2D_RECT_F, D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F,
                    D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_CLOSED,
                    D2D1_FIGURE_END_OPEN, D2D1_FILL_MODE_WINDING, D2D1_PIXEL_FORMAT,
                },
                D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_NONE,
                D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1, D2D1_CAP_STYLE_ROUND,
                D2D1_DASH_STYLE_SOLID, D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_DRAW_TEXT_OPTIONS_NONE,
                D2D1_ELLIPSE, D2D1_INTERPOLATION_MODE_LINEAR, D2D1_LINE_JOIN_ROUND,
                D2D1_STROKE_STYLE_PROPERTIES, D2D1CreateDevice, ID2D1Bitmap1, ID2D1DeviceContext,
                ID2D1Factory, ID2D1GeometrySink, ID2D1Image, ID2D1PathGeometry, ID2D1SolidColorBrush,
                ID2D1StrokeStyle,
            },
            Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP},
            Direct3D11::{
                D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice,
                ID3D11Device,
            },
            DirectComposition::{
                DCompositionCreateDevice, IDCompositionDevice, IDCompositionTarget,
                IDCompositionVisual,
            },
            DirectWrite::{
                DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_WEIGHT_NORMAL, DWRITE_MEASURING_MODE_NATURAL,
                DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER,
                DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat,
            },
            Dxgi::{
                Common::{
                    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
                },
                DXGI_PRESENT, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1,
                DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL, DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIDevice,
                IDXGIFactory2, IDXGIOutput, IDXGISurface, IDXGISwapChain1, IDXGISwapChain3,
            },
        },
        UI::WindowsAndMessaging::{HWND_TOPMOST, SWP_NOACTIVATE, SWP_SHOWWINDOW, SetWindowPos},
    },
    core::Interface,
};
use windows_numerics::Vector2;

pub(super) struct EspGpuRenderer {
    hwnd: HWND,
    origin: (i32, i32),
    size: (u32, u32),
    _d3d: ID3D11Device,
    swap_chain: IDXGISwapChain1,
    d2d: ID2D1DeviceContext,
    bitmap_properties: D2D1_BITMAP_PROPERTIES1,
    round_stroke_style: ID2D1StrokeStyle,
    dwrite: IDWriteFactory,
    _composition: IDCompositionDevice,
    _composition_target: IDCompositionTarget,
    _visual: IDCompositionVisual,
    brushes: HashMap<[u8; 4], ID2D1SolidColorBrush>,
    formats: HashMap<i32, IDWriteTextFormat>,
    bitmaps: HashMap<(Arc<str>, u32, u32, u32), ID2D1Bitmap1>,
    failed_bitmaps: HashSet<(Arc<str>, u32, u32, u32)>,
}

impl EspGpuRenderer {
    pub(super) fn new(hwnd: HWND) -> Result<Self> {
        unsafe {
            let (left, top, width, height) = super::window_list::virtual_screen_bounds();
            if width <= 0 || height <= 0 {
                bail!("invalid virtual screen bounds");
            }
            SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                left,
                top,
                width,
                height,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            )?;

            let d3d = create_d3d_device()?;
            let dxgi_device: IDXGIDevice = d3d.cast()?;
            let adapter = dxgi_device.GetAdapter()?;
            let factory: IDXGIFactory2 = adapter.GetParent()?;
            let desc = DXGI_SWAP_CHAIN_DESC1 {
                Width: width as u32,
                Height: height as u32,
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
            let swap_chain =
                factory.CreateSwapChainForComposition(&d3d, &desc, None::<&IDXGIOutput>)?;

            let composition: IDCompositionDevice = DCompositionCreateDevice(&dxgi_device)?;
            let composition_target = match composition.CreateTargetForHwnd(hwnd, true) {
                Ok(target) => target,
                Err(_) => composition.CreateTargetForHwnd(hwnd, false)?,
            };
            let visual = composition.CreateVisual()?;
            visual.SetContent(&swap_chain)?;
            composition_target.SetRoot(&visual)?;
            composition.Commit()?;

            let d2d_device = D2D1CreateDevice(&dxgi_device, None)?;
            let d2d = d2d_device.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
            let bitmap_properties = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
                bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                colorContext: Default::default(),
            };
            let surface: IDXGISurface = swap_chain.GetBuffer(0)?;
            let target_bitmap =
                d2d.CreateBitmapFromDxgiSurface(&surface, Some(&bitmap_properties))?;
            d2d.SetTarget(&target_bitmap);

            let d2d_factory: ID2D1Factory = d2d.GetFactory()?;
            let stroke_props = D2D1_STROKE_STYLE_PROPERTIES {
                startCap: D2D1_CAP_STYLE_ROUND,
                endCap: D2D1_CAP_STYLE_ROUND,
                dashCap: D2D1_CAP_STYLE_ROUND,
                lineJoin: D2D1_LINE_JOIN_ROUND,
                miterLimit: 10.0,
                dashStyle: D2D1_DASH_STYLE_SOLID,
                dashOffset: 0.0,
            };
            let round_stroke_style = d2d_factory.CreateStrokeStyle(&stroke_props, None)?;

            Ok(Self {
                hwnd,
                origin: (left, top),
                size: (width as u32, height as u32),
                _d3d: d3d,
                swap_chain,
                d2d,
                bitmap_properties,
                round_stroke_style,
                dwrite: DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?,
                _composition: composition,
                _composition_target: composition_target,
                _visual: visual,
                brushes: HashMap::new(),
                formats: HashMap::new(),
                bitmaps: HashMap::new(),
                failed_bitmaps: HashSet::new(),
            })
        }
    }

    pub(super) fn paint(&mut self, shapes: &[GeometryRenderShape]) -> Result<()> {
        unsafe {
            let (left, top, width, height) = super::window_list::virtual_screen_bounds();
            if (left, top) != self.origin || (width as u32, height as u32) != self.size {
                bail!("display layout changed");
            }

            let surface: IDXGISurface = self.swap_chain.GetBuffer(0)?;
            let target = self
                .d2d
                .CreateBitmapFromDxgiSurface(&surface, Some(&self.bitmap_properties))?;
            self.d2d.SetTarget(&target);

            self.d2d.BeginDraw();
            self.d2d.Clear(Some(&D2D1_COLOR_F {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            }));
            for shape in shapes {
                self.draw_shape(shape)?;
            }
            self.d2d.EndDraw(None, None).context("Direct2D EndDraw")?;
            self.d2d.SetTarget(None::<&ID2D1Image>);
            self.swap_chain
                .Present(1, DXGI_PRESENT(0))
                .ok()
                .context("DXGI Present")?;
            Ok(())
        }
    }

    unsafe fn draw_shape(&mut self, shape: &GeometryRenderShape) -> Result<()> {
        let ox = self.origin.0;
        let oy = self.origin.1;
        match &shape.draw {
            GeometryRenderDraw::Point { x, y, radius, fill } => {
                let brush = self.brush(*fill)?;
                self.d2d.FillEllipse(
                    &D2D1_ELLIPSE {
                        point: point(*x - ox, *y - oy),
                        radiusX: *radius as f32,
                        radiusY: *radius as f32,
                    },
                    &brush,
                );
            }
            GeometryRenderDraw::Line {
                x1,
                y1,
                x2,
                y2,
                stroke,
                thickness,
            } => {
                let brush = self.brush(*stroke)?;
                self.d2d.DrawLine(
                    point(*x1 - ox, *y1 - oy),
                    point(*x2 - ox, *y2 - oy),
                    &brush,
                    (*thickness).max(1) as f32,
                    Some(&self.round_stroke_style),
                );
            }
            GeometryRenderDraw::Circle {
                cx,
                cy,
                radius,
                stroke,
                fill,
                thickness,
            } => {
                let ellipse = D2D1_ELLIPSE {
                    point: point(*cx - ox, *cy - oy),
                    radiusX: *radius as f32,
                    radiusY: *radius as f32,
                };
                if let Some(fill) = fill {
                    let brush = self.brush(*fill)?;
                    self.d2d.FillEllipse(&ellipse, &brush);
                }
                let brush = self.brush(*stroke)?;
                self.d2d
                    .DrawEllipse(&ellipse, &brush, (*thickness).max(1) as f32, None);
            }
            GeometryRenderDraw::Arrow {
                x1,
                y1,
                x2,
                y2,
                stroke,
                thickness,
                head_size,
            } => {
                let brush = self.brush(*stroke)?;
                let p1 = point(*x1 - ox, *y1 - oy);
                let p2 = point(*x2 - ox, *y2 - oy);
                let thick = (*thickness).max(1) as f32;
                self.d2d.DrawLine(p1, p2, &brush, thick, Some(&self.round_stroke_style));

                let dx = (*x2 - *x1) as f32;
                let dy = (*y2 - *y1) as f32;
                let len = (dx * dx + dy * dy).sqrt().max(1.0);
                let ux = dx / len;
                let uy = dy / len;
                let angle = 28.0_f32.to_radians();
                let sin_a = angle.sin();
                let cos_a = angle.cos();
                for side in [-1.0_f32, 1.0_f32] {
                    let rx = ux * cos_a - side * uy * sin_a;
                    let ry = uy * cos_a + side * ux * sin_a;
                    let hx = (*x2 - ox) as f32 - rx * *head_size as f32;
                    let hy = (*y2 - oy) as f32 - ry * *head_size as f32;
                    self.d2d.DrawLine(
                        p2,
                        windows_numerics::Vector2 { X: hx, Y: hy },
                        &brush,
                        thick,
                        Some(&self.round_stroke_style),
                    );
                }
            }
            GeometryRenderDraw::Polyline {
                points,
                stroke,
                thickness,
            } => {
                if points.len() >= 2 {
                    let brush = self.brush(*stroke)?;
                    let thick = (*thickness).max(1) as f32;
                    let factory: ID2D1Factory = self.d2d.GetFactory()?;
                    if let Ok(path_geometry) = factory.CreatePathGeometry()
                        && let Ok(sink) = path_geometry.Open()
                    {
                        sink.SetFillMode(D2D1_FILL_MODE_WINDING);
                        let p0 = point(points[0].0 - ox, points[0].1 - oy);
                        sink.BeginFigure(p0, D2D1_FIGURE_BEGIN_HOLLOW);
                        let d2d_pts: Vec<windows_numerics::Vector2> = points[1..]
                            .iter()
                            .map(|p| point(p.0 - ox, p.1 - oy))
                            .collect();
                        sink.AddLines(&d2d_pts);
                        sink.EndFigure(D2D1_FIGURE_END_OPEN);
                        let _ = sink.Close();
                        self.d2d.DrawGeometry(
                            &path_geometry,
                            &brush,
                            thick,
                            Some(&self.round_stroke_style),
                        );
                    } else {
                        for window in points.windows(2) {
                            let p1 = point(window[0].0 - ox, window[0].1 - oy);
                            let p2 = point(window[1].0 - ox, window[1].1 - oy);
                            self.d2d.DrawLine(p1, p2, &brush, thick, Some(&self.round_stroke_style));
                        }
                    }
                }
            }
            GeometryRenderDraw::Polygon {
                points,
                stroke,
                fill,
                thickness,
            } => {
                if points.len() == 4 {
                    let min_x = points.iter().map(|p| p.0).min().unwrap_or(0);
                    let max_x = points.iter().map(|p| p.0).max().unwrap_or(0);
                    let min_y = points.iter().map(|p| p.1).min().unwrap_or(0);
                    let max_y = points.iter().map(|p| p.1).max().unwrap_or(0);
                    let is_axis_aligned = points
                        .iter()
                        .all(|p| (p.0 == min_x || p.0 == max_x) && (p.1 == min_y || p.1 == max_y));
                    if is_axis_aligned {
                        let rect = D2D_RECT_F {
                            left: (min_x - ox) as f32,
                            top: (min_y - oy) as f32,
                            right: (max_x - ox) as f32,
                            bottom: (max_y - oy) as f32,
                        };
                        if let Some(fill) = fill {
                            let brush = self.brush(*fill)?;
                            self.d2d.FillRectangle(&rect, &brush);
                        }
                        let brush = self.brush(*stroke)?;
                        self.d2d
                            .DrawRectangle(&rect, &brush, (*thickness).max(1) as f32, None);
                        return Ok(());
                    }
                }
                if points.len() >= 3 {
                    let factory: ID2D1Factory = self.d2d.GetFactory()?;
                    let path_geometry = factory.CreatePathGeometry()?;
                    let sink: ID2D1GeometrySink = path_geometry.Open()?;
                    sink.SetFillMode(D2D1_FILL_MODE_WINDING);
                    let p0 = point(points[0].0 - ox, points[0].1 - oy);
                    sink.BeginFigure(p0, D2D1_FIGURE_BEGIN_FILLED);
                        let d2d_pts: Vec<windows_numerics::Vector2> = points[1..]
                            .iter()
                            .map(|p| point(p.0 - ox, p.1 - oy))
                            .collect();
                        sink.AddLines(&d2d_pts);
                        sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                        sink.Close()?;

                        if let Some(fill) = fill {
                            let brush = self.brush(*fill)?;
                            self.d2d.FillGeometry(&path_geometry, &brush, None);
                        }
                        let brush = self.brush(*stroke)?;
                        self.d2d
                            .DrawGeometry(&path_geometry, &brush, (*thickness).max(1) as f32, None);
                    }
                }
            GeometryRenderDraw::Label(text) => {
                let brush = self.brush(text.color)?;
                let format = self.text_format(text.font_size)?;
                let (left, top, right, bottom) = shape.bounds;
                let rect = D2D_RECT_F {
                    left: (left - ox) as f32,
                    top: (top - oy) as f32,
                    right: (right - ox) as f32,
                    bottom: (bottom - oy) as f32,
                };
                let utf16 = text.text.encode_utf16().collect::<Vec<_>>();
                self.d2d.DrawText(
                    &utf16,
                    &format,
                    &rect,
                    &brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
            }
            GeometryRenderDraw::Svg {
                x,
                y,
                width,
                height,
                opacity,
                rotation,
                code,
            } => {
                if let Some(bitmap) = self.bitmap(code, *width, *height, *rotation)? {
                    let rect = D2D_RECT_F {
                        left: (*x - ox) as f32,
                        top: (*y - oy) as f32,
                        right: (*x - ox) as f32 + *width as f32,
                        bottom: (*y - oy) as f32 + *height as f32,
                    };
                    self.d2d.DrawBitmap(
                        &bitmap,
                        Some(&rect),
                        opacity.clamp(0.0, 1.0),
                        D2D1_INTERPOLATION_MODE_LINEAR,
                        None,
                        None,
                    );
                }
            }
            _ => {}
        }
        Ok(())
    }

    unsafe fn brush(&mut self, color: [u8; 4]) -> Result<ID2D1SolidColorBrush> {
        if let Some(brush) = self.brushes.get(&color) {
            return Ok(brush.clone());
        }
        let brush = self.d2d.CreateSolidColorBrush(&d2d_color(color), None)?;
        self.brushes.insert(color, brush.clone());
        Ok(brush)
    }

    unsafe fn text_format(&mut self, font_size: i32) -> Result<IDWriteTextFormat> {
        let font_size = font_size.clamp(8, 128);
        if let Some(format) = self.formats.get(&font_size) {
            return Ok(format.clone());
        }
        let format = self.dwrite.CreateTextFormat(
            windows::core::w!("Segoe UI"),
            None,
            DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            font_size as f32,
            windows::core::w!(""),
        )?;
        format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
        format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        self.formats.insert(font_size, format.clone());
        Ok(format)
    }

    unsafe fn bitmap(
        &mut self,
        source: &Arc<str>,
        width: u32,
        height: u32,
        rotation: f32,
    ) -> Result<Option<ID2D1Bitmap1>> {
        let aspect_bucket =
            (((width as f64 / height.max(1) as f64) * 20.0).round() as u32).clamp(1, 400);
        let key = (Arc::clone(source), aspect_bucket, 0, rotation.to_bits());
        if let Some(bitmap) = self.bitmaps.get(&key) {
            return Ok(Some(bitmap.clone()));
        }
        if self.failed_bitmaps.contains(&key) {
            return Ok(None);
        }
        let aspect = aspect_bucket as f32 / 20.0;
        let (cache_width, cache_height) = if aspect >= 1.0 {
            (1024, (1024.0 / aspect).round().max(2.0) as u32)
        } else {
            ((1024.0 * aspect).round().max(2.0) as u32, 1024)
        };
        let rendered = match crate::render::render_svg_image(
            source.as_ref(),
            cache_width,
            cache_height,
            1.0,
            rotation,
        ) {
            Ok(rendered) => rendered,
            Err(error) => {
                eprintln!("ESP marker asset: {error}");
                self.failed_bitmaps.insert(key);
                return Ok(None);
            }
        };
        let mut bgra = rendered.rgba;
        for pixel in bgra.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        let properties = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            bitmapOptions: D2D1_BITMAP_OPTIONS_NONE,
            colorContext: Default::default(),
        };
        let bitmap = self.d2d.CreateBitmap(
            D2D_SIZE_U {
                width: rendered.width,
                height: rendered.height,
            },
            Some(bgra.as_ptr().cast()),
            rendered.width * 4,
            &properties,
        )?;
        self.bitmaps.insert(key, bitmap.clone());
        Ok(Some(bitmap))
    }
}

fn create_d3d_device() -> Result<ID3D11Device> {
    unsafe {
        for driver in [D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP] {
            let mut device = None;
            if D3D11CreateDevice(
                None,
                driver,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                None,
            )
            .is_ok()
            {
                return device.context("D3D11CreateDevice returned no device");
            }
        }
        bail!("unable to create a Direct3D 11 device")
    }
}

impl Drop for EspGpuRenderer {
    fn drop(&mut self) {
        unsafe {
            let _ = self._composition_target.SetRoot(None::<&IDCompositionVisual>);
            let _ = self._composition.Commit();
        }
    }
}

fn point(x: i32, y: i32) -> Vector2 {
    Vector2 {
        X: x as f32,
        Y: y as f32,
    }
}

fn d2d_color([r, g, b, a]: [u8; 4]) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a: a as f32 / 255.0,
    }
}
