//! The look of the window: colours, type sizes, spacing, the interface font,
//! and the few building blocks every panel shares (cards, the primary button,
//! segmented choices, axis chips, figures).
//!
//! Panels use these instead of styling widgets one by one, so the window keeps
//! one look and a later restyle happens here.

use crate::i18n::tr;
use eframe::egui::{
    self, Align2, Color32, CornerRadius, CursorIcon, FontData, FontDefinitions, FontFamily, FontId,
    Margin, Rect, Response, RichText, Sense, Shadow, Stroke, StrokeKind, TextStyle, Theme, Ui,
    Vec2, WidgetInfo, WidgetType, vec2,
};
use std::sync::Arc;

/// The window's colours, for one theme.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    /// Panels and bars.
    pub panel: Color32,
    /// Cards inside panels, and windows.
    pub card: Color32,
    /// Input fields and idle buttons.
    pub input: Color32,
    pub hover: Color32,
    pub border: Color32,
    /// The outline of fields and boxes, stronger than `border`.
    pub edge: Color32,
    pub text: Color32,
    /// Labels and secondary text.
    pub dim: Color32,
    pub accent: Color32,
    pub accent_hover: Color32,
    pub accent_press: Color32,
    /// Text on an accent fill.
    pub on_accent: Color32,
    pub warn: Color32,
    pub ok: Color32,
    /// The 3D view's background, top and bottom.
    pub sky: Color32,
    pub ground: Color32,
}

pub const DARK: Palette = Palette {
    panel: Color32::from_rgb(0x1f, 0x22, 0x28),
    card: Color32::from_rgb(0x27, 0x2b, 0x33),
    input: Color32::from_rgb(0x32, 0x37, 0x41),
    hover: Color32::from_rgb(0x3b, 0x42, 0x4e),
    border: Color32::from_rgb(0x38, 0x3e, 0x48),
    edge: Color32::from_rgb(0x4a, 0x52, 0x5f),
    text: Color32::from_rgb(0xe6, 0xe9, 0xee),
    dim: Color32::from_rgb(0x9c, 0xa5, 0xb1),
    accent: Color32::from_rgb(0x2f, 0x7d, 0xf6),
    accent_hover: Color32::from_rgb(0x4a, 0x91, 0xff),
    accent_press: Color32::from_rgb(0x24, 0x67, 0xd1),
    on_accent: Color32::WHITE,
    warn: Color32::from_rgb(0xff, 0x7a, 0x70),
    ok: Color32::from_rgb(0x5c, 0xc8, 0x7a),
    sky: Color32::from_rgb(0x2c, 0x31, 0x3a),
    ground: Color32::from_rgb(0x15, 0x17, 0x1b),
};

pub const LIGHT: Palette = Palette {
    panel: Color32::from_rgb(0xf2, 0xf3, 0xf5),
    card: Color32::from_rgb(0xff, 0xff, 0xff),
    input: Color32::from_rgb(0xe3, 0xe6, 0xeb),
    hover: Color32::from_rgb(0xdf, 0xe3, 0xea),
    border: Color32::from_rgb(0xd6, 0xda, 0xe1),
    edge: Color32::from_rgb(0xbf, 0xc5, 0xcf),
    text: Color32::from_rgb(0x1f, 0x23, 0x29),
    dim: Color32::from_rgb(0x5d, 0x66, 0x73),
    accent: Color32::from_rgb(0x1f, 0x6f, 0xe5),
    accent_hover: Color32::from_rgb(0x3a, 0x82, 0xf0),
    accent_press: Color32::from_rgb(0x18, 0x5b, 0xc0),
    on_accent: Color32::WHITE,
    warn: Color32::from_rgb(0xc8, 0x3a, 0x30),
    ok: Color32::from_rgb(0x2a, 0x8c, 0x4a),
    sky: Color32::from_rgb(0xe6, 0xea, 0xf0),
    ground: Color32::from_rgb(0xc4, 0xcb, 0xd5),
};

/// Colours for the X, Y and Z axes, the same in both themes.
pub const AXES: [Color32; 3] = [
    Color32::from_rgb(0xe5, 0x53, 0x4b),
    Color32::from_rgb(0x4f, 0xb0, 0x5a),
    Color32::from_rgb(0x4a, 0x8e, 0xf0),
];

pub fn palette(dark: bool) -> &'static Palette {
    if dark { &DARK } else { &LIGHT }
}

/// The palette of the theme `ui` is drawn in.
pub fn of(ui: &Ui) -> &'static Palette {
    palette(ui.visuals().dark_mode)
}

/// The family used for titles and emphasis: a semibold system face when one
/// was found, otherwise the body face. Always bound, so using it never fails.
pub fn strong_family() -> FontFamily {
    FontFamily::Name(STRONG.into())
}

const STRONG: &str = "strong";
const BODY_FONT: &str = "system-ui";
const STRONG_FONT: &str = "system-ui-semibold";
/// A font file larger than this is not a UI font; it is not read.
const MAX_FONT_BYTES: u64 = 40 * 1024 * 1024;

pub fn strong(size: f32) -> FontId {
    FontId::new(size, strong_family())
}

/// Candidate interface fonts installed with the operating system: (regular,
/// semibold). They are read at run time, never copied into the program.
fn system_font_paths() -> &'static [(&'static str, &'static str)] {
    if cfg!(target_os = "windows") {
        &[
            (r"C:\Windows\Fonts\segoeui.ttf", r"C:\Windows\Fonts\seguisb.ttf"),
            (r"C:\Windows\Fonts\tahoma.ttf", r"C:\Windows\Fonts\tahomabd.ttf"),
        ]
    } else if cfg!(target_os = "macos") {
        &[
            (
                "/System/Library/Fonts/Supplemental/Arial.ttf",
                "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
            ),
            ("/Library/Fonts/Arial.ttf", "/Library/Fonts/Arial Bold.ttf"),
        ]
    } else {
        &[
            (
                "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
                "/usr/share/fonts/truetype/noto/NotoSans-SemiBold.ttf",
            ),
            (
                "/usr/share/fonts/noto/NotoSans-Regular.ttf",
                "/usr/share/fonts/noto/NotoSans-SemiBold.ttf",
            ),
            (
                "/usr/share/fonts/google-noto/NotoSans-Regular.ttf",
                "/usr/share/fonts/google-noto/NotoSans-SemiBold.ttf",
            ),
            (
                "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
                "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
            ),
            ("/usr/share/fonts/TTF/DejaVuSans.ttf", "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf"),
            (
                "/usr/local/share/fonts/dejavu/DejaVuSans.ttf",
                "/usr/local/share/fonts/dejavu/DejaVuSans-Bold.ttf",
            ),
        ]
    }
}

/// Read a font file if it is a size a UI font can be and it parses. A file
/// that fails here is skipped: handing a broken font to the text renderer
/// would stop the window.
fn read_font(path: &str) -> Option<Vec<u8>> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() == 0 || meta.len() > MAX_FONT_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    font_parses(&bytes).then_some(bytes)
}

/// The same check the text renderer makes before it uses a font.
pub fn font_parses(bytes: &[u8]) -> bool {
    skrifa::FontRef::from_index(bytes, 0).is_ok()
}

/// The fonts the window uses: the system interface font first when one is
/// installed and readable, with egui's built-in fonts behind it for any
/// character it lacks (symbols, emoji). `strong` is always bound.
pub fn fonts() -> FontDefinitions {
    let mut defs = FontDefinitions::default();
    let builtin = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let found = system_font_paths()
        .iter()
        .find_map(|(regular, semibold)| read_font(regular).map(|r| (r, read_font(semibold))));
    let mut body = builtin.clone();
    let mut heavy = builtin;
    if let Some((regular, semibold)) = found {
        defs.font_data.insert(BODY_FONT.into(), Arc::new(FontData::from_owned(regular)));
        body.insert(0, BODY_FONT.into());
        heavy.insert(0, BODY_FONT.into());
        if let Some(sb) = semibold {
            defs.font_data.insert(STRONG_FONT.into(), Arc::new(FontData::from_owned(sb)));
            heavy.insert(0, STRONG_FONT.into());
        }
    }
    defs.families.insert(FontFamily::Proportional, body);
    defs.families.insert(strong_family(), heavy);
    defs
}

/// Install the fonts once, at start-up.
pub fn install_fonts(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
}

/// Install both themes' styles and pick one.
pub fn apply(ctx: &egui::Context, dark: bool) {
    ctx.set_style_of(Theme::Dark, style(true));
    ctx.set_style_of(Theme::Light, style(false));
    ctx.set_theme(if dark { Theme::Dark } else { Theme::Light });
}

/// The complete style for one theme.
pub fn style(dark: bool) -> egui::Style {
    let p = palette(dark);
    let mut s = egui::Style {
        visuals: if dark { egui::Visuals::dark() } else { egui::Visuals::light() },
        ..Default::default()
    };
    s.text_styles = [
        (TextStyle::Small, FontId::proportional(12.0)),
        (TextStyle::Body, FontId::proportional(14.0)),
        (TextStyle::Button, FontId::proportional(14.0)),
        (TextStyle::Heading, strong(18.0)),
        (TextStyle::Monospace, FontId::monospace(13.0)),
    ]
    .into();

    let sp = &mut s.spacing;
    sp.item_spacing = vec2(8.0, 6.0);
    sp.button_padding = vec2(10.0, 4.0);
    sp.interact_size = vec2(40.0, 26.0);
    sp.window_margin = Margin::same(14);
    sp.menu_margin = Margin::same(6);
    sp.indent = 18.0;
    sp.combo_width = 160.0;
    sp.slider_width = 150.0;
    sp.text_edit_width = 200.0;
    sp.icon_width = 16.0;
    sp.icon_width_inner = 9.0;
    sp.icon_spacing = 6.0;
    sp.tooltip_width = 360.0;
    sp.scroll.bar_width = 8.0;

    let v = &mut s.visuals;
    // 4, not more: check boxes share this radius and would turn into circles
    let radius = CornerRadius::same(4);
    v.panel_fill = p.panel;
    v.window_fill = p.card;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_corner_radius = CornerRadius::same(10);
    v.window_shadow = Shadow {
        offset: [0, 10],
        blur: 28,
        spread: 0,
        color: Color32::from_black_alpha(if dark { 110 } else { 45 }),
    };
    v.popup_shadow = Shadow {
        offset: [0, 4],
        blur: 14,
        spread: 0,
        color: Color32::from_black_alpha(if dark { 90 } else { 35 }),
    };
    v.menu_corner_radius = CornerRadius::same(8);
    v.faint_bg_color = if dark { Color32::from_rgb(0x2c, 0x31, 0x3a) } else { p.input };
    v.extreme_bg_color = if dark { Color32::from_rgb(0x1a, 0x1d, 0x22) } else { p.card };
    v.text_edit_bg_color = Some(p.input);
    v.code_bg_color = p.input;
    v.warn_fg_color = Color32::from_rgb(0xe8, 0xb3, 0x4a);
    v.error_fg_color = p.warn;
    v.hyperlink_color = p.accent_hover;
    v.selection.bg_fill = p.accent;
    v.selection.stroke = Stroke::new(1.0, p.on_accent);
    v.slider_trailing_fill = true;
    v.collapsing_header_frame = false;
    v.indent_has_left_vline = false;
    v.resize_corner_size = 14.0;

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.card;
    w.noninteractive.weak_bg_fill = p.card;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    w.noninteractive.corner_radius = radius;

    w.inactive.bg_fill = p.input;
    w.inactive.weak_bg_fill = p.input;
    w.inactive.bg_stroke = Stroke::new(1.0, p.edge);
    w.inactive.fg_stroke = Stroke::new(1.0, p.text);
    w.inactive.corner_radius = radius;
    w.inactive.expansion = 0.0;

    w.hovered.bg_fill = p.hover;
    w.hovered.weak_bg_fill = p.hover;
    w.hovered.bg_stroke = Stroke::new(1.0, p.accent.gamma_multiply(0.7));
    w.hovered.fg_stroke = Stroke::new(1.5, p.text);
    w.hovered.corner_radius = radius;
    w.hovered.expansion = 0.0;

    w.active.bg_fill = p.accent_press;
    w.active.weak_bg_fill = p.accent_press;
    w.active.bg_stroke = Stroke::new(1.0, p.accent);
    w.active.fg_stroke = Stroke::new(2.0, p.on_accent);
    w.active.corner_radius = radius;
    w.active.expansion = 0.0;

    w.open.bg_fill = p.input;
    w.open.weak_bg_fill = p.hover;
    w.open.bg_stroke = Stroke::new(1.0, p.accent.gamma_multiply(0.7));
    w.open.fg_stroke = Stroke::new(1.0, p.text);
    w.open.corner_radius = radius;
    s
}

// ---------------------------------------------------------------------------
// building blocks

/// The frame of a card: a raised block holding one topic.
pub fn card_frame(ui: &Ui) -> egui::Frame {
    let p = of(ui);
    egui::Frame::new()
        .fill(p.card)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(8)
        .inner_margin(Margin::symmetric(12, 10))
}

/// A card with a title. The title is translated.
pub fn card<R>(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    card_with(ui, title, |_| {}, add)
}

/// A card with a title and something at the right of the title row (a
/// switch, a small button).
pub fn card_with<R>(
    ui: &mut Ui,
    title: &str,
    right: impl FnOnce(&mut Ui),
    add: impl FnOnce(&mut Ui) -> R,
) -> R {
    let r = card_frame(ui)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if !title.is_empty() {
                ui.horizontal(|ui| {
                    ui.label(title_text(ui, title));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), right);
                });
                ui.add_space(4.0);
            }
            add(ui)
        })
        .inner;
    ui.add_space(8.0);
    r
}

/// A card title.
pub fn title_text(ui: &Ui, text: &str) -> RichText {
    RichText::new(tr(text)).font(strong(15.0)).color(of(ui).text)
}

/// A panel title, above its cards.
pub fn panel_title(ui: &mut Ui, text: &str) {
    ui.add_space(2.0);
    ui.label(RichText::new(tr(text)).font(strong(19.0)).color(of(ui).text));
    ui.add_space(6.0);
}

/// A small caption above a group of rows inside a card.
pub fn caption(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(tr(text).to_uppercase()).size(11.0).color(of(ui).dim));
}

/// A label in the left column of a settings grid.
pub fn field(ui: &mut Ui, text: &str) -> Response {
    ui.label(RichText::new(tr(text)).color(of(ui).dim))
}

/// Secondary text.
pub fn dim(ui: &mut Ui, text: impl Into<String>) -> Response {
    ui.label(RichText::new(text.into()).color(of(ui).dim))
}

/// The main action of a panel: a filled accent button. `size.x == 0` fills
/// the width available.
pub fn primary(ui: &mut Ui, text: &str, enabled: bool, size: Vec2) -> Response {
    let p = of(ui);
    let width = if size.x > 0.0 { size.x } else { ui.available_width() };
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(vec2(width, size.y), sense);
    let fill = if !enabled {
        p.input
    } else if resp.is_pointer_button_down_on() {
        p.accent_press
    } else if resp.hovered() {
        p.accent_hover
    } else {
        p.accent
    };
    if ui.is_rect_visible(rect) {
        ui.painter().rect_filled(rect, 8, fill);
        if resp.has_focus() {
            ui.painter().rect_stroke(
                rect.expand(2.0),
                9,
                Stroke::new(1.5, p.accent_hover),
                StrokeKind::Outside,
            );
        }
        let colour = if enabled { p.on_accent } else { p.dim };
        ui.painter().text(rect.center(), Align2::CENTER_CENTER, text, strong(15.0), colour);
    }
    let label = text.to_string();
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, &label));
    if enabled { resp.on_hover_cursor(CursorIcon::PointingHand) } else { resp }
}

/// A plain full-width button, the companion of [`primary`].
pub fn secondary(ui: &mut Ui, text: &str, enabled: bool, height: f32) -> Response {
    let w = ui.available_width();
    ui.add_enabled(enabled, egui::Button::new(text).min_size(vec2(w, height)).corner_radius(8))
}

/// A row of mutually exclusive choices drawn as one control. Returns true when
/// the choice changed. Options marked false are shown but cannot be picked.
pub fn segmented<T: PartialEq + Copy>(
    ui: &mut Ui,
    value: &mut T,
    options: &[(T, &str, bool)],
) -> bool {
    let p = of(ui);
    let font = FontId::proportional(14.0);
    let height = 28.0;
    let pad = 14.0;
    let widths: Vec<f32> = options
        .iter()
        .map(|(_, label, _)| {
            ui.ctx()
                .fonts_mut(|f| f.layout_no_wrap(label.to_string(), font.clone(), p.text).size().x)
                + 2.0 * pad
        })
        .collect();
    let total: f32 = widths.iter().sum::<f32>() + 4.0;
    let (outer, _) = ui.allocate_exact_size(vec2(total, height), Sense::hover());
    ui.painter().rect_filled(outer, 8, p.input);
    let mut x = outer.left() + 2.0;
    let mut changed = false;
    for ((v, label, enabled), w) in options.iter().zip(&widths) {
        let rect = Rect::from_min_size(egui::pos2(x, outer.top() + 2.0), vec2(*w, height - 4.0));
        x += w;
        let id = ui.id().with(("segment", label));
        let sense = if *enabled { Sense::click() } else { Sense::hover() };
        let resp = ui.interact(rect, id, sense);
        let selected = *value == *v;
        let fill = if selected {
            p.accent
        } else if resp.hovered() && *enabled {
            p.hover
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, 6, fill);
        let colour = if selected {
            p.on_accent
        } else if *enabled {
            p.text
        } else {
            p.dim.gamma_multiply(0.6)
        };
        let f = if selected { strong(14.0) } else { font.clone() };
        ui.painter().text(rect.center(), Align2::CENTER_CENTER, *label, f, colour);
        let text = label.to_string();
        resp.widget_info(|| {
            WidgetInfo::selected(WidgetType::RadioButton, *enabled, selected, &text)
        });
        if resp.clicked() && !selected {
            *value = *v;
            changed = true;
        }
        if *enabled && !selected {
            resp.on_hover_cursor(CursorIcon::PointingHand);
        }
    }
    changed
}

/// A coloured chip naming an axis (0 = X, 1 = Y, 2 = Z).
pub fn axis_chip(ui: &mut Ui, axis: usize) -> Response {
    let colour = AXES.get(axis).copied().unwrap_or(Color32::GRAY);
    let letter = ["X", "Y", "Z"].get(axis).copied().unwrap_or("?");
    let (rect, resp) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
    ui.painter().rect_filled(rect, 5, colour.gamma_multiply(0.22));
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, letter, strong(12.0), colour);
    resp
}

/// One figure in a summary: a small label over a large value.
pub fn figure(ui: &mut Ui, label: &str, value: &str, width: f32) {
    let p = of(ui);
    egui::Frame::new().fill(p.input).corner_radius(8).inner_margin(Margin::symmetric(10, 8)).show(
        ui,
        |ui| {
            ui.set_width(width - 20.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(RichText::new(tr(label)).size(12.0).color(p.dim));
                ui.label(RichText::new(value).font(strong(18.0)).color(p.text));
            });
        },
    );
}

/// A short coloured tag after a name (a model's role, a planned setting).
pub fn tag(ui: &mut Ui, text: &str, colour: Color32) -> Response {
    let font = FontId::proportional(11.0);
    let galley = ui.ctx().fonts_mut(|f| f.layout_no_wrap(text.to_string(), font, colour));
    let size = galley.size() + vec2(10.0, 4.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(rect, 4, colour.gamma_multiply(0.18));
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, colour);
    resp
}

/// A key cap, for keyboard hints.
pub fn key(ui: &mut Ui, text: &str) {
    let p = of(ui);
    egui::Frame::new()
        .fill(p.input)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(4)
        .inner_margin(Margin::symmetric(6, 1))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(12.0).color(p.text));
        });
}

/// A thin vertical divider between groups in a bar.
pub fn divider(ui: &mut Ui) {
    let p = of(ui);
    let (rect, _) = ui.allocate_exact_size(vec2(9.0, 22.0), Sense::hover());
    ui.painter().vline(rect.center().x, rect.y_range(), Stroke::new(1.0, p.border));
}

/// Paint the 3D view's background: a soft vertical gradient.
pub fn paint_backdrop(painter: &egui::Painter, rect: Rect, dark: bool) {
    let p = palette(dark);
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), p.sky);
    mesh.colored_vertex(rect.right_top(), p.sky);
    mesh.colored_vertex(rect.right_bottom(), p.ground);
    mesh.colored_vertex(rect.left_bottom(), p.ground);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
}

/// A window that opens centred over the main view, and cannot be folded
/// away to its title bar.
pub fn window<'a>(ctx: &egui::Context, title: impl egui::IntoAtoms<'a>) -> egui::Window<'a> {
    egui::Window::new(title)
        .collapsible(false)
        .pivot(Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
}

/// The frame of a floating bar over the 3D view.
pub fn overlay_frame(ui: &Ui) -> egui::Frame {
    let p = of(ui);
    egui::Frame::new()
        .fill(p.card.gamma_multiply(0.94))
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(10)
        .inner_margin(Margin::same(4))
        .shadow(Shadow {
            offset: [0, 3],
            blur: 12,
            spread: 0,
            color: Color32::from_black_alpha(if ui.visuals().dark_mode { 80 } else { 30 }),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strong_family_is_always_bound() {
        let defs = fonts();
        let strong = defs.families.get(&strong_family()).cloned().unwrap_or_default();
        assert!(!strong.is_empty());
        for name in &strong {
            assert!(defs.font_data.contains_key(name), "{name} has no data");
        }
        let body = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
        assert!(!body.is_empty());
    }

    #[test]
    fn a_broken_font_file_is_refused() {
        assert!(!font_parses(b""));
        assert!(!font_parses(b"not a font at all"));
        assert!(!font_parses(&[0, 1, 0, 0, 0xff, 0xff, 0, 0]));
    }

    #[test]
    fn styles_differ_by_theme_and_keep_text_readable() {
        let d = style(true);
        let l = style(false);
        assert!(d.visuals.dark_mode && !l.visuals.dark_mode);
        assert_ne!(d.visuals.panel_fill, l.visuals.panel_fill);
        // body text must be bigger than egui's 12.5 px default
        let body = d.text_styles.get(&TextStyle::Body).map_or(0.0, |f| f.size);
        assert!(body >= 14.0);
    }
}
