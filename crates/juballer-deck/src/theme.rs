//! Theme module — Catppuccin-tonal palette + a swappable `Theme` struct.
//!
//! Widgets should read colors and font sizes from here (or from
//! `DeckApp::theme`) rather than hardcoding values. Two presets are
//! provided: Mocha (dark, default) and Latte (light).

use egui::Color32;

// ---- Catppuccin Mocha (dark) ----
pub const MOCHA_ROSEWATER: Color32 = rgb(0xf5, 0xe0, 0xdc);
pub const MOCHA_FLAMINGO: Color32 = rgb(0xf2, 0xcd, 0xcd);
pub const MOCHA_PINK: Color32 = rgb(0xf5, 0xc2, 0xe7);
pub const MOCHA_MAUVE: Color32 = rgb(0xcb, 0xa6, 0xf7);
pub const MOCHA_RED: Color32 = rgb(0xf3, 0x8b, 0xa8);
pub const MOCHA_MAROON: Color32 = rgb(0xeb, 0xa0, 0xac);
pub const MOCHA_PEACH: Color32 = rgb(0xfa, 0xb3, 0x87);
pub const MOCHA_YELLOW: Color32 = rgb(0xf9, 0xe2, 0xaf);
pub const MOCHA_GREEN: Color32 = rgb(0xa6, 0xe3, 0xa1);
pub const MOCHA_TEAL: Color32 = rgb(0x94, 0xe2, 0xd5);
pub const MOCHA_SKY: Color32 = rgb(0x89, 0xdc, 0xeb);
pub const MOCHA_SAPPHIRE: Color32 = rgb(0x74, 0xc7, 0xec);
pub const MOCHA_BLUE: Color32 = rgb(0x89, 0xb4, 0xfa);
pub const MOCHA_LAVENDER: Color32 = rgb(0xb4, 0xbe, 0xfe);
pub const MOCHA_TEXT: Color32 = rgb(0xcd, 0xd6, 0xf4);
pub const MOCHA_SUBTEXT1: Color32 = rgb(0xba, 0xc2, 0xde);
pub const MOCHA_SUBTEXT0: Color32 = rgb(0xa6, 0xad, 0xc8);
pub const MOCHA_OVERLAY2: Color32 = rgb(0x9c, 0xa0, 0xb0);
pub const MOCHA_OVERLAY1: Color32 = rgb(0x7f, 0x84, 0x9c);
pub const MOCHA_OVERLAY0: Color32 = rgb(0x6c, 0x70, 0x86);
pub const MOCHA_SURFACE2: Color32 = rgb(0x58, 0x5b, 0x70);
pub const MOCHA_SURFACE1: Color32 = rgb(0x45, 0x47, 0x5a);
pub const MOCHA_SURFACE0: Color32 = rgb(0x31, 0x32, 0x44);
pub const MOCHA_BASE: Color32 = rgb(0x1e, 0x1e, 0x2e);
pub const MOCHA_MANTLE: Color32 = rgb(0x18, 0x18, 0x25);
pub const MOCHA_CRUST: Color32 = rgb(0x11, 0x11, 0x1b);

// ---- Catppuccin Frappe (middle-dark) ----
pub const FRAPPE_ROSEWATER: Color32 = rgb(0xf2, 0xd5, 0xcf);
pub const FRAPPE_FLAMINGO: Color32 = rgb(0xee, 0xbe, 0xbe);
pub const FRAPPE_PINK: Color32 = rgb(0xf4, 0xb8, 0xe4);
pub const FRAPPE_MAUVE: Color32 = rgb(0xca, 0x9e, 0xe6);
pub const FRAPPE_RED: Color32 = rgb(0xe7, 0x82, 0x84);
pub const FRAPPE_MAROON: Color32 = rgb(0xea, 0x99, 0x9c);
pub const FRAPPE_PEACH: Color32 = rgb(0xef, 0x9f, 0x76);
pub const FRAPPE_YELLOW: Color32 = rgb(0xe5, 0xc8, 0x90);
pub const FRAPPE_GREEN: Color32 = rgb(0xa6, 0xd1, 0x89);
pub const FRAPPE_TEAL: Color32 = rgb(0x81, 0xc8, 0xbe);
pub const FRAPPE_SKY: Color32 = rgb(0x99, 0xd1, 0xdb);
pub const FRAPPE_SAPPHIRE: Color32 = rgb(0x85, 0xc1, 0xdc);
pub const FRAPPE_BLUE: Color32 = rgb(0x8c, 0xaa, 0xee);
pub const FRAPPE_LAVENDER: Color32 = rgb(0xba, 0xbb, 0xf1);
pub const FRAPPE_TEXT: Color32 = rgb(0xc6, 0xd0, 0xf5);
pub const FRAPPE_SUBTEXT1: Color32 = rgb(0xb5, 0xbf, 0xe2);
pub const FRAPPE_SUBTEXT0: Color32 = rgb(0xa5, 0xad, 0xce);
pub const FRAPPE_OVERLAY2: Color32 = rgb(0x94, 0x9c, 0xbb);
pub const FRAPPE_OVERLAY1: Color32 = rgb(0x83, 0x8b, 0xa7);
pub const FRAPPE_OVERLAY0: Color32 = rgb(0x73, 0x7a, 0x94);
pub const FRAPPE_SURFACE2: Color32 = rgb(0x62, 0x68, 0x80);
pub const FRAPPE_SURFACE1: Color32 = rgb(0x51, 0x57, 0x6d);
pub const FRAPPE_SURFACE0: Color32 = rgb(0x41, 0x45, 0x59);
pub const FRAPPE_BASE: Color32 = rgb(0x30, 0x34, 0x46);
pub const FRAPPE_MANTLE: Color32 = rgb(0x29, 0x2c, 0x3c);
pub const FRAPPE_CRUST: Color32 = rgb(0x23, 0x26, 0x34);

// ---- Catppuccin Latte (light) ----
pub const LATTE_ROSEWATER: Color32 = rgb(0xdc, 0x8a, 0x78);
pub const LATTE_FLAMINGO: Color32 = rgb(0xdd, 0x7f, 0x8e);
pub const LATTE_PINK: Color32 = rgb(0xea, 0x76, 0xcb);
pub const LATTE_MAUVE: Color32 = rgb(0x88, 0x39, 0xef);
pub const LATTE_RED: Color32 = rgb(0xd2, 0x0f, 0x39);
pub const LATTE_MAROON: Color32 = rgb(0xe6, 0x45, 0x53);
pub const LATTE_PEACH: Color32 = rgb(0xfe, 0x64, 0x0b);
pub const LATTE_YELLOW: Color32 = rgb(0xdf, 0x8e, 0x1d);
pub const LATTE_GREEN: Color32 = rgb(0x40, 0xa0, 0x2b);
pub const LATTE_TEAL: Color32 = rgb(0x17, 0x92, 0x99);
pub const LATTE_SKY: Color32 = rgb(0x04, 0xa5, 0xe5);
pub const LATTE_SAPPHIRE: Color32 = rgb(0x20, 0x9f, 0xb5);
pub const LATTE_BLUE: Color32 = rgb(0x1e, 0x66, 0xf5);
pub const LATTE_LAVENDER: Color32 = rgb(0x71, 0x87, 0xe7);
pub const LATTE_TEXT: Color32 = rgb(0x4c, 0x4f, 0x69);
pub const LATTE_SUBTEXT1: Color32 = rgb(0x5c, 0x5f, 0x77);
pub const LATTE_SUBTEXT0: Color32 = rgb(0x6c, 0x6f, 0x85);
pub const LATTE_OVERLAY2: Color32 = rgb(0x7c, 0x7f, 0x93);
pub const LATTE_OVERLAY1: Color32 = rgb(0x8c, 0x8f, 0xa1);
pub const LATTE_OVERLAY0: Color32 = rgb(0x9c, 0xa0, 0xb0);
pub const LATTE_SURFACE2: Color32 = rgb(0xac, 0xb0, 0xbe);
pub const LATTE_SURFACE1: Color32 = rgb(0xbc, 0xc0, 0xcc);
pub const LATTE_SURFACE0: Color32 = rgb(0xcc, 0xd0, 0xda);
pub const LATTE_BASE: Color32 = rgb(0xef, 0xf1, 0xf5);
pub const LATTE_MANTLE: Color32 = rgb(0xe6, 0xe9, 0xef);
pub const LATTE_CRUST: Color32 = rgb(0xdc, 0xe0, 0xe8);

// ---- Typography scale ----
pub const FONT_SMALL: f32 = 11.0;
pub const FONT_BODY: f32 = 14.0;
pub const FONT_HEADER: f32 = 18.0;
pub const FONT_LARGE: f32 = 24.0;

// ---- Spacing ----
pub const PAD_INNER: f32 = 8.0;

/// Runtime-selectable theme. Widgets read from `app.theme` so light/dark is a drop-in swap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub base: Color32,
    pub mantle: Color32,
    pub crust: Color32,
    pub surface0: Color32,
    pub surface1: Color32,
    pub surface2: Color32,
    pub overlay0: Color32,
    pub overlay1: Color32,
    pub overlay2: Color32,
    pub text: Color32,
    pub subtext0: Color32,
    pub subtext1: Color32,
    pub accent: Color32,
    pub accent_alt: Color32,
    pub ok: Color32,
    pub warn: Color32,
    pub err: Color32,
    pub info: Color32,
}

impl Theme {
    pub const fn mocha() -> Self {
        Self {
            base: MOCHA_BASE,
            mantle: MOCHA_MANTLE,
            crust: MOCHA_CRUST,
            surface0: MOCHA_SURFACE0,
            surface1: MOCHA_SURFACE1,
            surface2: MOCHA_SURFACE2,
            overlay0: MOCHA_OVERLAY0,
            overlay1: MOCHA_OVERLAY1,
            overlay2: MOCHA_OVERLAY2,
            text: MOCHA_TEXT,
            subtext0: MOCHA_SUBTEXT0,
            subtext1: MOCHA_SUBTEXT1,
            accent: MOCHA_LAVENDER,
            accent_alt: MOCHA_BLUE,
            ok: MOCHA_GREEN,
            warn: MOCHA_YELLOW,
            err: MOCHA_RED,
            info: MOCHA_SAPPHIRE,
        }
    }

    pub const fn frappe() -> Self {
        Self {
            base: FRAPPE_BASE,
            mantle: FRAPPE_MANTLE,
            crust: FRAPPE_CRUST,
            surface0: FRAPPE_SURFACE0,
            surface1: FRAPPE_SURFACE1,
            surface2: FRAPPE_SURFACE2,
            overlay0: FRAPPE_OVERLAY0,
            overlay1: FRAPPE_OVERLAY1,
            overlay2: FRAPPE_OVERLAY2,
            text: FRAPPE_TEXT,
            subtext0: FRAPPE_SUBTEXT0,
            subtext1: FRAPPE_SUBTEXT1,
            accent: FRAPPE_LAVENDER,
            accent_alt: FRAPPE_BLUE,
            ok: FRAPPE_GREEN,
            warn: FRAPPE_YELLOW,
            err: FRAPPE_RED,
            info: FRAPPE_SAPPHIRE,
        }
    }

    pub const fn latte() -> Self {
        Self {
            base: LATTE_BASE,
            mantle: LATTE_MANTLE,
            crust: LATTE_CRUST,
            surface0: LATTE_SURFACE0,
            surface1: LATTE_SURFACE1,
            surface2: LATTE_SURFACE2,
            overlay0: LATTE_OVERLAY0,
            overlay1: LATTE_OVERLAY1,
            overlay2: LATTE_OVERLAY2,
            text: LATTE_TEXT,
            subtext0: LATTE_SUBTEXT0,
            subtext1: LATTE_SUBTEXT1,
            accent: LATTE_LAVENDER,
            accent_alt: LATTE_BLUE,
            ok: LATTE_GREEN,
            warn: LATTE_YELLOW,
            err: LATTE_RED,
            info: LATTE_SAPPHIRE,
        }
    }

    /// Parse "mocha" | "frappe" | "latte" (default mocha).
    pub fn from_name(name: &str) -> Self {
        match name {
            "latte" => Self::latte(),
            "frappe" => Self::frappe(),
            _ => Self::mocha(),
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::mocha()
    }
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// Parse a color spec accepted by plugin messages: `"#rrggbb"`, `"#rrggbbaa"`,
/// or a Catppuccin-mocha token like `"red"`, `"green"`, `"surface0"`.
/// Returns an egui Color32 to match widget paint consumers.
pub fn parse_named_color(s: &str) -> Option<Color32> {
    if let Some(c) = named_catppuccin(s) {
        return Some(c);
    }
    let s = s.strip_prefix('#')?;
    let bytes = match s.len() {
        6 => hex::decode(s).ok()?,
        8 => hex::decode(s).ok()?,
        _ => return None,
    };
    let a = if bytes.len() == 4 { bytes[3] } else { 0xff };
    Some(Color32::from_rgba_unmultiplied(
        bytes[0], bytes[1], bytes[2], a,
    ))
}

/// Parse the same spec as [`parse_named_color`] but return a
/// `juballer_core::Color` suitable for the tile `state_color` slot.
pub fn parse_named_color_core(s: &str) -> Option<juballer_core::Color> {
    let c = parse_named_color(s)?;
    Some(juballer_core::Color::rgba(c.r(), c.g(), c.b(), c.a()))
}

fn named_catppuccin(s: &str) -> Option<Color32> {
    match s {
        "rosewater" => Some(MOCHA_ROSEWATER),
        "flamingo" => Some(MOCHA_FLAMINGO),
        "pink" => Some(MOCHA_PINK),
        "mauve" => Some(MOCHA_MAUVE),
        "red" => Some(MOCHA_RED),
        "maroon" => Some(MOCHA_MAROON),
        "peach" => Some(MOCHA_PEACH),
        "yellow" => Some(MOCHA_YELLOW),
        "green" => Some(MOCHA_GREEN),
        "teal" => Some(MOCHA_TEAL),
        "sky" => Some(MOCHA_SKY),
        "sapphire" => Some(MOCHA_SAPPHIRE),
        "blue" => Some(MOCHA_BLUE),
        "lavender" => Some(MOCHA_LAVENDER),
        "text" => Some(MOCHA_TEXT),
        "subtext1" => Some(MOCHA_SUBTEXT1),
        "subtext0" => Some(MOCHA_SUBTEXT0),
        "overlay2" => Some(MOCHA_OVERLAY2),
        "overlay1" => Some(MOCHA_OVERLAY1),
        "overlay0" => Some(MOCHA_OVERLAY0),
        "surface2" => Some(MOCHA_SURFACE2),
        "surface1" => Some(MOCHA_SURFACE1),
        "surface0" => Some(MOCHA_SURFACE0),
        "base" => Some(MOCHA_BASE),
        "mantle" => Some(MOCHA_MANTLE),
        "crust" => Some(MOCHA_CRUST),
        "white" => Some(Color32::WHITE),
        "black" => Some(Color32::BLACK),
        "transparent" => Some(Color32::TRANSPARENT),
        _ => None,
    }
}

/// Exponential approach helper. Moves `*current` toward `target` with time constant
/// `time_const` (seconds to 1-1/e of the gap) using the frame `dt` (seconds).
///
/// `current += (target - current) * (1 - exp(-dt/time_const))`.
pub fn ease_to(current: &mut f32, target: f32, dt: f32, time_const: f32) {
    if time_const <= 0.0 || dt <= 0.0 {
        *current = target;
        return;
    }
    let alpha = 1.0 - (-dt / time_const).exp();
    *current += (target - *current) * alpha;
}

/// Cubic ease-out: fast start, slow end. Input in `[0, 1]`.
pub fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let u = 1.0 - t;
    1.0 - u * u * u
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ease_to_converges() {
        let mut v = 0.0f32;
        for _ in 0..120 {
            ease_to(&mut v, 100.0, 1.0 / 60.0, 0.2);
        }
        assert!((v - 100.0).abs() < 0.5, "expected ~100, got {v}");
    }

    #[test]
    fn ease_to_snaps_on_zero_time_const() {
        let mut v = 0.0f32;
        ease_to(&mut v, 42.0, 0.016, 0.0);
        assert_eq!(v, 42.0);
    }

    #[test]
    fn ease_out_cubic_endpoints() {
        assert!((ease_out_cubic(0.0)).abs() < 1e-6);
        assert!((ease_out_cubic(1.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn theme_from_name() {
        assert_eq!(Theme::from_name("latte"), Theme::latte());
        assert_eq!(Theme::from_name("mocha"), Theme::mocha());
        assert_eq!(Theme::from_name("frappe"), Theme::frappe());
        assert_eq!(Theme::from_name("unknown"), Theme::mocha());
    }

    #[test]
    fn parse_named_color_hex() {
        let c = parse_named_color("#ff0080").unwrap();
        assert_eq!((c.r(), c.g(), c.b()), (0xff, 0x00, 0x80));
    }

    #[test]
    fn parse_named_color_name() {
        let c = parse_named_color("red").unwrap();
        assert_eq!(c, MOCHA_RED);
    }

    #[test]
    fn parse_named_color_invalid() {
        assert!(parse_named_color("not-a-color").is_none());
        assert!(parse_named_color("#abc").is_none());
    }

    #[test]
    fn parse_named_color_core_converts() {
        let c = parse_named_color_core("red").unwrap();
        assert_eq!(c.0, MOCHA_RED.r());
        assert_eq!(c.1, MOCHA_RED.g());
        assert_eq!(c.2, MOCHA_RED.b());
    }

    #[test]
    fn frappe_is_distinct() {
        let f = Theme::frappe();
        assert_ne!(f, Theme::mocha());
        assert_ne!(f, Theme::latte());
        assert_eq!(f.base, FRAPPE_BASE);
        assert_eq!(f.accent, FRAPPE_LAVENDER);
    }
}
