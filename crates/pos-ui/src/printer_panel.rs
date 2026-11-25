use pos_core::receipt::{ReceiptLine, ReceiptPayment};

use super::*;

pub(in crate::app) struct PrinterForm {
    transport: String,
    paper: Paper,
    cut: Cut,
    drawer: bool,
    drawer_transport: String,
    content_tab: bool,
    business: BusinessHeader,
    layout: ReceiptLayout,
    sample: Receipt,
    feedback: Option<&'static str>,
}

impl PrinterForm {
    pub(super) fn load(app: &App, content_tab: bool) -> Result<Self, StoreError> {
        let configured = setting(&app.conn, "printer_transport")?.unwrap_or_default();
        let transport = if configured == "file:receipt.bin" { String::new() } else { configured };
        let paper = if setting(&app.conn, "printer_width")?.as_deref() == Some("32") {
            Paper::Mm58
        } else {
            Paper::Mm80
        };
        let drawer = setting(&app.conn, "drawer_enabled")?.as_deref() != Some("0");
        Ok(Self {
            transport, paper, cut: app.settings.print_options.cut, drawer, content_tab,
            drawer_transport: setting(&app.conn, "drawer_transport")?.unwrap_or_default(),
            business: app.settings.business.clone(), layout: app.settings.receipt_layout.clone(),
            sample: sample_receipt(), feedback: None,
        })
    }

    pub(super) fn show(&mut self, app: &mut App, ui: &mut Ui) -> Result<(), Box<dyn Error>> {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.content_tab, false, "Impresora");
            ui.selectable_value(&mut self.content_tab, true, "Contenido del recibo");
        });
        ui.separator();
        if self.content_tab {
            self.receipt_panel(app, ui)?;
            if let Some(feedback) = self.feedback { ui.label(feedback); }
            display_notice(ui, &app.printer_notice);
        } else {
            self.device_panel(app, ui)?;
        }
        Ok(())
    }

    pub(super) fn device_panel(&mut self, app: &mut App, ui: &mut Ui) -> Result<(), Box<dyn Error>> {
        ui.set_max_width(650.0);
        let active = &app.settings;
        ui.label(format!("Destino activo: {}", active.destination.label()));
        ui.label(format!("Papel activo: {} mm", if active.print_options.paper == Paper::Mm58 { 58 } else { 80 }));
        ui.label(if active.drawer { format!("Cajón activo: {}", active.drawer_route().label()) } else { "Cajón desactivado o sin destino RAW".into() });
        if active.printer_override.is_some() || active.paper_override.is_some() {
            ui.weak("Los parámetros --printer y --paper tienen prioridad para los valores indicados al iniciar.");
        }
        ui.weak(if cfg!(windows) {
            "Ej.: windows_spool:Impresora, tcp:192.168.0.10:9100 o file:C:/Tirillas"
        } else {
            "Ej.: cups:Impresora, tcp:192.168.0.10:9100 o file:/ruta/tirillas"
        });
        input(ui, "Destino (vacío = guardar a archivo)", &mut self.transport, 255);
        ui.horizontal_wrapped(|ui| {
            ui.label("Papel");
            ui.selectable_value(&mut self.paper, Paper::Mm58, "58 mm");
            ui.selectable_value(&mut self.paper, Paper::Mm80, "80 mm");
            ui.checkbox(&mut self.drawer, "Abrir cajón al confirmar el pago");
        });
        input(ui, "Destino RAW separado del cajón (opcional; vacío = impresora principal)", &mut self.drawer_transport, 255);
        ui.horizontal_wrapped(|ui| {
            ui.label("Corte de papel");
            ui.selectable_value(&mut self.cut, Cut::None, "Sin corte");
            ui.selectable_value(&mut self.cut, Cut::Partial, "Parcial");
            ui.selectable_value(&mut self.cut, Cut::Full, "Completo");
        });
        ui.weak("El corte y la apertura requieren hardware ESC/POS compatible. Imprimir no abre el cajón.");
        let (mut save, mut test, mut test_drawer) = (false, false, false);
        ui.horizontal_wrapped(|ui| {
            save = ui.add(products::primary("Guardar impresora")).clicked();
            test = ui.add_enabled(!app.printing_receipt, Button::new("Probar destino activo")).clicked();
            test_drawer = ui.add_enabled(app.settings.drawer, Button::new("Probar cajón sin imprimir")).clicked();
        });
        if save {
            let transport = self.transport.trim();
            let destination = Settings::configured_destination(&app.db, transport)?;
            let drawer_destination = Settings::parse_drawer_destination(&self.drawer_transport)?;
            app.save_user_settings("PRINTER_SETTINGS", &[
                ("printer_transport", transport),
                ("printer_width", if self.paper == Paper::Mm58 { "32" } else { "48" }),
                ("printer_cut", match self.cut { Cut::None => "none", Cut::Partial => "partial", Cut::Full => "full" }),
                ("drawer_enabled", flag(self.drawer)),
                ("drawer_transport", &self.drawer_transport),
            ])?;
            let settings = &mut app.settings;
            settings.destination = settings.printer_override.clone().unwrap_or(destination);
            settings.print_options.paper = settings.paper_override.unwrap_or(self.paper);
            settings.print_options.cut = self.cut;
            settings.drawer_destination = drawer_destination;
            settings.drawer = self.drawer && settings.drawer_route().supports_drawer();
            self.feedback = Some("Configuración de impresora guardada.");
            app.printer_notice = None;
        }
        if test {
            app.submit_receipt("prueba".into(), "printer:test".into(), escpos::test_page(app.settings.print_options))?;
        }
        if test_drawer {
            let user = app.user.clone().ok_or("sesión no autenticada")?;
            app.pulse(&user, "printer:drawer-test".into());
        }
        if let Some(feedback) = self.feedback { ui.label(feedback); }
        display_notice(ui, &app.printer_notice);
        Ok(())
    }

    fn receipt_panel(&mut self, app: &mut App, ui: &mut Ui) -> Result<(), Box<dyn Error>> {
        ui.weak("Este formato se usa en Último pago (F8) y en las reimpresiones. El número, la fecha, los artículos y el total siempre se muestran.");
        let (mut save, mut test) = (false, false);
        ui.horizontal_wrapped(|ui| {
            save = ui.add(products::primary("Guardar contenido")).clicked();
            test = ui.add_enabled(!app.printing_receipt, Button::new("Imprimir muestra")).clicked();
        });
        let options = app.settings.print_options;
        ui.columns(2, |cols| {
            let [editor, preview] = cols else { unreachable!() };
            editor.strong("Encabezado y pie");
            editor.columns(2, |cols| {
                input(&mut cols[0], "Nombre del negocio", &mut self.business.name, 92);
                input(&mut cols[1], "NIT", &mut self.business.tax_id, 40);
                input(&mut cols[0], "Dirección", &mut self.business.address, 92);
                input(&mut cols[1], "Teléfono", &mut self.business.phone, 40);
                multiline(&mut cols[0], "Encabezado adicional", &mut self.layout.extra_header, 240);
                multiline(&mut cols[1], "Pie del recibo", &mut self.business.footer, 240);
            });
            editor.separator();
            editor.strong("Campos opcionales");
            editor.checkbox(&mut self.layout.cashier, "Nombre del cajero");
            editor.checkbox(&mut self.layout.unit_price, "Precio unitario sin IVA");
            editor.checkbox(&mut self.layout.product_code, "Código de barras")
                .on_hover_text("Se muestra si el código quedó guardado en el ticket.");
            editor.checkbox(&mut self.layout.notes, "Notas de los artículos");
            editor.checkbox(&mut self.layout.payment_references, "Referencias de pago");
            editor.checkbox(&mut self.layout.cash_detail, "Efectivo recibido y vueltas");
            editor.checkbox(&mut self.layout.show_zero_tax, "Subtotal e impuesto, incluso si el impuesto es $ 0");
            preview.strong("Vista previa del recibo");
            preview.weak("Datos de ejemplo · cambios sin guardar incluidos");
            let text = escpos::preview_sale_receipt(options, &self.business, &self.layout,
                &self.sample, ReceiptKind::Sample);
            classic::panel(Color32::WHITE).show(preview, |ui| {
                ScrollArea::both().id_salt("receipt-preview").max_height(355.0).show(ui, |ui| {
                    ui.add(egui::Label::new(RichText::new(text).monospace().color(Color32::BLACK)).wrap_mode(egui::TextWrapMode::Extend));
                });
            });
        });

        if save {
            app.save_user_settings("RECEIPT_SETTINGS", &[
                ("business_name", &self.business.name), ("business_taxid", &self.business.tax_id),
                ("business_address", &self.business.address), ("business_phone", &self.business.phone),
                ("receipt_extra_header", &self.layout.extra_header), ("receipt_footer", &self.business.footer),
                ("receipt_cashier", flag(self.layout.cashier)), ("receipt_unit_price", flag(self.layout.unit_price)),
                ("receipt_product_code", flag(self.layout.product_code)), ("receipt_notes", flag(self.layout.notes)),
                ("receipt_payment_references", flag(self.layout.payment_references)),
                ("receipt_cash_detail", flag(self.layout.cash_detail)), ("receipt_zero_tax", flag(self.layout.show_zero_tax)),
            ])?;
            for field in [&mut self.business.name, &mut self.business.tax_id, &mut self.business.address,
                &mut self.business.phone, &mut self.business.footer, &mut self.layout.extra_header]
            {
                *field = field.trim().to_owned();
            }
            app.settings.business = self.business.clone();
            app.settings.receipt_layout = self.layout.clone();
            self.feedback = Some("Contenido del recibo guardado.");
        }
        if test {
            let bytes = escpos::sale_receipt(options, &self.business, &self.layout, &self.sample, ReceiptKind::Sample);
            app.submit_receipt("muestra-recibo".into(), "printer:receipt-sample".into(), bytes)?;
        }
        Ok(())
    }
}

fn flag(value: bool) -> &'static str {
    if value { "1" } else { "0" }
}

fn multiline(ui: &mut Ui, label: &str, value: &mut String, limit: usize) {
    ui.label(label);
    ui.add(TextEdit::multiline(value).desired_width(f32::INFINITY).desired_rows(2).char_limit(limit));
}

fn sample_receipt() -> Receipt {
    let pesos = Money::from_pesos;
    Receipt {
        id: String::new(), number: 123, date: "2026-01-01 10:30:00".into(), cashier: "Cajero de ejemplo".into(),
        lines: vec![
            ReceiptLine {
                name: "Café molido".into(), code: Some("1234567890123".into()), unit_price_net: pesos(8_000),
                quantity: Qty::one(), unit: SaleUnit::Unit, amount: pesos(8_000), note: Some("Para llevar".into()),
            },
            ReceiptLine {
                name: "Pan".into(), code: Some("1234567890124".into()), unit_price_net: pesos(2_000),
                quantity: Qty::from_units(2), unit: SaleUnit::Unit, amount: pesos(4_000), note: None,
            },
        ],
        payments: vec![
            ReceiptPayment { method: PaymentMethod::Nequi, amount: pesos(7_000), tendered: None, reference: Some("000123".into()) },
            ReceiptPayment { method: PaymentMethod::Cash, amount: pesos(5_000), tendered: Some(pesos(10_000)), reference: None },
        ],
        net: pesos(12_000), tax: Money::ZERO, total: pesos(12_000), collected: pesos(12_000), change: pesos(5_000),
    }
}
