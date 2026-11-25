//! Apariencia de escritorio clásica, sin imágenes ni dependencias adicionales.
use eframe::egui::{
    self, Align, Button, Color32, Frame, Label, Layout, Pos2, Rect, RichText, Sense, Stroke,
    Ui, Vec2,
};

pub(crate) const PANEL: Color32 = Color32::from_rgb(220, 222, 223);
pub(crate) const DESK: Color32 = Color32::from_rgb(240, 241, 242);
pub(crate) const BORDER: Color32 = Color32::from_rgb(190, 192, 193);
pub(crate) const BLUE: Color32 = Color32::from_rgb(0, 111, 146);
pub(crate) const CYAN: Color32 = Color32::from_rgb(0, 163, 209);
pub(crate) const SELECTED: Color32 = Color32::from_rgb(202, 232, 240);
pub(crate) const STRIPE: Color32 = Color32::from_rgb(233, 236, 238);

pub(crate) fn style() -> egui::Style {
    let mut style = egui::Style { visuals: egui::Visuals::light(), ..Default::default() };
    style.spacing.item_spacing = egui::vec2(5.0, 5.0);
    style.spacing.interact_size.y = 28.0;
    style.visuals.panel_fill = DESK;
    style.visuals.window_fill = DESK;
    style.visuals.window_rounding = 3.0.into();
    style.visuals.extreme_bg_color = Color32::WHITE;
    style.visuals.selection.bg_fill = SELECTED;
    style.visuals.selection.stroke = Stroke::new(1.0_f32, BLUE);
    for widgets in [&mut style.visuals.widgets.inactive, &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active, &mut style.visuals.widgets.open,
        &mut style.visuals.widgets.noninteractive]
    {
        widgets.rounding = 2.0.into();
        widgets.bg_stroke = Stroke::new(1.0_f32, BORDER);
    }
    style.visuals.widgets.inactive.weak_bg_fill = Color32::from_gray(252);
    style.visuals.widgets.hovered.weak_bg_fill = SELECTED;
    style
}

pub(crate) fn panel(fill: Color32) -> Frame {
    Frame::none().fill(fill).stroke(Stroke::new(1.0_f32, BORDER)).inner_margin(5.0)
}

pub(crate) fn title(ui: &mut Ui, title: &str) {
    Frame::none().fill(BLUE).inner_margin(egui::Margin::symmetric(8.0, 4.0)).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.label(RichText::new(title).strong().color(Color32::WHITE));
    });
}

pub(crate) fn readout(ui: &mut Ui, label: &str, value: String, width: f32, primary: bool) {
    ui.allocate_ui_with_layout(egui::vec2(width, 51.0), Layout::top_down(Align::Center), |ui| {
        ui.label(RichText::new(label).strong());
        let font_size = if primary { 18.0 } else { 14.0 };
        let text_width = ui.fonts(|fonts| fonts.layout_no_wrap(value.clone(), egui::FontId::proportional(font_size), BLUE).size().x);
        let size = font_size * ((width - 10.0) / text_width.max(1.0)).clamp(0.5, 1.0);
        let text = RichText::new(&value).size(size).color(BLUE);
        Frame::none()
            .fill(if primary { SELECTED } else { Color32::WHITE })
            .stroke(Stroke::new(1.0_f32, BORDER))
            .inner_margin(2.0)
            .show(ui, |ui| {
                ui.add_sized([width - 6.0, 23.0], Label::new(text).truncate().halign(Align::Center))
                    .on_hover_text_at_pointer(format!("{label}: {value}"));
            });
    });
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum KeypadKey { Clear, Digit(char), Comma, Quantity, Minus, Plus, Pay }

/// El orden es el del teclado de ventas clásico: 1–3 arriba y 7–9 abajo.
pub(crate) fn keypad(
    ui: &mut Ui, width: f32, height: f32, allow_qty: bool, adjust_unit: bool, allow_pay: bool,
) -> Option<KeypadKey> {
    use KeypadKey::*;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let gap = 4.0;
    let w = (width - 3.0 * gap) / 4.0;
    let h = (height - 4.0 * gap) / 5.0;
    let mut chosen = None;
    let mut put = |key, label: &str, x: u8, y: u8, cols: u8, rows: u8, enabled, hint: &str| {
        let pos = rect.min + egui::vec2(f32::from(x) * (w + gap), f32::from(y) * (h + gap));
        let size = egui::vec2(f32::from(cols) * (w + gap) - gap, f32::from(rows) * (h + gap) - gap);
        let mut text = RichText::new(label).size(match key { Digit(_) => h.min(36.0) * 0.9, _ => 21.0 });
        if enabled && ui.is_enabled() {
            text = text.color(match key { Plus => CYAN, Minus => Color32::from_rgb(199, 38, 105),
                _ => Color32::from_gray(105) });
        }
        if !matches!(key, Digit(_)) { text = text.strong(); }
        let button = Button::new(text);
        ui.scope_builder(egui::UiBuilder::new().max_rect(Rect::from_min_size(pos, size)), |ui| {
            if ui.add_enabled(enabled, button.min_size(size)).on_hover_text(hint).clicked() {
                chosen = Some(key);
            }
        });
    };
    put(Clear, "CE", 0, 0, 2, 1, true, "Limpiar el código");
    put(Quantity, "*", 2, 0, 1, 1, allow_qty, "Cantidad (F3)");
    put(Minus, "−", 3, 0, 1, 1, adjust_unit, "Restar una unidad");
    put(Plus, "+", 3, 1, 1, 2, adjust_unit, "Sumar una unidad");
    put(Pay, "=", 3, 3, 1, 2, allow_pay, "Cobrar ticket (F6)");
    for (i, digit) in "123456789".chars().enumerate() {
        let i = i as u8;
        put(Digit(digit), &digit.to_string(), i % 3, i / 3 + 1, 1, 1, true, "Escribir código");
    }
    put(Digit('0'), "0", 0, 4, 2, 1, true, "Escribir código");
    put(Comma, ".", 2, 4, 1, 1, true, "Escribir punto");
    chosen
}

#[derive(Clone, Copy)]
pub(crate) enum Icon {
    Print, Delete, Search, Edit, Note, Drawer, Barcode, Menu, Cart, Payments, Calculator,
    Stock, Maintenance, Tools, Logout, User,
}

pub(crate) fn icon_button(
    ui: &mut Ui, icon: Icon, size: Vec2, enabled: bool, hint: &str, caption: Option<&str>,
) -> egui::Response {
    let caption = caption.map(|text| format!("     {text}")).unwrap_or_default();
    let labeled = !caption.is_empty();
    let response = ui.add_enabled(enabled, Button::new(caption).min_size(size)).on_hover_text(hint);
    if !labeled {
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled() && enabled, hint));
    }
    let painter = ui.painter_at(response.rect);
    let color = if ui.is_enabled() && enabled { CYAN } else { Color32::from_gray(145) };
    let mut center = response.rect.center();
    if labeled { center.x = response.rect.left() + 15.0; }
    draw_icon(&painter, center, icon, color);
    response
}

pub(crate) fn draw_icon(painter: &egui::Painter, center: Pos2, icon: Icon, color: Color32) {
    let s = Stroke::new(1.6_f32, color);
    let point = |x, y| center + egui::vec2(x, y);
    let line = |a: (f32, f32), b: (f32, f32)| painter.line_segment([point(a.0, a.1), point(b.0, b.1)], s);
    let rect = |a: (f32, f32), b: (f32, f32)| painter.rect_stroke(
        Rect::from_two_pos(point(a.0, a.1), point(b.0, b.1)), 1.0, s,
    );
    match icon {
        Icon::Print => {
            rect((-9.0, -4.0), (9.0, 5.0));
            rect((-5.0, -9.0), (5.0, -4.0));
            painter.rect_filled(Rect::from_two_pos(point(-5.0, 2.0), point(5.0, 9.0)), 0.0, Color32::WHITE);
            rect((-5.0, 2.0), (5.0, 9.0));
        }
        Icon::Delete => {
            rect((-5.0, -4.0), (5.0, 9.0));
            line((-8.0, -4.0), (8.0, -4.0)); line((-3.0, -8.0), (3.0, -8.0));
            line((0.0, -1.0), (0.0, 6.0));
        }
        Icon::Search => {
            painter.circle_stroke(point(-2.0, -2.0), 5.8, s);
            line((2.5, 2.5), (8.0, 8.0));
        }
        Icon::Edit => {
            painter.add(egui::Shape::closed_line(
                [(-7.0, 8.0), (-6.0, 3.0), (5.0, -8.0), (8.0, -5.0), (-3.0, 6.0)]
                    .map(|(x, y)| point(x, y)).to_vec(), s,
            ));
        }
        Icon::Note => {
            rect((-6.0, -8.0), (6.0, 8.0));
            for y in [-4.0, 0.0, 4.0] { line((-3.0, y), (3.0, y)); }
        }
        Icon::Drawer => {
            rect((-9.0, -5.0), (9.0, 6.0));
            line((-8.0, -1.0), (8.0, -1.0)); line((-2.0, 2.0), (2.0, 2.0));
        }
        Icon::Barcode => {
            for x in [-8.0, -5.0, -1.0, 2.0, 6.0, 8.0] {
                line((x, -7.0), (x, 7.0));
            }
        }
        Icon::Menu => {
            for y in [-6.0, 0.0, 6.0] {
                painter.line_segment([point(-9.0, y), point(9.0, y)], Stroke::new(3.0_f32, color));
            }
        }
        Icon::Cart => {
            for (a, b) in [((-10.0, -8.0), (-6.0, -8.0)), ((-6.0, -8.0), (-2.0, 3.0)),
                ((-2.0, 3.0), (6.0, 3.0)), ((6.0, 3.0), (9.0, -5.0)), ((9.0, -5.0), (-5.0, -5.0))]
            {
                line(a, b);
            }
            for x in [-1.0, 6.0] { painter.circle_filled(point(x, 7.0), 2.0, color); }
        }
        Icon::Payments => {
            for y in [-6.0, 0.0, 6.0] { line((-1.0, y), (9.0, y)); }
            line((-10.0, 0.0), (-3.0, 0.0));
            line((-7.0, -4.0), (-3.0, 0.0)); line((-7.0, 4.0), (-3.0, 0.0));
        }
        Icon::Calculator => {
            rect((-7.0, -9.0), (7.0, 9.0));
            line((-4.0, -4.0), (4.0, -4.0));
            for y in [0.0, 4.0] {
                for x in [-4.0, 0.0, 4.0] { painter.circle_filled(point(x, y), 1.0, color); }
            }
        }
        Icon::Stock => {
            painter.add(egui::Shape::closed_line(
                [(-9.0, -5.0), (0.0, -9.0), (9.0, -5.0), (9.0, 5.0), (0.0, 9.0), (-9.0, 5.0)]
                    .map(|(x, y)| point(x, y)).to_vec(), s,
            ));
            line((-9.0, -5.0), (0.0, -1.0)); line((0.0, -1.0), (9.0, -5.0)); line((0.0, -1.0), (0.0, 9.0));
        }
        Icon::Maintenance => {
            painter.circle_stroke(center, 5.2, s);
            painter.circle_stroke(center, 2.4, s);
            for (x, y) in [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0),
                (0.71, 0.71), (0.71, -0.71), (-0.71, 0.71), (-0.71, -0.71)] {
                line((5.0 * x, 5.0 * y), (8.5 * x, 8.5 * y));
            }
        }
        Icon::Tools => {
            line((-7.0, 7.0), (6.0, -6.0));
            line((-6.0, -6.0), (7.0, 7.0));
            line((-8.0, -3.0), (-3.0, -8.0)); line((3.0, -8.0), (8.0, -3.0));
        }
        Icon::Logout => {
            line((-7.0, -8.0), (2.0, -8.0)); line((-7.0, -8.0), (-7.0, 8.0));
            line((-7.0, 8.0), (2.0, 8.0)); line((-3.0, 0.0), (9.0, 0.0));
            line((5.0, -4.0), (9.0, 0.0)); line((5.0, 4.0), (9.0, 0.0));
        }
        Icon::User => {
            painter.circle_filled(point(0.0, -5.0), 4.0, color);
            painter.rect_filled(Rect::from_two_pos(point(-8.0, 0.0), point(8.0, 8.0)), 4.0, color);
        }
    }
}

pub(crate) fn cell(ui: &mut Ui, x: f32, y: f32, width: f32, height: f32, text: RichText, align: Align) {
    ui.put(
        Rect::from_min_size(Pos2::new(x, y), egui::vec2(width.max(0.0), height)),
        Label::new(text).truncate().halign(align).selectable(false),
    );
}

pub(crate) fn product_name(ui: &Ui, rect: Rect, name: &str) {
    let text = egui::WidgetText::RichText(RichText::new(name).strong().size(13.0));
    let mut job = text.into_layout_job(ui.style(), egui::FontSelection::Default, Align::Center);
    job.halign = Align::Center;
    job.wrap.max_width = (rect.width() - 10.0).max(1.0);
    job.wrap.max_rows = 2;
    let galley = ui.fonts(|fonts| fonts.layout_job(job));
    let pos = egui::pos2(rect.center().x, rect.top() + 20.0 - galley.size().y / 2.0);
    ui.painter_at(rect).galley(pos, galley, ui.visuals().text_color());
}
