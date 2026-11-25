use eframe::egui::{Button, ScrollArea, TextEdit, Ui};
use pos_store::csv_tools::{self, Batch, BatchReport, ImportKind, LogKind, LogRow};

use super::*;
use crate::classic::{self, Icon};

#[derive(Default)]
pub(super) struct ToolTiles {
    panel: Option<Tool>,
    path: String,
    scan: String,
    scans: Vec<String>,
    preview: Option<(Batch, BatchReport)>,
    result: Option<BatchReport>,
    log: Option<Vec<LogRow>>,
    focus: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool { File(ImportKind), Scanner, Archive, Transfer, Report(LogKind) }

const IMPORTS: &[(Tool, &str, Icon)] = &[
    (Tool::File(ImportKind::Products), "Products CSV File", Icon::Stock),
    (Tool::Scanner, "Products Scanner Import", Icon::Search),
    (Tool::File(ImportKind::Customers), "Customers CSV File", Icon::Note),
    (Tool::Archive, "Clear Import Log", Icon::Delete),
    (Tool::Transfer, "Transfer Database", Icon::Tools),
];
const REPORTS: &[(Tool, &str, Icon)] = &[
    (Tool::Report(LogKind::Added), "Products Added", Icon::Stock),
    (Tool::Report(LogKind::Price), "Product Price Changes", Icon::Edit),
    (Tool::Report(LogKind::Stock), "Stock Qty Update", Icon::Drawer),
    (Tool::Report(LogKind::Category), "Invalid Category", Icon::Search),
    (Tool::Report(LogKind::Missing), "Missing Data / Bad Prices", Icon::Note),
    (Tool::Report(LogKind::Duplicate), "Invalid / Duplicates", Icon::Delete),
    (Tool::Report(LogKind::Customer), "Customers Added", Icon::Note),
];

impl ToolTiles {
    pub(super) fn show(&mut self, app: &mut App, ui: &mut Ui) -> Result<bool, Box<dyn Error>> {
        if let Some(tool) = self.panel {
            if ui.button("← Volver a Tools").clicked() {
                *self = Self::default();
                app.dialog_error = None;
                return Ok(true);
            }
            ui.separator();
            let title = IMPORTS.iter().chain(REPORTS).find(|(item, _, _)| *item == tool)
                .map_or("Tools", |(_, title, _)| *title);
            ui.heading(title);
            match tool {
                Tool::File(kind) => self.file_panel(app, ui, kind)?,
                Tool::Scanner => self.scanner_panel(app, ui)?,
                Tool::Archive => {
                    ui.label("Oculta los resultados de los reportes de importación. No borra registros ni movimientos.");
                    if ui.button("Confirmar archivo del registro").clicked() {
                        let user = app.user.as_ref().ok_or("sesión no autenticada")?;
                        let now = app.timestamp()?;
                        let n = csv_tools::archive_log(&mut app.conn, &user.id, &now)?;
                        app.notice = Some((false, format!("{n} resultados archivados. Se conserva la auditoría.")));
                        *self = Self::default();
                        app.dialog_error = None;
                    }
                }
                Tool::Transfer => {
                    ui.label("Crea una copia SQLite verificada para llevar a otro equipo. No reemplaza ninguna base de datos.");
                    ui.label("Carpeta de destino (por ejemplo, tu memoria USB)");
                    ui.add(TextEdit::singleline(&mut self.path).desired_width(490.0).char_limit(1024));
                    let ready = app.backup_done.is_none() && !self.path.trim().is_empty();
                    if ui.add_enabled(ready, products::primary("Crear copia para transferir")).clicked() {
                        let dir = PathBuf::from(self.path.trim());
                        if !dir.is_dir() { return Err("la carpeta de destino debe existir y estar montada".into()); }
                        app.notice = Some((false, "Creando y verificando la copia…".into()));
                        app.run_backup_to(ui.ctx(), true, dir);
                    }
                    if let Some((error, msg)) = &app.notice { notice(ui, *error, msg); }
                }
                Tool::Report(kind) => {
                    let user = app.user.as_ref().ok_or("sesión no autenticada")?;
                    if self.log.is_none() || ui.button("Actualizar reporte").clicked() {
                        self.log = Some(csv_tools::log(&app.conn, &user.id, kind)?);
                    }
                    if let Some(rows) = &self.log {
                        ui.weak("Últimos 500 registros no archivados");
                        show_log(ui, rows);
                    }
                }
            }
            return Ok(true);
        }
        ui.add_space(12.0);
        let backoffice = app.can(Capability::Backoffice);
        let settings = app.can(Capability::Settings);
        let reports = app.can(Capability::Reports);
        for (label, actions) in [("CSV Import", IMPORTS), ("CSV Import Reports", REPORTS)] {
            ui.strong(label);
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                let w = ((ui.available_width() / 5.0) - 8.0).clamp(140.0, 210.0);
                for &(action, name, icon) in actions {
                    let enabled = match action {
                        Tool::Transfer => settings,
                        Tool::Report(_) => reports,
                        _ => backoffice,
                    };
                    if classic::icon_button(ui, icon, egui::vec2(w, 76.0), enabled, name, Some(name)).clicked() {
                        *self = Self { panel: Some(action), focus: true, ..Self::default() };
                        app.dialog_error = None;
                    }
                }
            });
            ui.add_space(15.0);
        }
        Ok(false)
    }

    fn file_panel(&mut self, app: &mut App, ui: &mut Ui, kind: ImportKind) -> Result<(), Box<dyn Error>> {
        ui.label("Ruta del archivo CSV (UTF-8; máximo 1 MB y 1.000 filas)");
        let field = ui.add(TextEdit::singleline(&mut self.path).desired_width(490.0).char_limit(1024));
        if std::mem::take(&mut self.focus) { field.request_focus(); }
        if field.changed() {
            self.preview = None;
            self.result = None;
            app.dialog_error = None;
        }
        if kind == ImportKind::Products {
            ui.monospace("accion;codigo;referencia;nombre;categoria;precio;costo;unidad;cantidad");
            ui.label("Acciones: alta, precio y entrada. Para nuevas existencias, añade una fila «entrada» separada.");
            ui.weak("El precio es el del catálogo (IVA incluido en la configuración actual si así está definido).");
        } else {
            ui.monospace("codigo;nombre;nit;telefono;correo");
            ui.weak("Código y nombre obligatorios. No modifica clientes existentes.");
        }
        if ui.add_enabled(!self.path.trim().is_empty(), Button::new("Previsualizar CSV")).clicked() {
            self.preview = None;
            self.result = None;
            let batch = Batch::from_file(Path::new(self.path.trim()), kind)?;
            self.prepare(app, batch)?;
        }
        self.show_batch(app, ui, kind != ImportKind::Customers)
    }

    fn scanner_panel(&mut self, app: &mut App, ui: &mut Ui) -> Result<(), Box<dyn Error>> {
        ui.label("Registra una entrada de mercancía con lector USB en modo teclado (HID). Cada lectura suma una unidad.");
        ui.weak("Para productos por peso, usa un CSV con la cantidad en kg.");
        ui.horizontal(|ui| {
            let field = ui.add(TextEdit::singleline(&mut self.scan).hint_text("Escanea y pulsa Enter")
                .desired_width(260.0).char_limit(128));
            if std::mem::take(&mut self.focus) { field.request_focus(); }
            let enter = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            if ui.button("Añadir lectura").clicked() || enter {
                if !self.scan.trim().is_empty() && self.scans.len() < csv_tools::MAX_ROWS {
                    self.scans.push(std::mem::take(&mut self.scan));
                    self.preview = None;
                    self.result = None;
                }
                field.request_focus();
            }
            if ui.button("Quitar última").clicked() {
                self.scans.pop();
                self.preview = None;
            }
        });
        ui.label(format!("{} unidades escaneadas", self.scans.len()));
        if let Some(last) = self.scans.last() { ui.label(format!("Último código: {last}")); }
        if ui.add_enabled(!self.scans.is_empty(), Button::new("Previsualizar entrada")).clicked() {
            self.preview = None;
            let batch = Batch::from_scans(&self.scans)?;
            self.prepare(app, batch)?;
        }
        self.show_batch(app, ui, true)?;
        if self.result.is_some() { self.scans.clear(); }
        Ok(())
    }

    fn prepare(&mut self, app: &mut App, batch: Batch) -> Result<(), Box<dyn Error>> {
        let now = app.timestamp()?;
        let user = app.user.as_ref().ok_or("sesión no autenticada")?;
        let report = csv_tools::process(&mut app.conn, &batch, &user.id, &app.catalog.location.id, &now, false)?;
        self.preview = Some((batch, report));
        self.result = None;
        app.dialog_error = None;
        Ok(())
    }

    fn show_batch(&mut self, app: &mut App, ui: &mut Ui, touches_products: bool) -> Result<(), Box<dyn Error>> {
        if let Some((_, report)) = &self.preview {
            ui.strong(format!("Vista previa: {} de {} filas aplicables. No se guardó nada.", report.accepted, report.rows.len()));
            show_log(ui, &report.rows);
            let blocked = touches_products && app.has_pending_sales();
            if blocked { ui.weak("Termina las ventas abiertas antes de aplicar cambios al catálogo o inventario."); }
            if ui.add_enabled(!blocked, products::primary("Aplicar filas válidas y registrar el resultado")).clicked() {
                let now = app.timestamp()?;
                let user = app.user.as_ref().ok_or("sesión no autenticada")?;
                let batch = &self.preview.as_ref().expect("vista previa visible").0;
                let report = csv_tools::process(&mut app.conn, batch, &user.id, &app.catalog.location.id, &now, true)?;
                self.preview = None;
                self.result = Some(report);
                app.dialog_error = None;
                if touches_products { app.reload_catalog()?; }
            }
        } else if let Some(report) = &self.result {
            ui.strong(format!("Finalizado: {} de {} filas aplicadas.", report.accepted, report.rows.len()));
            show_log(ui, &report.rows);
        }
        Ok(())
    }
}

fn show_log(ui: &mut Ui, rows: &[LogRow]) {
    if rows.is_empty() { ui.weak("No hay registros."); return; }
    ScrollArea::vertical().id_salt("csv-log-results").max_height(285.0).show(ui, |ui| {
        for row in rows {
            ui.label(format!("{} · #{} · {} · {}: {}", row.date, row.line, row.code, kind_label(&row.kind), row.detail));
        }
    });
}

fn kind_label(kind: &str) -> &str {
    match kind {
        "product_added" => "Alta", "price_change" => "Precio", "stock_entry" => "Inventario",
        "invalid_category" => "Categoría inválida", "missing_bad_values" => "Datos inválidos",
        "invalid_duplicate" => "Código no encontrado o duplicado", "customer_added" => "Cliente nuevo",
        "unchanged" => "Sin cambios", _ => kind,
    }
}

fn notice(ui: &mut Ui, error: bool, text: &str) {
    if error { ui.colored_label(ui.visuals().error_fg_color, text); }
    else { ui.label(text); }
}
