use eframe::egui::{Align, Button, Color32, Rect, ScrollArea, Sense, TextEdit, Ui};
use pos_core::{PaymentMethod, StockMovement};
use pos_store::{backup::set_setting, ReportCsv, SalesReport};

use super::*;
use crate::classic::{self, Icon};

#[path = "printer_panel.rs"]
mod printer_panel;
#[path = "configuration.rs"]
mod configuration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Section {
    Sales, EditSales, Payments, CloseCash, Stock, Maintenance, Tools, Configuration, Printers, Logout,
}

const REGISTER: &[(Section, &str, Icon)] = &[
    (Section::Sales, "Sales", Icon::Cart),
    (Section::EditSales, "Edit Sales", Icon::Edit),
    (Section::Payments, "Payments", Icon::Payments),
    (Section::CloseCash, "Close Cash", Icon::Calculator),
];
const ADMINISTRATION: &[(Section, &str, Icon)] = &[
    (Section::Stock, "Stock", Icon::Stock),
    (Section::Maintenance, "Maintenance", Icon::Maintenance),
];
const UTILITIES: &[(Section, &str, Icon)] = &[
    (Section::Tools, "Tools", Icon::Tools),
    (Section::Printers, "Printers", Icon::Print),
];
const SYSTEM: &[(Section, &str, Icon)] = &[
    (Section::Configuration, "Configuration", Icon::Maintenance),
    (Section::Logout, "Logout", Icon::Logout),
];

pub(super) enum MenuDialog {
    Stock,
    Products(Box<products::ProductsForm>),
    StockEntry(StockForm),
    Inventory(StockForm),
    Maintenance,
    Tools(ToolsForm),
    Configuration(Box<configuration::ConfigurationForm>),
    Printers(Box<printer_panel::PrinterForm>),
    ClosedCash,
}

impl MenuDialog {
    pub(super) fn title(&self) -> &'static str {
        match self {
            Self::Stock => "Stock",
            Self::Products(_) => "Products",
            Self::StockEntry(_) => "Stock Management",
            Self::Inventory(_) => "Inventory Current",
            Self::Maintenance => "Maintenance",
            Self::Tools(_) => "Tools",
            Self::Configuration(_) => "Configuration",
            Self::Printers(_) => "Printers",
            Self::ClosedCash => "Close Cash",
        }
    }
}

#[derive(Default)]
pub(super) struct StockForm {
    query: String,
    product: Option<ProductId>,
    quantity: String,
    cost: String,
    reference: String,
}

pub(super) struct ToolsForm {
    first: String,
    last: String,
    report: Option<SalesReport>,
    tiles: tools::ToolTiles,
}

impl App {
    pub(super) fn render_sidebar(&mut self, ctx: &Context, disabled: bool) {
        let expanded = self.menu_open;
        let pane = (ctx.screen_rect().width() * 0.15).clamp(148.0, 239.0);
        let rail = if ctx.screen_rect().width() > 1200.0 { 34.0 } else { 27.0 };
        let width = if expanded { pane + rail } else { rail };
        let mut change = None;
        egui::SidePanel::left("side-navigation").exact_width(width).resizable(false).show_separator_line(false)
            .frame(egui::Frame::none().fill(classic::DESK)).show(ctx, |ui| {
                let available = ui.available_rect_before_wrap();
                ui.add_enabled_ui(!disabled, |ui| {
                    if expanded {
                        let panel_rect = Rect::from_min_size(
                            available.left_top() + egui::vec2(7.0, 7.0),
                            egui::vec2(pane - 13.0, available.height() - 14.0),
                        );
                        ui.scope_builder(egui::UiBuilder::new().max_rect(panel_rect), |ui| {
                            ScrollArea::vertical().id_salt("side-menu-scroll").show(ui, |ui| {
                                for (group, rows) in [
                                    ("Register", REGISTER), ("Administration", ADMINISTRATION),
                                    ("Utilities", UTILITIES), ("System", SYSTEM),
                                ] {
                                    if !rows.iter().any(|(section, _, _)| self.menu_allowed(*section)) { continue; }
                                    menu_group(ui, group, |ui| {
                                        for &(section, label, icon) in rows {
                                            if self.menu_allowed(section) && menu_row(ui, label, icon) {
                                                change = Some(section);
                                            }
                                        }
                                    });
                                }
                            });
                        });
                    }
                    let lever_height = (available.height() * 0.26).clamp(103.0, 274.0);
                    let at = egui::pos2(available.left() + if expanded { pane + 1.0 } else { 1.0 },
                        available.top() + available.height() * 0.47 - lever_height / 2.0);
                    let lever = Rect::from_min_size(at, egui::vec2(rail - 3.0, lever_height));
                    let response = ui.put(lever, Button::new("").min_size(lever.size()))
                        .on_hover_text(if expanded { "Ocultar menú (F10)" } else { "Abrir menú (F10)" });
                    let mark = Rect::from_center_size(response.rect.center(), egui::vec2((rail - 4.0).min(29.0), 27.0));
                    ui.painter().rect_filled(mark, 2.0, classic::CYAN);
                    ui.painter().text(mark.center(), egui::Align2::CENTER_CENTER,
                        if expanded { "‹" } else { "›" }, egui::FontId::proportional(26.0), Color32::WHITE);
                    if response.clicked() {
                        self.menu_open = !expanded;
                        self.focus_search = self.sale_ready();
                    }
                });
            });
        if let Some(section) = change {
            let result = self.select_menu(section);
            self.report_error(result);
        }
    }

    fn menu_allowed(&self, section: Section) -> bool {
        match section {
            Section::Sales => self.can(Capability::Sell),
            Section::EditSales => self.can(Capability::Reprint),
            Section::Payments => self.can(Capability::CashMovement),
            Section::CloseCash => self.can(Capability::CloseSession),
            Section::Stock | Section::Maintenance => self.can(Capability::Backoffice),
            Section::Tools => self.can(Capability::Reports) || self.can(Capability::Settings),
            Section::Configuration | Section::Printers => self.can(Capability::Settings),
            Section::Logout => self.user.is_some(),
        }
    }

    fn select_menu(&mut self, section: Section) -> Result<(), Box<dyn Error>> {
        if self.dialog.as_ref().is_some_and(Dialog::editing_product) {
            return Err("guarda o cancela la edición del producto antes de salir".into());
        }
        if !self.menu_allowed(section) { return Err("el usuario no tiene acceso a esta sección".into()); }
        self.menu_open = false;
        match section {
            Section::Sales if self.session.is_none() => self.show_dialog(Dialog::Opening("0".into())),
            Section::Sales => self.hide_dialog(),
            Section::EditSales => self.show_dialog(Dialog::Lookup(String::new(), None)),
            Section::Payments if self.sale_ready() => self.show_dialog(Dialog::Cash(CashDirection::In, String::new(), String::new())),
            Section::Payments => return Err("abre la caja antes de registrar un movimiento".into()),
            Section::CloseCash if self.session.is_some() => self.prepare_close()?,
            Section::CloseCash => self.open_menu_dialog(MenuDialog::ClosedCash),
            Section::Stock => {
                self.reload_catalog()?;
                self.open_menu_dialog(MenuDialog::Stock);
            }
            Section::Maintenance => self.open_menu_dialog(MenuDialog::Maintenance),
            Section::Tools => {
                let timestamp = self.timestamp()?;
                let today = timestamp[..10].to_owned();
                self.open_menu_dialog(MenuDialog::Tools(ToolsForm {
                    first: today.clone(), last: today, report: None, tiles: tools::ToolTiles::default(),
                }));
            }
            Section::Configuration => {
                let form = configuration::ConfigurationForm::load(self)?;
                self.open_menu_dialog(MenuDialog::Configuration(Box::new(form)));
            }
            Section::Printers => {
                let form = printer_panel::PrinterForm::load(self, false)?;
                self.open_menu_dialog(MenuDialog::Printers(Box::new(form)));
            }
            Section::Logout => self.logout()?,
        }
        Ok(())
    }

    pub(super) fn logout(&mut self) -> Result<(), Box<dyn Error>> {
        if self.has_pending_sales() || self.printing_receipt {
            return Err("termina las ventas abiertas y espera la tirilla antes de salir".into());
        }
        self.user = None;
        self.last_closed = None;
        self.notice = None;
        self.printer_notice = None;
        self.search.clear();
        self.category = None;
        self.menu_open = false;
        self.refresh_products();
        self.hide_dialog();
        Ok(())
    }

    fn open_menu_dialog(&mut self, dialog: MenuDialog) {
        self.show_dialog(Dialog::Menu(Box::new(dialog)));
    }

    pub(super) fn show_menu_dialog(&mut self, ui: &mut Ui, dialog: &mut MenuDialog) -> Result<bool, Box<dyn Error>> {
        match dialog {
            MenuDialog::Stock => {
                let w = (ui.available_width() * 0.25).clamp(148.0, 295.0);
                ui.add_space(14.0);
                ui.strong("Manage");
                ui.separator();
                let mut next = None;
                ui.horizontal_wrapped(|ui| {
                    if classic::icon_button(ui, Icon::Stock, egui::vec2(w, 88.0), true,
                        "Buscar, crear y editar productos", Some("Products")).clicked() {
                        next = Some(MenuDialog::Products(Box::new(products::ProductsForm::new(&self.catalog))));
                    }
                    if classic::icon_button(ui, Icon::Drawer, egui::vec2(w, 88.0), true,
                        "Registrar entradas de inventario", Some("Stock Management")).clicked() {
                        next = Some(MenuDialog::StockEntry(StockForm::default()));
                    }
                });
                ui.add_space(23.0);
                ui.strong("Reports");
                ui.separator();
                if classic::icon_button(ui, Icon::Note, egui::vec2(w, 88.0), true,
                    "Consultar existencias", Some("Inventory Current")).clicked() {
                    next = Some(MenuDialog::Inventory(StockForm::default()));
                }
                if let Some(next) = next {
                    *dialog = next;
                    self.focus_dialog = true;
                    self.dialog_error = None;
                }
            }
            MenuDialog::Products(form) => form.show(self, ui)?,
            MenuDialog::StockEntry(form) => return self.stock_panel(ui, form, true),
            MenuDialog::Inventory(form) => return self.stock_panel(ui, form, false),
            MenuDialog::Maintenance => {
                ui.strong(format!("Ubicación: {}", self.catalog.location.name));
                ui.separator();
                ui.strong("Usuarios y perfiles");
                for user in &self.catalog.users {
                    ui.label(format!("{} · {}", user.name.trim(), user.role.label()));
                }
                ui.separator();
                ui.strong("Categorías de impuestos");
                for tax in &self.catalog.taxes {
                    ui.label(format!("{} · {}", tax.name, tax.rate.format_percent()));
                }
            }
            MenuDialog::Tools(form) => self.tools_panel(ui, form)?,
            MenuDialog::Configuration(form) => {
                if form.show(self, ui)? {
                    *dialog = MenuDialog::Printers(Box::new(printer_panel::PrinterForm::load(self, true)?));
                }
            }
            MenuDialog::Printers(form) => form.show(self, ui)?,
            MenuDialog::ClosedCash => {
                if let Some(closed) = &self.last_closed {
                    screens::close_summary(ui, &closed.session, Some((&closed.timestamp, &closed.close)),
                        &closed.close.totals, &closed.taxes);
                    if ui.add_enabled(!self.printing_receipt, products::primary("Imprimir cierre")).clicked() {
                        self.request_close_receipt()?;
                    }
                    display_notice(ui, &self.printer_notice);
                } else {
                    ui.label("No hay una caja abierta para cerrar.");
                }
            }
        }
        Ok(false)
    }

    pub(super) fn reload_catalog(&mut self) -> Result<(), StoreError> {
        self.catalog = pos_store::load_catalog(&self.conn, self.catalog.price_mode)?;
        self.refresh_products();
        Ok(())
    }

    fn stock_panel(&mut self, ui: &mut Ui, form: &mut StockForm, entries: bool) -> Result<bool, Box<dyn Error>> {
        input(ui, "Producto, referencia o código de barras", &mut form.query, 128);
        let results = if form.query.trim().is_empty() { Vec::new() } else { self.catalog.search(&form.query, 40) };
        if !form.query.trim().is_empty() && results.is_empty() {
            ui.weak("No se encontraron productos.");
        }
        ScrollArea::vertical().max_height(120.0).show(ui, |ui| {
            for product in results {
                let current = product.stock.unwrap_or(Qty::ZERO).format(product.unit);
                if ui.selectable_label(form.product.as_ref() == Some(&product.id),
                    format!("{} · {} · {current}", product.name.trim(), product.code.trim())).clicked() {
                    form.product = Some(product.id.clone());
                    form.cost = product.cost.to_plain_string();
                }
            }
        });
        if let Some(product) = form.product.as_ref().and_then(|id| self.catalog.product_by_id(id.as_str())).filter(|_| entries) {
            ui.separator();
            ui.strong(format!("Entrada: {}", product.name.trim()));
            input(ui, if product.unit.is_weighted() { "Cantidad (kg)" } else { "Cantidad (unidades)" }, &mut form.quantity, 16);
            input(ui, "Costo unitario (COP)", &mut form.cost, 20);
            input(ui, "Referencia (opcional)", &mut form.reference, 60);
            if ui.button("Registrar entrada").clicked() {
                let quantity = quantity_field(&form.quantity)?;
                let user = self.user.as_ref().ok_or("sesión no autenticada")?;
                let movement = StockMovement::entry(product.id.clone(), self.catalog.location.id.clone(),
                    quantity, money_field(&form.cost)?, user.id.clone(), Some(form.reference.clone()))?;
                let now = self.timestamp()?;
                pos_store::record_stock_entry(&mut self.conn, &movement, &now)?;
                form.quantity.clear();
                form.reference.clear();
                let result = self.reload_catalog();
                self.notice = Some((false, format!("Entrada registrada: {}", quantity.to_plain_string())));
                if let Err(error) = result {
                    self.notice = Some((true, format!("Entrada registrada; no se pudo refrescar el catálogo: {error}")));
                }
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn tools_panel(&mut self, ui: &mut Ui, form: &mut ToolsForm) -> Result<(), Box<dyn Error>> {
        if form.tiles.show(self, ui)? { return Ok(()); }
        ui.separator();
        ui.strong("Reportes y respaldo");
        if self.can(Capability::Settings) {
            if ui.add_enabled(self.backup_done.is_none(), Button::new("Crear respaldo ahora")).clicked() {
                self.run_backup_worker(ui.ctx(), true);
            }
            display_notice(ui, &self.notice);
        }
        if !self.can(Capability::Reports) { return Ok(()); }
        ui.separator();
        ui.strong("Reporte de ventas por fecha");
        ui.horizontal(|ui| {
            ui.label("Desde");
            if ui.add(TextEdit::singleline(&mut form.first).desired_width(100.0).char_limit(10)).changed() { form.report = None; }
            ui.label("Hasta");
            if ui.add(TextEdit::singleline(&mut form.last).desired_width(100.0).char_limit(10)).changed() { form.report = None; }
        });
        if ui.button("Consultar (AAAA-MM-DD)").clicked() {
            form.report = None;
            form.report = Some(pos_store::sales_report(&self.conn, form.first.trim(), form.last.trim())?);
            self.dialog_error = None;
        }
        let Some(report) = &form.report else { return Ok(()) };
        let sales = Money::from_decimal(report.days.values().map(|day| day.total()).sum());
        let collected = Money::from_decimal(report.days.values().map(|day| day.collected).sum());
        ui.label(format!("Ventas: {}  ·  Cobrado: {}", sales.format_cop_exact(), collected.format_cop_exact()));
        ScrollArea::vertical().max_height(105.0).show(ui, |ui| {
            for (code, total) in &report.payments {
                ui.label(format!("{} · {}", PaymentMethod::from_code(code).label(), Money::from_decimal(*total).format_cop_exact()));
            }
        });
        let mut export = None;
        ui.horizontal_wrapped(|ui| {
            for (label, tag, kind) in [
                ("CSV por fecha", "fecha", ReportCsv::ByDay), ("CSV por producto", "producto", ReportCsv::ByProduct),
                ("CSV por medio", "medio", ReportCsv::ByPayment),
            ] {
                if ui.button(label).clicked() { export = Some((tag, kind)); }
            }
        });
        if let Some((tag, kind)) = export {
            let dir = beside_database(&self.db, "reportes");
            std::fs::create_dir_all(&dir)?;
            let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis();
            let file = dir.join(format!("ventas-{tag}-{stamp}.csv"));
            let mut bytes = Vec::new();
            report.write_csv(&mut bytes, kind)?;
            let mut handle = std::fs::OpenOptions::new().write(true).create_new(true).open(&file)?;
            use std::io::Write;
            handle.write_all(&bytes)?;
            self.dialog_error = None;
            self.notice = Some((false, format!("CSV: {}", file.display())));
        }
        Ok(())
    }

    fn save_user_settings(&mut self, action: &str, entries: &[(&str, &str)]) -> Result<(), StoreError> {
        let now = self.timestamp()?;
        let user = self.user.as_ref().ok_or(StoreError::PermissionDenied(Capability::Settings))?;
        let tx = self.conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        pos_store::auth::require_capability(&tx, user.id.as_str(), Capability::Settings)?;
        for &(key, value) in entries { set_setting(&tx, key, value.trim())?; }
        pos_store::audit(&tx, &now, Some(user.id.as_str()), action, "")?;
        tx.commit()?;
        Ok(())
    }
}

fn menu_row(ui: &mut Ui, label: &str, icon: Icon) -> bool {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 29.0), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label));
    if response.hovered() || response.has_focus() {
        ui.painter().rect_filled(rect, 1.0, classic::SELECTED);
    }
    classic::draw_icon(ui.painter(), rect.left_center() + egui::vec2(12.0, 0.0), icon, classic::CYAN);
    classic::cell(ui, rect.left() + 26.0, rect.top(), rect.width() - 28.0, rect.height(), RichText::new(label), Align::LEFT);
    response.clicked()
}

fn menu_group(ui: &mut Ui, title: &str, contents: impl FnOnce(&mut Ui)) {
    let id = ui.make_persistent_id(("navigation-group", title));
    let mut open = ui.data(|data| data.get_temp::<bool>(id).unwrap_or(true));
    classic::panel(classic::DESK).inner_margin(0.0).show(ui, |ui| {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), Sense::click());
        response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::CollapsingHeader, true, open, title));
        if response.clicked() {
            open = !open;
            ui.data_mut(|data| data.insert_temp(id, open));
        }
        ui.painter().rect_filled(rect, 1.0, if response.hovered() { classic::SELECTED } else { classic::PANEL });
        classic::cell(ui, rect.left() + 9.0, rect.top(), rect.width() - 27.0, rect.height(),
            RichText::new(title), Align::LEFT);
        classic::cell(ui, rect.right() - 24.0, rect.top(), 17.0, rect.height(),
            RichText::new(if open { "⌃" } else { "⌄" }), Align::RIGHT);
        if open {
            egui::Frame::none().inner_margin(6.0).show(ui, contents);
        }
    });
    ui.add_space(4.0);
}

fn input(ui: &mut Ui, label: &str, value: &mut String, limit: usize) {
    ui.label(label);
    ui.add(TextEdit::singleline(value).desired_width(f32::INFINITY).char_limit(limit));
}

fn display_notice(ui: &mut Ui, notice: &Option<(bool, String)>) {
    if let Some((error, message)) = notice {
        if *error { ui.colored_label(ui.visuals().error_fg_color, message); }
        else { ui.label(message); }
    }
}
