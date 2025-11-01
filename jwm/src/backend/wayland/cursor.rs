// src/backend/wayland/cursor.rs

use smithay::backend::renderer::element::memory::MemoryRenderBuffer;
use smithay::backend::renderer::element::{
    memory::MemoryRenderBufferRenderElement, AsRenderElements, Kind,
};
// FIX: Import `ImportMem` for memory buffer rendering. ImportAll is still needed for surfaces.
use smithay::backend::renderer::{ImportAll, ImportMem, Renderer, Texture};
use smithay::input::pointer::CursorImageStatus;
use smithay::utils::Physical;
use smithay::utils::{IsAlive, Point, Scale};
use std::time::Duration;
use tracing::warn;
use xcursor::{parser::Image, CursorTheme};

use crate::backend::api::{CursorHandle, CursorProvider, StdCursorKind};

static FALLBACK_CURSOR_DATA: &[u8] = include_bytes!("./resources/cursor.rgba");

// ... (Cursor struct and its impl remain the same) ...
#[derive(Debug)]
pub struct Cursor {
    theme: String,
    size: u32,
    images: Vec<(String, Vec<Image>)>, // (name, images)
}

impl Cursor {
    pub fn load() -> Cursor {
        let theme = std::env::var("XCURSOR_THEME").unwrap_or_else(|_| "default".into());
        let size = std::env::var("XCURSOR_SIZE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(24);

        let mut images = Vec::new();
        let icon_names = [
            "left_ptr",
            "hand1",
            "xterm",
            "watch",
            "crosshair",
            "fleur",
            "size_hor",
            "size_ver",
            "top_left_corner",
            "top_right_corner",
            "bottom_left_corner",
            "bottom_right_corner",
            "sizing",
        ];

        let theme_loader = CursorTheme::load(&theme);
        for name in icon_names {
            if let Some(icon_path) = theme_loader.load_icon(name) {
                if let Ok(mut file) = std::fs::File::open(icon_path) {
                    let mut data = Vec::new();
                    if std::io::Read::read_to_end(&mut file, &mut data).is_ok() {
                        if let Some(imgs) = xcursor::parser::parse_xcursor(&data) {
                            images.push((name.to_string(), imgs));
                        }
                    }
                }
            }
        }

        if images.is_empty() {
            warn!(
                "Could not load any cursor from theme {}, using fallback",
                theme
            );
            images.push((
                "left_ptr".to_string(),
                vec![Image {
                    size: 32,
                    width: 64,
                    height: 64,
                    xhot: 1,
                    yhot: 1,
                    delay: 1,
                    pixels_rgba: Vec::from(FALLBACK_CURSOR_DATA),
                    pixels_argb: vec![],
                }],
            ));
        }

        Cursor {
            theme,
            size,
            images,
        }
    }

    pub fn get_image(&self, name: &str, time: Duration) -> Image {
        let size = self.size;
        let images = self
            .images
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, i)| i)
            .unwrap_or_else(|| &self.images[0].1); // Fallback to the first available cursor

        frame(time.as_millis() as u32, size, images)
    }
}

fn nearest_images(size: u32, images: &[Image]) -> impl Iterator<Item = &Image> {
    let nearest_image = images
        .iter()
        .min_by_key(|image| (size as i32 - image.size as i32).abs())
        .unwrap();
    images.iter().filter(move |image| {
        image.width == nearest_image.width && image.height == nearest_image.height
    })
}

fn frame(mut millis: u32, size: u32, images: &[Image]) -> Image {
    let total = nearest_images(size, images).fold(0, |acc, image| acc + image.delay);
    if total == 0 {
        return nearest_images(size, images).next().unwrap().clone();
    }
    millis %= total;
    for img in nearest_images(size, images) {
        if millis < img.delay {
            return img.clone();
        }
        millis -= img.delay;
    }
    unreachable!()
}

#[derive(Debug)]
pub struct PointerElement {
    buffer: Option<MemoryRenderBuffer>,
    status: CursorImageStatus,
}

impl Default for PointerElement {
    fn default() -> Self {
        Self {
            buffer: Default::default(),
            status: CursorImageStatus::default_named(),
        }
    }
}

impl PointerElement {
    pub fn set_status(&mut self, status: CursorImageStatus) {
        self.status = status;
    }
    pub fn set_buffer(&mut self, buffer: MemoryRenderBuffer) {
        self.buffer = Some(buffer);
    }
}

// FIX: Add `ImportMem` to the where clause for the `Memory` variant.
// `ImportAll` is needed for the `Surface` variant.
smithay::render_elements! {
    pub PointerRenderElement<R> where R: Renderer + ImportAll + ImportMem;
    Memory=MemoryRenderBufferRenderElement<R>,
    Surface=smithay::backend::renderer::element::surface::WaylandSurfaceRenderElement<R>,
}

impl<T, R> AsRenderElements<R> for PointerElement
where
    T: Texture + Clone + Send + 'static,
    // FIX: Ensure R satisfies both ImportAll (for surfaces) and ImportMem (for memory buffers).
    R: Renderer<TextureId = T> + ImportAll + ImportMem,
{
    type RenderElement = PointerRenderElement<R>;
    fn render_elements<E>(
        &self,
        renderer: &mut R,
        location: Point<i32, Physical>,
        scale: Scale<f64>,
        alpha: f32,
    ) -> Vec<E>
    where
        E: From<PointerRenderElement<R>>,
    {
        match &self.status {
            CursorImageStatus::Hidden => vec![],
            CursorImageStatus::Named(_) => {
                if let Some(buffer) = self.buffer.as_ref() {
                    let element = MemoryRenderBufferRenderElement::from_buffer(
                        renderer,
                        location.to_f64(),
                        buffer,
                        None,
                        None,
                        None,
                        Kind::Cursor,
                    )
                    .expect("Lost system pointer buffer");
                    vec![PointerRenderElement::<R>::from(element).into()]
                } else {
                    vec![]
                }
            }
            CursorImageStatus::Surface(surface) => {
                if !surface.alive() {
                    return vec![];
                }
                let elements: Vec<PointerRenderElement<R>> =
                    smithay::backend::renderer::element::surface::render_elements_from_surface_tree(
                        renderer,
                        surface,
                        location,
                        scale,
                        alpha,
                        Kind::Cursor,
                    );
                elements.into_iter().map(E::from).collect()
            }
        }
    }
}

pub struct WaylandCursorProvider {}

impl WaylandCursorProvider {
    pub fn new() -> Self {
        Self {}
    }

    pub fn map_kind(kind: StdCursorKind) -> &'static str {
        match kind {
            StdCursorKind::LeftPtr => "left_ptr",
            StdCursorKind::Hand => "hand1",
            StdCursorKind::XTerm => "xterm",
            StdCursorKind::Watch => "watch",
            StdCursorKind::Crosshair => "crosshair",
            StdCursorKind::Fleur => "fleur",
            StdCursorKind::HDoubleArrow => "size_hor",
            StdCursorKind::VDoubleArrow => "size_ver",
            StdCursorKind::TopLeftCorner => "top_left_corner",
            StdCursorKind::TopRightCorner => "top_right_corner",
            StdCursorKind::BottomLeftCorner => "bottom_left_corner",
            StdCursorKind::BottomRightCorner => "bottom_right_corner",
            StdCursorKind::Sizing => "sizing",
        }
    }
}

impl CursorProvider for WaylandCursorProvider {
    fn preload_common(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn get(&mut self, _kind: StdCursorKind) -> Result<CursorHandle, Box<dyn std::error::Error>> {
        Ok(CursorHandle(0))
    }

    fn apply(
        &mut self,
        _window_id: u64,
        _kind: StdCursorKind,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn cleanup(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
}
