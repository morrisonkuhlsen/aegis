//! mkaeGIS's colours: every panel reads them from [`Tokens`], light or dark with egui's theme.

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

const DARK: Tokens = Tokens {
    window: Color32::from_rgb(0x1c, 0x1f, 0x24),
    ribbon: Color32::from_rgb(0x24, 0x28, 0x2e),
    panel: Color32::from_rgb(0x20, 0x23, 0x29),
    border: Color32::from_rgb(0x3a, 0x3f, 0x47),
    text: Color32::from_rgb(0xe4, 0xe7, 0xeb),
    muted: Color32::from_rgb(0x9a, 0xa1, 0xab),
    accent: Color32::from_rgb(0x5c, 0xc0, 0xc8),
    accent_soft: Color32::from_rgb(0x1f, 0x3a, 0x3d),
    selected_row: Color32::from_rgb(0x1d, 0x4f, 0x55),
    status: Color32::from_rgb(0x19, 0x1c, 0x20),
    error: Color32::from_rgb(0xf0, 0x7a, 0x6e),
    label_halo: Color32::from_rgba_premultiplied(0xff, 0xff, 0xff, 0xd0),
};

impl Tokens {
    pub fn get(ctx: &egui::Context) -> Tokens {
        if ctx.global_style().visuals.dark_mode { DARK } else { LIGHT }
    }
}

/// egui's style tuned to mkaeGIS: tighter spacing, our accent for selection.
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
