use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, Context, Key, RichText};
use pos_core::receipt::Receipt;
use pos_core::{
    Capability, CashRounding, Discount, LineKey, Money, Payments, PriceMode, ProductId, Qty, Rate,
    SaleUnit, SessionClose, SessionTotals, TicketDraft, User,
};
use pos_hardware::escpos::{self, BusinessHeader, CloseHeader, Cut, Paper, PrinterOptions, ReceiptKind, ReceiptLayout};
use pos_store::backup::setting;
use pos_store::{CashDirection, Catalog, SaleContext, SessionInfo, StoreError};
use rusqlite::Connection;

use crate::checkout::{money_field, CheckoutForm};
use crate::printing::{Destination, Job, Outcome, Printer};

#[path = "screens.rs"]
mod screens;
#[path = "menu.rs"]
mod menu;
#[path = "products.rs"]
mod products;
#[path = "tools.rs"]
mod tools;
#[path = "branding.rs"]
mod branding;
#[path = "customer_display.rs"]
mod customer_display;

#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;

pub struct LaunchOptions {
    pub db: PathBuf,
    pub printer: Option<String>,
    pub paper_mm: Option<u8>,
}

pub fn run(options: LaunchOptions) -> Result<(), Box<dyn Error>> {
    // Validar el archivo antes de habilitar escritura o aplicar migraciones.
    drop(open_prepared_database(&options.db)?);
    let conn = pos_store::open(&options.db)?;
    let settings = Settings::load(&conn, &options)?;
    let mode = if setting(&conn, "prices_include_tax")?.as_deref() == Some("0") {
        PriceMode::TaxExcluded
    } else {
        PriceMode::TaxIncluded
    };
    let catalog = pos_store::load_catalog(&conn, mode)?;
    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 730.0])
            .with_min_inner_size([820.0, 600.0]),
        renderer: eframe::Renderer::Glow,
        vsync: true,
        multisampling: 0,
        ..Default::default()
    };
    eframe::run_native("POSrust · Ventas", native, Box::new(move |cc| {
        cc.egui_ctx.set_style(crate::classic::style());
        let printer = Printer::start(cc.egui_ctx.clone())?;
        let mut app = App::new(conn, catalog, options.db, settings, printer);
        app.brand_logo = branding::load_logo(&cc.egui_ctx, &app.db, &app.settings.brand_logo).ok().flatten();
        Ok(Box::new(app))
    }))?;
    Ok(())
}

fn open_prepared_database(path: &Path) -> Result<Connection, Box<dyn Error>> {
    let read = pos_store::open_readonly(path)?;
    let version: String = read.query_row(
        "SELECT VALUE FROM pos_meta WHERE KEY = 'schema_version'", [], |r| r.get(0),
    ).map_err(|e| format!("base no preparada; use pos-import ({e})"))?;
    if version != "1" {
        return Err(format!("versión de base no admitida: {version}").into());
    }
    for table in pos_store::TABLES.iter().map(|table| table.name).chain(["pos_settings", "pos_audit"]) {
        let exists: bool = read.query_row(
            "SELECT EXISTS (SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?1)", [table],
            |row| row.get(0),
        )?;
        if !exists { return Err(format!("base POSrust incompleta: falta la tabla {table}").into()); }
    }
    Ok(read)
}

struct Settings {
    offset: i32,
    rounding: CashRounding,
    business: BusinessHeader,
    receipt_layout: ReceiptLayout,
    print_options: PrinterOptions,
    destination: Destination,
    printer_override: Option<Destination>,
    paper_override: Option<Paper>,
    drawer: bool,
    drawer_destination: Option<Destination>,
    brand_logo: String,
    brand_text: String,
    customer_display: bool,
    backups: PathBuf,
}

fn beside_database(db: &Path, value: &str) -> PathBuf {
    db.parent().unwrap_or_else(|| Path::new(".")).join(value)
}

impl Settings {
    fn configured_destination(db: &Path, configured: &str) -> Result<Destination, String> {
        if configured.is_empty() || configured == "file:receipt.bin" {
            Ok(Destination::Files(beside_database(db, "tirillas")))
        } else {
            Destination::parse(configured)
        }
    }

    fn load(conn: &Connection, options: &LaunchOptions) -> Result<Self, Box<dyn Error>> {
        use rusqlite::OptionalExtension;
        let get = |key| -> Result<String, StoreError> { Ok(setting(conn, key)?.unwrap_or_default()) };
        let offset: Option<String> = conn.query_row(
            "SELECT VALUE FROM pos_meta WHERE KEY = 'timezone_offset_minutes'", [], |r| r.get(0),
        ).optional()?;
        let configured = get("timezone_offset_minutes")?;
        let offset = if configured.is_empty() { offset.unwrap_or_else(|| "-300".into()) } else { configured };
        let offset = offset.parse()?;
        pos_store::now_local(offset)?;
        let rounding = CashRounding::from_cop(get("cash_rounding")?.parse().unwrap_or(0))?;
        let business = BusinessHeader {
            name: get("business_name")?, tax_id: get("business_taxid")?,
            address: get("business_address")?, phone: get("business_phone")?,
            footer: get("receipt_footer")?,
        };
        let receipt_layout = ReceiptLayout {
            extra_header: get("receipt_extra_header")?,
            cashier: get("receipt_cashier")? != "0",
            unit_price: get("receipt_unit_price")? == "1",
            product_code: get("receipt_product_code")? == "1",
            notes: get("receipt_notes")? != "0",
            payment_references: get("receipt_payment_references")? != "0",
            cash_detail: get("receipt_cash_detail")? != "0",
            show_zero_tax: get("receipt_zero_tax")? == "1",
        };
        let paper_override = match options.paper_mm {
            Some(58) => Some(Paper::Mm58),
            Some(80) => Some(Paper::Mm80),
            Some(_) => return Err("el papel debe ser de 58 u 80 mm".into()),
            None => None,
        };
        let stored_paper = if get("printer_width")? == "32" { Paper::Mm58 } else { Paper::Mm80 };
        let cut = match get("printer_cut")?.as_str() {
            "full" => Cut::Full,
            "partial" => Cut::Partial,
            _ => Cut::None,
        };
        let print_options = PrinterOptions {
            paper: paper_override.unwrap_or(stored_paper), cut, ..PrinterOptions::default()
        };
        let printer_override = options.printer.as_deref().map(Destination::parse).transpose()?;
        let destination = match &printer_override {
            Some(value) => value.clone(),
            None => Self::configured_destination(&options.db, &get("printer_transport")?)?,
        };
        let drawer_destination = Self::parse_drawer_destination(&get("drawer_transport")?)?;
        let drawer = drawer_destination.as_ref().unwrap_or(&destination).supports_drawer() && get("drawer_enabled")? != "0";
        let backups = get("backup_dir")?;
        let backups = beside_database(&options.db, if backups.is_empty() { "backups" } else { &backups });
        Ok(Self {
            offset, rounding, business, receipt_layout, print_options, destination,
            printer_override, paper_override, drawer, drawer_destination, backups,
            brand_logo: get("brand_logo")?, brand_text: get("brand_text")?,
            customer_display: get("customer_display")? == "screen",
        })
    }

    fn parse_drawer_destination(value: &str) -> Result<Option<Destination>, String> {
        if value.trim().is_empty() { return Ok(None); }
        let destination = Destination::parse(value.trim())?;
        if !destination.supports_drawer() { return Err("el cajón requiere un destino RAW, no un archivo".into()); }
        Ok(Some(destination))
    }

    fn drawer_route(&self) -> &Destination {
        self.drawer_destination.as_ref().unwrap_or(&self.destination)
    }
}

enum Dialog {
    Opening(String),
    Edit(EditKind, String),
    Remove(LineKey),
    CancelTicket,
    Change { ticket: i64, total: Money, amount: Money },
    OpenSales,
    ProductSearch(Box<products::ProductSearch>),
    Checkout(Box<CheckoutForm>),
    Cash(CashDirection, String, String),
    Close(Box<CloseForm>),
    Lookup(String, Option<Box<Receipt>>),
    Menu(Box<menu::MenuDialog>),
}

impl Dialog {
    fn editing_product(&self) -> bool {
        matches!(self, Self::Menu(menu) if matches!(&**menu,
            menu::MenuDialog::Products(form) if form.is_editing()))
    }
}

#[derive(Clone)]
enum EditKind {
    Weight(ProductId),
    Line { key: LineKey, price: String, price_active: bool },
    Note(LineKey),
    Discount(Option<LineKey>, bool),
}

struct CloseForm {
    totals: SessionTotals,
    taxes: Vec<pos_store::sessions::SessionTax>,
    cash: pos_core::CashCount,
    counted: String,
    confirmed: bool,
}

struct Closed {
    session: SessionInfo,
    timestamp: String,
    cashier: String,
    close: SessionClose,
    taxes: Vec<pos_store::sessions::SessionTax>,
}

type BackupOutcome = Result<Option<PathBuf>, String>;
const DRAWER_WAIT: Duration = Duration::from_millis(1000);

struct PendingDrawer {
    key: String,
    until: Instant,
}

struct ParkedSale {
    number: u64,
    draft: TicketDraft,
    selected_line: Option<LineKey>,
    opened_at: String,
}

struct App {
    conn: Connection,
    db: PathBuf,
    catalog: Catalog,
    settings: Settings,
    brand_logo: Option<egui::TextureHandle>,
    customer_display_closed: bool,
    user: Option<User>,
    session: Option<SessionInfo>,
    draft: TicketDraft,
    selected_line: Option<LineKey>,
    sale_number: u64,
    next_sale_number: u64,
    sale_opened_at: String,
    parked_sales: Vec<ParkedSale>,
    category: Option<String>,
    search: String,
    visible_products: Vec<usize>,
    page: usize,
    focus_search: bool,
    scanner_id: Option<egui::Id>,
    focus_dialog: bool,
    menu_open: bool,
    dialog: Option<Dialog>,
    dialog_error: Option<String>,
    notice: Option<(bool, String)>,
    printer_notice: Option<(bool, String)>,
    printer: Printer,
    sale_drawer: Option<PendingDrawer>,
    printing_receipt: bool,
    last_closed: Option<Closed>,
    next_backup_check: Instant,
    backup_done: Option<(bool, Receiver<BackupOutcome>)>,
}

impl App {
    fn new(conn: Connection, catalog: Catalog, db: PathBuf, settings: Settings, printer: Printer) -> Self {
        let mut draft = TicketDraft::new();
        draft.set_cash_rounding(settings.rounding);
        let mut app = Self {
            conn, db, catalog, settings, printer, draft, brand_logo: None, customer_display_closed: false,
            user: None,
            session: None, selected_line: None, category: None, search: String::new(),
            sale_number: 1, next_sale_number: 2, sale_opened_at: String::new(),
            parked_sales: Vec::new(),
            visible_products: Vec::new(), page: 0, focus_search: false, scanner_id: None, focus_dialog: false,
            menu_open: false,
            dialog: None, dialog_error: None, notice: None, printer_notice: None,
            printing_receipt: false, last_closed: None, sale_drawer: None,
            next_backup_check: Instant::now(), backup_done: None,
        };
        app.refresh_products();
        app
    }

    fn refresh_products(&mut self) {
        self.page = 0;
        self.visible_products = self.catalog.visible_indices(&self.search, self.category.as_deref());
    }

    fn timestamp(&self) -> Result<String, StoreError> {
        pos_store::now_local(self.settings.offset)
    }

    fn session_user(&self) -> Option<(String, User)> {
        Some((self.session.as_ref()?.money.clone(), self.user.clone()?))
    }

    fn can(&self, capability: Capability) -> bool {
        self.user.as_ref().is_some_and(|user| user.role.can(capability))
    }

    fn sale_ready(&self) -> bool {
        self.session.is_some() && self.can(Capability::Sell)
    }

    fn has_pending_sales(&self) -> bool {
        !self.draft.is_empty() || !self.parked_sales.is_empty()
    }

    fn product_in_pending_sale(&self, id: &ProductId) -> bool {
        std::iter::once(&self.draft).chain(self.parked_sales.iter().map(|sale| &sale.draft))
            .flat_map(|draft| draft.lines()).any(|line| &line.product().product_id == id)
    }

    fn switch_sale(&mut self, sale: ParkedSale) {
        let previous = ParkedSale {
            number: std::mem::replace(&mut self.sale_number, sale.number),
            draft: std::mem::replace(&mut self.draft, sale.draft),
            selected_line: std::mem::replace(&mut self.selected_line, sale.selected_line),
            opened_at: std::mem::replace(&mut self.sale_opened_at, sale.opened_at),
        };
        if !previous.draft.is_empty() {
            self.parked_sales.push(previous);
            self.parked_sales.sort_by_key(|sale| sale.number);
        }
        self.search.clear();
        self.refresh_products();
        self.notice = None;
        self.hide_dialog();
    }

    fn new_sale(&mut self) {
        if !self.sale_ready() || self.draft.is_empty() {
            return;
        }
        let mut draft = TicketDraft::new();
        draft.set_cash_rounding(self.settings.rounding);
        let number = self.next_sale_number;
        self.next_sale_number += 1;
        self.switch_sale(ParkedSale { number, draft, selected_line: None, opened_at: String::new() });
    }

    fn resume_sale(&mut self, number: u64) {
        if !self.sale_ready() { return; }
        if let Some(index) = self.parked_sales.iter().position(|sale| sale.number == number) {
            let sale = self.parked_sales.remove(index);
            self.switch_sale(sale);
        }
    }

    fn show_dialog(&mut self, dialog: Dialog) {
        self.dialog = Some(dialog);
        self.dialog_error = None;
        self.focus_dialog = true;
    }

    fn hide_dialog(&mut self) {
        self.dialog = None;
        self.dialog_error = None;
        self.focus_search = self.sale_ready();
    }

    fn login(&mut self, id: &str) -> Result<(), Box<dyn Error>> {
        let user = self.catalog.user(id).ok_or("usuario no disponible")?.clone();
        pos_store::auth::require_capability(&self.conn, id, Capability::Sell)?;
        self.session = pos_store::current_session(&self.conn)?.filter(|s| s.dateend.is_none());
        self.user = Some(user);
        self.notice = None;
        if self.session.is_none() {
            self.show_dialog(Dialog::Opening("0".into()));
        } else {
            self.focus_search = true;
        }
        Ok(())
    }

    fn open_session(&mut self, amount: Money) -> Result<(), StoreError> {
        let now = self.timestamp()?;
        let user = self.user.as_ref().expect("cajero autenticado");
        let session = pos_store::ensure_session(&mut self.conn, user.id.as_str(), &now, amount)?;
        self.notice = Some((false, format!("Caja {} abierta", session.sequence)));
        self.session = Some(session);
        self.hide_dialog();
        Ok(())
    }

    fn add_product(&mut self, id: ProductId) {
        if !self.sale_ready() {
            return;
        }
        let Some(product) = self.catalog.product_by_id(id.as_str()) else { return };
        if product.unit == SaleUnit::Kilogram {
            self.show_dialog(Dialog::Edit(EditKind::Weight(id), String::new()));
        } else {
            let outcome = self.draft.add_product(product, Qty::one(), self.catalog.price_mode);
            match outcome {
                Ok(key) => self.added(key),
                Err(error) => self.notice = Some((true, error.to_string())),
            }
        }
    }

    fn added(&mut self, key: LineKey) {
        if self.sale_opened_at.is_empty() {
            self.sale_opened_at = self.timestamp().unwrap_or_default();
        }
        self.selected_line = Some(key);
        if !self.search.is_empty() {
            self.search.clear();
            self.refresh_products();
        }
        self.notice = None;
        self.focus_search = true;
    }

    fn apply_edit(&mut self, kind: &EditKind, value: &str) -> Result<(), Box<dyn Error>> {
        match kind {
            EditKind::Weight(id) => {
                let product = self.catalog.product_by_id(id.as_str()).ok_or("producto no disponible")?;
                let key = self.draft.add_product(product, quantity_field(value)?, self.catalog.price_mode)?;
                self.added(key);
            }
            EditKind::Line { key, price, .. } => {
                let product = self.draft.lines().iter().find(|line| line.key() == *key)
                    .ok_or("línea no disponible")?.product();
                let override_net = if self.can(Capability::ChangePrice) {
                    let gross = money_field(price)?;
                    if gross.is_negative() { return Err("el precio no puede ser negativo".into()); }
                    (gross != product_price_with_tax(product)).then(|| {
                        Money::from_decimal(gross.as_decimal() / product.tax_rate.multiplier())
                    })
                } else { None };
                self.draft.edit_line(*key, quantity_field(value)?, override_net)?;
            }
            EditKind::Note(key) => self.draft.set_note(*key, Some(value))?,
            EditKind::Discount(line, percent) => {
                let discount = if *percent { Discount::percent(Rate::parse_percent(value)?) }
                else { Discount::amount(money_field(value)?) };
                let discount = (!discount.is_zero()).then_some(discount);
                if let Some(key) = line {
                    self.draft.set_line_discount(*key, discount)?;
                } else {
                    self.draft.set_ticket_discount(discount)?;
                }
            }
        }
        Ok(())
    }

    fn delete_lines(&mut self, key: Option<LineKey>) -> Result<(), StoreError> {
        let Some((session, user)) = self.session_user() else { return Ok(()) };
        let now = self.timestamp()?;
        let mut draft = self.draft.clone();
        let removed = if let Some(key) = key {
            vec![draft.remove_line(key)?]
        } else {
            draft.set_ticket_discount(None)?;
            let keys: Vec<_> = draft.lines().iter().map(|line| line.key()).collect();
            let lines: Result<Vec<_>, _> = keys.into_iter().map(|key| draft.remove_line(key)).collect();
            lines?
        };
        pos_store::sales::record_removed_lines(&mut self.conn, &session, &removed, &user, &now)?;
        self.draft = draft;
        if self.draft.is_empty() { self.sale_opened_at.clear(); }
        self.selected_line = self.draft.lines().last().map(|line| line.key());
        self.notice = None;
        self.hide_dialog();
        Ok(())
    }

    fn commit_sale(&mut self, payments: &Payments) -> Result<(), StoreError> {
        let Some((session, user)) = self.session_user() else { return Ok(()) };
        let now = self.timestamp()?;
        let drawer = self.settings.drawer && !payments.is_empty();
        let sale = pos_store::sales::save_sale_with_drawer(&mut self.conn, &self.draft, payments, SaleContext {
            session_money: &session, user: &user, location: &self.catalog.location, now: &now,
        }, drawer)?;
        // Desde aquí la venta está confirmada. Un fallo de impresora nunca
        // deja el diálogo de pago listo para confirmar por segunda vez.
        self.draft.clear();
        self.selected_line = None;
        self.sale_number = self.next_sale_number;
        self.next_sale_number += 1;
        self.sale_opened_at.clear();
        self.notice = Some((false, format!(
            "Ticket {} guardado · Total {}",
            sale.ticket_number, sale.total.format_cop_exact(),
        )));
        if drawer {
            self.printer_notice = None;
            let key = format!("ticket:{}", sale.ticket_id);
            if self.pulse(&user, key.clone()) {
                self.sale_drawer = Some(PendingDrawer { key, until: Instant::now() + DRAWER_WAIT });
            }
        }
        self.show_dialog(Dialog::Change { ticket: sale.ticket_number, total: sale.total, amount: sale.change });
        Ok(())
    }

    fn pulse(&mut self, user: &User, key: String) -> bool {
        if self.settings.drawer {
            let result = self.printer.submit(Job {
                destination: self.settings.drawer_route().clone(),
                filename: "cajon".into(), audit_key: key.clone(), user_id: user.id.to_string(),
                receipt: false, bytes: escpos::drawer_pulse(false, 25, 250).to_vec(),
            });
            match result {
                Err(error) => {
                    let audited = self.timestamp().and_then(|now| pos_store::audit(
                        &self.conn, &now, Some(user.id.as_str()), "DRAWER_ERROR", &key));
                    let suffix = if audited.is_err() { " No se pudo auditar el fallo." } else { "" };
                    self.printer_notice = Some((true, format!("Cajón no enviado: {error}.{suffix}")));
                    return false;
                }
                Ok(()) if key == "printer:drawer-test" => self.printer_notice = Some((false, "Enviando pulso…".into())),
                Ok(()) => {}
            }
            return true;
        }
        false
    }

    fn waiting_for_sale_drawer(&mut self, ctx: &Context) -> bool {
        let Some(pending) = &self.sale_drawer else { return false };
        let remaining = pending.until.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            self.sale_drawer = None;
            self.printer_notice = Some((true, "El pulso del cajón sigue pendiente. El pago ya quedó guardado.".into()));
            false
        } else {
            ctx.request_repaint_after(remaining);
            true
        }
    }

    fn manual_drawer(&mut self) -> Result<(), Box<dyn Error>> {
        if !self.settings.drawer {
            return Err("no hay un cajón configurado para esta prueba".into());
        }
        let Some((session, user)) = self.session_user() else { return Ok(()) };
        let now = self.timestamp()?;
        pos_store::record_drawer_opening(&mut self.conn, &session, &user, &now)?;
        self.pulse(&user, format!("manual:{session}"));
        self.focus_search = true;
        Ok(())
    }

    fn request_receipt(&mut self, receipt: Option<&Receipt>, explicit_reprint: bool) -> Result<(), Box<dyn Error>> {
        if !self.can(Capability::Reprint) || self.printing_receipt {
            return Ok(());
        }
        let loaded;
        let receipt = match receipt {
            Some(value) => value,
            None => {
                loaded = pos_store::last_sale(&self.conn)?.ok_or("aún no hay un pago para imprimir")?;
                &loaded
            }
        };
        let key = format!("ticket:{}", receipt.id);
        let previous: bool = self.conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM pos_audit WHERE ACTION = 'PRINT_SENT' AND DETAIL = ?1)",
            [&key], |row| row.get(0),
        )?;
        let kind = if explicit_reprint || previous { ReceiptKind::Reprint } else { ReceiptKind::Original };
        let bytes = escpos::sale_receipt(self.settings.print_options, &self.settings.business,
            &self.settings.receipt_layout, receipt, kind);
        self.submit_receipt(format!("ticket-{}", receipt.number), key, bytes)?;
        self.focus_search = self.dialog.is_none() && self.sale_ready();
        Ok(())
    }

    fn submit_receipt(&mut self, filename: String, audit_key: String, bytes: Vec<u8>) -> Result<(), String> {
        let user = self.user.as_ref().expect("cajero autenticado");
        self.printer.submit(Job {
            destination: self.settings.destination.clone(), filename, audit_key, bytes,
            user_id: user.id.to_string(), receipt: true,
        })?;
        self.printing_receipt = true;
        self.printer_notice = Some((false, "Enviando tirilla…".into()));
        Ok(())
    }

    fn process_prints(&mut self) {
        while let Ok(outcome) = self.printer.done.try_recv() {
            self.process_print(outcome);
        }
    }

    fn process_print(&mut self, outcome: Outcome) {
        let success = outcome.result.is_ok();
        if !outcome.job.receipt && self.sale_drawer.as_ref().is_some_and(|pending| pending.key == outcome.job.audit_key) {
            self.sale_drawer = None;
        }
        let label = match (outcome.job.receipt, success) {
            (true, true) => "PRINT_SENT",
            (true, false) => "PRINT_ERROR",
            (false, true) => "DRAWER_SENT",
            (false, false) => "DRAWER_ERROR",
        };
        let audited = self.timestamp().and_then(|now| pos_store::audit(
            &self.conn, &now, Some(&outcome.job.user_id), label, &outcome.job.audit_key,
        ));
        if outcome.job.receipt {
            self.printing_receipt = false;
        }
        self.printer_notice = match (outcome.result, audited, outcome.job.receipt) {
            (Ok(message), Ok(()), true) => Some((false, message)),
            (Ok(_), Ok(()), false) if outcome.job.audit_key == "printer:drawer-test" =>
                Some((false, "Pulso enviado. Comprueba que el cajón se abrió.".into())),
            (Ok(_), Ok(()), false) => return,
            (Err(error), _, receipt) => Some((true, format!(
                "{}: {error}. Verifica el dispositivo antes de repetir.",
                if receipt { "No se confirmó el envío de la tirilla" } else { "No se confirmó el pulso del cajón" },
            ))),
            (_, Err(error), _) => Some((true, format!("Enviado, pero no se pudo auditar: {error}"))),
        };
    }

    fn prepare_close(&mut self) -> Result<(), Box<dyn Error>> {
        if self.has_pending_sales() {
            return Err("cobra o cancela todas las ventas abiertas antes de cerrar caja".into());
        }
        let Some((session, _)) = self.session_user() else { return Ok(()) };
        let totals = pos_store::session_totals(&self.conn, &session)?;
        let taxes = pos_store::sessions::session_taxes(&self.conn, &session)?;
        self.show_dialog(Dialog::Close(Box::new(CloseForm {
            totals, taxes, cash: pos_core::CashCount::new(), counted: String::new(), confirmed: false,
        })));
        Ok(())
    }

    fn finish_close(&mut self, counted: Money) -> Result<(), StoreError> {
        let Some(session) = self.session.clone() else { return Ok(()) };
        let user = self.user.clone().expect("cajero autenticado");
        let now = self.timestamp()?;
        let taxes = pos_store::sessions::session_taxes(&self.conn, &session.money)?;
        let close = pos_store::close_session(&mut self.conn, &session.money, counted, &user, &now)?;
        self.notice = Some((false, format!(
            "Caja {} cerrada · Diferencia {}", session.sequence, close.difference.format_cop_exact(),
        )));
        self.pulse(&user, format!("close:{}", session.money));
        self.last_closed = Some(Closed { session, timestamp: now, cashier: user.name, close, taxes });
        self.session = None;
        self.hide_dialog();
        Ok(())
    }

    fn request_close_receipt(&mut self) -> Result<(), String> {
        let Some(closed) = &self.last_closed else { return Ok(()) };
        let header = CloseHeader {
            host: &closed.session.host, sequence: closed.session.sequence,
            opened_at: &closed.session.datestart, closed_at: &closed.timestamp, cashier: &closed.cashier,
        };
        let bytes = escpos::close_receipt(self.settings.print_options, &self.settings.business, &header, &closed.close);
        self.submit_receipt(format!("cierre-{}", closed.session.sequence), format!("close:{}", closed.session.money), bytes)
    }

    fn run_backup_worker(&mut self, ctx: &Context, manual: bool) {
        self.run_backup_to(ctx, manual, self.settings.backups.clone());
    }

    fn run_backup_to(&mut self, ctx: &Context, manual: bool, dir: PathBuf) {
        if self.backup_done.is_some() {
            return;
        }
        self.next_backup_check = Instant::now() + Duration::from_secs(3600);
        let (send, receive) = mpsc::channel();
        let (db, offset, ctx) = (self.db.clone(), self.settings.offset, ctx.clone());
        let start = std::thread::Builder::new().name("pos-backup".into()).spawn(move || {
            let result = (|| -> Result<_, StoreError> {
                let conn = pos_store::open(db)?;
                if manual {
                    return Ok(Some(pos_store::backup_now(&conn, &dir)?));
                }
                let now = pos_store::now_local(offset)?;
                pos_store::maybe_weekly_backup(&conn, &now, Some(&dir))
            })();
            let _ = send.send(result.map_err(|error| error.to_string()));
            ctx.request_repaint();
        });
        match start {
            Ok(_) => self.backup_done = Some((manual, receive)),
            Err(error) => self.notice = Some((true, format!("No se pudo iniciar el respaldo: {error}"))),
        }
    }

    fn process_backups(&mut self, ctx: &Context) {
        if let Some((manual, rx)) = &self.backup_done {
            use mpsc::TryRecvError;
            match rx.try_recv() {
                Ok(Ok(Some(path))) if *manual => self.notice = Some((false, format!("Respaldo: {}", path.display()))),
                Ok(Ok(_)) => {}
                Ok(Err(error)) => self.notice = Some((true, format!("Respaldo no realizado: {error}"))),
                Err(TryRecvError::Disconnected) => self.notice = Some((true, "Respaldo interrumpido".into())),
                Err(TryRecvError::Empty) => return,
            }
            self.backup_done = None;
        }
        let wait = self.next_backup_check.saturating_duration_since(Instant::now());
        if wait.is_zero() {
            self.run_backup_worker(ctx, false);
        } else {
            ctx.request_repaint_after(wait);
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.process_prints();
        self.process_backups(ctx);
        if ctx.input(|input| input.viewport().close_requested()) {
            if self.dialog.as_ref().is_some_and(Dialog::editing_product) {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.dialog_error = Some("Guarda o cancela la edición del producto antes de salir.".into());
            } else if self.sale_drawer.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            } else if self.has_pending_sales() || self.printing_receipt {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.notice = Some((true, "Termina o cancela las ventas abiertas y espera la tirilla antes de salir.".into()));
            }
        }
        if self.user.is_none() {
            self.render_login(ctx);
        } else {
            self.render_sales(ctx);
            self.render_dialog(ctx);
        }
        self.render_customer_display(ctx);
    }
}

fn product_price_with_tax(product: &pos_core::ProductSnapshot) -> Money {
    Money::from_decimal(product.unit_price_net.as_decimal() * product.tax_rate.multiplier())
}

fn quantity_field(value: &str) -> Result<Qty, pos_core::CoreError> {
    // Impide que un error al digitar se redondee a otro peso.
    let value = value.trim();
    let mut parts = value.split(['.', ',']);
    let integer = parts.next().unwrap_or_default();
    let fraction = parts.next();
    if integer.is_empty() || !integer.bytes().all(|x| x.is_ascii_digit())
        || fraction.is_some_and(|p| p.is_empty() || p.len() > 3 || !p.bytes().all(|x| x.is_ascii_digit()))
        || parts.next().is_some()
    {
        return Err(pos_core::CoreError::InvalidQuantity("usa hasta tres decimales, sin separador de miles".into()));
    }
    Qty::parse(value)
}
