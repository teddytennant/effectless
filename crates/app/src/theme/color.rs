//! Color math the token pipeline needs: CSS `color-mix()` in sRGB and Oklab with
//! premultiplied alpha, and compositing over an opaque surface.

use gpui::{Hsla, Rgba};

/// Premultiplied components in some 3-channel space plus alpha.
#[derive(Clone, Copy, Debug)]
struct Pre {
    c: [f32; 3],
    a: f32,
}

fn srgb_channels(color: Hsla) -> ([f32; 3], f32) {
    let rgba: Rgba = color.into();
    ([rgba.r, rgba.g, rgba.b], rgba.a)
}

fn from_srgb(c: [f32; 3], a: f32) -> Hsla {
    Rgba {
        r: c[0].clamp(0., 1.),
        g: c[1].clamp(0., 1.),
        b: c[2].clamp(0., 1.),
        a: a.clamp(0., 1.),
    }
    .into()
}

fn to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn from_linear(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1. / 2.4) - 0.055
    }
}

fn srgb_to_oklab(c: [f32; 3]) -> [f32; 3] {
    let [r, g, b] = c.map(to_linear);
    let l = 0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b;
    let m = 0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b;
    let s = 0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b;
    let (l, m, s) = (l.cbrt(), m.cbrt(), s.cbrt());
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

fn oklab_to_srgb(c: [f32; 3]) -> [f32; 3] {
    let [ll, a, b] = c;
    let l = ll + 0.396_337_78 * a + 0.215_803_76 * b;
    let m = ll - 0.105_561_346 * a - 0.063_854_17 * b;
    let s = ll - 0.089_484_18 * a - 1.291_485_5 * b;
    let (l, m, s) = (l * l * l, m * m * m, s * s * s);
    [
        4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s,
        -1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s,
        -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s,
    ]
    .map(from_linear)
}

fn mix_pre(a: Pre, b: Pre, weight_a: f32) -> Pre {
    let wb = 1. - weight_a;
    let alpha = a.a * weight_a + b.a * wb;
    let mut c = [0.; 3];
    for (i, slot) in c.iter_mut().enumerate() {
        let premul = a.c[i] * a.a * weight_a + b.c[i] * b.a * wb;
        *slot = if alpha > 0. { premul / alpha } else { 0. };
    }
    Pre { c, a: alpha }
}

/// `color-mix(in srgb, a weight_a, b)`. `weight_a` is 0..=1.
pub fn mix_srgb(a: Hsla, b: Hsla, weight_a: f32) -> Hsla {
    let (ac, aa) = srgb_channels(a);
    let (bc, ba) = srgb_channels(b);
    let out = mix_pre(Pre { c: ac, a: aa }, Pre { c: bc, a: ba }, weight_a);
    from_srgb(out.c, out.a)
}

/// `color-mix(in oklab, a weight_a, b)`. `weight_a` is 0..=1.
pub fn mix_oklab(a: Hsla, b: Hsla, weight_a: f32) -> Hsla {
    let (ac, aa) = srgb_channels(a);
    let (bc, ba) = srgb_channels(b);
    let out = mix_pre(
        Pre {
            c: srgb_to_oklab(ac),
            a: aa,
        },
        Pre {
            c: srgb_to_oklab(bc),
            a: ba,
        },
        weight_a,
    );
    from_srgb(oklab_to_srgb(out.c), out.a)
}

/// Source-over composite of `top` onto `bottom` (any alphas).
pub fn over(top: Hsla, bottom: Hsla) -> Hsla {
    let (tc, ta) = srgb_channels(top);
    let (bc, ba) = srgb_channels(bottom);
    let a = ta + ba * (1. - ta);
    if a <= 0. {
        return from_srgb([0.; 3], 0.);
    }
    let mut c = [0.; 3];
    for i in 0..3 {
        c[i] = (tc[i] * ta + bc[i] * ba * (1. - ta)) / a;
    }
    from_srgb(c, a)
}

/// `#rrggbbaa` to Hsla, same as `tokens::c`.
pub fn hex(rgba: u32) -> Hsla {
    crate::tokens::c(rgba)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Hsla, b: Hsla) -> bool {
        let (ac, aa) = srgb_channels(a);
        let (bc, ba) = srgb_channels(b);
        (0..3).all(|i| (ac[i] - bc[i]).abs() < 0.004) && (aa - ba).abs() < 0.004
    }

    #[test]
    fn oklab_round_trips() {
        for c in [0x1b4ed8ff, 0xfcfcfcff, 0x000000ff, 0xfb2c36ff, 0x71717bff] {
            let color = hex(c);
            assert!(close(mix_oklab(color, color, 0.5), color), "{c:x}");
        }
    }

    #[test]
    fn message_action_hover_matches_spec() {
        // message-action-hover = mix(srgb, primary 90%, background); spec 1.2.
        assert!(close(
            mix_srgb(hex(0x1b4ed8ff), hex(0xfcfcfcff), 0.9),
            hex(0x3160dbff)
        ));
        assert!(close(
            mix_srgb(hex(0x346bf1ff), hex(0x000000ff), 0.9),
            hex(0x2f60d9ff)
        ));
    }

    #[test]
    fn sidebar_icon_color_matches_spec() {
        // mix(srgb, sidebar-muted-fg 85%, sidebar); spec 1.2.
        assert!(close(
            mix_srgb(hex(0x71717bff), hex(0xfafafaff), 0.85),
            hex(0x86858eff)
        ));
    }

    #[test]
    fn premultiplied_mix_with_transparent_keeps_color() {
        let c = mix_srgb(hex(0xe4e4e7ff), hex(0x00000000), 0.5);
        assert!(close(c, hex(0xe4e4e780)));
    }
}
