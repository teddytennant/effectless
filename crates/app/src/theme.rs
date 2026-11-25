//! The `Theme` global: every design token from spec/01 resolved for the current
//! appearance and contrast, plus typography, radii, shadows, layout variables and
//! motion constants.
//!
//! Reading colors:
//!
//! ```ignore
//! use crate::theme::ActiveTheme as _;
//! let t = cx.tokens();          // contrast-adjusted Palette for the current scope
//! div().bg(t.background).text_color(t.foreground)
//! let s = cx.states();          // spec 1.5 state colors (hover fills, rings, …)
//! let theme = cx.tern_theme();  // everything else: glass, diff, shadows, fonts
//! ```
//!
//! The left sidebar re-scopes its palette (`[data-app-sidebar]`, spec 1.3). Wrap the
//! sidebar subtree in [`scoped`]`(Scope::Sidebar, …)` and every primitive inside reads
//! the sidebar tokens from `cx.tokens()`, the way CSS custom properties cascade.
//! Popups are deferred and render outside the scope, as T3's portals do.

pub mod color;

use std::time::Duration;

use gpui::{
    AnyElement, App, Bounds, BoxShadow, Element, ElementId, FontWeight, GlobalElementId, Hsla,
    InspectorElementId, IntoElement, LayoutId, Pixels, Window, WindowAppearance, point, px,
};

use crate::tokens::{DiffPalette, Palette, SidebarPalette};
use color::{hex, mix_oklab, mix_srgb};

// ---------------------------------------------------------------------------
// Mode and appearance
// ---------------------------------------------------------------------------

/// The user's theme preference (spec 01 §6). `System` follows the OS live.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "system" => Some(Self::System),
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            _ => None,
        }
    }
}

/// The appearance actually painted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Appearance {
    #[default]
    Light,
    Dark,
}

impl Appearance {
    pub fn is_dark(self) -> bool {
        self == Self::Dark
    }

    pub fn from_window(appearance: WindowAppearance) -> Self {
        match appearance {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Self::Dark,
            WindowAppearance::Light | WindowAppearance::VibrantLight => Self::Light,
        }
    }
}

// ---------------------------------------------------------------------------
// Typography (spec 01 §2)
// ---------------------------------------------------------------------------

/// Font families. Inter (static instances of Inter 4.1, which match Inter Variable at
/// these weights) and Liberation Mono are bundled and registered at startup, so text
/// matches T3 on Linux whatever is installed.
pub mod font {
    use gpui::FontWeight;

    pub const SANS: &str = "Inter";
    pub const MONO: &str = "Liberation Mono";

    pub const NORMAL: FontWeight = FontWeight::NORMAL;
    pub const MEDIUM: FontWeight = FontWeight::MEDIUM;
    pub const SEMIBOLD: FontWeight = FontWeight::SEMIBOLD;
    pub const BOLD: FontWeight = FontWeight::BOLD;

    /// Default font sizes from the settings (`settings.ts:168-190`), in px.
    pub const INTERFACE_SIZE: f32 = 16.;
    pub const PROMPT_SIZE: f32 = 14.;
    pub const CODE_SIZE: f32 = 13.;
    pub const TERMINAL_SIZE: f32 = 12.;
}

/// The text scale (`index.css:186-193` plus Tailwind defaults). Use
/// [`TextExt::text_scale`] to set size and line height together.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextSize {
    /// 7 / 7
    Xs5,
    /// 8 / 8
    Xs4,
    /// 10 / 14
    Xs3,
    /// 11 / 16
    Xs2,
    /// 12 / 16
    Xs,
    /// 14 / 20
    Sm,
    /// 16 / 24
    Base,
    /// 18 / 28
    Lg,
    /// 20 / 28
    Xl,
    /// 24 / 32
    Xl2,
    /// 30 / 36
    Xl3,
}

impl TextSize {
    pub fn size(self) -> Pixels {
        px(match self {
            Self::Xs5 => 7.,
            Self::Xs4 => 8.,
            Self::Xs3 => 10.,
            Self::Xs2 => 11.,
            Self::Xs => 12.,
            Self::Sm => 14.,
            Self::Base => 16.,
            Self::Lg => 18.,
            Self::Xl => 20.,
            Self::Xl2 => 24.,
            Self::Xl3 => 30.,
        })
    }

    pub fn line_height(self) -> Pixels {
        px(match self {
            Self::Xs5 => 7.,
            Self::Xs4 => 8.,
            Self::Xs3 => 14.,
            Self::Xs2 | Self::Xs => 16.,
            Self::Sm => 20.,
            Self::Base => 24.,
            Self::Lg | Self::Xl => 28.,
            Self::Xl2 => 32.,
            Self::Xl3 => 36.,
        })
    }
}

/// Leading utilities as multipliers (`leading-tight` etc.).
pub mod leading {
    pub const NONE: f32 = 1.;
    pub const TIGHT: f32 = 1.25;
    pub const SNUG: f32 = 1.375;
    pub const NORMAL: f32 = 1.5;
    pub const RELAXED: f32 = 1.625;
}

/// Styling helpers shared by every view.
pub trait TextExt: gpui::Styled + Sized {
    /// Font size and line height from the scale, e.g. `.text_scale(TextSize::Sm)` = 14/20.
    fn text_scale(self, size: TextSize) -> Self {
        self.text_size(size.size()).line_height(size.line_height())
    }

    /// `font-medium` (500), the default weight of controls and labels.
    fn font_medium(self) -> Self {
        self.font_weight(font::MEDIUM)
    }

    fn font_semibold(self) -> Self {
        self.font_weight(font::SEMIBOLD)
    }

    fn font_sans(self) -> Self {
        self.font_family(font::SANS)
    }

    fn font_mono(self) -> Self {
        self.font_family(font::MONO)
    }
}

impl<T: gpui::Styled> TextExt for T {}

// ---------------------------------------------------------------------------
// Geometry (spec 01 §3)
// ---------------------------------------------------------------------------

/// Radii (§3.1). `--radius` is 8px.
pub mod radius {
    use gpui::{Pixels, px};
    pub const XS: Pixels = px(2.);
    /// Bare `rounded`, and `rounded-sm`.
    pub const SM: Pixels = px(4.);
    pub const MD: Pixels = px(6.);
    pub const LG: Pixels = px(8.);
    pub const XL: Pixels = px(12.);
    pub const XL2: Pixels = px(16.);
    pub const XL3: Pixels = px(20.);
    pub const FULL: Pixels = px(9999.);
    /// `--control-radius`, buttons and input groups.
    pub const CONTROL: Pixels = px(8.);
}

/// `:root` layout variables (§3.2) and the Linux shell metrics from spec 02 §1.
pub mod layout {
    use gpui::{Pixels, px};
    pub const SCROLLBAR_WIDTH: Pixels = px(6.);
    pub const SIDEBAR_CONTENT_INSET: Pixels = px(8.);
    pub const SIDEBAR_CONTROL_GAP: Pixels = px(8.);
    pub const SIDEBAR_ROW_CONTENT_INSET: Pixels = px(10.);
    pub const COMMAND_SHELL_INSET: Pixels = px(8.);
    pub const COMMAND_CONTENT_INSET: Pixels = px(16.);
    pub const FLOATING_CONTENT_INSET: Pixels = px(12.);
    /// `--workspace-topbar-height` on Linux.
    pub const TOPBAR_HEIGHT: Pixels = px(40.);
    /// `--workspace-controls-left` on Linux.
    pub const CONTROLS_LEFT: Pixels = px(12.);
    pub const TITLEBAR_CONTROL_SIZE: Pixels = px(28.);
    pub const TITLEBAR_CONTROL_GAP: Pixels = px(12.);
    /// `--workspace-titlebar-content-left`: 12 + 28 + 12.
    pub const TITLEBAR_CONTENT_LEFT: Pixels = px(52.);
    pub const TITLEBAR_SCROLL_FADE_HEIGHT: Pixels = px(24.);
    pub const CHAT_CONTENT_MAX_WIDTH: Pixels = px(736.);
    pub const CHAT_CONTENT_MAX_WIDTH_WIDE: Pixels = px(1152.);
    pub const THREAD_DETAILS_PANEL_WIDTH: Pixels = px(280.);
    /// `--workspace-gutter` at >= 640px.
    pub const WORKSPACE_GUTTER: Pixels = px(20.);
    /// Chat lane side padding at >= 640px.
    pub const CHAT_LANE_PADDING: Pixels = px(48.);
    /// Tern's own window controls: 3 buttons of 46px.
    pub const WINDOW_CONTROL_WIDTH: Pixels = px(46.);
    pub const WINDOW_CONTROLS_WIDTH: Pixels = px(138.);
}

/// Z order (§3.5). GPUI paints in tree order; these document which host is on top.
pub mod z {
    pub const TERMINAL_DRAWER: u16 = 45;
    pub const SHEET: u16 = 46;
    pub const DIALOG: u16 = 50;
    pub const TOAST: u16 = 100;
    pub const POPUP: u16 = 130;
    pub const TOOLTIP: u16 = 140;
}

/// Motion constants (§3.6).
pub mod motion {
    use std::time::Duration;

    /// Default transition: 150ms `cubic-bezier(0.4, 0, 0.2, 1)`.
    pub const DEFAULT: Duration = Duration::from_millis(150);
    pub const DIALOG: Duration = Duration::from_millis(200);
    pub const TOAST: Duration = Duration::from_millis(500);
    pub const SKELETON_PERIOD: Duration = Duration::from_millis(2400);
    pub const STATUS_PULSE_PERIOD: Duration = Duration::from_millis(2000);
    pub const STATUS_PING_PERIOD: Duration = Duration::from_millis(2000);
    pub const SPIN_PERIOD: Duration = Duration::from_millis(1000);
    pub const STEP_IN: Duration = Duration::from_millis(180);
    pub const STEP_PING: Duration = Duration::from_millis(520);
    pub const CHECK_DRAW: Duration = Duration::from_millis(200);
    pub const LIVE_TOOL_SHINE_PERIOD: Duration = Duration::from_millis(2200);
    /// `panelAnimationDurationMs` default: panels open and close instantly.
    pub const PANEL: Duration = Duration::ZERO;
    /// Tooltip open delay, reopen window and sidebar delay (§4.9).
    pub const TOOLTIP_DELAY: Duration = Duration::from_millis(600);
    pub const TOOLTIP_REOPEN_WINDOW: Duration = Duration::from_millis(400);
    pub const TOOLTIP_SIDEBAR_DELAY: Duration = Duration::from_millis(150);
    pub const TOAST_TIMEOUT: Duration = Duration::from_millis(5000);

    /// CSS `cubic-bezier(x1, y1, x2, y2)` as an easing function for `Animation::with_easing`.
    pub fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32) -> impl Fn(f32) -> f32 + Clone {
        move |t: f32| {
            let t = t.clamp(0., 1.);
            // Solve x(s) = t for s with Newton steps, then return y(s).
            let bez = |a: f32, b: f32, s: f32| {
                3. * (1. - s) * (1. - s) * s * a + 3. * (1. - s) * s * s * b + s * s * s
            };
            let dbez = |a: f32, b: f32, s: f32| {
                3. * (1. - s) * (1. - s) * a + 6. * (1. - s) * s * (b - a) + 3. * s * s * (1. - b)
            };
            let mut s = t;
            for _ in 0..8 {
                let x = bez(x1, x2, s) - t;
                let d = dbez(x1, x2, s);
                if d.abs() < 1e-6 {
                    break;
                }
                s = (s - x / d).clamp(0., 1.);
            }
            bez(y1, y2, s)
        }
    }

    /// `cubic-bezier(0.4, 0, 0.2, 1)`, Tailwind's default `ease-in-out`.
    pub fn ease_standard() -> impl Fn(f32) -> f32 + Clone {
        cubic_bezier(0.4, 0., 0.2, 1.)
    }

    /// `ease-drawer`: `cubic-bezier(0.32, 0.72, 0, 1)`.
    pub fn ease_drawer() -> impl Fn(f32) -> f32 + Clone {
        cubic_bezier(0.32, 0.72, 0., 1.)
    }

    /// Toast motion: `cubic-bezier(0.22, 1, 0.36, 1)`.
    pub fn ease_toast() -> impl Fn(f32) -> f32 + Clone {
        cubic_bezier(0.22, 1., 0.36, 1.)
    }

    /// CSS `steps(n)` (jump-end): the indicators are deliberately stepped.
    pub fn steps(n: u32) -> impl Fn(f32) -> f32 + Clone {
        move |t: f32| ((t * n as f32).floor() / n as f32).min(1.)
    }
}

// ---------------------------------------------------------------------------
// State colors (spec 01 §1.5)
// ---------------------------------------------------------------------------

/// The `token/n` expressions primitives use, resolved for one appearance and scope.
/// Composite them over the surface beneath (GPUI blends alpha the same way).
#[derive(Clone, Copy, Debug)]
pub struct StateColors {
    /// primary/90, button hover.
    pub primary_hover: Hsla,
    /// primary/24, button drop shadow color.
    pub primary_shadow: Hsla,
    /// ring/24, the field focus band.
    pub ring_focus: Hsla,
    /// ring/50, default outline color.
    pub ring_outline: Hsla,
    /// destructive/90.
    pub destructive_hover: Hsla,
    /// destructive/24, destructive button shadow.
    pub destructive_shadow: Hsla,
    /// destructive/36, invalid border.
    pub invalid_border: Hsla,
    /// destructive/64, invalid + focus border.
    pub invalid_focus_border: Hsla,
    /// destructive/16 light, /24 dark.
    pub invalid_ring: Hsla,
    /// destructive/8 light, /16 dark: error badge background.
    pub error_badge_bg: Hsla,
    /// destructive/4.
    pub destructive_outline_hover_bg: Hsla,
    /// destructive/32.
    pub destructive_outline_hover_border: Hsla,
    /// Outline button hover: accent/50 light, input/64 dark.
    pub outline_hover: Hsla,
    /// Outline control fill: popover light, input/32 dark.
    pub outline_fill: Hsla,
    /// Input-wrapper fill: background light, input/32 dark.
    pub field_fill: Hsla,
    /// Outline badge hover: accent/50 light, input/48 dark.
    pub outline_badge_hover: Hsla,
    /// input/64, toggle pressed.
    pub toggle_pressed: Hsla,
    /// Segmented item pressed: background light, input/72 dark.
    pub segmented_pressed: Hsla,
    /// input/40, segmented track.
    pub segmented_track: Hsla,
    /// Segmented hover: background/55 light, input/32 dark.
    pub segmented_hover: Hsla,
    /// foreground/4 light, /8 dark.
    pub autofill: Hsla,
    /// foreground/6, command/combobox selected.
    pub command_selected: Hsla,
    /// foreground/7, kbd fill.
    pub kbd_fill: Hsla,
    /// foreground/8, select/radio selected, palette kbd.
    pub selected: Hsla,
    /// foreground/9, command/combobox highlight.
    pub command_highlight: Hsla,
    /// foreground/15, kbd outline border.
    pub kbd_border: Hsla,
    /// foreground/55, checkbox `contrast` border.
    pub checkbox_contrast_border: Hsla,
    /// muted-foreground/15, skeleton fill.
    pub skeleton: Hsla,
    /// muted-foreground/55, search icon in popups.
    pub popup_search_icon: Hsla,
    /// muted/72, dialog footer.
    pub dialog_footer: Hsla,
    /// border/60, glass button and toast close orb border.
    pub glass_border: Hsla,
    /// border/70, popup search underline.
    pub popup_search_underline: Hsla,
    /// popover/92, toast close orb.
    pub toast_close_bg: Hsla,
    /// info/8 light, /16 dark.
    pub info_surface: Hsla,
    /// success/8 light, /16 dark.
    pub success_surface: Hsla,
    /// warning/8 light, /16 dark.
    pub warning_badge_surface: Hsla,
    /// warning/32.
    pub warning_border: Hsla,
    /// warning/40.
    pub warning_border_hover: Hsla,
    /// warning/16 light, /24 dark.
    pub warning_hover: Hsla,
    /// secondary/90 and secondary/80.
    pub secondary_hover: Hsla,
    pub secondary_active: Hsla,
}

impl StateColors {
    fn new(p: &Palette, dark: bool) -> Self {
        let by = |light: f32, dark_: f32| if dark { dark_ } else { light };
        Self {
            primary_hover: p.primary.opacity(0.9),
            primary_shadow: p.primary.opacity(0.24),
            ring_focus: p.ring.opacity(0.24),
            ring_outline: p.ring.opacity(0.5),
            destructive_hover: p.destructive.opacity(0.9),
            destructive_shadow: p.destructive.opacity(0.24),
            invalid_border: p.destructive.opacity(0.36),
            invalid_focus_border: p.destructive.opacity(0.64),
            invalid_ring: p.destructive.opacity(by(0.16, 0.24)),
            error_badge_bg: p.destructive.opacity(by(0.08, 0.16)),
            destructive_outline_hover_bg: p.destructive.opacity(0.04),
            destructive_outline_hover_border: p.destructive.opacity(0.32),
            outline_hover: if dark {
                p.input.opacity(0.64)
            } else {
                p.accent.opacity(0.5)
            },
            outline_fill: if dark {
                p.input.opacity(0.32)
            } else {
                p.popover
            },
            field_fill: if dark {
                p.input.opacity(0.32)
            } else {
                p.background
            },
            outline_badge_hover: if dark {
                p.input.opacity(0.48)
            } else {
                p.accent.opacity(0.5)
            },
            toggle_pressed: p.input.opacity(0.64),
            segmented_pressed: if dark {
                p.input.opacity(0.72)
            } else {
                p.background
            },
            segmented_track: p.input.opacity(0.4),
            segmented_hover: if dark {
                p.input.opacity(0.32)
            } else {
                p.background.opacity(0.55)
            },
            autofill: p.foreground.opacity(by(0.04, 0.08)),
            command_selected: p.foreground.opacity(0.06),
            kbd_fill: p.foreground.opacity(0.07),
            selected: p.foreground.opacity(0.08),
            command_highlight: p.foreground.opacity(0.09),
            kbd_border: p.foreground.opacity(0.15),
            checkbox_contrast_border: p.foreground.opacity(0.55),
            skeleton: p.muted_foreground.opacity(0.15),
            popup_search_icon: p.muted_foreground.opacity(0.55),
            dialog_footer: p.muted.opacity(0.72),
            glass_border: p.border.opacity(0.6),
            popup_search_underline: p.border.opacity(0.7),
            toast_close_bg: p.popover.opacity(0.92),
            info_surface: p.info.opacity(by(0.08, 0.16)),
            success_surface: p.success.opacity(by(0.08, 0.16)),
            warning_badge_surface: p.warning.opacity(by(0.08, 0.16)),
            warning_border: p.warning.opacity(0.32),
            warning_border_hover: p.warning.opacity(0.4),
            warning_hover: p.warning.opacity(by(0.16, 0.24)),
            secondary_hover: p.secondary.opacity(0.9),
            secondary_active: p.secondary.opacity(0.8),
        }
    }
}

// ---------------------------------------------------------------------------
// Glass (spec 01 §1.4)
// ---------------------------------------------------------------------------

/// Glass surfaces. GPUI has no backdrop blur, so fills use the CSS fallback the spec
/// prescribes: `surface` and `alert` paint solid background, `dropdown` and `dialog`
/// paint solid popover.
#[derive(Clone, Copy, Debug)]
pub struct Glass {
    /// `surface-glass` fill.
    pub surface: Hsla,
    /// `dropdown-glass` fill (menus, popovers, selects, comboboxes, toasts).
    pub dropdown: Hsla,
    /// `dropdown-glass` border: foreground/10.
    pub dropdown_border: Hsla,
    /// `dialog-glass` fill (dialogs, command palette).
    pub dialog: Hsla,
    /// `dialog-glass` border: foreground/10 light, white/8 dark.
    pub dialog_border: Hsla,
    /// `dialog-backdrop`: background/60 light, /64 dark (no blur).
    pub dialog_backdrop: Hsla,
    /// Translucent fills, for places where the content behind is known to be plain.
    pub surface_translucent: Hsla,
    pub dropdown_translucent: Hsla,
}

impl Glass {
    fn new(p: &Palette, dark: bool) -> Self {
        Self {
            surface: p.background,
            dropdown: p.popover,
            dropdown_border: p.foreground.opacity(0.1),
            dialog: p.popover,
            dialog_border: if dark {
                hex(0xffffff14)
            } else {
                p.foreground.opacity(0.1)
            },
            dialog_backdrop: p.background.opacity(if dark { 0.64 } else { 0.6 }),
            surface_translucent: p.background.opacity(0.8),
            dropdown_translucent: mix_srgb(p.popover, p.popover.opacity(0.8), 0.18),
        }
    }

    /// `alert-glass`: surface glass plus a 4% tint of the variant color.
    pub fn alert(&self, tint: Hsla) -> Hsla {
        color::over(tint.opacity(0.04), self.surface)
    }
}

// ---------------------------------------------------------------------------
// Shadows (spec 01 §3.3)
// ---------------------------------------------------------------------------

fn shadow(x: f32, y: f32, blur: f32, spread: f32, color: Hsla) -> BoxShadow {
    BoxShadow {
        color,
        offset: point(px(x), px(y)),
        blur_radius: px(blur),
        spread_radius: px(spread),
        inset: false,
    }
}

fn inset_shadow(y: f32, color: Hsla) -> BoxShadow {
    BoxShadow {
        color,
        offset: point(px(0.), px(y)),
        blur_radius: px(0.),
        spread_radius: px(0.),
        inset: true,
    }
}

fn black(a: f32) -> Hsla {
    gpui::black().opacity(a)
}

fn white(a: f32) -> Hsla {
    gpui::white().opacity(a)
}

/// Shadow recipes. Each returns the layers to pass to `.shadow(...)`.
#[derive(Clone, Copy, Debug)]
pub struct Shadows {
    dark: bool,
}

impl Shadows {
    /// `shadow-xs` at the given alpha (`shadow-xs/5` = 0.05): `0 1px 2px 0`.
    pub fn xs(&self, color: Hsla) -> Vec<BoxShadow> {
        vec![shadow(0., 1., 2., 0., color)]
    }

    /// `shadow-xs/5`.
    pub fn xs5(&self) -> Vec<BoxShadow> {
        self.xs(black(0.05))
    }

    /// `shadow-sm` at alpha (`shadow-sm` = 0.1, `shadow-sm/5` = 0.05).
    pub fn sm(&self, alpha: f32) -> Vec<BoxShadow> {
        vec![
            shadow(0., 1., 3., 0., black(alpha)),
            shadow(0., 1., 2., -1., black(alpha)),
        ]
    }

    /// `shadow-md` at alpha (`shadow-md/5` = 0.05).
    pub fn md(&self, alpha: f32) -> Vec<BoxShadow> {
        vec![
            shadow(0., 4., 6., -1., black(alpha)),
            shadow(0., 2., 4., -2., black(alpha)),
        ]
    }

    /// `shadow-lg` at alpha (`shadow-lg/5` = 0.05).
    pub fn lg(&self, alpha: f32) -> Vec<BoxShadow> {
        vec![
            shadow(0., 10., 15., -3., black(alpha)),
            shadow(0., 4., 6., -4., black(alpha)),
        ]
    }

    /// `shadow-xl shadow-black/25`.
    pub fn xl(&self, alpha: f32) -> Vec<BoxShadow> {
        vec![
            shadow(0., 20., 25., -5., black(alpha)),
            shadow(0., 8., 10., -6., black(alpha)),
        ]
    }

    /// `shadow-2xl` at alpha: `0 25px 50px -12px`.
    pub fn xl2(&self, alpha: f32) -> Vec<BoxShadow> {
        vec![shadow(0., 25., 50., -12., black(alpha))]
    }

    /// Menus, popovers, selects, comboboxes (`menu.tsx:58`).
    pub fn popup(&self) -> Vec<BoxShadow> {
        if self.dark {
            vec![shadow(0., 18., 44., -18., black(0.8))]
        } else {
            vec![shadow(0., 16., 40., -18., black(0.55))]
        }
    }

    /// `dialog-glass`.
    pub fn dialog(&self) -> Vec<BoxShadow> {
        if self.dark {
            vec![
                inset_shadow(1., white(0.04)),
                shadow(0., 24., 72., -20., black(0.9)),
            ]
        } else {
            vec![shadow(0., 24., 64., -24., black(0.65))]
        }
    }

    /// `shadow-composer` / `shadow-composer-dark`.
    pub fn composer(&self) -> Vec<BoxShadow> {
        if self.dark {
            vec![shadow(0., 14., 32., -18., black(0.75))]
        } else {
            vec![shadow(0., 1., 2., 0., black(0.05))]
        }
    }

    /// `inset-shadow-control-highlight`: `inset 0 1px white/16%`.
    pub fn control_highlight(&self) -> BoxShadow {
        inset_shadow(1., white(0.16))
    }

    /// `inset-shadow-control-pressed`: `inset 0 1px black/8%`.
    pub fn control_pressed(&self) -> BoxShadow {
        inset_shadow(1., black(0.08))
    }

    /// Edge line color (§0): drawn on the bottom border in light (black/4%) and on
    /// the top border in dark (white/6%). See `ui::edge_line`.
    pub fn edge_line(&self) -> (Hsla, bool) {
        if self.dark {
            (white(0.06), true)
        } else {
            (black(0.04), false)
        }
    }
}

// ---------------------------------------------------------------------------
// Appearance contrast (spec 01 §1.7)
// ---------------------------------------------------------------------------

/// `appearanceContrast`, 50..=200, default 100.
pub const DEFAULT_CONTRAST: u16 = 100;

struct Contrast {
    base: f32,
    boost: f32,
    border_boost: f32,
    target: Hsla,
}

impl Contrast {
    fn new(c: u16, dark: bool) -> Self {
        let c = c.clamp(50, 200) as f32;
        Self {
            base: c.min(100.) / 100.,
            boost: (c - 100.).max(0.) / 100.,
            border_boost: (c - 100.).max(0.) / 4. / 100.,
            target: if dark { gpui::white() } else { gpui::black() },
        }
    }

    fn is_identity(&self) -> bool {
        self.base >= 1. && self.boost <= 0.
    }

    /// `mix(oklab, mix(oklab, X base, SURFACE), target boost)`.
    fn text(&self, x: Hsla, surface: Hsla) -> Hsla {
        if self.is_identity() {
            return x;
        }
        let faded = mix_oklab(x, surface, self.base);
        mix_oklab(faded, self.target, 1. - self.boost)
    }

    /// `mix(srgb, mix(srgb, X base, transparent), FG border-boost)`.
    fn line(&self, x: Hsla, fg: Hsla) -> Hsla {
        if self.is_identity() {
            return x;
        }
        let faded = x.opacity(self.base);
        mix_srgb(faded, fg, 1. - self.border_boost)
    }

    fn apply(&self, raw: &Palette) -> Palette {
        let mut p = *raw;
        if self.is_identity() {
            return p;
        }
        let bg = raw.background;
        p.foreground = self.text(raw.foreground, bg);
        p.muted_foreground = self.text(raw.muted_foreground, bg);
        p.placeholder = self.text(raw.placeholder, bg);
        p.secondary_label = self.text(raw.secondary_label, bg);
        p.icon_muted = self.text(raw.icon_muted, bg);
        p.card_foreground = self.text(raw.card_foreground, raw.card);
        p.popover_foreground = self.text(raw.popover_foreground, raw.popover);
        p.accent_foreground = self.text(raw.accent_foreground, raw.accent);
        p.secondary_foreground = self.text(raw.secondary_foreground, raw.secondary);
        p.message_foreground = self.text(raw.message_foreground, raw.message_surface);
        p.sidebar_foreground = self.text(raw.sidebar_foreground, raw.sidebar);
        p.sidebar_muted_foreground = self.text(raw.sidebar_muted_foreground, raw.sidebar);
        p.toolbar_foreground = self.text(raw.toolbar_foreground, raw.toolbar_background);
        p.toolbar_control_foreground =
            self.text(raw.toolbar_control_foreground, raw.toolbar_control);
        p.border = self.line(raw.border, raw.foreground);
        p.input = self.line(raw.input, raw.foreground);
        p.sidebar_border = self.line(raw.sidebar_border, raw.sidebar_foreground);
        p.toolbar_border = self.line(raw.toolbar_border, raw.toolbar_foreground);
        p
    }
}

/// The root palette with the `[data-app-sidebar]` overrides applied (spec 1.3).
fn sidebar_scope(root: &Palette, s: &SidebarPalette) -> Palette {
    let mut p = *root;
    p.background = s.background;
    p.foreground = s.foreground;
    p.card = s.card;
    p.accent = s.accent;
    p.accent_foreground = s.accent_foreground;
    p.muted = s.muted;
    p.muted_foreground = s.muted_foreground;
    p.secondary_label = s.secondary_label;
    p.border = s.border;
    p.input = s.input;
    p.sidebar = s.sidebar;
    p.sidebar_foreground = s.sidebar_foreground;
    p.sidebar_muted_foreground = s.sidebar_muted_foreground;
    p.sidebar_control_surface = s.sidebar_control_surface;
    p.sidebar_row_hover = s.sidebar_row_hover;
    p.sidebar_row_active = s.sidebar_row_active;
    p.sidebar_row_selected = s.sidebar_row_selected;
    p.sidebar_border = s.sidebar_border;
    // --sidebar-icon-color: mix(srgb, sidebar-muted-fg 85%, sidebar).
    p.sidebar_icon_color = mix_srgb(s.sidebar_muted_foreground, s.sidebar, 0.85);
    p
}

// ---------------------------------------------------------------------------
// Theme global
// ---------------------------------------------------------------------------

/// Extra tokens that are not in the generated palette.
#[derive(Clone, Copy, Debug)]
pub struct Extra {
    pub find_highlight_fg: Hsla,
    pub find_highlight_bg: Hsla,
    pub active_find_highlight_fg: Hsla,
    pub active_find_highlight_bg: Hsla,
    /// Linux window-control glyph color (`titleBarOverlay.symbolColor`).
    pub window_symbol: Hsla,
    /// Text selection inside inputs (`selection:bg-primary`).
    pub selection: Hsla,
}

impl Extra {
    fn new(p: &Palette, dark: bool) -> Self {
        Self {
            find_highlight_fg: hex(if dark { 0xf5f5f5ff } else { 0x21180aff }),
            find_highlight_bg: hex(if dark { 0x8c6c1fff } else { 0xffde8bff }),
            active_find_highlight_fg: hex(0x21180aff),
            active_find_highlight_bg: hex(0xeea52bff),
            window_symbol: hex(if dark { 0xf8fafcff } else { 0x1f2937ff }),
            selection: p.primary.opacity(0.3),
        }
    }
}

/// The resolved theme. Rebuilt whenever mode, system appearance or contrast change.
#[derive(Clone, Debug)]
pub struct Theme {
    pub mode: ThemeMode,
    pub appearance: Appearance,
    pub contrast: u16,
    /// Root tokens, contrast-adjusted (what `--color-*` utilities read).
    pub colors: Palette,
    /// Root tokens with the sidebar subtree overrides, contrast-adjusted.
    pub sidebar_colors: Palette,
    /// The raw (unadjusted) root tokens, for code that needs the base value.
    pub raw: Palette,
    pub diff: DiffPalette,
    pub states: StateColors,
    pub sidebar_states: StateColors,
    pub glass: Glass,
    pub shadows: Shadows,
    pub extra: Extra,
}

impl gpui::Global for Theme {}

impl Theme {
    pub fn new(mode: ThemeMode, system: Appearance, contrast: u16) -> Self {
        let appearance = match mode {
            ThemeMode::System => system,
            ThemeMode::Light => Appearance::Light,
            ThemeMode::Dark => Appearance::Dark,
        };
        let dark = appearance.is_dark();
        let (raw, raw_sidebar, diff) = if dark {
            (Palette::dark(), SidebarPalette::dark(), DiffPalette::dark())
        } else {
            (
                Palette::light(),
                SidebarPalette::light(),
                DiffPalette::light(),
            )
        };
        let contrast_rules = Contrast::new(contrast, dark);
        let colors = contrast_rules.apply(&raw);
        let sidebar_colors = contrast_rules.apply(&sidebar_scope(&raw, &raw_sidebar));
        Self {
            mode,
            appearance,
            contrast,
            states: StateColors::new(&colors, dark),
            sidebar_states: StateColors::new(&sidebar_colors, dark),
            glass: Glass::new(&colors, dark),
            shadows: Shadows { dark },
            extra: Extra::new(&colors, dark),
            colors,
            sidebar_colors,
            raw,
            diff,
        }
    }

    pub fn is_dark(&self) -> bool {
        self.appearance.is_dark()
    }

    pub fn global(cx: &App) -> &Theme {
        cx.global::<Theme>()
    }

    /// Installs the theme for `mode`, resolving `System` against the window.
    pub fn init(mode: ThemeMode, window: &Window, cx: &mut App) {
        let theme = Theme::new(
            mode,
            Appearance::from_window(window.appearance()),
            DEFAULT_CONTRAST,
        );
        cx.set_global(theme);
        sync_component_theme(cx);
    }

    /// Changes the preference. Repaints every window.
    pub fn set_mode(mode: ThemeMode, window: &Window, cx: &mut App) {
        let contrast = cx.global::<Theme>().contrast;
        let system = Appearance::from_window(window.appearance());
        cx.set_global(Theme::new(mode, system, contrast));
        sync_component_theme(cx);
        cx.refresh_windows();
    }

    /// Changes the appearance contrast (50..=200).
    pub fn set_contrast(contrast: u16, window: &Window, cx: &mut App) {
        let mode = cx.global::<Theme>().mode;
        let system = Appearance::from_window(window.appearance());
        cx.set_global(Theme::new(mode, system, contrast));
        sync_component_theme(cx);
        cx.refresh_windows();
    }

    /// Call when the window's system appearance changes; only `System` reacts.
    pub fn sync_system_appearance(window: &Window, cx: &mut App) {
        let theme = cx.global::<Theme>();
        if theme.mode != ThemeMode::System {
            return;
        }
        let next = Appearance::from_window(window.appearance());
        if next != theme.appearance {
            let contrast = theme.contrast;
            cx.set_global(Theme::new(ThemeMode::System, next, contrast));
            sync_component_theme(cx);
            cx.refresh_windows();
        }
    }
}

/// Read access to the theme from any context.
pub trait ActiveTheme {
    fn tern_theme(&self) -> &Theme;
    /// The palette for the current [`Scope`] (root, or sidebar inside the sidebar).
    fn tokens(&self) -> &Palette;
    /// The §1.5 state colors for the current [`Scope`].
    fn states(&self) -> &StateColors;
}

impl ActiveTheme for App {
    fn tern_theme(&self) -> &Theme {
        self.global::<Theme>()
    }

    fn tokens(&self) -> &Palette {
        let theme = self.global::<Theme>();
        match current_scope(self) {
            Scope::Root => &theme.colors,
            Scope::Sidebar => &theme.sidebar_colors,
        }
    }

    fn states(&self) -> &StateColors {
        let theme = self.global::<Theme>();
        match current_scope(self) {
            Scope::Root => &theme.states,
            Scope::Sidebar => &theme.sidebar_states,
        }
    }
}

// ---------------------------------------------------------------------------
// Palette scope
// ---------------------------------------------------------------------------

/// Which token set `cx.tokens()` returns while a subtree is built.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scope {
    #[default]
    Root,
    /// `[data-app-sidebar]`.
    Sidebar,
}

#[derive(Default)]
struct ScopeStack(Vec<Scope>);
impl gpui::Global for ScopeStack {}

fn current_scope(cx: &App) -> Scope {
    cx.try_global::<ScopeStack>()
        .and_then(|s| s.0.last().copied())
        .unwrap_or_default()
}

/// Builds `child` (and every component and view inside it) with `scope` active.
pub fn scoped(scope: Scope, child: impl IntoElement) -> Scoped {
    Scoped {
        scope,
        child: Some(child.into_any_element()),
    }
}

/// See [`scoped`].
pub struct Scoped {
    scope: Scope,
    child: Option<AnyElement>,
}

impl Element for Scoped {
    type RequestLayoutState = AnyElement;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, AnyElement) {
        let mut child = self.child.take().expect("Scoped laid out twice");
        cx.default_global::<ScopeStack>().0.push(self.scope);
        let layout_id = child.request_layout(window, cx);
        cx.default_global::<ScopeStack>().0.pop();
        (layout_id, child)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        child: &mut AnyElement,
        window: &mut Window,
        cx: &mut App,
    ) {
        cx.default_global::<ScopeStack>().0.push(self.scope);
        child.prepaint(window, cx);
        cx.default_global::<ScopeStack>().0.pop();
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        child: &mut AnyElement,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        child.paint(window, cx);
    }
}

// ---------------------------------------------------------------------------
// gpui-component
// ---------------------------------------------------------------------------

/// Points gpui-component's theme at these tokens so its widgets (inputs, scrollbars,
/// lists, the editor) paint like T3's.
pub fn sync_component_theme(cx: &mut App) {
    use gpui_component::{Theme as CTheme, ThemeMode as CMode, scroll::ScrollbarMode};

    let theme = cx.global::<Theme>().clone();
    let p = theme.colors;
    let s = theme.states;
    let mode = if theme.is_dark() {
        CMode::Dark
    } else {
        CMode::Light
    };
    if CTheme::global(cx).mode != mode {
        CTheme::change(mode, None, cx);
    }
    CTheme::update(cx, |t| {
        t.font_family = font::SANS.into();
        t.font_size = px(14.);
        t.mono_font_family = font::MONO.into();
        t.mono_font_size = px(font::CODE_SIZE);
        t.radius = radius::LG;
        t.radius_lg = radius::XL2;
        t.shadow = true;
        t.scrollbar_mode = ScrollbarMode::Hover;

        let c = &mut t.colors;
        c.background = p.background;
        c.foreground = p.foreground;
        c.border = p.border;
        c.input = p.input;
        c.ring = p.ring;
        c.caret = p.foreground;
        c.selection = theme.extra.selection;
        c.muted = p.muted;
        c.muted_foreground = p.muted_foreground;
        c.accent = p.accent;
        c.accent_foreground = p.accent_foreground;
        c.popover = p.popover;
        c.popover_foreground = p.popover_foreground;
        c.primary = p.primary;
        c.primary_hover = s.primary_hover;
        c.primary_active = s.primary_hover;
        c.primary_foreground = p.primary_foreground;
        c.secondary = p.secondary;
        c.secondary_hover = s.secondary_hover;
        c.secondary_active = s.secondary_active;
        c.secondary_foreground = p.secondary_foreground;
        c.danger = p.destructive;
        c.danger_hover = s.destructive_hover;
        c.danger_active = s.destructive_hover;
        c.danger_foreground = gpui::white();
        c.info = p.info;
        c.info_foreground = p.info_foreground;
        c.success = p.success;
        c.success_foreground = p.success_foreground;
        c.warning = p.warning;
        c.warning_foreground = p.warning_foreground;
        c.link = p.primary;
        c.link_hover = s.primary_hover;
        c.link_active = p.primary;
        c.list = gpui::transparent_black();
        c.list_hover = p.accent;
        c.list_active = s.selected;
        c.list_active_border = gpui::transparent_black();
        c.list_even = gpui::transparent_black();
        c.list_head = p.background;
        c.list_highlight_foreground = p.foreground;
        c.scrollbar = gpui::transparent_black();
        c.scrollbar_thumb = p.app_scrollbar_thumb;
        c.scrollbar_thumb_hover = p.app_scrollbar_thumb_hover;
        c.skeleton = s.skeleton;
        c.switch = p.input;
        c.switch_thumb = p.background;
        c.sidebar = p.sidebar;
        c.sidebar_foreground = p.sidebar_foreground;
        c.sidebar_border = p.sidebar_border;
        c.sidebar_accent = p.sidebar_row_hover;
        c.sidebar_accent_foreground = p.sidebar_foreground;
        c.title_bar = p.app_chrome_background;
        c.title_bar_border = p.border;
        c.overlay = theme.glass.dialog_backdrop;
        c.drag_border = p.ring;
        c.window_border = p.border;
        c.tab_bar_segmented = s.segmented_track;
    });
}

/// How long `Duration` constants above compare with zero (panel motion is off by default).
pub fn animates(duration: Duration) -> bool {
    !duration.is_zero()
}

/// `FontWeight` re-export so callers do not need gpui's path.
pub type Weight = FontWeight;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_contrast_is_identity() {
        let t = Theme::new(ThemeMode::Light, Appearance::Light, 100);
        assert_eq!(t.colors.foreground, Palette::light().foreground);
        assert_eq!(t.colors.border, Palette::light().border);
    }

    #[test]
    fn low_contrast_fades_text_and_lines() {
        let t = Theme::new(ThemeMode::Light, Appearance::Light, 50);
        let raw = Palette::light();
        assert!(t.colors.foreground.l > raw.foreground.l);
        assert!(t.colors.border.a < raw.border.a);
    }

    #[test]
    fn high_contrast_moves_toward_target() {
        let t = Theme::new(ThemeMode::Dark, Appearance::Dark, 200);
        let raw = Palette::dark();
        assert!(t.colors.muted_foreground.l > raw.muted_foreground.l);
    }

    #[test]
    fn sidebar_scope_in_dark_is_black() {
        let t = Theme::new(ThemeMode::Dark, Appearance::Dark, 100);
        assert_eq!(t.sidebar_colors.sidebar, hex(0x000000ff));
        assert_eq!(t.colors.sidebar, hex(0x111111ff));
    }

    #[test]
    fn steps_are_discrete() {
        let f = motion::steps(4);
        assert_eq!(f(0.1), 0.);
        assert_eq!(f(0.3), 0.25);
    }

    #[test]
    fn standard_ease_is_monotonic() {
        let f = motion::ease_standard();
        let mut last = 0.;
        for i in 0..=20 {
            let v = f(i as f32 / 20.);
            assert!(v >= last - 1e-4);
            last = v;
        }
        assert!((f(1.) - 1.).abs() < 1e-3);
    }
}
