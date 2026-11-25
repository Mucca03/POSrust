use super::*;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Tab {
    #[default]
    General,
    Peripherals,
    Database,
}

pub(in crate::app) struct ConfigurationForm {
    tab: Tab,
    logo: String,
    text: String,
    text_file: String,
    logo_preview: Option<egui::TextureHandle>,
    customer_display: bool,
    printer: printer_panel::PrinterForm,
    scanner: String,
    scanner_result: Option<String>,
    candidate_db: String,
    backup_dir: String,
    backup_retention: String,
    check: Option<Receiver<Result<String, String>>>,
    check_result: Option<(bool, String)>,
    feedback: Option<&'static str>,
}

impl ConfigurationForm {
    pub(super) fn load(app: &App) -> Result<Self, StoreError> {
        Ok(Self {
            tab: Tab::default(),
            logo: app.settings.brand_logo.clone(), text: app.settings.brand_text.clone(),
            text_file: String::new(), logo_preview: app.brand_logo.clone(),
            customer_display: app.settings.customer_display,
            printer: printer_panel::PrinterForm::load(app, false)?,
            scanner: String::new(), scanner_result: None,
            candidate_db: String::new(), check: None, check_result: None,
            backup_dir: setting(&app.conn, "backup_dir")?.unwrap_or_default(),
            backup_retention: pos_store::retention_weeks(&app.conn)?.to_string(),
            feedback: None,
        })
    }

    pub(super) fn show(&mut self, app: &mut App, ui: &mut Ui) -> Result<bool, Box<dyn Error>> {
        let previous = self.tab;
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tab, Tab::General, "General");
            ui.selectable_value(&mut self.tab, Tab::Peripherals, "Peripherals");
            ui.selectable_value(&mut self.tab, Tab::Database, "Database Setup");
        });
        if previous != self.tab {
            self.feedback = None;
            app.dialog_error = None;
        }
        ui.separator();
        let receipt = match self.tab {
            Tab::General => { self.general(app, ui)?; false }
            Tab::Peripherals => self.peripherals(app, ui)?,
            Tab::Database => { self.database(app, ui)?; false }
        };
        if let Some(message) = self.feedback { ui.label(message); }
        Ok(receipt)
    }

    fn general(&mut self, app: &mut App, ui: &mut Ui) -> Result<(), Box<dyn Error>> {
        ui.weak("Logo y texto se muestran en la pantalla de acceso. No cambian lo que se imprime.");
        input(ui, "Logo (ruta a un PNG local)", &mut self.logo, 512);
        if ui.button("Previsualizar logo").clicked() {
            self.logo_preview = branding::load_logo(ui.ctx(), &app.db, &self.logo)?;
            app.dialog_error = None;
        }
        if let Some(logo) = &self.logo_preview {
            ui.add(egui::Image::new(logo).max_size(egui::vec2(230.0, 110.0)));
        } else {
            ui.weak("Sin logo, se usa la marca POSrust.");
        }
        ui.label("Text (mensaje de bienvenida)");
        ui.add(TextEdit::multiline(&mut self.text).char_limit(240).desired_width(480.0).desired_rows(3));
        input(ui, "Archivo de texto UTF-8 (opcional)", &mut self.text_file, 512);
        if ui.button("Cargar texto").clicked() {
            let text = branding::load_text(&app.db, &self.text_file)?;
            if text.chars().count() > 240 { return Err("el texto debe tener como máximo 240 caracteres".into()); }
            self.text = text;
            app.dialog_error = None;
        }
        ui.horizontal(|ui| -> Result<(), Box<dyn Error>> {
            if ui.add(products::primary("Guardar General")).clicked() {
                let logo = branding::load_logo(ui.ctx(), &app.db, &self.logo)?;
                let (path, text) = (self.logo.trim(), self.text.trim());
                app.save_user_settings("GENERAL_SETTINGS", &[("brand_logo", path), ("brand_text", text)])?;
                app.settings.brand_logo = path.to_owned();
                app.settings.brand_text = text.to_owned();
                self.logo_preview = logo.clone();
                app.brand_logo = logo;
                app.dialog_error = None;
                self.feedback = Some("Logo y texto guardados.");
            }
            if ui.button("Restaurar General").clicked() {
                self.logo.clone_from(&app.settings.brand_logo);
                self.text.clone_from(&app.settings.brand_text);
                self.logo_preview = app.brand_logo.clone();
                self.text_file.clear();
                app.dialog_error = None;
                self.feedback = Some("Se recuperaron los últimos cambios guardados.");
            }
            Ok(())
        }).inner
    }

    fn peripherals(&mut self, app: &mut App, ui: &mut Ui) -> Result<bool, Box<dyn Error>> {
        ui.horizontal(|ui| {
            ui.label("Customer Display");
            egui::ComboBox::from_id_salt("customer-display-choice")
                .selected_text(if self.customer_display { "Pantalla (segunda ventana)" } else { "No definido" })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.customer_display, false, "No definido");
                    ui.selectable_value(&mut self.customer_display, true, "Pantalla (segunda ventana)");
                });
        });
        if self.customer_display {
            ui.weak("Muestra el último artículo, el total y las vueltas. Puedes mover la ventana al segundo monitor.");
        }
        ui.horizontal(|ui| -> Result<(), Box<dyn Error>> {
            if ui.add(products::primary("Guardar pantalla")).clicked() {
                app.save_user_settings("DISPLAY_SETTINGS", &[("customer_display", if self.customer_display { "screen" } else { "none" })])?;
                app.settings.customer_display = self.customer_display;
                app.customer_display_closed = false;
                self.feedback = Some("Pantalla de cliente guardada.");
                app.dialog_error = None;
            }
            if ui.button("Restaurar periféricos").clicked() {
                self.customer_display = app.settings.customer_display;
                self.printer = printer_panel::PrinterForm::load(app, false)?;
                self.scanner.clear();
                self.scanner_result = None;
                app.dialog_error = None;
                self.feedback = None;
            }
            Ok(())
        }).inner?;
        ui.separator();
        ui.strong("Printer 1 · Recibos y cajón");
        let receipt = ui.button("Configurar contenido del recibo").clicked();
        self.printer.device_panel(app, ui)?;
        ui.separator();
        ui.horizontal_wrapped(|ui| { ui.strong("Scale"); ui.label("Peso manual en kg · balanza serie no configurada"); });
        ui.horizontal_wrapped(|ui| { ui.strong("Scanner"); ui.label("USB HID (modo teclado, con sufijo Enter)"); });
        ui.weak("Prueba el lector aquí. Esta lectura no agrega productos ni registra una venta.");
        let mut test = false;
        ui.horizontal_wrapped(|ui| {
            let response = ui.add(TextEdit::singleline(&mut self.scanner).desired_width(250.0).char_limit(128)
                .hint_text("Escanea un código aquí"));
            test = (response.has_focus() || response.lost_focus()) && ui.input(|i| i.key_pressed(Key::Enter));
            test |= ui.button("Comprobar código").clicked();
        });
        if test && !self.scanner.trim().is_empty() {
            let code = std::mem::take(&mut self.scanner);
            self.scanner_result = Some(match app.catalog.product_by_code(&code) {
                Some(product) => format!("{} · {}", code.trim(), product.name.trim()),
                None => format!("Lectura recibida: {} · sin coincidencia en catálogo", code.trim()),
            });
        }
        if let Some(result) = &self.scanner_result { ui.label(result); }
        ui.weak("Impresoras 2–6, impresora de informes, protocolos serie y iButton no están habilitados en esta versión.");
        Ok(receipt)
    }

    fn database(&mut self, app: &mut App, ui: &mut Ui) -> Result<(), Box<dyn Error>> {
        ui.horizontal(|ui| { ui.label("Database"); ui.strong("SQLite · un solo terminal"); });
        ui.label(format!("Base activa: {}", app.db.display()));
        ui.weak("La verificación no modifica los datos. Para abrir otra copia verificada, cierra POSrust e inicia con --db seguido de su ruta.");
        let mut verify = None;
        if ui.add_enabled(self.check.is_none(), Button::new("Verificar base activa")).clicked() {
            verify = Some(app.db.clone());
        }
        input(ui, "Ruta a otra copia SQLite (opcional)", &mut self.candidate_db, 512);
        if ui.add_enabled(self.check.is_none() && !self.candidate_db.trim().is_empty(), Button::new("Verificar copia")).clicked() {
            verify = Some(beside_database(&app.db, self.candidate_db.trim()));
        }
        if let Some(path) = verify {
            let (send, receive) = mpsc::channel();
            let ctx = ui.ctx().clone();
            std::thread::Builder::new().name("pos-db-check".into()).spawn(move || {
                let result = verify_file(&path).map_err(|error| format!("{}: {error}", path.display()));
                let _ = send.send(result);
                ctx.request_repaint();
            })?;
            self.check = Some(receive);
            self.check_result = None;
            app.dialog_error = None;
        }
        if let Some(receive) = &self.check {
            match receive.try_recv() {
                Ok(Ok(message)) => self.check_result = Some((false, message)),
                Ok(Err(error)) => self.check_result = Some((true, error)),
                Err(mpsc::TryRecvError::Disconnected) => self.check_result = Some((true, "Verificación interrumpida.".into())),
                Err(mpsc::TryRecvError::Empty) => { ui.spinner(); }
            }
            if self.check_result.is_some() { self.check = None; }
        }
        display_notice(ui, &self.check_result);
        ui.separator();
        ui.strong("Respaldos semanales");
        input(ui, "Carpeta de respaldos (vacía = backups junto a la base)", &mut self.backup_dir, 512);
        input(ui, "Semanas de retención (1–520)", &mut self.backup_retention, 4);
        if ui.add_enabled(app.backup_done.is_none(), products::primary("Guardar respaldos")).clicked() {
            let weeks = self.backup_retention.trim().parse::<u32>().ok()
                .filter(|weeks| (1..=520).contains(weeks)).ok_or("la retención debe estar entre 1 y 520 semanas")?;
            let value = self.backup_dir.trim();
            let dir = beside_database(&app.db, if value.is_empty() { "backups" } else { value });
            if dir.try_exists()? && !dir.is_dir() { return Err("la ruta de respaldos debe ser una carpeta".into()); }
            app.save_user_settings("BACKUP_SETTINGS", &[("backup_dir", value), ("backup_retention_weeks", &weeks.to_string())])?;
            app.settings.backups = dir;
            app.dialog_error = None;
            self.feedback = Some("Configuración de respaldos guardada.");
        }
        if ui.add_enabled(app.backup_done.is_none(), Button::new("Crear respaldo ahora")).clicked() {
            app.run_backup_worker(ui.ctx(), true);
        }
        display_notice(ui, &app.notice);
        Ok(())
    }
}

fn verify_file(path: &Path) -> Result<String, Box<dyn Error>> {
    if !path.is_file() { return Err("no se encontró un archivo SQLite en esa ruta".into()); }
    let conn = open_prepared_database(path)?;
    if !pos_store::integrity_check(&conn)? { return Err("SQLite reportó un problema de integridad".into()); }
    Ok(format!("Base POSrust verificada: {}", path.display()))
}
