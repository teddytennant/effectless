//! Appearance (spec 06 §9): color scheme tiles, the theme grid, Interface and
//! Typography. Device-only page.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use gpui::{
    AnyElement, App, Bounds, Context, Hsla, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, RenderImage, SharedString, StatefulInteractiveElement as _,
    Styled as _, Window, canvas, div, fill, img, point, prelude::FluentBuilder as _, px, size,
};
use tern_core::settings::{ChatWidth, ClientSettings, EffortDisplay};

use super::{client_select, client_switch, fold, write_client};
use crate::theme::color::{hex, mix_oklab};
use crate::theme::{ActiveTheme as _, TextExt as _, TextSize, ThemeMode, font, radius};
use crate::tokens::Palette;
use crate::ui::{
    Button, ButtonSize, ButtonVariant, DropdownMenu, Icon, IconName, Menu, MenuItem, Tooltip,
    TooltipExt as _, h_flex, v_flex,
};
use crate::views::settings::font_picker::FontPicker;
use crate::views::settings::model::ui_keys;
use crate::views::settings::themes::{self, THEMES};
use crate::views::settings::widgets::{Row, Section, Slider, select};
use crate::views::settings::{SettingsView, content_width};

const CHAT_WIDTH: &[(ChatWidth, &str)] = &[
    (ChatWidth::Comfortable, "Comfortable (default)"),
    (ChatWidth::Wide, "Wide"),
    (ChatWidth::Full, "Full"),
];

const EFFORT: &[(EffortDisplay, &str)] = &[
    (EffortDisplay::Label, "Word (default)"),
    (EffortDisplay::Bars, "Bars"),
];

/// `THREAD_DETAILS_ITEMS` (`threadDetailsItems.ts`): id, label, visible by default.
pub const THREAD_DETAILS_ITEMS: &[(&str, &str, bool)] = &[
    ("environment", "Environment and worktree", true),
    ("openIn", "Open in editor", true),
    ("scripts", "Project scripts", true),
    ("branch", "Branch and pull request", true),
    ("gitActions", "Git actions", true),
    ("changes", "Changes", true),
    ("servers", "Servers", true),
    ("automations", "Automations", true),
    ("lineage", "Lineage", true),
    ("sideChats", "Side chats", true),
    ("usage", "Usage limits", false),
    ("context", "Context window", false),
];

/// Whether a thread details item shows, from the sparse `threadDetailsItems` map.
pub fn thread_detail_visible(c: &ClientSettings, id: &str) -> bool {
    c.thread_details_items.get(id).copied().unwrap_or_else(|| {
        THREAD_DETAILS_ITEMS
            .iter()
            .find(|(i, _, _)| *i == id)
            .is_some_and(|(_, _, d)| *d)
    })
}

pub fn render(
    view: &mut SettingsView,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
) -> Vec<AnyElement> {
    vec![
        color_section(view, window, cx),
        interface_section(view, cx),
        typography_section(view, cx),
    ]
}

// --- color scheme and themes -------------------------------------------------------

fn color_section(
    view: &SettingsView,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
) -> AnyElement {
    let t = *cx.tokens();
    let model = view.model().clone();
    let mode = model.read(cx).appearance_mode();
    let light_id = model.read(cx).theme_for(false);
    let dark_id = model.read(cx).theme_for(true);
    let heading = |text: &'static str| {
        div()
            .text_scale(TextSize::Base)
            .font_semibold()
            .text_color(t.foreground)
            .child(text)
    };
    let tiles = [
        (ThemeMode::System, "System"),
        (ThemeMode::Light, "Light"),
        (ThemeMode::Dark, "Dark"),
    ];
    let light_palette = themed_palette(&light_id, false);
    let dark_palette = themed_palette(&dark_id, true);
    let tile_row = h_flex()
        .gap(px(12.))
        .children(tiles.into_iter().map(|(m, label)| {
            let active = m == mode;
            let model = model.clone();
            let (lp, dp) = (light_palette, dark_palette);
            v_flex()
                .id(SharedString::from(format!("scheme-{label}")))
                .flex_1()
                .min_w_0()
                .gap(px(6.))
                .p(px(8.))
                .rounded(radius::XL)
                .border_1()
                .cursor_pointer()
                .map(|el| {
                    if active {
                        el.border_color(gpui::transparent_black())
                            .bg(t.accent.opacity(t.accent.a * 0.3))
                            .shadow(vec![gpui::BoxShadow {
                                color: t.ring,
                                offset: point(px(0.), px(0.)),
                                blur_radius: px(0.),
                                spread_radius: px(2.),
                                inset: true,
                            }])
                    } else {
                        el.border_color(t.border.opacity(t.border.a * 0.7))
                            .bg(t.card.opacity(0.6))
                            .hover(move |s| s.bg(t.accent.opacity(t.accent.a * 0.1)))
                    }
                })
                .on_click(move |_, window, cx| {
                    model.update(cx, |m, cx| {
                        m.set_appearance_mode(m_mode(m, label), window, cx)
                    })
                })
                .child(wireframe(m, lp, dp, t))
                .child(
                    div()
                        .flex()
                        .justify_center()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .font_medium()
                        .text_color(if active {
                            t.foreground
                        } else {
                            t.muted_foreground
                        })
                        .child(label),
                )
        }));

    let width = content_width(window) - px(48.);
    let columns = (((width + px(8.)) / px(264.)).floor() as usize).clamp(1, 3);
    let dark_now = cx.tern_theme().is_dark();
    let mut grid = v_flex().gap(px(8.));
    for chunk in THEMES.chunks(columns) {
        let mut row = h_flex().gap(px(8.));
        for theme in chunk {
            row = row.child(theme_card(view, theme, &light_id, &dark_id, dark_now, cx));
        }
        for _ in chunk.len()..columns {
            row = row.child(div().flex_1());
        }
        grid = grid.child(row);
    }

    v_flex()
        .id("color-scheme")
        .gap(px(4.))
        .child(
            v_flex()
                .gap(px(10.))
                .child(
                    h_flex()
                        .min_h(px(28.))
                        .px(px(16.))
                        .child(heading("Color scheme")),
                )
                .child(tile_row),
        )
        .child(
            h_flex()
                .id("themes")
                .min_h(px(32.))
                .pt(px(20.))
                .px(px(16.))
                .pb(px(6.))
                .justify_between()
                .child(heading("Themes"))
                .child(
                    h_flex()
                        .gap(px(8.))
                        .child(
                            Button::new("create-theme")
                                .variant(ButtonVariant::Outline)
                                .size(ButtonSize::Xs)
                                .icon(IconName::Paintbrush)
                                .label("Create theme")
                                .disabled(true),
                        )
                        .child(
                            Button::new("add-theme")
                                .variant(ButtonVariant::Outline)
                                .size(ButtonSize::Xs)
                                .icon(IconName::Plus)
                                .label("Add theme")
                                .disabled(true),
                        ),
                ),
        )
        .child(grid)
        .into_any_element()
}

fn m_mode(_: &crate::views::settings::SettingsModel, label: &str) -> ThemeMode {
    match label {
        "Light" => ThemeMode::Light,
        "Dark" => ThemeMode::Dark,
        _ => ThemeMode::System,
    }
}

/// The stock palette with a named theme's roles painted over it.
fn themed_palette(id: &str, dark: bool) -> Palette {
    let mut p = if dark {
        Palette::dark()
    } else {
        Palette::light()
    };
    if let Some(o) = themes::palette_override(id, id) {
        let mut s = if dark {
            crate::tokens::SidebarPalette::dark()
        } else {
            crate::tokens::SidebarPalette::light()
        };
        (o.apply)(&mut p, &mut s, dark);
    }
    p
}

/// `ThemeWireframe.tsx`: a 140px sketch of the app in one palette (System splits it).
fn wireframe(mode: ThemeMode, light: Palette, dark: Palette, t: Palette) -> impl IntoElement {
    div()
        .relative()
        .h(px(140.))
        .w_full()
        .rounded(radius::LG)
        .border_1()
        .border_color(t.border.opacity(t.border.a * 0.6))
        .overflow_hidden()
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| match mode {
                    ThemeMode::Light => paint_wireframe(bounds, &light, window),
                    ThemeMode::Dark => paint_wireframe(bounds, &dark, window),
                    ThemeMode::System => {
                        let half = bounds.size.width / 2.;
                        let left =
                            Bounds::new(bounds.origin, size(half - px(1.), bounds.size.height));
                        let right = Bounds::new(
                            point(bounds.origin.x + half + px(1.), bounds.origin.y),
                            size(half - px(1.), bounds.size.height),
                        );
                        window.paint_quad(fill(bounds, hex(0x7f7f7f40)));
                        window.with_content_mask(Some(gpui::ContentMask { bounds: left }), |w| {
                            paint_wireframe(bounds, &light, w)
                        });
                        window.with_content_mask(Some(gpui::ContentMask { bounds: right }), |w| {
                            paint_wireframe(bounds, &dark, w)
                        });
                    }
                },
            )
            .size_full(),
        )
}

fn paint_wireframe(b: Bounds<Pixels>, p: &Palette, window: &mut Window) {
    let line = hex(0x7f7f7f40);
    let rect = |l: f32, t: f32, w: f32, h: f32| {
        Bounds::new(
            point(
                b.origin.x + b.size.width * l,
                b.origin.y + b.size.height * t,
            ),
            size(b.size.width * w, b.size.height * h),
        )
    };
    let quad = |window: &mut Window, r: Bounds<Pixels>, color: Hsla, radius: f32| {
        window.paint_quad(fill(r, color).corner_radii(px(radius)));
    };
    quad(window, b, p.background, 0.);
    // sidebar with an inset right border
    let side = rect(0., 0., 0.22, 1.);
    quad(window, side, p.sidebar, 0.);
    quad(
        window,
        Bounds::new(
            point(side.origin.x + side.size.width - px(1.), side.origin.y),
            size(px(1.), side.size.height),
        ),
        line,
        0.,
    );
    let item = rect(0.03, 0.08, 0.16, 0.08);
    window.paint_quad(
        fill(item, p.card)
            .corner_radii(px(6.))
            .border_widths(px(1.))
            .border_color(line),
    );
    quad(window, rect(0.03, 0.22, 0.16, 0.07), p.accent, 6.);
    quad(
        window,
        rect(0.03, 0.32, 0.16, 0.07),
        p.message_surface.opacity(0.7),
        6.,
    );
    quad(
        window,
        rect(0.03, 0.42, 0.16, 0.07),
        p.message_surface.opacity(0.5),
        6.,
    );
    // user bubble and text lines
    quad(window, rect(0.48, 0.11, 0.24, 0.09), p.message_surface, 8.);
    quad(window, rect(0.27, 0.28, 0.34, 0.05), line, 9999.);
    quad(window, rect(0.27, 0.38, 0.26, 0.05), line, 9999.);
    // composer with a pill and the send dot
    let composer = rect(0.26, 0.77, 0.68, 0.15);
    window.paint_quad(
        fill(composer, p.card)
            .corner_radii(px(6.))
            .border_widths(px(1.))
            .border_color(line),
    );
    let ch = composer.size.height;
    quad(
        window,
        Bounds::new(
            point(
                composer.origin.x + px(6.),
                composer.origin.y + ch / 2. - px(2.),
            ),
            size(composer.size.width * 0.55, px(4.)),
        ),
        line,
        9999.,
    );
    let dot = ch * 0.5;
    quad(
        window,
        Bounds::new(
            point(
                composer.origin.x + composer.size.width - dot - px(6.),
                composer.origin.y + (ch - dot) / 2.,
            ),
            size(dot, dot),
        ),
        p.message_action,
        9999.,
    );
    // floating panel
    let panel = rect(0.75, 0.08, 0.20, 0.46);
    window.paint_quad(gpui::quad(
        Bounds::new(panel.origin + point(px(0.), px(2.)), panel.size),
        px(8.),
        hex(0x00000024),
        px(0.),
        gpui::transparent_black(),
        gpui::BorderStyle::Solid,
    ));
    window.paint_quad(
        fill(panel, p.card)
            .corner_radii(px(8.))
            .border_widths(px(1.))
            .border_color(line),
    );
    let dots = [
        hex(0x34d399ff),
        p.message_action,
        hex(0xfbbf24ff).opacity(0.55),
    ];
    for (i, color) in dots.into_iter().enumerate() {
        let y = panel.origin.y + panel.size.height * (0.22 + 0.28 * i as f32);
        quad(
            window,
            Bounds::new(
                point(panel.origin.x + px(6.), y - px(1.5)),
                size(px(3.), px(3.)),
            ),
            color,
            9999.,
        );
        quad(
            window,
            Bounds::new(
                point(panel.origin.x + px(13.), y - px(1.5)),
                size(panel.size.width * 0.45, px(3.)),
            ),
            line,
            9999.,
        );
    }
}

fn theme_card(
    view: &SettingsView,
    theme: &'static themes::BuiltinTheme,
    light_id: &str,
    dark_id: &str,
    dark_now: bool,
    cx: &mut Context<SettingsView>,
) -> AnyElement {
    let t = *cx.tokens();
    let model = view.model().clone();
    let light_on = light_id == theme.id;
    let dark_on = dark_id == theme.id;
    let tooltip = match (light_on, dark_on) {
        (true, true) => "Use for both light and dark",
        (true, false) => "Use for light mode only",
        (false, true) => "Use for dark mode only",
        _ => "Use for both light and dark",
    };
    let circle =
        |dark: bool, picked: bool| {
            let model = model.clone();
            let stock = if dark {
                Palette::dark()
            } else {
                Palette::light()
            };
            let (canvas_c, accent, action) = themes::preview_colors(theme, dark, &stock);
            let image = orb_image(canvas_c, accent, action, dark, t.background);
            div()
                .id(SharedString::from(format!("orb-{}-{dark}", theme.id)))
                .relative()
                .p(px(6.))
                .rounded(radius::FULL)
                .cursor_pointer()
                .when(picked, |el| {
                    el.shadow(vec![gpui::BoxShadow {
                        color: t.ring,
                        offset: point(px(0.), px(0.)),
                        blur_radius: px(0.),
                        spread_radius: px(2.),
                        inset: true,
                    }])
                })
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    model.update(cx, |m, cx| m.set_theme_half(theme.id, dark, window, cx))
                })
                .child(img(image).size(px(56.)).rounded(radius::FULL).shadow(
                    cx.tern_theme().shadows.xs(gpui::black().opacity(if dark {
                        0.18
                    } else {
                        0.08
                    })),
                ))
                .when(picked, |el| {
                    el.child(
                        div()
                            .absolute()
                            .right(px(-2.))
                            .bottom(px(-2.))
                            .size(px(20.))
                            .rounded(radius::FULL)
                            .border_1()
                            .border_color(t.border.opacity(t.border.a * 0.7))
                            .bg(t.background)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                Icon::new(if dark { IconName::Moon } else { IconName::Sun })
                                    .size(px(12.))
                                    .color(t.foreground),
                            ),
                    )
                })
        };
    let m2 = model.clone();
    let _ = dark_now;
    div()
        .id(SharedString::from(format!("theme-card-{}", theme.id)))
        .flex_1()
        .min_w_0()
        .rounded(radius::XL)
        .border_1()
        .border_color(t.border.opacity(t.border.a * 0.7))
        .bg(t.card.opacity(0.6))
        .hover(move |s| s.bg(t.accent.opacity(t.accent.a * 0.1)))
        .cursor_pointer()
        .t3_tooltip(Tooltip::text(tooltip))
        .on_click(move |_, window, cx| m2.update(cx, |m, cx| m.set_theme(theme.id, window, cx)))
        .child(
            h_flex()
                .min_h(px(64.))
                .pt(px(12.))
                .px(px(12.))
                .gap(px(10.))
                .justify_center()
                .child(circle(false, light_on))
                .child(circle(true, dark_on)),
        )
        .child(
            h_flex()
                .gap(px(8.))
                .pt(px(8.))
                .px(px(12.))
                .pb(px(12.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_scale(TextSize::Sm)
                        .font_medium()
                        .text_color(t.foreground)
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(theme.label),
                )
                .child(
                    Button::new(SharedString::from(format!("dup-{}", theme.id)))
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::IconXs)
                        .icon(IconName::Copy)
                        .disabled(true)
                        .tooltip(Tooltip::text("Duplicate theme")),
                ),
        )
        .into_any_element()
}

/// The preview circle (`ThemePreviewCircles.tsx`): canvas mixed 80% with white or
/// #09090b, two radial glows, a 2px background ring and an inner edge line. GPUI has
/// no radial gradients, so the circle is rasterized once per color set at 2x.
fn orb_image(canvas: Hsla, accent: Hsla, action: Hsla, dark: bool, ring: Hsla) -> Arc<RenderImage> {
    type Key = [u32; 5];
    static CACHE: OnceLock<Mutex<HashMap<Key, Arc<RenderImage>>>> = OnceLock::new();
    let rgb = |c: Hsla| {
        let r = c.to_rgb();
        [r.r, r.g, r.b, r.a]
    };
    let pack = |c: Hsla| {
        let [r, g, b, a] = rgb(c);
        ((r * 255.) as u32) << 24
            | ((g * 255.) as u32) << 16
            | ((b * 255.) as u32) << 8
            | (a * 255.) as u32
    };
    let key = [
        pack(canvas),
        pack(accent),
        pack(action),
        pack(ring),
        dark as u32,
    ];
    let cache = CACHE.get_or_init(Default::default);
    if let Some(img) = cache.lock().ok().and_then(|c| c.get(&key).cloned()) {
        return img;
    }
    let base = mix_oklab(
        canvas,
        if dark {
            hex(0x09090bff)
        } else {
            hex(0xffffffff)
        },
        0.8,
    );
    let base = rgb(base);
    let acc = rgb(accent);
    let act = rgb(action);
    let ring_c = rgb(ring);
    let n: u32 = 112;
    let mut buf = image::RgbaImage::new(n, n);
    let r_outer = n as f32 / 2.;
    // Glow centers and their stops (fraction of the gradient ray, alpha).
    let (g1, g2) = if dark {
        (
            (0.28, 0.78, [(0., 1.), (0.28, 0.62), (0.58, 0.)]),
            (0.82, 0.18, [(0., 0.45), (0.55, 0.), (0.55, 0.)]),
        )
    } else {
        (
            (0.72, 0.22, [(0., 1.), (0.28, 0.72), (0.58, 0.)]),
            (0.18, 0.82, [(0., 0.45), (0.55, 0.), (0.55, 0.)]),
        )
    };
    let glow = |x: f32, y: f32, (cx_, cy_, stops): (f32, f32, [(f32, f32); 3])| -> f32 {
        let fx = cx_ * n as f32;
        let fy = cy_ * n as f32;
        let far = [
            (0., 0.),
            (n as f32, 0.),
            (0., n as f32),
            (n as f32, n as f32),
        ]
        .iter()
        .map(|(a, b)| ((a - fx).powi(2) + (b - fy).powi(2)).sqrt())
        .fold(0., f32::max);
        let d = ((x - fx).powi(2) + (y - fy).powi(2)).sqrt() / far;
        let mut a = 0.;
        for w in stops.windows(2) {
            let ((p0, a0), (p1, a1)) = (w[0], w[1]);
            if d >= p0 && d <= p1 && p1 > p0 {
                a = a0 + (a1 - a0) * (d - p0) / (p1 - p0);
            }
        }
        if d < stops[0].0 { stops[0].1 } else { a }
    };
    for (px_, py_, pixel) in buf.enumerate_pixels_mut() {
        let x = px_ as f32 + 0.5;
        let y = py_ as f32 + 0.5;
        let dist = ((x - r_outer).powi(2) + (y - r_outer).powi(2)).sqrt();
        let coverage = (r_outer - dist + 0.5).clamp(0., 1.);
        if coverage <= 0. {
            *pixel = image::Rgba([0, 0, 0, 0]);
            continue;
        }
        // The gradient layer is scaled 1.1 about the center.
        let sx = r_outer + (x - r_outer) / 1.1;
        let sy = r_outer + (y - r_outer) / 1.1;
        let mut c = [base[0], base[1], base[2]];
        for (color, a) in [(acc, glow(sx, sy, g1)), (act, glow(sx, sy, g2))] {
            for i in 0..3 {
                c[i] = color[i] * a + c[i] * (1. - a);
            }
        }
        // 2px ring of the page background (at 2x: 4px), then a 1px inner edge.
        let ring_w = 4.;
        if dist > r_outer - ring_w {
            c = [ring_c[0], ring_c[1], ring_c[2]];
        } else if dist > r_outer - ring_w - 2. {
            let (edge, a) = if dark { (1., 0.14) } else { (0., 0.10) };
            for ch in &mut c {
                *ch = edge * a + *ch * (1. - a);
            }
        }
        // GPUI sprites are BGRA.
        *pixel = image::Rgba([
            (c[2] * 255.) as u8,
            (c[1] * 255.) as u8,
            (c[0] * 255.) as u8,
            (coverage * 255.) as u8,
        ]);
    }
    let image = Arc::new(RenderImage::new([image::Frame::new(buf)]));
    if let Ok(mut c) = cache.lock() {
        c.insert(key, image.clone());
    }
    image
}

// --- interface ---------------------------------------------------------------------

fn interface_section(view: &SettingsView, cx: &mut Context<SettingsView>) -> AnyElement {
    let model = view.model().clone();
    let c = model.read(cx).client().clone();
    let d = ClientSettings::default();
    let slider_row = |id: &'static str,
                      title: &'static str,
                      desc: &'static str,
                      value: u32,
                      default: u32,
                      range: (u32, u32, u32),
                      suffix: (&'static str, f32),
                      set: fn(&mut ClientSettings, u32),
                      cx: &App| {
        let m = model.clone();
        let m2 = model.clone();
        Row::new(id, title)
            .description(desc, cx)
            .highlight(view.is_pulsing(id))
            .probe(view.probe_for(id))
            .reset_when(value != default, move |window, cx| {
                write_client(&m2, |c| set(c, default), window, cx)
            })
            .control(
                Slider::new(
                    SharedString::from(format!("{id}-slider")),
                    value,
                    range.0,
                    range.1,
                    range.2,
                )
                .suffix(suffix.0, suffix.1)
                .on_change(move |v, window, cx| write_client(&m, |c| set(c, v), window, cx)),
            )
            .into_any_element()
    };
    let enabled = THREAD_DETAILS_ITEMS
        .iter()
        .filter(|(id, _, _)| thread_detail_visible(&c, id))
        .count();
    let details_default = c.thread_details_items.is_empty();
    let m_details = model.clone();
    let m_reset = model.clone();
    let details = Row::new("thread-details", "Thread details")
        .description(
            "Choose what the thread details card shows. You can also right-click the card.",
            cx,
        )
        .highlight(view.is_pulsing("thread-details"))
        .probe(view.probe_for("thread-details"))
        .reset_when(!details_default, move |window, cx| {
            write_client(&m_reset, |c| c.thread_details_items.clear(), window, cx)
        })
        .control(
            DropdownMenu::new(
                "thread-details-menu",
                move |_, cx| {
                    let c = m_details.read(cx).client().clone();
                    let mut menu = Menu::new().min_width(px(224.));
                    for (id, label, default) in THREAD_DETAILS_ITEMS {
                        let on = thread_detail_visible(&c, id);
                        let m = m_details.clone();
                        menu = menu.item(MenuItem::checkbox(*label, on).on_select(
                            move |window, cx| {
                                write_client(
                                    &m,
                                    |c| {
                                        if !on == *default {
                                            c.thread_details_items.remove(*id);
                                        } else {
                                            c.thread_details_items.insert((*id).to_string(), !on);
                                        }
                                    },
                                    window,
                                    cx,
                                )
                            },
                        ));
                    }
                    menu
                },
                move |open| {
                    Button::new("thread-details-trigger")
                        .variant(ButtonVariant::Outline)
                        .size(ButtonSize::Sm)
                        .label(format!("{enabled} of {} on", THREAD_DETAILS_ITEMS.len()))
                        .icon_end(IconName::ChevronDown)
                        .pressed(open)
                        .into_any_element()
                },
            )
            .align(gpui_base::Align::End),
        )
        .into_any_element();

    let mut rows = vec![
        slider_row(
            "contrast",
            "Contrast",
            "Adjust the contrast of colors and borders across the interface.",
            c.appearance_contrast,
            d.appearance_contrast,
            (50, 200, 5),
            ("%", 48.),
            |c, v| c.appearance_contrast = v,
            cx,
        ),
        slider_row(
            "glass-opacity",
            "Glass opacity",
            "Higher values make menus, dialogs, and the composer more solid.",
            c.glass_opacity,
            d.glass_opacity,
            (40, 100, 5),
            ("%", 48.),
            |c, v| c.glass_opacity = v,
            cx,
        ),
        client_select(
            view,
            "chat-width",
            "Chat width",
            "Set how wide messages and the composer can grow on large screens.",
            CHAT_WIDTH,
            160.,
            |c| c.chat_width,
            |c, v| c.chat_width = v,
            cx,
        )
        .into_any_element(),
        details,
        client_select(
            view,
            "effort-display",
            "Effort display",
            "Show reasoning effort in the composer as its name, such as High, or as bars.",
            EFFORT,
            160.,
            |c| c.effort_display,
            |c, v| c.effort_display = v,
            cx,
        )
        .into_any_element(),
        client_switch(
            view,
            "render-math",
            "Render math",
            "Typeset LaTeX math, such as $x^2$ or \\[x^2\\], in messages and Markdown previews. The composer keeps TeX source editable.",
            |c| c.math_rendering_enabled,
            |c, v| c.math_rendering_enabled = v,
            cx,
        )
        .into_any_element(),
    ];
    let animations = slider_row(
        "panel-animations",
        "Panel animations",
        "Set how fast panels open and close.",
        c.panel_animation_duration_ms,
        d.panel_animation_duration_ms,
        (0, 400, 25),
        (" ms", 64.),
        |c, v| c.panel_animation_duration_ms = v,
        cx,
    );
    let context = client_switch(
        view,
        "composer-context",
        "Composer context",
        "Keep branch and worktree controls below the composer after a thread starts.",
        |c| c.persist_composer_context_strip,
        |c, v| c.persist_composer_context_strip = v,
        cx,
    )
    .into_any_element();
    rows.extend(fold(
        view,
        "appearance-interface-advanced",
        || vec![animations, context],
        cx,
    ));
    Section::new("appearance-interface", "Interface")
        .children(rows)
        .into_any_element()
}

// --- typography --------------------------------------------------------------------

fn size_select(
    id: SharedString,
    value: u32,
    range: (u32, u32),
    set: impl Fn(u32, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let options = (range.0..=range.1)
        .map(|n| {
            (
                SharedString::from(n.to_string()),
                SharedString::from(format!("{n} px")),
            )
        })
        .collect();
    select(id, options, value.to_string(), 88., move |v, window, cx| {
        if let Ok(n) = v.parse() {
            set(n, window, cx);
        }
    })
    .into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn font_row(
    view: &SettingsView,
    id: &'static str,
    title: &'static str,
    desc: &'static str,
    family: (
        fn(&ClientSettings) -> String,
        fn(&mut ClientSettings, String),
    ),
    size: (
        fn(&ClientSettings) -> u32,
        fn(&mut ClientSettings, u32),
        (u32, u32),
    ),
    mono: bool,
    preview: Option<AnyElement>,
    cx: &mut Context<SettingsView>,
) -> AnyElement {
    let model = view.model().clone();
    let c = model.read(cx).client().clone();
    let d = ClientSettings::default();
    let (get_f, set_f) = family;
    let (get_s, set_s, range) = size;
    let differs = get_f(&c) != get_f(&d) || get_s(&c) != get_s(&d);
    let (m1, m2, m3) = (model.clone(), model.clone(), model.clone());
    let default_family = if mono { font::MONO } else { font::SANS };
    let mut picker = FontPicker::new(
        SharedString::from(format!("{id}-family")),
        get_f(&c),
        default_family,
        move |f, window, cx| {
            if mono && !f.is_empty() && !crate::views::settings::font_picker::is_monospace(&f, cx) {
                crate::ui::Toasts::push(
                    crate::ui::Toast::error(format!("\"{f}\" isn't monospace")).description(
                        "Code and terminal need a fixed-width font, so the current font was kept.",
                    ),
                    cx,
                );
                return;
            }
            write_client(&m1, |c| set_f(c, f), window, cx)
        },
    );
    if mono {
        picker = picker.monospace_only();
    }
    let row = Row::new(id, title)
        .description(desc, cx)
        .highlight(view.is_pulsing(id))
        .probe(view.probe_for(id))
        .reset_when(differs, move |window, cx| {
            let d = ClientSettings::default();
            write_client(
                &m3,
                |c| {
                    set_f(c, get_f(&d));
                    set_s(c, get_s(&d));
                },
                window,
                cx,
            )
        })
        .control(picker)
        .control(size_select(
            SharedString::from(format!("{id}-size")),
            get_s(&c),
            range,
            move |n, window, cx| write_client(&m2, |c| set_s(c, n), window, cx),
        ));
    match preview {
        Some(p) => row.below(div().pb(px(8.)).child(p)).into_any_element(),
        None => row.into_any_element(),
    }
}

fn typography_section(view: &SettingsView, cx: &mut Context<SettingsView>) -> AnyElement {
    let model = view.model().clone();
    let separate = model.read(cx).ui_bool(ui_keys::TYPOGRAPHY_ADVANCED);
    let typo = model.read(cx).typography();
    let mut rows: Vec<AnyElement> = Vec::new();
    rows.push(font_row(
        view,
        "interface-font",
        "Interface font",
        "Everything outside code blocks and the terminal.",
        (
            |c| c.font_family_sans.clone(),
            |c, v| c.font_family_sans = v,
        ),
        (
            |c| c.font_size_interface,
            |c, v| c.font_size_interface = v,
            (12, 20),
        ),
        false,
        (!separate).then(|| prompt_preview(&typo, cx)),
        cx,
    ));
    if separate {
        rows.push(font_row(
            view,
            "prompt-font",
            "Prompt font",
            "Only the box you write prompts in. Mono works well here.",
            (
                |c| c.font_family_composer.clone(),
                |c, v| c.font_family_composer = v,
            ),
            (
                |c| c.font_size_prompt,
                |c, v| c.font_size_prompt = v,
                (12, 20),
            ),
            false,
            Some(prompt_preview(&typo, cx)),
            cx,
        ));
        rows.push(font_row(
            view,
            "code-font",
            "Code font",
            "Code blocks, diffs, and file previews.",
            (
                |c| c.font_family_code.clone(),
                |c, v| c.font_family_code = v,
            ),
            (|c| c.font_size_code, |c, v| c.font_size_code = v, (10, 18)),
            true,
            Some(code_preview(&typo, cx)),
            cx,
        ));
        rows.push(font_row(
            view,
            "terminal-font",
            "Terminal font",
            "Terminal output, independent from code blocks and diffs.",
            (
                |c| c.font_family_terminal.clone(),
                |c, v| c.font_family_terminal = v,
            ),
            (
                |c| c.font_size_terminal,
                |c, v| c.font_size_terminal = v,
                (8, 20),
            ),
            true,
            Some(terminal_preview(&typo, cx)),
            cx,
        ));
    } else {
        rows.push(font_row(
            view,
            "monospace-font",
            "Monospace font",
            "Code blocks, diffs, file previews, and the terminal.",
            (
                |c| c.font_family_code.clone(),
                |c, v| c.font_family_code = v,
            ),
            (|c| c.font_size_code, |c, v| c.font_size_code = v, (10, 18)),
            true,
            Some(
                v_flex()
                    .gap(px(8.))
                    .child(code_preview(&typo, cx))
                    .child(terminal_preview(&typo, cx))
                    .into_any_element(),
            ),
            cx,
        ));
    }
    rows.push(
        client_switch(
            view,
            "word-wrap",
            "Word wrap",
            "Wrap long lines in code blocks, diffs, and file previews by default.",
            |c| c.word_wrap,
            |c, v| c.word_wrap = v,
            cx,
        )
        .into_any_element(),
    );
    let m = model.clone();
    let m2 = model.clone();
    let separate_row = Row::new("separate-fonts", "Separate fonts")
        .description(
            "Set the prompt, code, and terminal fonts separately instead of one monospace font.",
            cx,
        )
        .highlight(view.is_pulsing("separate-fonts"))
        .probe(view.probe_for("separate-fonts"))
        .reset_when(separate, move |window, cx| {
            m2.update(cx, |m, cx| {
                m.set_ui(ui_keys::TYPOGRAPHY_ADVANCED, None, cx);
                m.apply_client_effects(window, cx);
            })
        })
        .control(
            crate::ui::Switch::new("separate-fonts-switch", separate).on_change(
                move |on, window, cx| {
                    m.update(cx, |m, cx| {
                        m.set_ui(
                            ui_keys::TYPOGRAPHY_ADVANCED,
                            on.then(|| serde_json::Value::Bool(true)),
                            cx,
                        );
                        m.apply_client_effects(window, cx);
                    })
                },
            ),
        )
        .into_any_element();
    rows.extend(fold(
        view,
        "appearance-typography-advanced",
        || vec![separate_row],
        cx,
    ));
    Section::new("typography", "Typography")
        .children(rows)
        .into_any_element()
}

fn preview_box(cx: &App) -> gpui::Div {
    let t = cx.tokens();
    v_flex()
        .mt(px(4.))
        .rounded(radius::LG)
        .border_1()
        .border_color(t.border)
        .overflow_hidden()
}

fn chip(text: &'static str, family: SharedString, size: f32, cx: &App) -> AnyElement {
    let t = cx.tokens();
    div()
        .px(px(4.))
        .rounded(radius::SM)
        .bg(t.accent)
        .text_color(t.foreground)
        .font_family(family)
        .text_size(px(size))
        .child(text)
        .into_any_element()
}

fn prompt_preview(typo: &crate::theme::Typography, cx: &App) -> AnyElement {
    let t = cx.tokens();
    let family = typo.prompt_family();
    let size = typo.prompt_size;
    let word = |w: &'static str| {
        div()
            .font_family(family.clone())
            .text_size(px(size))
            .child(w)
            .into_any_element()
    };
    let mut parts: Vec<AnyElement> = vec![
        word("Use"),
        chip("$frontend-design", family.clone(), size, cx),
    ];
    for w in "to fix the flaky test in".split(' ') {
        parts.push(word(w));
    }
    parts.push(chip("surface.test.ts", family.clone(), size, cx));
    for w in "and align the header with".split(' ') {
        parts.push(word(w));
    }
    parts.push(chip("SettingsPanels.tsx", family.clone(), size, cx));
    parts.push(word("before"));
    parts.push(word("shipping."));
    preview_box(cx)
        .bg(t.card)
        .child(
            h_flex()
                .flex_wrap()
                .gap_x(px(4.))
                .gap_y(px(2.))
                .px(px(12.))
                .py(px(10.))
                .line_height(px(size * 1.6))
                .text_color(t.foreground)
                .children(parts),
        )
        .into_any_element()
}

fn code_preview(typo: &crate::theme::Typography, cx: &App) -> AnyElement {
    let t = cx.tokens();
    let diff = cx.tern_theme().diff;
    let family = typo.code_family();
    let size = typo.code_size;
    let line = |sign: &'static str, text: &'static str, bg: Hsla, fg: Hsla| {
        h_flex()
            .px(px(12.))
            .bg(bg)
            .text_color(fg)
            .font_family(family.clone())
            .text_size(px(size))
            .line_height(px(size * 1.6))
            .child(div().w(px(16.)).child(sign))
            .child(text)
    };
    let _ = diff;
    preview_box(cx)
        .bg(t.code_background)
        .child(
            div()
                .px(px(12.))
                .py(px(6.))
                .border_b_1()
                .border_color(t.border)
                .font_family(family.clone())
                .text_size(px(12.))
                .text_color(t.muted_foreground)
                .child("src/formatUser.ts"),
        )
        .child(
            v_flex()
                .py(px(4.))
                .child(line(
                    "-",
                    "  return user.name.toUpperCase();",
                    t.destructive.opacity(0.1),
                    t.foreground,
                ))
                .child(line(
                    "+",
                    "  return `${user.name} <${user.email}>`; // 0O 1lI",
                    t.success.opacity(0.1),
                    t.foreground,
                )),
        )
        .into_any_element()
}

fn terminal_preview(typo: &crate::theme::Typography, cx: &App) -> AnyElement {
    let t = cx.tokens();
    let family = typo.terminal_family();
    let size = typo.terminal_size;
    let lines: [(&str, Hsla); 6] = [
        ("→ tern git:(main) ✗ vpr dev", t.terminal_foreground),
        ("VITE v7.1.1  ready in 1.24s", t.success),
        ("→  Local:    http://127.0.0.1:5173/", t.terminal_foreground),
        ("→  Network:  http://192.168.1.24:5173/", t.muted_foreground),
        (
            "✓ 85 passed   △ 2 warnings   ✗ 0 failed",
            t.terminal_foreground,
        ),
        (" READY  watching for changes, press q to quit", t.info),
    ];
    preview_box(cx)
        .h(px(208.))
        .bg(t.terminal_background)
        .p(px(12.))
        .gap(px(2.))
        .children(lines.into_iter().map(|(text, color)| {
            div()
                .font_family(family.clone())
                .text_size(px(size))
                .line_height(px(size * 1.5))
                .text_color(color)
                .child(text)
        }))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_details_defaults_and_overrides() {
        let mut c = ClientSettings::default();
        assert!(thread_detail_visible(&c, "branch"));
        assert!(!thread_detail_visible(&c, "usage"));
        c.thread_details_items.insert("usage".into(), true);
        c.thread_details_items.insert("branch".into(), false);
        assert!(thread_detail_visible(&c, "usage"));
        assert!(!thread_detail_visible(&c, "branch"));
    }
}
