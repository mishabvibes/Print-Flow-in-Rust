//! Design tokens. The look follows the conventions of professional creative tools: a neutral
//! mid-dark grey, one blue accent, flat controls with a 2 px radius, small type and dense panels.
//! It uses no third-party artwork, fonts or icons; every colour and size lives here.

use egui::{Color32, Context, CornerRadius, FontId, Shadow, Stroke, TextStyle, Visuals};

#[derive(Clone, Copy, Debug)]
pub struct Tokens {
    /// Gaps and separators between docked panels.
    pub bg: Color32,
    /// Panel and menu surfaces.
    pub panel: Color32,
    /// Panel title strips and the tool strip.
    pub header: Color32,
    /// Buttons.
    pub raised: Color32,
    pub hover: Color32,
    /// Text inputs and drag values.
    pub field: Color32,
    /// The pasteboard around the artwork.
    pub canvas: Color32,
    pub line: Color32,
    pub text: Color32,
    pub dim: Color32,
    pub accent: Color32,
    pub ok: Color32,
    pub warn: Color32,
    pub bad: Color32,
}

pub const RADIUS: u8 = 2;
pub const GAP: f32 = 6.0;
pub const DOCK_W: f32 = 300.0;
pub const TOOL_W: f32 = 40.0;

const fn c(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

impl Tokens {
    pub const DARK: Tokens = Tokens {
        bg: c(0x1d, 0x1d, 0x1d),
        panel: c(0x32, 0x32, 0x32),
        header: c(0x2b, 0x2b, 0x2b),
        raised: c(0x47, 0x47, 0x47),
        hover: c(0x56, 0x56, 0x56),
        field: c(0x25, 0x25, 0x25),
        canvas: c(0x40, 0x40, 0x40),
        line: c(0x1d, 0x1d, 0x1d),
        text: c(0xd2, 0xd2, 0xd2),
        dim: c(0x91, 0x91, 0x91),
        accent: c(0x26, 0x80, 0xeb),
        ok: c(0x2d, 0x9d, 0x78),
        warn: c(0xe6, 0x86, 0x19),
        bad: c(0xe3, 0x48, 0x50),
    };
    pub const LIGHT: Tokens = Tokens {
        bg: c(0xb9, 0xb9, 0xb9),
        panel: c(0xf0, 0xf0, 0xf0),
        header: c(0xe1, 0xe1, 0xe1),
        raised: c(0xdc, 0xdc, 0xdc),
        hover: c(0xcc, 0xcc, 0xcc),
        field: c(0xff, 0xff, 0xff),
        canvas: c(0xc9, 0xc9, 0xc9),
        line: c(0xbd, 0xbd, 0xbd),
        text: c(0x25, 0x25, 0x25),
        dim: c(0x6b, 0x6b, 0x6b),
        accent: c(0x14, 0x73, 0xe6),
        ok: c(0x12, 0x80, 0x5c),
        warn: c(0xc3, 0x6a, 0x00),
        bad: c(0xd3, 0x1f, 0x2b),
    };

    pub fn of(dark: bool) -> Tokens {
        if dark { Self::DARK } else { Self::LIGHT }
    }
}

pub fn apply(ctx: &Context, dark: bool) {
    let t = Tokens::of(dark);
    let mut v = if dark { Visuals::dark() } else { Visuals::light() };
    v.panel_fill = t.panel;
    v.window_fill = t.panel;
    v.extreme_bg_color = t.field;
    v.faint_bg_color = t.header;
    v.code_bg_color = t.field;
    v.override_text_color = Some(t.text);
    v.window_stroke = Stroke::new(1.0, if dark { c(0x12, 0x12, 0x12) } else { t.line });
    v.window_shadow = Shadow { offset: [0, 4], blur: 14, spread: 0, color: Color32::from_black_alpha(if dark { 150 } else { 60 }) };
    v.popup_shadow = v.window_shadow;
    v.selection.bg_fill = if dark { t.accent.gamma_multiply(0.6) } else { t.accent };
    v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    v.hyperlink_color = t.accent;
    v.window_corner_radius = CornerRadius::same(3);
    v.menu_corner_radius = CornerRadius::same(3);
    let r = CornerRadius::same(RADIUS);
    let edge = if dark { c(0x1a, 0x1a, 0x1a) } else { c(0xa8, 0xa8, 0xa8) };
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = r;
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, t.line);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, t.text);
    v.widgets.inactive.bg_fill = t.raised;
    v.widgets.inactive.weak_bg_fill = t.raised;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, edge);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, t.text);
    v.widgets.hovered.bg_fill = t.hover;
    v.widgets.hovered.weak_bg_fill = t.hover;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, edge);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, if dark { Color32::WHITE } else { Color32::BLACK });
    v.widgets.active.bg_fill = t.accent;
    v.widgets.active.weak_bg_fill = t.accent;
    v.widgets.active.bg_stroke = Stroke::new(1.0, t.accent);
    v.widgets.active.fg_stroke = Stroke::new(1.0, if dark { Color32::WHITE } else { t.text });
    v.widgets.open.bg_fill = t.hover;
    v.widgets.open.weak_bg_fill = t.hover;
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(GAP, 4.0);
        s.spacing.button_padding = egui::vec2(8.0, 2.0);
        s.spacing.interact_size = egui::vec2(24.0, 20.0);
        s.spacing.menu_margin = egui::Margin::symmetric(4, 4);
        s.spacing.icon_width = 14.0;
        s.spacing.icon_width_inner = 8.0;
        s.spacing.combo_width = 80.0;
        s.text_styles.insert(TextStyle::Body, FontId::proportional(12.0));
        s.text_styles.insert(TextStyle::Button, FontId::proportional(12.0));
        s.text_styles.insert(TextStyle::Small, FontId::proportional(10.5));
        s.text_styles.insert(TextStyle::Monospace, FontId::monospace(11.0));
        s.text_styles.insert(TextStyle::Heading, FontId::proportional(14.0));
    });
    // Ctrl +/- and Ctrl+0 belong to the canvas (zoom), not to the whole interface.
    ctx.options_mut(|o| o.zoom_with_keyboard = false);
}
