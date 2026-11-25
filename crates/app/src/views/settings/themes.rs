//! Built-in named themes (spec 06 §9.1, spec 01 §6 and Appendix A): T3 Chat, Grove,
//! Ocean, Ember and Iris, each with a light and a dark half. A theme's roles are
//! painted over the stock palette through [`crate::theme::PaletteOverride`].

use std::sync::Arc;

use gpui::Hsla;

use crate::theme::PaletteOverride;
use crate::theme::color::hex;
use crate::tokens::{Palette, SidebarPalette};

/// The stock theme (`default`; selecting it stores the bare mode string).
pub const DEFAULT_ID: &str = "default";

pub struct BuiltinTheme {
    pub id: &'static str,
    pub label: &'static str,
    /// Column of light mode in [`ROLES`]; dark is the next one.
    column: usize,
}

pub const THEMES: &[BuiltinTheme] = &[
    BuiltinTheme {
        id: DEFAULT_ID,
        label: "Tern",
        column: 0,
    },
    BuiltinTheme {
        id: "t3-chat",
        label: "T3 Chat",
        column: 2,
    },
    BuiltinTheme {
        id: "grove",
        label: "Grove",
        column: 4,
    },
    BuiltinTheme {
        id: "ocean",
        label: "Ocean",
        column: 6,
    },
    BuiltinTheme {
        id: "ember",
        label: "Ember",
        column: 8,
    },
    BuiltinTheme {
        id: "iris",
        label: "Iris",
        column: 10,
    },
];

/// Legacy ids alias to the current ones (`themePalette.ts:409-415`).
pub fn canonical_id(id: &str) -> &str {
    match id {
        "t3-chat-dark" => "t3-chat",
        "t3-grove" => "grove",
        "t3-ocean" => "ocean",
        "t3-ember" => "ember",
        "t3-iris" => "iris",
        other => other,
    }
}

pub fn find(id: &str) -> Option<&'static BuiltinTheme> {
    let id = canonical_id(id);
    THEMES.iter().find(|t| t.id == id)
}

/// A role's color for a theme half, from Appendix A.
pub fn role(theme: &BuiltinTheme, dark: bool, name: &str) -> Option<Hsla> {
    let col = theme.column + usize::from(dark);
    ROLES
        .iter()
        .find(|(r, _)| *r == name)
        .map(|(_, v)| hex((v[col] << 8) | 0xff))
}

/// The preview circle colors: (canvas, accent, messageAction).
pub fn preview_colors(theme: &BuiltinTheme, dark: bool, stock: &Palette) -> (Hsla, Hsla, Hsla) {
    if theme.id == DEFAULT_ID {
        // The stock theme paints from spec 01 §1.2, not the shared reference palette.
        return (stock.background, stock.primary, stock.message_action);
    }
    let get = |n| role(theme, dark, n).unwrap_or(stock.background);
    (get("canvas"), get("accent"), get("messageAction"))
}

fn apply(theme: &BuiltinTheme, dark: bool, p: &mut Palette, s: &mut SidebarPalette) {
    let r = |n: &str| role(theme, dark, n);
    macro_rules! set {
        ($dst:expr, $role:literal) => {
            if let Some(c) = r($role) {
                $dst = c;
            }
        };
    }
    // index.css:1358-1424
    set!(p.background, "canvas");
    set!(p.app_chrome_background, "chrome");
    set!(p.card, "surface");
    set!(p.card_foreground, "text");
    set!(p.popover, "surfaceOverlay");
    set!(p.popover_foreground, "text");
    set!(p.surface_raised, "surfaceRaised");
    set!(p.foreground, "text");
    set!(p.primary, "messageAction");
    set!(p.primary_foreground, "messageActionForeground");
    set!(p.secondary, "secondary");
    set!(p.secondary_foreground, "secondaryForeground");
    set!(p.muted, "muted");
    set!(p.muted_foreground, "mutedForeground");
    set!(p.placeholder, "placeholder");
    set!(p.secondary_label, "secondaryLabel");
    set!(p.icon_muted, "iconMuted");
    set!(p.accent, "accentSurface");
    set!(p.accent_foreground, "accentSurfaceForeground");
    set!(p.message_surface, "messageSurface");
    set!(p.message_foreground, "messageForeground");
    set!(p.message_action, "messageAction");
    set!(p.message_action_foreground, "messageActionForeground");
    set!(p.message_action_hover, "messageActionHover");
    set!(p.error, "error");
    set!(p.error_foreground, "errorForeground");
    set!(p.error_surface, "errorSurface");
    set!(p.warning, "warning");
    set!(p.warning_foreground, "warningForeground");
    set!(p.warning_surface, "warningSurface");
    set!(p.update, "update");
    set!(p.update_foreground, "updateForeground");
    set!(p.border, "border");
    set!(p.input, "input");
    set!(p.ring, "focus");
    set!(p.code_background, "codeBackground");
    set!(p.code_foreground, "codeForeground");
    set!(p.terminal_background, "terminalBackground");
    set!(p.terminal_foreground, "terminalForeground");
    set!(p.terminal_cursor, "terminalCursor");
    set!(p.terminal_selection_background, "terminalSelection");
    set!(p.toolbar_background, "toolbar");
    set!(p.toolbar_foreground, "toolbarForeground");
    set!(p.toolbar_border, "toolbarBorder");
    set!(p.toolbar_control, "toolbarControl");
    set!(p.toolbar_control_foreground, "toolbarControlForeground");
    set!(p.toolbar_control_hover, "toolbarControlHover");
    set!(p.sidebar, "sidebar");
    set!(p.sidebar_foreground, "sidebarForeground");
    set!(p.sidebar_muted_foreground, "sidebarMutedForeground");
    set!(p.sidebar_control_surface, "sidebarControlSurface");
    set!(p.sidebar_row_hover, "sidebarRowHover");
    set!(p.sidebar_row_active, "sidebarRowActive");
    set!(p.sidebar_row_selected, "sidebarRowSelected");
    set!(p.sidebar_border, "sidebarBorder");
    // index.css:1587-1615: the sidebar subtree reads the sidebar roles.
    set!(s.background, "sidebar");
    set!(s.foreground, "sidebarForeground");
    set!(s.card, "sidebarControlSurface");
    set!(s.accent, "sidebarRowHover");
    set!(s.accent_foreground, "sidebarForeground");
    set!(s.muted, "sidebarControlSurface");
    set!(s.muted_foreground, "sidebarMutedForeground");
    set!(s.secondary_label, "sidebarMutedForeground");
    set!(s.border, "sidebarBorder");
    set!(s.input, "sidebarBorder");
    set!(s.sidebar, "sidebar");
    set!(s.sidebar_foreground, "sidebarForeground");
    set!(s.sidebar_muted_foreground, "sidebarMutedForeground");
    set!(s.sidebar_control_surface, "sidebarControlSurface");
    set!(s.sidebar_row_hover, "sidebarRowHover");
    set!(s.sidebar_row_active, "sidebarRowActive");
    set!(s.sidebar_row_selected, "sidebarRowSelected");
    set!(s.sidebar_border, "sidebarBorder");
}

/// The override for a (light theme, dark theme) pair; None when both are stock.
pub fn palette_override(light: &str, dark: &str) -> Option<PaletteOverride> {
    let light = find(light).filter(|t| t.id != DEFAULT_ID);
    let dark = find(dark).filter(|t| t.id != DEFAULT_ID);
    if light.is_none() && dark.is_none() {
        return None;
    }
    let id = format!(
        "{}/{}",
        light.map_or(DEFAULT_ID, |t| t.id),
        dark.map_or(DEFAULT_ID, |t| t.id)
    );
    Some(PaletteOverride {
        id: id.into(),
        apply: Arc::new(move |p, s, is_dark| {
            if let Some(theme) = if is_dark { dark } else { light } {
                apply(theme, is_dark, p, s);
            }
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_column_has_every_role() {
        assert!(ROLES.len() >= 60);
        for t in THEMES {
            assert!(role(t, true, "canvas").is_some(), "{}", t.id);
        }
    }

    #[test]
    fn legacy_ids_resolve() {
        assert_eq!(find("t3-ocean").map(|t| t.id), Some("ocean"));
        assert!(find("nope").is_none());
    }

    #[test]
    fn stock_pair_has_no_override() {
        assert!(palette_override("default", "default").is_none());
        let o = palette_override("default", "grove").unwrap();
        let mut p = Palette::dark();
        let mut s = SidebarPalette::dark();
        (o.apply)(&mut p, &mut s, true);
        assert_eq!(p.background, hex(0x1b2821ff));
        let mut p = Palette::light();
        (o.apply)(&mut p, &mut s, false);
        assert_eq!(p.background, Palette::light().background);
    }
}

// Generated from spec/01 Appendix A (T3 `themePalettes.ts`). Columns: T3_CODE light,
// dark, then light/dark for T3 Chat, Grove, Ocean, Ember, Iris.
pub(super) const ROLES: &[(&str, [u32; 12])] = &[
    (
        "canvas",
        [
            0xfcfcfc, 0x0a0a0a, 0xfdf7fd, 0x1f1a24, 0xf3f7f4, 0x1b2821, 0xf5f7f8, 0x17212b,
            0xf9f7f5, 0x291e1a, 0xf8f7f9, 0x1d1929,
        ],
    ),
    (
        "chrome",
        [
            0xfcfcfc, 0x0a0a0a, 0xfdf7fd, 0x1f1a24, 0xf3f7f4, 0x1b2821, 0xf5f7f8, 0x17212b,
            0xf9f7f5, 0x291e1a, 0xf8f7f9, 0x1d1929,
        ],
    ),
    (
        "toolbar",
        [
            0xfcfcfc, 0x0a0a0a, 0xfdf7fd, 0x1f1a24, 0xf3f7f4, 0x1b2821, 0xf5f7f8, 0x17212b,
            0xf9f7f5, 0x291e1a, 0xf8f7f9, 0x1d1929,
        ],
    ),
    (
        "toolbarForeground",
        [
            0x27272a, 0xf5f5f5, 0x501854, 0xf9f8fb, 0x241523, 0xfffaff, 0x241523, 0xfffaff,
            0x241523, 0xfffaff, 0x241523, 0xfffaff,
        ],
    ),
    (
        "toolbarBorder",
        [
            0xe4e4e7, 0x191919, 0xefbdeb, 0x27242c, 0xd5e6dd, 0x36654c, 0xd8e4ee, 0x36566f,
            0xeee0d9, 0x6e4934, 0xe5e0f0, 0x4a3c70,
        ],
    ),
    (
        "toolbarControl",
        [
            0xffffff, 0x111111, 0xf3e6f5, 0x362d3d, 0xe2ede7, 0x2a4b39, 0xe4ecf2, 0x293f52,
            0xf3eae5, 0x513728, 0xedeaf4, 0x362d51,
        ],
    ),
    (
        "toolbarControlForeground",
        [
            0x27272a, 0xf5f5f5, 0x501854, 0xd4c7e1, 0x241523, 0xfffaff, 0x241523, 0xfffaff,
            0x241523, 0xfffaff, 0x241523, 0xfffaff,
        ],
    ),
    (
        "toolbarControlHover",
        [
            0xf4f4f5, 0x141414, 0xeccfe3, 0x463753, 0xd5e6dd, 0x325c46, 0xd8e4ee, 0x324e66,
            0xeee0d9, 0x644330, 0xe5e0f0, 0x433765,
        ],
    ),
    (
        "surface",
        [
            0xffffff, 0x111111, 0xfaf3fb, 0x29232d, 0xf3f7f4, 0x1b2821, 0xf5f7f8, 0x17212b,
            0xf9f7f5, 0x291e1a, 0xf8f7f9, 0x1d1929,
        ],
    ),
    (
        "surfaceRaised",
        [
            0xfcfcfc, 0x111111, 0xfdfafd, 0x2c2631, 0xecefed, 0x36413c, 0xedeff1, 0x333b45,
            0xf1efee, 0x433835, 0xf0eff2, 0x383443,
        ],
    ),
    (
        "surfaceOverlay",
        [
            0xffffff, 0x111111, 0xffffff, 0x100a0e, 0xe7e9e8, 0x444d49, 0xe8e9eb, 0x414851,
            0xece9e9, 0x4f4543, 0xebe9ed, 0x454250,
        ],
    ),
    (
        "text",
        [
            0x27272a, 0xf5f5f5, 0x501854, 0xf9f8fb, 0x241523, 0xfffaff, 0x241523, 0xfffaff,
            0x241523, 0xfffaff, 0x241523, 0xfffaff,
        ],
    ),
    (
        "textMuted",
        [
            0x71717b, 0x818181, 0xac1668, 0xe7d0dd, 0x746c73, 0x919595, 0x746c75, 0x8d8f97,
            0x766c74, 0x968e8f, 0x766c76, 0x8e8a95,
        ],
    ),
    (
        "border",
        [
            0xe4e4e7, 0x191919, 0xeee1ed, 0x27242c, 0xcbd5d1, 0x415f4f, 0xcdd4dc, 0x405567,
            0xddd2ce, 0x664c3f, 0xd6d1de, 0x4d4366,
        ],
    ),
    (
        "input",
        [
            0xd4d4d8, 0x1e1e1e, 0xe7c1dc, 0x302029, 0xbecbc5, 0x4f725f, 0xc0c9d4, 0x4f677b,
            0xd4c6c1, 0x7a5d4d, 0xccc5d6, 0x5d527b,
        ],
    ),
    (
        "focus",
        [
            0x1b4ed8, 0x346bf1, 0xdb2777, 0xdb2777, 0x1b7d50, 0x69d69a, 0x2672af, 0x70b9ee,
            0xae552a, 0xf09a64, 0x7253b9, 0x9d7df2,
        ],
    ),
    (
        "accent",
        [
            0x1b4ed8, 0x346bf1, 0xdb2777, 0xa3004c, 0x1b7d50, 0x69d69a, 0x2672af, 0x70b9ee,
            0xae552a, 0xf09a64, 0x7253b9, 0x9d7df2,
        ],
    ),
    (
        "accentForeground",
        [
            0xffffff, 0xffffff, 0xffffff, 0xfbd0e8, 0xfffaff, 0x241523, 0xfffaff, 0x241523,
            0xfffaff, 0x241523, 0xfffaff, 0x241523,
        ],
    ),
    (
        "secondary",
        [
            0xfafafa, 0x111111, 0xf1c4e6, 0x362d3d, 0xe2ede7, 0x2a4b39, 0xe4ecf2, 0x293f52,
            0xf3eae5, 0x513728, 0xedeaf4, 0x362d51,
        ],
    ),
    (
        "secondaryForeground",
        [
            0x27272a, 0xf5f5f5, 0x77347c, 0xd4c7e1, 0x241523, 0xfffaff, 0x241523, 0xfffaff,
            0x241523, 0xfffaff, 0x241523, 0xfffaff,
        ],
    ),
    (
        "muted",
        [
            0xfafafa, 0x111111, 0xeaa7cb, 0x423a45, 0xe6f0ea, 0x253e31, 0xe8eff4, 0x233544,
            0xf4ede9, 0x432e23, 0xf0edf6, 0x2d2643,
        ],
    ),
    (
        "mutedForeground",
        [
            0x71717b, 0x818181, 0x8d1255, 0xe7d0dd, 0x6e696f, 0x9da5a2, 0x6f6873, 0x969ca6,
            0x74686f, 0xa59996, 0x726874, 0x9690a1,
        ],
    ),
    (
        "placeholder",
        [
            0x71717b, 0x818181, 0x8b5f90, 0x968d9f, 0x716971, 0xa9abab, 0x716972, 0xa4a4ac,
            0x736971, 0xaba3a5, 0x736973, 0xa29ea8,
        ],
    ),
    (
        "secondaryLabel",
        [
            0x71717b, 0x818181, 0xac1668, 0xe7d0dd, 0x746c73, 0x919595, 0x746c75, 0x8d8f97,
            0x766c74, 0x968e8f, 0x766c76, 0x8e8a95,
        ],
    ),
    (
        "iconMuted",
        [
            0x71717b, 0x818181, 0xac1668, 0xd4c7e1, 0x746c73, 0x919595, 0x746c75, 0x8d8f97,
            0x766c74, 0x968e8f, 0x766c76, 0x8e8a95,
        ],
    ),
    (
        "error",
        [
            0xfb2c36, 0xfb414a, 0xf7086c, 0x9d174d, 0xfb2c36, 0xfb414a, 0xfb2c36, 0xfb414a,
            0xfb2c36, 0xfb414a, 0xfb2c36, 0xfb414a,
        ],
    ),
    (
        "errorForeground",
        [
            0xc10007, 0xff6467, 0x9d174d, 0xfbd0e8, 0xc10007, 0xff6668, 0xc10007, 0xff6467,
            0xc10007, 0xff6467, 0xc10007, 0xff6467,
        ],
    ),
    (
        "errorSurface",
        [
            0xfcebec, 0x301214, 0xfde4f1, 0x331a2b, 0xf4e7e5, 0x3f2c28, 0xf5e6e9, 0x3c2630,
            0xf9e7e6, 0x4a2321, 0xf8e6ea, 0x40202e,
        ],
    ),
    (
        "warning",
        [
            0xfe9a00, 0xfe9a00, 0xf59e0b, 0xf59e0b, 0xfe9a00, 0xfe9a00, 0xfe9a00, 0xfe9a00,
            0xfe9a00, 0xfe9a00, 0xfe9a00, 0xfe9a00,
        ],
    ),
    (
        "warningForeground",
        [
            0xbb4d00, 0xffb900, 0xb05109, 0xfbbf24, 0xb64a00, 0xffb900, 0xb74b00, 0xffb900,
            0xb84b00, 0xffb900, 0xb84b00, 0xffb900,
        ],
    ),
    (
        "warningSurface",
        [
            0xfcf4e8, 0x312108, 0xfcf0ea, 0x412f20, 0xf4f0e1, 0x3f3a1c, 0xf6efe4, 0x3c3424,
            0xf9efe2, 0x4b3215, 0xf8efe5, 0x412e23,
        ],
    ),
    (
        "update",
        [
            0x1b4ed8, 0x346bf1, 0xdb2777, 0xa3004c, 0x1b7d50, 0x69d69a, 0x2672af, 0x70b9ee,
            0xae552a, 0xf09a64, 0x7253b9, 0x9d7df2,
        ],
    ),
    (
        "updateForeground",
        [
            0x1b4ed8, 0x51a2ff, 0xac1668, 0xfbd0e8, 0x125134, 0x9ee4be, 0x194a72, 0xa2d2f4,
            0x71381b, 0xf5bd9a, 0x4a3678, 0xbfabf7,
        ],
    ),
    (
        "updateSurface",
        [
            0xe0e6f7, 0x121b34, 0xfadfef, 0x37152b, 0xd0e3da, 0x345f48, 0xd4e1ed, 0x345269,
            0xedddd5, 0x684631, 0xe2ddef, 0x463969,
        ],
    ),
    (
        "accentSurface",
        [
            0xf4f4f5, 0x141414, 0xf3e6f5, 0x463753, 0xd5e6dd, 0x325c46, 0xd8e4ee, 0x324e66,
            0xeee0d9, 0x644330, 0xe5e0f0, 0x433765,
        ],
    ),
    (
        "accentSurfaceForeground",
        [
            0x18181b, 0xf5f5f5, 0x454554, 0xf8f1f5, 0x241523, 0xfffaff, 0x241523, 0xfffaff,
            0x241523, 0xfffaff, 0x241523, 0xfffaff,
        ],
    ),
    (
        "messageSurface",
        [
            0xf4f4f5, 0x141414, 0xf7def2, 0x2b2431, 0xcce1d7, 0x37664d, 0xd0dfeb, 0x375871,
            0xebdad1, 0x704b34, 0xe0d9ee, 0x4b3d72,
        ],
    ),
    (
        "messageForeground",
        [
            0x27272a, 0xf5f5f5, 0x492c61, 0xf2ebfa, 0x241523, 0xfffaff, 0x241523, 0xfffaff,
            0x241523, 0xfffaff, 0x241523, 0xfffaff,
        ],
    ),
    (
        "messageAction",
        [
            0x1b4ed8, 0x346bf1, 0xdb2777, 0xa3004c, 0x8f6410, 0xe3b34e, 0x0a6f75, 0x5bd0d6,
            0xb23535, 0xf78a7a, 0xa82c87, 0xf099d8,
        ],
    ),
    (
        "messageActionForeground",
        [
            0xffffff, 0xffffff, 0xffffff, 0xfbd0e8, 0xfffaff, 0x241523, 0xfffaff, 0x241523,
            0xfffaff, 0x241523, 0xfffaff, 0x241523,
        ],
    ),
    (
        "messageActionHover",
        [
            0x3160db, 0x3061d9, 0xc12269, 0xa2004c, 0x7e580e, 0xe6bc63, 0x096267, 0x6fd6db,
            0x9d2f2f, 0xf8988a, 0x942777, 0xf2a5dd,
        ],
    ),
    (
        "codeBackground",
        [
            0xffffff, 0x111111, 0xf5ecf9, 0x1f1a24, 0xeef1ef, 0x28342e, 0xf0f1f3, 0x252e38,
            0xf3f1f0, 0x362b27, 0xf2f1f4, 0x2a2736,
        ],
    ),
    (
        "codeForeground",
        [
            0x27272a, 0xf5f5f5, 0x673c8b, 0xd8c3ef, 0x241523, 0xfffaff, 0x241523, 0xfffaff,
            0x241523, 0xfffaff, 0x241523, 0xfffaff,
        ],
    ),
    (
        "searchMatchBackground",
        [
            0xffde8b, 0x8c6c1f, 0xffde8b, 0x8c6c1f, 0xffde8b, 0x8c6c1f, 0xffde8b, 0x8c6c1f,
            0xffde8b, 0x8c6c1f, 0xffde8b, 0x8c6c1f,
        ],
    ),
    (
        "searchMatchForeground",
        [
            0x21180a, 0xf5f5f5, 0x21180a, 0xf5f5f5, 0x21180a, 0xf5f5f5, 0x21180a, 0xf5f5f5,
            0x21180a, 0xf5f5f5, 0x21180a, 0xf5f5f5,
        ],
    ),
    (
        "searchMatchActiveBackground",
        [
            0xeea52b, 0xeea52b, 0xeea52b, 0xeea52b, 0xeea52b, 0xeea52b, 0xeea52b, 0xeea52b,
            0xeea52b, 0xeea52b, 0xeea52b, 0xeea52b,
        ],
    ),
    (
        "searchMatchActiveForeground",
        [
            0x21180a, 0x21180a, 0x21180a, 0x21180a, 0x21180a, 0x21180a, 0x21180a, 0x21180a,
            0x21180a, 0x21180a, 0x21180a, 0x21180a,
        ],
    ),
    (
        "sidebar",
        [
            0xfafafa, 0x000000, 0xf2e1f4, 0x171018, 0xe2ede7, 0x21362b, 0xe4ecf2, 0x1e2d3b,
            0xf3eae5, 0x39281f, 0xedeaf4, 0x272139,
        ],
    ),
    (
        "sidebarForeground",
        [
            0x27272a, 0xf1f3f7, 0x454554, 0xf4f4f5, 0x241523, 0xfffaff, 0x241523, 0xfffaff,
            0x241523, 0xfffaff, 0x241523, 0xfffaff,
        ],
    ),
    (
        "sidebarMutedForeground",
        [
            0x71717b, 0xa3a3a3, 0xac1668, 0xe7d0dd, 0x6b666c, 0x9da3a2, 0x6c6570, 0x989ca5,
            0x71646b, 0xa49998, 0x6f6471, 0x9792a0,
        ],
    ),
    (
        "sidebarControlSurface",
        [
            0xf4f4f5, 0x0a0a0a, 0xf8f8f7, 0x261922, 0xd3dcd8, 0x45554d, 0xd5dbe2, 0x424e5a,
            0xe2d9d6, 0x584943, 0xddd9e3, 0x494459,
        ],
    ),
    (
        "sidebarRowHover",
        [
            0xfcfcfc, 0x131313, 0xf8f8f7, 0x261922, 0xcae0d5, 0x2a4938, 0xcdddea, 0x283e50,
            0xead8cf, 0x4f3528, 0xded8ed, 0x352c4f,
        ],
    ),
    (
        "sidebarRowActive",
        [
            0xffffff, 0x1a1b1b, 0xf8f8f7, 0x261922, 0xbad7c9, 0x2f5641, 0xbed4e5, 0x2f495f,
            0xe5ccc0, 0x5d3f2d, 0xd4cce8, 0x3f345e,
        ],
    ),
    (
        "sidebarRowSelected",
        [
            0xffffff, 0x111111, 0xf8f8f7, 0x261922, 0xb2d2c3, 0x325c46, 0xb7cfe2, 0x324f66,
            0xe2c6b8, 0x654330, 0xcfc6e6, 0x433766,
        ],
    ),
    (
        "sidebarBorder",
        [
            0xe4e4e7, 0x141414, 0xeceae9, 0x322028, 0xcbd3d0, 0x6f7a75, 0xcdd2d9, 0x6d757f,
            0xdad0ce, 0x7e716e, 0xd5d0db, 0x736d7e,
        ],
    ),
    (
        "terminalBackground",
        [
            0xfcfcfc, 0x0a0a0a, 0xfdf7fd, 0x1f1a24, 0xf3f7f4, 0x1b2821, 0xf5f7f8, 0x17212b,
            0xf9f7f5, 0x291e1a, 0xf8f7f9, 0x1d1929,
        ],
    ),
    (
        "terminalForeground",
        [
            0x27272a, 0xf5f5f5, 0x501854, 0xf9f8fb, 0x241523, 0xfffaff, 0x241523, 0xfffaff,
            0x241523, 0xfffaff, 0x241523, 0xfffaff,
        ],
    ),
    (
        "terminalCursor",
        [
            0x26384e, 0xb4cbff, 0xdb2777, 0xdb2777, 0x1b7d50, 0x69d69a, 0x2672af, 0x70b9ee,
            0xae552a, 0xf09a64, 0x7253b9, 0x9d7df2,
        ],
    ),
    (
        "terminalSelection",
        [
            0xd0d6dd, 0x343a47, 0xf1c4e6, 0x362d3d, 0xcce1d7, 0x36654c, 0xd0dfeb, 0x36566f,
            0xebdad1, 0x6e4934, 0xe0d9ee, 0x4a3c70,
        ],
    ),
    (
        "terminalScrollbar",
        [
            0xd6d6d6, 0x222222, 0xe7c1dc, 0x302029, 0xc5c5c6, 0x7b807e, 0xc7c5c9, 0x797c84,
            0xcac5c7, 0x837a7a, 0xc9c5ca, 0x7c7883,
        ],
    ),
    (
        "terminalScrollbarHover",
        [
            0xbdbdbd, 0x363636, 0xeaa7cb, 0x423a45, 0xb1afb1, 0x989b9b, 0xb2aeb4, 0x9798a0,
            0xb5afb2, 0x9f9798, 0xb4aeb5, 0x99959f,
        ],
    ),
];
