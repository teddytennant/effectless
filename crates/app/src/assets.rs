//! Bundled assets: lucide icons, the Linux window-control glyphs, fonts and the
//! surface grain tile. Everything is compiled into the binary.
//!
//! Icon paths are `tern/icons/lucide/<name>.svg`. A path with a stroke suffix,
//! `tern/icons/lucide/<name>.svg@2.25`, serves the same icon with that stroke width;
//! plain paths get lucide's `stroke-width="2"` rewritten to T3's 1.75 (spec 01 §5).
//! Anything else falls through to gpui-component's own icon set.

use std::borrow::Cow;
use std::sync::{Arc, OnceLock};

use anyhow::Result;
use gpui::{App, AssetSource, RenderImage, SharedString};

include!(concat!(env!("OUT_DIR"), "/icons.rs"));

/// The stroke width every lucide icon is drawn at.
pub const ICON_STROKE: f32 = 1.75;

const PREFIX: &str = "tern/icons/";

pub struct Assets;

fn icon_bytes(path: &str) -> Option<&'static [u8]> {
    EMBEDDED_ICONS
        .iter()
        .find_map(|(p, bytes)| (*p == path).then_some(*bytes))
}

fn with_stroke(svg: &[u8], stroke: f32) -> Vec<u8> {
    let text = String::from_utf8_lossy(svg);
    text.replacen(
        "stroke-width=\"2\"",
        &format!("stroke-width=\"{stroke}\""),
        1,
    )
    .into_bytes()
}

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(rest) = path.strip_prefix(PREFIX) {
            let (file, stroke) = match rest.split_once('@') {
                Some((file, stroke)) => (file, stroke.parse::<f32>().ok()),
                None => (rest, None),
            };
            let Some(bytes) = icon_bytes(file) else {
                anyhow::bail!("no bundled icon at {path}");
            };
            if file.starts_with("window/") {
                return Ok(Some(Cow::Borrowed(bytes)));
            }
            return Ok(Some(Cow::Owned(with_stroke(
                bytes,
                stroke.unwrap_or(ICON_STROKE),
            ))));
        }
        gpui_kit_assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        if path.starts_with(PREFIX) || PREFIX.starts_with(path) {
            return Ok(EMBEDDED_ICONS
                .iter()
                .map(|(p, _)| SharedString::from(format!("{PREFIX}{p}")))
                .filter(|p| p.starts_with(path))
                .collect());
        }
        gpui_kit_assets::Assets.list(path)
    }
}

/// The asset path for a lucide icon at the default stroke.
pub fn icon_path(name: IconName) -> SharedString {
    format!("{PREFIX}lucide/{}.svg", name.stem()).into()
}

/// The asset path for a lucide icon at a custom stroke width (toast close 2.25, checks 3).
pub fn icon_path_with_stroke(name: IconName, stroke: f32) -> SharedString {
    format!("{PREFIX}lucide/{}.svg@{stroke}", name.stem()).into()
}

/// Window-control glyphs (10x10, 1px strokes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowGlyph {
    Minimize,
    Maximize,
    Restore,
    Close,
}

impl WindowGlyph {
    pub fn path(self) -> SharedString {
        let stem = match self {
            Self::Minimize => "minimize",
            Self::Maximize => "maximize",
            Self::Restore => "restore",
            Self::Close => "close",
        };
        format!("{PREFIX}window/{stem}.svg").into()
    }
}

/// Registers the bundled fonts with the text system.
pub fn load_fonts(cx: &mut App) {
    let fonts: Vec<Cow<'static, [u8]>> = vec![
        Cow::Borrowed(include_bytes!("../assets/fonts/Inter-Regular.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/Inter-Italic.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/Inter-Medium.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/Inter-SemiBold.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/Inter-Bold.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/LiberationMono-Regular.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/LiberationMono-Bold.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/LiberationMono-Italic.ttf")),
    ];
    if let Err(err) = cx.text_system().add_fonts(fonts) {
        eprintln!("tern: failed to load bundled fonts: {err:#}");
    }
}

fn decode_png(bytes: &[u8]) -> Option<Arc<RenderImage>> {
    let image = image::load_from_memory_with_format(bytes, image::ImageFormat::Png).ok()?;
    let mut rgba = image.into_rgba8();
    // GPUI sprites are BGRA.
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    Some(Arc::new(RenderImage::new([image::Frame::new(rgba)])))
}

/// The 256px grain tile at 1x and 2x device scale (spec 01 §1.8), decoded once.
pub fn grain_tile(scale_factor: f32) -> Option<Arc<RenderImage>> {
    static ONE: OnceLock<Option<Arc<RenderImage>>> = OnceLock::new();
    static TWO: OnceLock<Option<Arc<RenderImage>>> = OnceLock::new();
    if scale_factor > 1.25 {
        TWO.get_or_init(|| decode_png(include_bytes!("../assets/textures/grain@2x.png")))
            .clone()
    } else {
        ONE.get_or_init(|| decode_png(include_bytes!("../assets/textures/grain.png")))
            .clone()
    }
}
