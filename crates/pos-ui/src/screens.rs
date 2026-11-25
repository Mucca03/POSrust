use eframe::egui::{Align, Button, Id, Layout, Modifiers, ScrollArea, Sense, TextEdit, Ui};
use pos_core::{Denomination, TicketLine};

use super::*;
use crate::checkout::CheckoutAction;
use crate::classic::{self, Icon, KeypadKey};

const SCAN_LIMIT: usize = 128;
const TICKET_COLUMNS: [(&str, f32); 6] = [
    ("Artículo", 0.30), ("Precio", 0.15), ("Cant.", 0.12), ("IVA", 0.11), ("Importe", 0.17), ("Impresora", 0.15),
];

impl Dialog {
    fn page_title(&self) -> Option<&'static str> {
        match self {
            Self::Lookup(..) => Some("Edit Sales"),
            Self::Cash(..) => Some("Payments"),
            Self::Close(_) => Some("Close Cash"),
            Self::Menu(menu) => Some(menu.title()),
            _ => None,
        }
    }
}

impl App {
    pub(super) fn render_login(&mut self, ctx: &Context) {
        self.header(ctx);
        egui::CentralPanel::default().show(ctx, |ui| {
            let area = ui.available_rect_before_wrap();
            let rail = 232.0;
            let brand = egui::Rect::from_min_max(
                area.left_top() + egui::vec2(0.0, area.height() * 0.30),
                area.right_bottom() - egui::vec2(rail + 6.0, 30.0),
            );
            ui.scope_builder(egui::UiBuilder::new().max_rect(brand), |ui| {
                ui.set_clip_rect(brand.intersect(ui.clip_rect()));
                ui.vertical_centered(|ui| {
                    if let Some(logo) = &self.brand_logo {
                        ui.add(egui::Image::new(logo).max_size(egui::vec2(300.0, 130.0)));
                    } else {
                        ui.label(RichText::new("POSrust").size(46.0).color(classic::CYAN));
                    }
                    let text = if self.settings.brand_text.is_empty() { "Punto de venta" } else { &self.settings.brand_text };
                    ui.add(egui::Label::new(RichText::new(text).size(17.0)).wrap());
                });
            });
            ui.scope_builder(egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
                egui::pos2(area.right() - rail, area.top() + 14.0), egui::vec2(rail, 260.0),
            )), |ui| {
                let mut login = None;
                for row in self.catalog.users.chunks(2) {
                    ui.horizontal(|ui| {
                        for user in row {
                            let name = user.name.trim();
                            let response = ui.add_sized([111.0, 48.0], Button::new("")).on_hover_text(name);
                            response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
                            classic::draw_icon(ui.painter(), response.rect.center_top() + egui::vec2(0.0, 15.0),
                                Icon::User, classic::CYAN);
                            classic::cell(ui, response.rect.left() + 3.0, response.rect.top() + 28.0, 105.0, 17.0,
                                RichText::new(name).size(12.0), Align::Center);
                            if response.clicked() {
                                login = Some(user.id.to_string());
                            }
                        }
                    });
                }
                if let Some(id) = login {
                    let result = self.login(&id);
                    self.report_error(result);
                }
                show_notice(ui, &self.notice);
            });
            ui.scope_builder(egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
                egui::pos2(area.right() - rail, area.bottom() - 37.0), egui::vec2(rail, 36.0),
            )), |ui| {
                if ui.add_sized([rail, 35.0], Button::new("Cerrar POSrust")).clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
        });
    }

    pub(super) fn render_sales(&mut self, ctx: &Context) {
        if self.dialog.is_none() {
            self.sale_keys(ctx);
        }
        self.header(ctx);
        let disable = self.dialog.as_ref().is_some_and(|dialog| dialog.page_title().is_none());
        egui::TopBottomPanel::bottom("status").frame(classic::panel(classic::DESK)).show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.small("Espacio / F6 Cobrar  ·  + / − Unidades  ·  F1 Lector  ·  F2 Buscar  ·  F8 Último pago");
                if self.notice.is_some() { ui.separator(); show_notice(ui, &self.notice); }
                if self.printer_notice.is_some() { ui.separator(); show_notice(ui, &self.printer_notice); }
            });
        });
        self.render_sidebar(ctx, disable);
        if self.user.is_none() { return; }
        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(title) = self.dialog.as_ref().and_then(Dialog::page_title) {
                self.render_page(ui, title);
            } else {
                ui.add_enabled_ui(self.dialog.is_none(), |ui| self.sales_workspace(ui));
            }
        });
        if !disable && self.dialog.is_none() && self.user.is_some() {
            self.shortcuts(ctx);
        }
    }

    fn sales_workspace(&mut self, ui: &mut Ui) {
        self.top_bar(ui);
        let area = ui.available_size();
        let pad = (area.x * 0.225).clamp(182.0, 290.0);
        let tools = if area.x > 1000.0 { 61.0 } else { 44.0 };
        let gap = ui.spacing().item_spacing.x;
        let upper = (area.y * 0.68).min((area.y - 114.0).max(206.0));
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(egui::vec2(tools, upper), Layout::top_down(Align::Min), |ui| {
                self.line_tools(ui, tools);
            });
            let width = (area.x - pad - tools - 2.0 * gap).max(120.0);
            ui.allocate_ui_with_layout(egui::vec2(width, upper), Layout::top_down(Align::Min), |ui| {
                self.cart(ui, width, upper);
            });
            ui.allocate_ui_with_layout(egui::vec2(pad, upper), Layout::top_down(Align::Min), |ui| {
                self.ticket_controls(ui, (upper - 68.0).clamp(148.0, 241.0));
            });
        });
        let area = ui.available_size();
        let width = (area.x * 0.14).clamp(114.0, 248.0);
        ui.add_enabled_ui(self.sale_ready(), |ui| {
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(egui::vec2(width, area.y), Layout::top_down(Align::Min), |ui| {
                    classic::panel(classic::DESK).show(ui, |ui| self.categories(ui));
                });
                ui.allocate_ui_with_layout(egui::vec2(area.x - width - gap, area.y), Layout::top_down(Align::Min), |ui| {
                    classic::panel(classic::DESK).show(ui, |ui| self.products(ui));
                });
            });
        });
    }

    fn header(&self, ctx: &Context) {
        ctx.request_repaint_after(Duration::from_secs(1));
        egui::TopBottomPanel::top("pos-header").exact_height(34.0)
            .frame(egui::Frame::none().fill(classic::DESK).inner_margin(5.0)).show(ctx, |ui| {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), Sense::hover());
                classic::cell(ui, rect.left(), rect.top(), 180.0, 24.0,
                    RichText::new(self.timestamp().unwrap_or_default()).strong(), Align::LEFT);
                classic::cell(ui, rect.center().x - 160.0, rect.top(), 320.0, 24.0,
                    RichText::new("POSrust · Punto de venta").strong().size(16.0), Align::Center);
                classic::cell(ui, rect.right() - 90.0, rect.top(), 90.0, 24.0,
                    RichText::new("POSrust").color(classic::CYAN).strong(), Align::RIGHT);
                ui.painter().hline(rect.x_range(), rect.bottom() + 4.0, egui::Stroke::new(1.0_f32, classic::BORDER));
            });
    }

    fn top_bar(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            let width = if self.session.is_none() { 455.0 } else { 670.0 };
            ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
            let square = egui::vec2(44.0, 34.0);
            if classic::icon_button(ui, Icon::Menu, square, true, "Menú lateral (F10)", None).clicked() {
                self.menu_open = !self.menu_open;
            }
            if classic::icon_button(ui, Icon::Search, square, self.can(Capability::Reprint), "Edit Sales · Consultar ticket", None).clicked() {
                self.show_dialog(Dialog::Lookup(String::new(), None));
            }
            if classic::icon_button(ui, Icon::Print, egui::vec2(120.0, 34.0),
                !self.printing_receipt && self.can(Capability::Reprint),
                "Imprimir el último pago (F8)", Some("Último pago")).clicked()
            {
                let result = self.request_receipt(None, false);
                self.report_error(result);
            }
            if classic::icon_button(ui, Icon::Drawer, square, self.sale_ready() && self.settings.drawer && self.can(Capability::OpenDrawer),
                "Abrir cajón (F7)", None).clicked()
            {
                let result = self.manual_drawer();
                self.report_error(result);
            }
            if classic::icon_button(ui, Icon::Delete, square, !self.draft.is_empty() && self.can(Capability::RemoveLine),
                "Cancelar ticket (Esc)", None).clicked()
            {
                self.show_dialog(Dialog::CancelTicket);
            }
            if classic::icon_button(ui, Icon::Logout, square, true, "Logout · Cerrar sesión", None).clicked() {
                let result = self.logout();
                self.report_error(result);
            }
            if self.session.is_none() && ui.button("Abrir caja").clicked() {
                self.show_dialog(Dialog::Opening("0".into()));
            }
            if self.sale_ready() {
                ui.separator();
                ui.strong(format!("Venta #{}", self.sale_number));
                if ui.add_enabled(!self.draft.is_empty(), Button::new("Nueva venta").min_size(egui::vec2(90.0, 34.0)))
                    .on_hover_text("Dejar la venta actual pendiente y atender otra").clicked()
                {
                    self.new_sale();
                }
                let count = self.parked_sales.len() + usize::from(!self.draft.is_empty());
                if ui.add(Button::new(format!("Ventas abiertas ({count})")).min_size(egui::vec2(121.0, 34.0))).clicked() {
                    self.show_dialog(Dialog::OpenSales);
                }
            }
        });
    }

    fn categories(&mut self, ui: &mut Ui) {
        ui.strong("Categorías");
        let mut selected = None;
        ScrollArea::vertical().id_salt("classic-categories").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            for entry in std::iter::once((None, "Todas")).chain(self.catalog.categories.iter().map(|category| {
                let name = category.name.trim();
                (Some(category.id.as_str()), if name.is_empty() { "(Sin nombre)" } else { name })
            })) {
                let (id, label) = entry;
                let active = self.category.as_deref() == id;
                let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 26.0), Sense::click());
                response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, ui.is_enabled(), active, label));
                if active || response.hovered() {
                    ui.painter().rect_filled(rect, 0.0, classic::SELECTED);
                }
                let icon_at = rect.left_center() + egui::vec2(9.0, 0.0);
                for y in [-6.0, 0.0, 6.0] {
                    ui.painter().line_segment([icon_at + egui::vec2(-3.0, y), icon_at + egui::vec2(5.0, y)],
                        egui::Stroke::new(2.0_f32, classic::CYAN));
                }
                classic::cell(ui, rect.left() + 22.0, rect.top(), rect.width() - 24.0, rect.height(),
                    if active { RichText::new(label).strong() } else { RichText::new(label) }, Align::LEFT);
                if response.on_hover_text(label).clicked() {
                    selected = Some(id.map(str::to_owned));
                }
            }
        });
        if let Some(next) = selected {
            self.category = next;
            self.search.clear();
            self.refresh_products();
            self.focus_search = true;
        }
    }

    fn products(&mut self, ui: &mut Ui) {
        let category = self.catalog.categories.iter()
            .find(|cat| Some(cat.id.as_str()) == self.category.as_deref())
            .map(|cat| cat.name.trim()).filter(|name| !name.is_empty()).unwrap_or("Todas");
        let title = if !self.search.trim().is_empty() {
            "Productos · Búsqueda".to_owned()
        } else {
            format!("Productos · {category}")
        };
        ui.label(RichText::new(&title).strong().color(classic::BLUE));
        let columns = ((ui.available_width() / 116.0) as usize).clamp(2, 16);
        let rows = (((ui.available_height() - 35.0) / 68.0) as usize).clamp(1, 10);
        let per_page = columns * rows;
        let pages = self.visible_products.len().div_ceil(per_page).max(1);
        self.page = self.page.min(pages - 1);
        let mut chosen = None;
        let grid_height = (ui.available_height() - 35.0).max(1.0);
        ScrollArea::vertical().id_salt("grid").max_height(grid_height).min_scrolled_height(grid_height)
            .auto_shrink([false, false]).show(ui, |ui| {
            let start = self.page * per_page;
            let end = (start + per_page).min(self.visible_products.len());
            if start == end {
                ui.weak("Sin productos para mostrar.");
            }
            let width = ((ui.available_width() - ui.spacing().item_spacing.x * (columns - 1) as f32) / columns as f32).min(126.0);
            for row in self.visible_products[start..end].chunks(columns) {
                ui.horizontal(|ui| {
                    for &index in row {
                        let product = &self.catalog.products[index];
                        let unit = if product.unit.is_weighted() { "/kg" } else { "" };
                        let price = format!("{}{unit}", product.list_price.format_cop_exact());
                        let (rect, item) = ui.allocate_exact_size(egui::vec2(width, 63.0), Sense::click());
                        item.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), product.name.trim()));
                        let fill = if item.hovered() { classic::SELECTED } else { Color32::from_gray(250) };
                        ui.painter().rect(rect, 2.0, fill, egui::Stroke::new(1.0_f32, classic::BORDER));
                        classic::product_name(ui, rect, product.name.trim());
                        classic::cell(ui, rect.left() + 4.0, rect.bottom() - 23.0, rect.width() - 8.0, 21.0,
                            RichText::new(price).color(classic::BLUE), Align::Center);
                        if item.on_hover_text(format!("{} · {}", product.name.trim(), product.code.trim())).clicked() {
                            chosen = Some(product.id.clone());
                        }
                    }
                });
            }
        });
        ui.horizontal(|ui| {
            if ui.add_enabled(self.page > 0, Button::new("◀").min_size(egui::vec2(44.0, 25.0)))
                .on_hover_text("Página anterior").clicked() { self.page -= 1; }
            ui.label(format!("{} / {} · {}", self.page + 1, pages, self.visible_products.len()));
            if ui.add_enabled(self.page + 1 < pages, Button::new("▶").min_size(egui::vec2(44.0, 25.0)))
                .on_hover_text("Página siguiente").clicked() { self.page += 1; }
        });
        if let Some(id) = chosen {
            self.add_product(id);
            ui.ctx().request_repaint();
        }
    }

    fn submit_scan(&mut self) {
        let input = self.search.trim();
        if input.is_empty() || !self.sale_ready() {
            return;
        }
        let numeric = input.bytes().all(|c| c.is_ascii_digit());
        let chosen = self.catalog.product_by_code(input).or_else(|| {
            self.visible_products.iter().map(|&i| &self.catalog.products[i])
                .find(|p| p.reference.trim().eq_ignore_ascii_case(input))
        }).or_else(|| {
            (!numeric && self.visible_products.len() == 1).then(|| &self.catalog.products[self.visible_products[0]])
        }).map(|product| product.id.clone());
        if let Some(id) = chosen {
            self.add_product(id);
        } else if self.visible_products.is_empty() || numeric {
            self.notice = Some((true, "Código o producto no encontrado".into()));
            self.search.clear();
            self.refresh_products();
            self.focus_search = true;
        }
    }

    fn cart(&mut self, ui: &mut Ui, width: f32, upper: f32) {
        let height = (upper - 104.0).max(70.0);
        classic::panel(classic::DESK).show(ui, |ui| {
            ui.set_min_width((width - 10.0).max(1.0));
            let (header, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 25.0), Sense::hover());
            ticket_cells(ui, header, TICKET_COLUMNS.map(|(label, _)| RichText::new(label).small().strong()));
            ScrollArea::vertical().id_salt("cart").max_height(height).min_scrolled_height(height).auto_shrink([false, false])
                .stick_to_bottom(true).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    if self.draft.is_empty() {
                        ui.weak(if self.sale_ready() { "Escanea o selecciona un producto." } else { "Abre la caja para vender." });
                    }
                    for (index, line) in self.draft.lines().iter().enumerate() {
                        if ticket_row(ui, line, self.selected_line == Some(line.key()), index % 2 != 0) {
                            self.selected_line = Some(line.key());
                        }
                    }
                });
        });
        ui.horizontal_top(|ui| {
            let readout_width = ((width - 2.0 * ui.spacing().item_spacing.x) / 3.0).clamp(34.0, 121.0);
            let user_width = width - 3.0 * (readout_width + ui.spacing().item_spacing.x);
            if user_width >= 85.0 {
                if let Some(user) = &self.user {
                    let register = self.session.as_ref().map(|s| format!(" · Caja {}", s.sequence)).unwrap_or_default();
                    ui.add_sized([user_width, 25.0], egui::Label::new(
                        RichText::new(format!("{}{register}", user.name.trim())).small().strong()
                    ).truncate());
                }
            }
            if let Ok(totals) = self.draft.totals() {
                for (label, value, primary) in [("Subtotal", totals.net, false), ("IVA", totals.tax, false), ("Total", totals.total, true)] {
                    classic::readout(ui, label, value.format_cop_exact(), readout_width, primary);
                }
            }
        });
    }

    fn line_tools(&mut self, ui: &mut Ui, width: f32) {
        let line = self.selected_line;
        let selling = self.sale_ready();
        let ready = selling && !self.draft.is_empty();
        let size = egui::vec2(width, if width > 50.0 { 41.0 } else { 32.0 });
        if classic::icon_button(ui, Icon::Delete, size, ready && line.is_some() && self.can(Capability::RemoveLine), "Borrar línea (Supr)", None).clicked() {
            self.request_remove();
        }
        if classic::icon_button(ui, Icon::Search, size, selling, "Buscar artículo (F2)", None).clicked() {
            self.open_product_search();
        }
        if classic::icon_button(ui, Icon::Edit, size, ready && line.is_some(), "Editar cantidad o peso (F3)", None).clicked() {
            self.edit_selected(false);
        }
        if classic::icon_button(ui, Icon::Note, size, ready && line.is_some(), "Nota de la línea (F5)", None).clicked() {
            self.edit_selected(true);
        }
        if self.can(Capability::LineDiscount) || self.can(Capability::TicketDiscount) {
            ui.menu_button("%", |ui| {
                if ui.add_enabled(ready && line.is_some() && self.can(Capability::LineDiscount), Button::new("Descuento por línea (F4)")).clicked() {
                    ui.close_menu();
                    self.show_dialog(Dialog::Edit(EditKind::Discount(line, true), "0".into()));
                }
                if ui.add_enabled(ready && self.can(Capability::TicketDiscount), Button::new("Descuento sobre el total")).clicked() {
                    ui.close_menu();
                    self.show_dialog(Dialog::Edit(EditKind::Discount(None, true), "0".into()));
                }
            });
        }
    }

    fn ticket_controls(&mut self, ui: &mut Ui, keyboard_height: f32) {
        let line = self.selected_line;
        let selling = self.sale_ready();
        let ready = selling && !self.draft.is_empty();
        let adjust = ready && self.draft.lines().iter()
            .any(|item| Some(item.key()) == line && !item.product().unit.is_weighted());
        let mut submit = false;
        ui.add_enabled_ui(selling, |ui| {
            ui.horizontal(|ui| {
                let width = (ui.available_width() - ui.spacing().item_spacing.x - 41.0).max(80.0);
                let scan = ui.add_sized([width, 31.0], TextEdit::singleline(&mut self.search)
                    .id_salt("scanner").hint_text("Código de barras").char_limit(SCAN_LIMIT));
                self.scanner_id = Some(scan.id);
                if std::mem::take(&mut self.focus_search) {
                    scan.request_focus();
                }
                if scan.changed() {
                    self.refresh_products();
                }
                submit = scan.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                submit |= classic::icon_button(ui, Icon::Barcode, egui::vec2(41.0, 31.0),
                    !self.search.trim().is_empty(), "Agregar por código de barras", None).clicked();
            });
        });
        ui.label(RichText::new("Buscar por código o nombre · F1").weak().size(12.0));
        let keypad = ui.add_enabled_ui(selling, |ui| {
            classic::keypad(ui, ui.available_width(), keyboard_height, ready && line.is_some(), adjust, ready)
        }).inner;
        if let Some(key) = keypad {
            self.touch_key(key);
        }
        if submit {
            self.submit_scan();
        }
    }

    fn touch_key(&mut self, key: KeypadKey) {
        if !self.sale_ready() { return; }
        match key {
            KeypadKey::Clear | KeypadKey::Digit(_) | KeypadKey::Comma => {
                match key {
                    KeypadKey::Clear => self.search.clear(),
                    KeypadKey::Digit(c) if self.search.chars().count() < SCAN_LIMIT => self.search.push(c),
                    KeypadKey::Comma if self.search.chars().count() < SCAN_LIMIT => self.search.push('.'),
                    _ => {}
                }
                self.refresh_products();
                self.focus_search = true;
            }
            KeypadKey::Quantity => self.edit_selected(false),
            KeypadKey::Minus | KeypadKey::Plus => {
                let Some(line) = self.draft.lines().iter().find(|line| Some(line.key()) == self.selected_line) else { return };
                if line.product().unit.is_weighted() {
                    self.edit_selected(false);
                    return;
                }
                let quantity = line.quantity() + if key == KeypadKey::Plus { Qty::one() } else { -Qty::one() };
                if !quantity.is_positive() {
                    if self.can(Capability::RemoveLine) {
                        self.request_remove();
                    } else {
                        self.notice = Some((true, "No tienes permiso para eliminar la línea.".into()));
                    }
                    return;
                }
                let result = self.draft.set_quantity(line.key(), quantity);
                self.report_error(result);
                self.focus_search = true;
            }
            KeypadKey::Pay => self.request_checkout(),
        }
    }

    fn sale_keys(&mut self, ctx: &Context) {
        if !self.sale_ready() || self.draft.is_empty() { return; }
        let scan_focused = self.scanner_id.is_some_and(|id| ctx.memory(|mem| mem.has_focus(id)));
        // Un código o una búsqueda en curso puede contener espacios y signos.
        if scan_focused && !self.search.is_empty() { return; }
        let selected = self.selected_line.is_some();
        let shortcut = |event: &egui::Event| match event {
            egui::Event::Key { key, pressed: true, modifiers, .. }
                if !modifiers.alt && !modifiers.ctrl && !modifiers.command => match key {
                    Key::Space if !modifiers.shift => Some(KeypadKey::Pay),
                    Key::Plus if selected => Some(KeypadKey::Plus),
                    Key::Minus if selected => Some(KeypadKey::Minus),
                    _ => None,
                },
            // Algunos layouts entregan "+" como texto junto a Key::Equals.
            egui::Event::Text(text) if text == "+" && selected => Some(KeypadKey::Plus),
            _ => None,
        };
        let action = ctx.input_mut(|input| {
            if scan_focused && input.events.iter().any(|event| matches!(event,
                egui::Event::Text(text) if !matches!(text.as_str(), " " | "+" | "-")
            )) { return None; }
            let action = input.events.iter().find_map(shortcut)?;
            let repeated = input.events.iter().any(|event| {
                matches!(event, egui::Event::Key { pressed: true, repeat: true, key, .. }
                    if matches!(key, Key::Space | Key::Plus | Key::Minus | Key::Equals))
            });
            input.events.retain(|event| shortcut(event).is_none() && !matches!(event,
                egui::Event::Text(text) if matches!(text.as_str(), " " | "+" | "-")
            ));
            (!repeated).then_some(action)
        });
        if let Some(action) = action {
            self.touch_key(action);
        }
    }

    fn shortcuts(&mut self, ctx: &Context) {
        let pressed = |key| press_once(ctx, key);
        if pressed(Key::F10) {
            self.menu_open = !self.menu_open;
            ctx.request_repaint();
        } else if self.sale_ready() && pressed(Key::F1) {
            self.focus_search = true;
            ctx.request_repaint();
        } else if self.sale_ready() && pressed(Key::F2) {
            self.open_product_search();
        } else if pressed(Key::F3) {
            self.edit_selected(false);
        } else if pressed(Key::F4) && self.can(Capability::LineDiscount) {
            if let Some(line) = self.selected_line {
                self.show_dialog(Dialog::Edit(EditKind::Discount(Some(line), true), "0".into()));
            }
        } else if pressed(Key::F5) {
            self.edit_selected(true);
        } else if pressed(Key::F6) {
            self.request_checkout();
        } else if self.sale_ready() && self.settings.drawer && self.can(Capability::OpenDrawer) && pressed(Key::F7) {
            let result = self.manual_drawer();
            self.report_error(result);
        } else if pressed(Key::F8) {
            let result = self.request_receipt(None, false);
            self.report_error(result);
        } else if self.sale_ready() && self.can(Capability::CloseSession) && pressed(Key::F9) {
            let result = self.prepare_close();
            self.report_error(result);
        } else if pressed(Key::Delete) && self.search.is_empty() {
            self.request_remove();
        } else if pressed(Key::Escape) && !self.draft.is_empty() {
            if self.search.is_empty() {
                self.show_dialog(Dialog::CancelTicket);
            } else {
                self.search.clear();
                self.refresh_products();
                self.focus_search = true;
            }
        }
    }

    fn edit_selected(&mut self, note: bool) {
        if let Some(line) = self.draft.lines().iter().find(|l| Some(l.key()) == self.selected_line) {
            let dialog = if note {
                Dialog::Edit(EditKind::Note(line.key()), line.note().unwrap_or("").to_owned())
            } else {
                Dialog::Edit(EditKind::Line {
                    key: line.key(),
                    price: product_price_with_tax(line.product()).to_plain_string(),
                    price_active: false,
                }, line.quantity().to_plain_string())
            };
            self.show_dialog(dialog);
        }
    }

    fn open_product_search(&mut self) {
        let search = products::ProductSearch::new(&self.search, &self.catalog);
        self.show_dialog(Dialog::ProductSearch(Box::new(search)));
    }

    fn request_remove(&mut self) {
        if self.can(Capability::RemoveLine) {
            if let Some(key) = self.selected_line {
                self.show_dialog(Dialog::Remove(key));
            }
        }
    }

    fn request_checkout(&mut self) {
        if self.sale_ready() && !self.draft.is_empty() {
            self.show_dialog(Dialog::Checkout(Box::default()));
        }
    }

    pub(super) fn render_dialog(&mut self, ctx: &Context) {
        if self.dialog.as_ref().and_then(Dialog::page_title).is_some() { return; }
        let Some(mut dialog) = self.dialog.take() else { return };
        let waiting_drawer = matches!(dialog, Dialog::Change { .. }) && self.waiting_for_sale_drawer(ctx);
        if waiting_drawer {
            egui::Modal::new(Id::new("pos-drawer-pending")).show(ctx, |ui| {
                ui.set_width(310.0);
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Pago guardado. Enviando el pulso del cajón…");
                });
            });
            self.dialog = Some(dialog);
            return;
        }
        let (suffix, width): (&str, f32) = match &dialog {
            Dialog::Checkout(_) => ("checkout", 738.0),
            Dialog::ProductSearch(_) => ("product-search", 735.0),
            Dialog::OpenSales => ("pending-tickets", 496.0),
            Dialog::Edit(EditKind::Weight(_) | EditKind::Line { .. }, _) => ("line-editor", 686.0),
            Dialog::Change { .. } => ("change", 370.0),
            Dialog::Remove(_) | Dialog::CancelTicket => ("delete-confirm", 298.0),
            _ => ("dialog", 420.0),
        };
        let modal = egui::Modal::new(Id::new(("pos-dialog", suffix))).show(ctx, |ui| {
            ui.set_width(width.min(ctx.screen_rect().width() - 32.0));
            self.form_contents(ui, &mut dialog)
        });
        let cancel = modal.is_top_modal && !modal.any_popup_open
            && ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape));
        self.handle_form(dialog, if cancel { Intent::Dismiss } else { modal.inner });
    }

    fn render_page(&mut self, ui: &mut Ui, title: &'static str) {
        let Some(mut dialog) = self.dialog.take() else { return };
        let stock_subpage = matches!(title, "Products" | "Stock Management" | "Inventory Current");
        if press_once(ui.ctx(), Key::F10) {
            self.menu_open = !self.menu_open;
            ui.ctx().request_repaint();
        }
        let back = ui.horizontal(|ui| {
            ui.label(RichText::new(title).strong().color(classic::BLUE));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui|
                ui.button(if stock_subpage { "Volver a Stock" } else { "Volver a Sales" }).clicked()
            ).inner
        }).inner;
        ui.separator();
        let intent = if back || ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            if dialog.editing_product() {
                self.dialog_error = Some("Guarda o cancela la edición del producto antes de salir.".into());
                self.dialog = Some(dialog);
                return;
            }
            if stock_subpage {
                self.show_dialog(Dialog::Menu(Box::new(menu::MenuDialog::Stock)));
                return;
            }
            Intent::Dismiss
        } else if title == "Products" {
            ui.set_max_width(1400.0);
            self.form_contents(ui, &mut dialog)
        } else {
            ScrollArea::vertical().id_salt(title).show(ui, |ui| {
                ui.set_max_width(match dialog {
                    Dialog::Cash(..) | Dialog::Lookup(..) => 1600.0,
                     Dialog::Menu(_) if matches!(title, "Stock" | "Close Cash" | "Tools") => 1400.0,
                      Dialog::Menu(_) if matches!(title, "Printers" | "Configuration") => 980.0,
                    Dialog::Close(_) => 1200.0,
                    _ => 480.0,
                });
                self.form_contents(ui, &mut dialog)
            }).inner
        };
        self.handle_form(dialog, intent);
    }

    fn form_contents(&mut self, ui: &mut Ui, dialog: &mut Dialog) -> Intent {
            match dialog {
                Dialog::Checkout(form) => {
                    return match form.show(ui, &self.draft) {
                        CheckoutAction::Wait => Intent::Wait,
                        CheckoutAction::Cancel => Intent::Dismiss,
                        CheckoutAction::Save => Intent::Commit,
                    };
                }
                Dialog::Opening(value) => {
                    ui.heading("Abrir caja");
                    ui.label("Fondo inicial de efectivo (COP)");
                    let enter = field(ui, "opening", value, 20, &mut self.focus_dialog);
                    if ui.button("Abrir caja").clicked() || enter { return Intent::Apply; }
                }
                Dialog::ProductSearch(form) => {
                    return match form.show(ui, &self.catalog, &mut self.focus_dialog, &mut self.dialog_error) {
                        products::SearchAction::Wait => Intent::Wait,
                        products::SearchAction::Cancel => Intent::Dismiss,
                        products::SearchAction::Add(id) => Intent::AddFoundProduct(id),
                    };
                }
                Dialog::OpenSales => {
                    return self.open_sales_form(ui);
                }
                Dialog::Edit(kind, value) => {
                    if matches!(kind, EditKind::Weight(_) | EditKind::Line { .. }) {
                        return self.line_editor(ui, kind, value);
                    }
                    let title = match kind {
                        EditKind::Weight(_) | EditKind::Line { .. } => unreachable!(),
                        EditKind::Note(_) => "Nota de la línea",
                        EditKind::Discount(key, percent) => {
                            ui.heading(if key.is_some() { "Descuento por línea" } else { "Descuento total" });
                            ui.horizontal(|ui| {
                                ui.selectable_value(percent, true, "Porcentaje");
                                ui.selectable_value(percent, false, "Valor COP");
                            });
                            "Importe · Usa 0 para quitar el descuento"
                        }
                    };
                    ui.label(title);
                    let limit = if matches!(kind, EditKind::Note(_)) { 80 } else { 15 };
                    let enter = field(ui, "edit", value, limit, &mut self.focus_dialog);
                    if ui.button("Aceptar").clicked() || enter { return Intent::Apply; }
                }
                Dialog::Remove(_) | Dialog::CancelTicket => {
                    let whole = matches!(dialog, Dialog::CancelTicket);
                    let title = if whole { "Cancelar venta" } else { "Eliminar línea" };
                    egui::Frame::none().fill(Color32::from_rgb(27, 23, 30)).inner_margin(4.0).show(ui, |ui| {
                        ui.vertical_centered(|ui| ui.label(RichText::new(title).color(Color32::WHITE)));
                    });
                    ui.horizontal(|ui| {
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(33.0, 38.0), Sense::hover());
                        ui.painter().circle_filled(rect.center(), 14.0, classic::CYAN);
                        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "?",
                            egui::FontId::proportional(22.0), Color32::WHITE);
                        ui.label(if whole { "¿Quieres eliminar todo el ticket?" } else { "¿Quieres eliminar esta línea?" });
                    });
                    if let Some(error) = &self.dialog_error { ui.colored_label(ui.visuals().error_fg_color, error); }
                    return ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add_sized([76.0, 32.0], Button::new("No")).clicked() { return Intent::Dismiss; }
                        if ui.add(products::primary("Sí")).clicked() { Intent::Apply } else { Intent::Wait }
                    }).inner;
                }
                Dialog::Change { ticket, total, amount } => {
                    classic::title(ui, "Vueltas");
                    ui.vertical_centered(|ui| {
                        ui.label(format!("Ticket {ticket} · Total {}", total.format_cop_exact()));
                        ui.add_space(10.0);
                        ui.label(RichText::new(amount.format_cop_exact()).size(42.0).strong().color(classic::BLUE));
                        ui.add_space(10.0);
                    });
                    if self.printer_notice.as_ref().is_some_and(|(error, _)| *error) {
                        show_notice(ui, &self.printer_notice);
                    }
                    if press_once(ui.ctx(), Key::F8) { return Intent::Print; }
                    return if press_once(ui.ctx(), Key::Enter) || ui.add(products::primary("Continuar")).clicked() {
                        Intent::Dismiss
                    } else { Intent::Wait };
                }
                Dialog::Cash(direction, amount, reason) => {
                    ui.horizontal(|ui| {
                        ui.label("Tipo de movimiento");
                        ui.selectable_value(direction, CashDirection::In, "Entrada de caja");
                        ui.selectable_value(direction, CashDirection::Out, "Salida de caja");
                    });
                    let pad = (ui.available_width() * 0.25).clamp(165.0, 286.0);
                    let mut save = false;
                    ui.horizontal_top(|ui| {
                        let form_width = ui.available_width() - pad - ui.spacing().item_spacing.x;
                        ui.allocate_ui_with_layout(egui::vec2(form_width, 1.0), Layout::top_down(Align::LEFT), |ui| {
                            ui.label("Importe COP");
                            ui.scope(|ui| {
                                ui.set_width(200.0);
                                field(ui, "cash-amount", amount, 20, &mut self.focus_dialog);
                            });
                            ui.label("Motivo");
                            ui.add(TextEdit::multiline(reason).desired_width(200.0).desired_rows(3).char_limit(100));
                            save = ui.button("Guardar movimiento").clicked();
                        });
                        if let Some(key) = classic::keypad(ui, pad, 208.0, false, false, false) {
                            match key {
                                KeypadKey::Clear => amount.clear(),
                                KeypadKey::Digit(c) if amount.len() < 20 => amount.push(c),
                                KeypadKey::Comma if !amount.contains(['.', ',']) => amount.push(','),
                                _ => {}
                            }
                        }
                    });
                    if save { return Intent::Apply; }
                }
                Dialog::Close(form) => {
                    if close_form(ui, form, self.session.as_ref()) { return Intent::Apply; }
                }
                Dialog::Lookup(number, found) => {
                    let intent = ui.horizontal(|ui| {
                        ui.label("N.º de ticket");
                        let enter = ui.scope(|ui| {
                            ui.set_width(160.0);
                            field(ui, "ticket-search", number, 18, &mut self.focus_dialog)
                        }).inner;
                        if classic::icon_button(ui, Icon::Search, egui::vec2(78.0, 34.0), true, "Buscar el ticket", Some("Buscar")).clicked() || enter {
                            return Intent::Query;
                        }
                        if classic::icon_button(ui, Icon::Print, egui::vec2(115.0, 34.0),
                            found.is_some() && !self.printing_receipt, "Reimprimir el ticket", Some("Reimprimir")).clicked()
                        {
                            return Intent::Print;
                        }
                        Intent::Wait
                    }).inner;
                    if !matches!(intent, Intent::Wait) { return intent; }
                    let mut query = false;
                    let pad = (ui.available_width() * 0.25).clamp(165.0, 286.0);
                    let left = ui.available_width() - pad - ui.spacing().item_spacing.x;
                    ui.horizontal_top(|ui| {
                        ui.allocate_ui_with_layout(egui::vec2(left, 240.0), Layout::top_down(Align::LEFT), |ui| {
                            classic::panel(classic::DESK).show(ui, |ui| {
                                ui.set_min_size(egui::vec2((left - 10.0).max(1.0), 240.0));
                                if let Some(receipt) = found {
                                    ui.strong(format!("Ticket {} · {} · {}", receipt.number, receipt.date, receipt.cashier));
                                    for line in &receipt.lines {
                                        ui.label(format!("{} × {}: {}", line.quantity.format(line.unit),
                                            line.name, line.amount.format_cop_exact()));
                                    }
                                    ui.strong(format!("Total {}", receipt.total.format_cop_exact()));
                                    if receipt.collected != receipt.total {
                                        ui.label(format!("Cobrado (histórico): {}", receipt.collected.format_cop_exact()));
                                    }
                                    show_notice(ui, &self.printer_notice);
                                } else {
                                    ui.weak("Consulta y reimpresión de tickets. El historial no se modifica.");
                                }
                            });
                        });
                        if let Some(key) = classic::keypad(ui, pad, 225.0, false, false, !number.is_empty()) {
                            match key {
                                KeypadKey::Digit(c) if number.len() < 18 => number.push(c),
                                KeypadKey::Clear => number.clear(),
                                KeypadKey::Pay => query = true,
                                _ => {}
                            }
                        }
                    });
                    if query { return Intent::Query; }
                }
                Dialog::Menu(form) => match self.show_menu_dialog(ui, form) {
                    Ok(true) => return Intent::Dismiss,
                    Ok(false) => {}
                    Err(error) => self.dialog_error = Some(error.to_string()),
                }
            }
            if let Some(error) = &self.dialog_error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            if dialog.page_title().is_none() && ui.button("Volver").clicked() { Intent::Dismiss } else { Intent::Wait }
    }

    fn handle_form(&mut self, mut dialog: Dialog, intent: Intent) {
        if matches!(intent, Intent::Dismiss) {
            self.hide_dialog();
            return;
        }
        let intent = match intent {
            Intent::AddFoundProduct(id) => {
                self.hide_dialog();
                self.add_product(id);
                return;
            }
            other => other,
        };
        let result: Result<bool, Box<dyn Error>> = (|| {
            match (&mut dialog, intent) {
                (Dialog::Opening(value), Intent::Apply) => self.open_session(money_field(value)?)?,
                (Dialog::Edit(kind, value), Intent::Apply) => self.apply_edit(kind, value)?,
                (Dialog::Remove(key), Intent::Apply) => self.delete_lines(Some(*key))?,
                (Dialog::CancelTicket, Intent::Apply) => self.delete_lines(None)?,
                (Dialog::OpenSales, Intent::ResumeSale(number)) => self.resume_sale(number),
                (Dialog::Checkout(form), Intent::Commit) => {
                    if let Err(error) = self.commit_sale(&form.payments) {
                        form.fail(error.to_string());
                        return Ok(false);
                    }
                    return Ok(true);
                }
                (Dialog::Change { .. }, Intent::Print) => {
                    let result = self.request_receipt(None, false);
                    self.report_error(result);
                }
                (Dialog::Cash(direction, value, reason), Intent::Apply) => {
                    let (session, user) = self.session_user().ok_or("abre la caja primero")?;
                    let amount = money_field(value)?;
                    if reason.trim().is_empty() { return Err("escribe el motivo del movimiento".into()); }
                    let now = self.timestamp()?;
                    pos_store::record_cash_movement(&mut self.conn, &session, &user, *direction, amount, reason.trim(), &now)?;
                    self.notice = Some((false, format!("Movimiento registrado: {}", amount.format_cop_exact())));
                }
                (Dialog::Close(form), Intent::Apply) => {
                    self.finish_close(money_field(&form.counted)?)?;
                    dialog = Dialog::Menu(Box::new(menu::MenuDialog::ClosedCash));
                    return Ok(false);
                }
                (Dialog::Lookup(number, found), Intent::Query) => {
                    *found = None;
                    let number: i64 = number.trim().parse().map_err(|_| "número de ticket inválido")?;
                    *found = Some(Box::new(pos_store::ticket_by_number(&self.conn, number)?.ok_or("ticket no encontrado")?));
                    self.dialog_error = None;
                    return Ok(false);
                }
                (Dialog::Lookup(_, Some(receipt)), Intent::Print) => {
                    self.request_receipt(Some(receipt), true)?;
                    self.dialog_error = None;
                    return Ok(false);
                }
                _ => return Ok(false),
            }
            Ok(true)
        })();
        match result {
            Ok(true) => {
                if self.dialog.is_none() { self.hide_dialog(); }
            }
            Ok(false) => self.dialog = Some(dialog),
            Err(error) => {
                self.dialog_error = Some(error.to_string());
                self.dialog = Some(dialog);
            }
        }
    }

    fn open_sales_form(&self, ui: &mut Ui) -> Intent {
        classic::title(ui, "Tickets · Ventas abiertas");
        let user = self.user.as_ref().map_or("", |user| user.name.trim());
        let chosen = classic::panel(classic::DESK).show(ui, |ui| {
            let height = (ui.ctx().screen_rect().height() - 150.0).clamp(240.0, 408.0);
            ScrollArea::vertical().auto_shrink([false, false]).max_height(height).min_scrolled_height(height)
                .show(ui, |ui| {
                    let mut tickets: Vec<_> = self.parked_sales.iter()
                        .map(|sale| (sale.number, &sale.draft, sale.opened_at.as_str(), false)).collect();
                    if !self.draft.is_empty() {
                        tickets.push((self.sale_number, &self.draft, &self.sale_opened_at, true));
                    }
                    tickets.sort_by_key(|(number, ..)| *number);
                    if tickets.is_empty() {
                        ui.weak("No hay tickets pendientes.");
                    }
                    for (number, draft, time, current) in tickets {
                        let total = draft.totals().map(|total| total.total.format_cop_exact()).unwrap_or_else(|_| "—".into());
                        let time = time.get(11..16).unwrap_or("—");
                        let mark = if current { " · Actual" } else { "" };
                        let label = format!("#{number} · {user} · {time} · {total}{mark}");
                        if ui.add_sized([ui.available_width(), 38.0], Button::new(label).truncate().selected(current)).clicked() {
                            return if current { Intent::Dismiss } else { Intent::ResumeSale(number) };
                        }
                    }
                    Intent::Wait
                }).inner
        }).inner;
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.add_sized([90.0, 34.0], Button::new("Cerrar")).clicked() { Intent::Dismiss } else { chosen }
        }).inner
    }

    fn line_editor(&mut self, ui: &mut Ui, kind: &mut EditKind, value: &mut String) -> Intent {
        let line = match kind {
            EditKind::Line { key, .. } => self.draft.lines().iter().find(|line| line.key() == *key),
            _ => None,
        };
        let snapshot;
        let (product, heading) = if let Some(line) = line {
            (line.product(), "Editar línea")
        } else if let EditKind::Weight(id) = kind {
            let Some(product) = self.catalog.product_by_id(id.as_str()) else { return Intent::Dismiss };
            snapshot = product.snapshot(self.catalog.price_mode);
            (&snapshot, "Peso del producto")
        } else {
            return Intent::Dismiss;
        };
        let mut editable = match kind {
            EditKind::Line { price, price_active, .. } if self.can(Capability::ChangePrice) => Some((price, price_active)),
            _ => None,
        };
        classic::title(ui, heading);
        let width = ui.available_width();
        let pad = (width * 0.43).min(285.0);
        let mut submit = false;
        ui.horizontal_top(|ui| {
            let left = width - pad - ui.spacing().item_spacing.x;
            ui.allocate_ui_with_layout(egui::vec2(left, 1.0), Layout::top_down(Align::Min), |ui| {
                ui.add(egui::Label::new(RichText::new(product.name.trim()).strong()).truncate());
                ui.add_space(8.0);
                let input = ui.horizontal(|ui| {
                    ui.label(if product.unit.is_weighted() { "Peso en kg" } else { "Unidades" });
                    ui.add(TextEdit::singleline(value).id_salt("line-quantity").desired_width(112.0).char_limit(15))
                }).inner;
                let restore = std::mem::take(&mut self.focus_dialog);
                if let Some((_, active)) = editable.as_mut() {
                    if input.clicked() || input.gained_focus() { **active = false; }
                }
                if restore && !editable.as_ref().is_some_and(|(_, active)| **active) { input.request_focus(); }
                submit = input.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                let with_tax = product_price_with_tax(product);
                let unit_price = if let Some((amount, active)) = editable.as_mut() {
                    let input = ui.horizontal(|ui| {
                        ui.label("Precio con IVA");
                        ui.add(TextEdit::singleline(*amount).id_salt("line-price").desired_width(112.0).char_limit(15))
                    }).inner;
                    if input.clicked() || input.gained_focus() { **active = true; }
                    if restore && **active { input.request_focus(); }
                    submit |= input.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    money_field(amount).ok().filter(|v| !v.is_negative()).map(|v| {
                        if v == with_tax { product.unit_price_net }
                        else { Money::from_decimal(v.as_decimal() / product.tax_rate.multiplier()) }
                    })
                } else {
                    boxed_stat(ui, "Precio con IVA", &with_tax.format_cop_exact());
                    Some(product.unit_price_net)
                };
                let base = unit_price.and_then(|unit_price| {
                    let qty = quantity_field(value).ok().filter(|qty| qty.is_positive())?;
                    let base = unit_price * qty;
                    let discount = line.and_then(TicketLine::discount).map_or(Ok(Money::ZERO), |d| d.amount_for(base));
                    Some(base - discount.ok()?)
                });
                let show = |amount: Option<Money>| amount.map_or("—".to_owned(), Money::format_cop_exact);
                for (label, amount) in [
                    ("Precio sin IVA", show(unit_price)),
                    ("Impuesto", product.tax_rate.format_percent()),
                    ("Subtotal", show(base)),
                    ("Total", show(base.map(|base| base + Money::from_decimal(base.as_decimal() * product.tax_rate.as_decimal())))),
                ] {
                    boxed_stat(ui, label, &amount);
                }
                if let Some(error) = &self.dialog_error { ui.colored_label(ui.visuals().error_fg_color, error); }
            });
            ui.allocate_ui_with_layout(egui::vec2(pad, 1.0), Layout::top_down(Align::Min), |ui| {
                let (target, decimal) = match editable {
                    Some((price, active)) if *active => (price, true),
                    _ => (value, product.unit.is_weighted()),
                };
                if products::numeric_pad(ui, pad, target, decimal, 15) {
                    self.dialog_error = None;
                    self.focus_dialog = true;
                    ui.ctx().request_repaint();
                }
            });
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.add(products::primary("Aceptar")).clicked() || submit { return Intent::Apply; }
            if ui.button("Cancelar").clicked() { Intent::Dismiss } else { Intent::Wait }
        }).inner
    }

    pub(super) fn report_error<T, E: std::fmt::Display>(&mut self, result: Result<T, E>) {
        if let Err(error) = result {
            self.notice = Some((true, error.to_string()));
        }
    }
}

enum Intent { Wait, Dismiss, Apply, Commit, Query, Print, ResumeSale(u64), AddFoundProduct(ProductId) }

fn press_once(ctx: &Context, key: Key) -> bool {
    ctx.input_mut(|i| {
        let first_press = i.events.iter().any(|event| {
            matches!(event, egui::Event::Key {
                key: actual, pressed: true, repeat: false, modifiers, ..
            } if *actual == key && *modifiers == Modifiers::NONE)
        });
        first_press && i.consume_key(Modifiers::NONE, key)
    })
}

fn ticket_cells(ui: &mut Ui, rect: egui::Rect, values: [RichText; 6]) {
    let mut x = rect.left();
    for (index, ((_, fraction), text)) in TICKET_COLUMNS.into_iter().zip(values).enumerate() {
        let width = rect.width() * fraction;
        classic::cell(ui, x + 3.0, rect.top(), width - 6.0, rect.height(), text,
            if index == 0 { Align::LEFT } else { Align::RIGHT });
        x += width;
    }
}

fn ticket_row(ui: &mut Ui, line: &TicketLine, selected: bool, striped: bool) -> bool {
    let product = line.product();
    let discount = line.discount_amount().unwrap_or(Money::ZERO);
    let mut detail = line.note().unwrap_or_default().to_owned();
    if !discount.is_zero() {
        if !detail.is_empty() { detail.push_str(" · "); }
        detail.push_str(&format!("Desc. {}", discount.format_cop_exact()));
    }
    let (rect, row) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), if detail.is_empty() { 28.0 } else { 45.0 }), Sense::click(),
    );
    row.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, ui.is_enabled(), selected, product.name.trim()));
    let background = if selected || row.hovered() { classic::SELECTED }
        else if striped { classic::STRIPE }
        else { classic::DESK };
    ui.painter().rect_filled(rect, 0.0, background);
    let mut top = rect;
    top.max.y = top.min.y + 28.0;
    let price = product.unit_price_net.format_cop_exact();
    let amount = line.net().unwrap_or(Money::ZERO).format_cop_exact();
    ticket_cells(ui, top, [
        RichText::new(product.name.trim()), RichText::new(&price).size(12.0),
        RichText::new(line.quantity().format(product.unit)).small(),
        RichText::new(product.tax_rate.format_percent()).small(), RichText::new(&amount).size(12.0).strong(),
        RichText::new("—").small().weak(),
    ]);
    if !detail.is_empty() {
        classic::cell(ui, rect.left() + 5.0, top.bottom() - 2.0, rect.width() - 10.0, 16.0,
            RichText::new(&detail).small().color(classic::BLUE), Align::LEFT);
    }
    row.on_hover_text(format!("{} · {price} · {amount}\n{detail}", product.name.trim())).clicked()
}

fn field(ui: &mut Ui, id: &'static str, value: &mut String, limit: usize, focus: &mut bool) -> bool {
    let response = ui.add(TextEdit::singleline(value).id_salt(id).desired_width(235.0).char_limit(limit));
    if std::mem::take(focus) {
        response.request_focus();
    }
    response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter))
}

fn show_notice(ui: &mut Ui, notice: &Option<(bool, String)>) {
    if let Some((error, text)) = notice {
        let color = if *error { ui.visuals().error_fg_color } else { Color32::from_rgb(27, 102, 55) };
        ui.colored_label(color, text);
    }
}

fn close_form(ui: &mut Ui, form: &mut CloseForm, session: Option<&SessionInfo>) -> bool {
    let Some(session) = session else { return false };
    close_summary(ui, session, None, &form.totals, &form.taxes);
    let expected = form.totals.expected_cash();
    ui.separator();
    ui.horizontal(|ui| {
        ui.strong("Efectivo contado");
        if ui.add(TextEdit::singleline(&mut form.counted).desired_width(154.0).char_limit(20)).changed() {
            form.cash.clear();
            form.confirmed = false;
        }
        if let Ok(counted) = money_field(&form.counted) {
            ui.strong(format!("Diferencia: {}", (counted - expected).format_cop_exact()));
        }
    });
    ui.horizontal_wrapped(|ui| {
        for denomination in Denomination::ALL {
            if ui.button(denomination.label()).clicked() && form.cash.add(denomination, 1).is_ok() {
                form.counted = form.cash.total().to_plain_string();
                form.confirmed = false;
            }
        }
    });
    ui.horizontal_wrapped(|ui| {
        ui.checkbox(&mut form.confirmed, "Confirmo que ya conté el efectivo");
        ui.add_enabled(form.confirmed && money_field(&form.counted).is_ok(), products::primary("Confirmar cierre")).clicked()
    }).inner
}

pub(super) fn close_summary(
    ui: &mut Ui, session: &SessionInfo, closed: Option<(&str, &SessionClose)>,
    totals: &SessionTotals, taxes: &[pos_store::sessions::SessionTax],
) {
    ui.horizontal_wrapped(|ui| {
        ui.strong("Caja");
        ui.monospace(session.sequence.to_string());
        ui.separator();
        ui.strong("Inicio");
        ui.monospace(&session.datestart);
        ui.separator();
        ui.label(&session.host);
    });
    ui.add_space(5.0);
    let left = (ui.available_width() * 0.52).min(600.0);
    let right = (ui.available_width() - left - ui.spacing().item_spacing.x).min(550.0);
    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(egui::vec2(left, 1.0), Layout::top_down(Align::Min), |ui| {
            close_table(ui, "close-payments", &["Medio de pago", "Importe"], |ui| {
                for (method, amount) in totals.collected_by_method() {
                    close_row(ui, &[method.label(), &amount.format_cop_exact()]);
                }
                close_row(ui, &["Entradas", &totals.cash_in.format_cop_exact()]);
                close_row(ui, &["Salidas", &totals.cash_out.format_cop_exact()]);
                close_row(ui, &["Fondo inicial", &totals.opening_float.format_cop_exact()]);
            });
            close_table(ui, "close-taxes", &["Categoría de impuesto", "Impuesto", "Base neta"], |ui| {
                if taxes.is_empty() { close_row(ui, &["Sin movimientos", "—", "—"]); }
                for row in taxes {
                    close_row(ui, &[&row.category, &row.tax.format_cop_exact(), &row.base.format_cop_exact()]);
                }
            });
        });
        ui.allocate_ui_with_layout(egui::vec2(right, 1.0), Layout::top_down(Align::Min), |ui| {
            boxed_stat(ui, "Fin", closed.map_or("Caja abierta", |c| c.0));
            boxed_stat(ui, "Transacciones", &totals.ticket_count.to_string());
            boxed_stat(ui, "Subtotal", &totals.sales_net.format_cop_exact());
            boxed_stat(ui, "Impuestos", &totals.sales_tax.format_cop_exact());
            boxed_stat(ui, "Total de ventas", &totals.sales_total.format_cop_exact());
            boxed_stat(ui, "Total cobrado", &totals.collected_sales().format_cop_exact());
            boxed_stat(ui, "Efectivo esperado", &totals.expected_cash().format_cop_exact());
            if let Some((_, balance)) = closed {
                boxed_stat(ui, "Efectivo contado", &balance.counted_cash.format_cop_exact());
                boxed_stat(ui, "Diferencia", &balance.difference.format_cop_exact());
            }
            ui.separator();
            boxed_stat(ui, "Líneas borradas", &totals.removed_lines.to_string());
            boxed_stat(ui, "Cajón sin venta", &totals.no_sale_count.to_string());
        });
    });
}

fn close_table(ui: &mut Ui, id: &str, headers: &[&str], rows: impl FnOnce(&mut Ui)) {
    classic::panel(classic::DESK).show(ui, |ui| {
        ui.set_min_width((ui.available_width() - 1.0).max(1.0));
        let column = (ui.available_width() / headers.len() as f32 - 8.0).min(150.0);
        egui::Grid::new(id).num_columns(headers.len()).striped(true).min_col_width(column).show(ui, |ui| {
            for title in headers { ui.strong(*title); }
            ui.end_row();
            rows(ui);
        });
    });
}

fn close_row(ui: &mut Ui, cells: &[&str]) {
    for cell in cells { ui.label(*cell); }
    ui.end_row();
}

fn boxed_stat(ui: &mut Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            classic::panel(classic::DESK).inner_margin(3.0).show(ui, |ui| {
                ui.add_sized([(ui.available_width() * 0.47).clamp(70.0, 172.0), 18.0],
                    egui::Label::new(value).truncate()).on_hover_text(value);
            });
        });
    });
}
