//! MK aeGIS's colours: every panel reads them from [`Tokens`], light or dark with egui's theme.

use egui::Color32;

#[derive(Debug, Clone, Copy)]
pub struct Tokens {
    pub window: Color32,
    pub ribbon: Color32,
    pub panel: Color32,
    pub border: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    pub selected_row: Color32,
    pub status: Color32,
    pub error: Color32,
    pub label_halo: Color32,
}

const LIGHT: Tokens = Tokens {
    window: Color32::from_rgb(0xf3, 0xf4, 0xf6),
    ribbon: Color32::from_rgb(0xfb, 0xfb, 0xfc),
    panel: Color32::from_rgb(0xff, 0xff, 0xff),
    border: Color32::from_rgb(0xd9, 0xdc, 0xe1),
    text: Color32::from_rgb(0x22, 0x26, 0x2e),
    muted: Color32::from_rgb(0x6b, 0x72, 0x80),
    accent: Color32::from_rgb(0x1f, 0x6f, 0x78),
    accent_soft: Color32::from_rgb(0xdc, 0xee, 0xef),
    selected_row: Color32::from_rgb(0xc9, 0xf4, 0xf6),
    status: Color32::from_rgb(0xec, 0xee, 0xf1),
    error: Color32::from_rgb(0xb3, 0x26, 0x1e),
    label_halo: Color32::from_rgba_premultiplied(0xff, 0xff, 0xff, 0xd0),
};

// MK Sigma's dark palette, so the two apps look like one family: each token takes the colour of
// the Sigma token in the same role (window, ribbon, grid, border, text, accent, danger).
const DARK: Tokens = Tokens {
    window: Color32::from_rgb(0x1f, 0x20, 0x25),
    ribbon: Color32::from_rgb(0x2a, 0x2b, 0x31),
    panel: Color32::from_rgb(0x24, 0x25, 0x2b),
    border: Color32::from_rgb(0x3b, 0x3c, 0x44),
    text: Color32::from_rgb(0xec, 0xec, 0xf1),
    muted: Color32::from_rgb(0xa8, 0xaa, 0xb6),
    accent: Color32::from_rgb(0x8e, 0x9c, 0xff),
    accent_soft: Color32::from_rgb(0x2e, 0x34, 0x5e),
    // Sigma has no selected-row token: the accent, a step stronger than accent_soft.
    selected_row: Color32::from_rgb(0x3a, 0x43, 0x80),
    status: Color32::from_rgb(0x1a, 0x1b, 0x20),
    error: Color32::from_rgb(0xf1, 0x70, 0x5f),
    label_halo: Color32::from_rgba_premultiplied(0xff, 0xff, 0xff, 0xd0),
};

impl Tokens {
    pub fn get(ctx: &egui::Context) -> Tokens {
        if ctx.global_style().visuals.dark_mode { DARK } else { LIGHT }
    }
}

/// egui's style tuned to MK aeGIS: tighter spacing, our accent for selection.
pub fn apply(ctx: &egui::Context) {
    for (theme, t) in [(egui::Theme::Light, LIGHT), (egui::Theme::Dark, DARK)] {
        ctx.style_mut_of(theme, |style| {
            style.spacing.item_spacing = egui::vec2(6.0, 4.0);
            style.spacing.button_padding = egui::vec2(6.0, 3.0);
            style.visuals.selection.bg_fill = t.accent_soft;
            style.visuals.selection.stroke = egui::Stroke::new(1.0, t.accent);
            style.visuals.panel_fill = t.panel;
            style.visuals.window_fill = t.panel;
            style.visuals.hyperlink_color = t.accent;
        });
    }
}

pub fn color(c: mkaegis_core::Color) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
}
