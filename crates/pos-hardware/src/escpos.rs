//! Tirillas para papel de 58 y 80 mm, sin fuentes gráficas ni drivers de Java.

use pos_core::receipt::Receipt;
use pos_core::{Money, SessionClose};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Paper {
    Mm58,
    Mm80,
}

impl Paper {
    pub const fn columns(self) -> usize {
        match self {
            Self::Mm58 => 32,
            Self::Mm80 => 48,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodePage {
    Cp850,
    Cp858,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cut {
    None,
    Full,
    Partial,
}

#[derive(Clone, Copy, Debug)]
pub struct PrinterOptions {
    pub paper: Paper,
    pub page: CodePage,
    /// Número ESC t del manual de la impresora. Puede variar en las genéricas.
    pub page_slot: u8,
    pub cut: Cut,
}

impl Default for PrinterOptions {
    fn default() -> Self {
        Self { paper: Paper::Mm80, page: CodePage::Cp850, page_slot: 2, cut: Cut::None }
    }
}

#[derive(Clone, Debug, Default)]
pub struct BusinessHeader {
    pub name: String,
    pub tax_id: String,
    pub address: String,
    pub phone: String,
    pub footer: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReceiptLayout {
    pub extra_header: String,
    pub cashier: bool,
    pub unit_price: bool,
    pub product_code: bool,
    pub notes: bool,
    pub payment_references: bool,
    pub cash_detail: bool,
    pub show_zero_tax: bool,
}

impl Default for ReceiptLayout {
    fn default() -> Self {
        Self {
            extra_header: String::new(), cashier: true, unit_price: false, product_code: false,
            notes: true, payment_references: true, cash_detail: true, show_zero_tax: false,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum ReceiptKind {
    Original,
    Reprint,
    Sample,
}

pub struct CloseHeader<'a> {
    pub host: &'a str,
    pub sequence: i64,
    pub opened_at: &'a str,
    pub closed_at: &'a str,
    pub cashier: &'a str,
}

pub fn sale_receipt(
    options: PrinterOptions,
    business: &BusinessHeader,
    layout: &ReceiptLayout,
    sale: &Receipt,
    kind: ReceiptKind,
) -> Vec<u8> {
    let mut out = Slip::new(options);
    render_sale(&mut out, business, layout, sale, kind);
    out.finish()
}

/// Vista previa en columnas, con el mismo contenido y ajuste que los bytes ESC/POS.
pub fn preview_sale_receipt(
    options: PrinterOptions,
    business: &BusinessHeader,
    layout: &ReceiptLayout,
    sale: &Receipt,
    kind: ReceiptKind,
) -> String {
    let mut out = Slip::new(options);
    out.preview = Some(String::new());
    render_sale(&mut out, business, layout, sale, kind);
    out.preview.unwrap_or_default()
}

fn render_sale(
    out: &mut Slip,
    business: &BusinessHeader,
    layout: &ReceiptLayout,
    sale: &Receipt,
    kind: ReceiptKind,
) {
    out.business(business);
    out.centered_lines(&layout.extra_header);
    out.bold(true);
    match kind {
        ReceiptKind::Reprint => out.text("REIMPRESIÓN"),
        ReceiptKind::Sample => out.text("MUESTRA - NO ES UNA VENTA"),
        ReceiptKind::Original => {}
    }
    out.text(&format!("Ticket {}", sale.number));
    out.bold(false);
    out.text(&sale.date);
    if layout.cashier {
        out.text(&format!("Caja: {}", sale.cashier));
    }
    out.rule();
    for line in &sale.lines {
        out.pair(&line.quantity.format(line.unit), &money(line.amount));
        out.text(&line.name);
        if let Some(code) = line.code.as_ref().filter(|_| layout.product_code) {
            out.text(&format!("Código: {code}"));
        }
        if layout.unit_price {
            out.pair("Precio unit. sin IVA", &money(line.unit_price_net));
        }
        if let Some(note) = line.note.as_ref().filter(|_| layout.notes) {
            out.text(&format!("Nota: {note}"));
        }
    }
    out.rule();
    if layout.show_zero_tax || !sale.tax.is_zero() {
        out.pair("Subtotal", &money(sale.net));
        out.pair("Impuesto", &money(sale.tax));
    }
    out.bold(true);
    out.pair("TOTAL", &money(sale.total));
    out.bold(false);
    for payment in &sale.payments {
        out.pair(payment.method.label(), &money(payment.amount));
        if let Some(reference) = payment.reference.as_ref().filter(|_| layout.payment_references) {
            out.text(&format!("Ref.: {reference}"));
        }
    }
    if sale.collected != sale.total {
        out.pair("Cobrado (histórico)", &money(sale.collected));
    }
    if layout.cash_detail && sale.payments.iter().any(|p| p.method.is_cash()) {
        let tendered: Option<Money> = sale.payments.iter()
            .filter(|p| p.method.is_cash()).map(|p| p.tendered).sum();
        if let Some(received) = tendered {
            out.pair("Recibido en efectivo", &money(received));
        }
        out.pair("Devuelta", &money(sale.change));
    }
    out.centered_lines(&business.footer);
}

pub fn close_receipt(
    options: PrinterOptions,
    business: &BusinessHeader,
    header: &CloseHeader<'_>,
    close: &SessionClose,
) -> Vec<u8> {
    let mut out = Slip::new(options);
    out.business(business);
    out.bold(true);
    out.text(&format!("Cierre {} - {}", header.host, header.sequence));
    out.bold(false);
    out.text(&format!("Desde: {}", header.opened_at));
    out.text(&format!("Hasta: {}", header.closed_at));
    out.text(&format!("Cerrado por: {}", header.cashier));
    out.rule();
    let totals = &close.totals;
    out.pair("Tickets", &totals.ticket_count.to_string());
    out.pair("Ventas", &money(totals.sales_total));
    out.pair("Impuestos", &money(totals.sales_tax));
    out.pair("Cobrado", &money(totals.collected_sales()));
    if !totals.sales_difference().is_zero() {
        out.pair("Ventas - cobrado", &money(totals.sales_difference()));
    }
    for (method, amount) in totals.collected_by_method() {
        out.pair(method.label(), &money(amount));
    }
    out.rule();
    out.pair("Fondo inicial", &money(totals.opening_float));
    out.pair("Entradas", &money(totals.cash_in));
    out.pair("Salidas", &money(totals.cash_out));
    out.pair("Efectivo esperado", &money(close.expected_cash));
    out.pair("Efectivo contado", &money(close.counted_cash));
    out.bold(true);
    out.pair("Diferencia", &money(close.difference));
    out.bold(false);
    out.pair("Líneas borradas", &totals.removed_lines.to_string());
    out.pair("Aperturas de cajón", &totals.drawer_openings.to_string());
    out.pair("Sin venta", &totals.no_sale_count.to_string());
    out.text("Firma: __________________");
    out.finish()
}

pub fn test_page(options: PrinterOptions) -> Vec<u8> {
    let mut out = Slip::new(options);
    out.text("Prueba de impresión ESC/POS");
    out.text("áéíóú ÁÉÍÓÚ ñÑ ¿¡ üÜ");
    out.rule();
    out.pair("Columnas", &options.paper.columns().to_string());
    out.finish()
}

/// Trabajo separado de la tirilla: reimprimir nunca activa el cajón.
pub const fn drawer_pulse(pin5: bool, on: u8, off: u8) -> [u8; 5] {
    [0x1b, b'p', pin5 as u8, on, off]
}

fn money(value: Money) -> String {
    value.format_cop_exact()
}

struct Slip {
    options: PrinterOptions,
    bytes: Vec<u8>,
    preview: Option<String>,
    centered: bool,
}

impl Slip {
    fn new(options: PrinterOptions) -> Self {
        Self {
            options, bytes: vec![0x1b, b'@', 0x1b, b't', options.page_slot],
            preview: None, centered: false,
        }
    }

    fn center(&mut self, yes: bool) {
        self.centered = yes;
        self.bytes.extend_from_slice(&[0x1b, b'a', u8::from(yes)]);
    }

    fn bold(&mut self, yes: bool) {
        self.bytes.extend_from_slice(&[0x1b, b'E', u8::from(yes)]);
    }

    fn business(&mut self, business: &BusinessHeader) {
        self.center(true);
        self.bold(true);
        if !business.name.trim().is_empty() {
            self.text(&business.name);
        }
        self.bold(false);
        if !business.tax_id.trim().is_empty() {
            self.text(&format!("NIT: {}", business.tax_id));
        }
        for value in [&business.address, &business.phone] {
            if !value.trim().is_empty() {
                self.text(value);
            }
        }
        self.center(false);
    }

    fn line(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
        self.bytes.push(b'\n');
        if let Some(text) = &mut self.preview {
            if self.centered {
                text.push_str(&" ".repeat(self.options.paper.columns().saturating_sub(bytes.len()) / 2));
            }
            text.extend(bytes.iter().map(|&byte| decode(byte, self.options.page)));
            text.push('\n');
        }
    }

    fn centered_lines(&mut self, text: &str) {
        if !text.trim().is_empty() {
            self.center(true);
            for line in text.trim().lines() {
                if line.trim().is_empty() { self.line(b""); } else { self.text(line); }
            }
            self.center(false);
        }
    }

    fn text(&mut self, text: &str) {
        let width = self.options.paper.columns();
        let text = encode(text.trim(), self.options.page);
        for part in text.chunks(width) {
            self.line(part);
        }
    }

    fn pair(&mut self, label: &str, value: &str) {
        let width = self.options.paper.columns();
        let mut label_bytes = encode(label, self.options.page);
        let value_bytes = encode(value, self.options.page);
        if label_bytes.len() + value_bytes.len() < width {
            label_bytes.resize(width - value_bytes.len(), b' ');
            label_bytes.extend_from_slice(&value_bytes);
            self.line(&label_bytes);
        } else {
            self.text(label);
            self.text(value);
        }
    }

    fn rule(&mut self) {
        self.line(&vec![b'-'; self.options.paper.columns()]);
    }

    fn finish(mut self) -> Vec<u8> {
        self.bytes.extend_from_slice(b"\n\n\n");
        match self.options.cut {
            Cut::None => {}
            Cut::Full => self.bytes.extend_from_slice(&[0x1d, b'V', 0]),
            Cut::Partial => self.bytes.extend_from_slice(&[0x1d, b'V', 1]),
        }
        self.bytes
    }
}

const HIGH: &str = concat!(
    "ÇüéâäàåçêëèïîìÄÅ", "ÉæÆôöòûùÿÖÜø£Ø×ƒ", "áíóúñÑªº¿®¬½¼¡«»", "░▒▓│┤ÁÂÀ©╣║╗╝¢¥┐",
    "└┴┬├─┼ãÃ╚╔╩╦╠═╬¤", "ðÐÊËÈıÍÎÏ┘┌█▄¦Ì▀", "ÓßÔÒõÕµþÞÚÛÙýÝ¯´", "\u{ad}±‗¾¶§÷¸°¨·¹³²■ ",
);

fn encode(value: &str, page: CodePage) -> Vec<u8> {
    value.chars().map(|ch| match ch {
        ' '..='~' => ch as u8,
        c if c.is_whitespace() || c.is_control() => b' ',
        '€' if page == CodePage::Cp858 => 0xd5,
        'ı' if page == CodePage::Cp858 => b'?',
        c => HIGH.chars().position(|candidate| candidate == c).map_or(b'?', |i| 0x80 + i as u8),
    }).collect()
}

fn decode(byte: u8, page: CodePage) -> char {
    match byte {
        0..=127 => char::from(byte),
        0xd5 if page == CodePage::Cp858 => '€',
        _ => HIGH.chars().nth(usize::from(byte - 0x80)).unwrap_or('?'),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pos_core::receipt::{ReceiptLine, ReceiptPayment};
    use pos_core::{PaymentMethod, Qty, SaleUnit};

    #[test]
    fn respeta_ancho_acentos_y_configuracion_de_corte() {
        for paper in [Paper::Mm58, Paper::Mm80] {
            let data = test_page(PrinterOptions { paper, page: CodePage::Cp858, page_slot: 19, cut: Cut::Partial });
            assert!(data.starts_with(&[27, b'@', 27, b't', 19]));
            assert!(data.ends_with(&[29, b'V', 1]));
            assert!(data.windows(paper.columns()).any(|s| s.iter().all(|b| *b == b'-')));
            assert_eq!(encode("ñÑ¿¡€", CodePage::Cp858), [0xa4, 0xa5, 0xa8, 0xad, 0xd5]);
            assert_eq!(encode("\x1bp\x00\n", CodePage::Cp850), b" p  ");
        }
    }

    #[test]
    fn imprime_una_venta_manual_sin_accionar_el_cajon() {
        let total = Money::from_cents(151_025);
        let mut receipt = Receipt {
            id: "r1".into(), number: 23006, date: "2026-11-24 10:00:00".into(),
            cashier: "Empleado".into(),
            lines: vec![ReceiptLine {
                name: "Café".into(), code: Some("123456".into()), unit_price_net: Money::from_pesos(1777),
                quantity: Qty::from_grams(850), unit: SaleUnit::Kilogram,
                amount: total, note: Some("nota\x1bp".into()),
            }],
            payments: vec![ReceiptPayment {
                method: PaymentMethod::Cash, amount: total,
                tendered: Some(Money::from_pesos(2_000)), reference: None,
            }],
            net: total, tax: Money::ZERO, total, collected: total,
            change: Money::from_cents(48_975),
        };
        let data = sale_receipt(PrinterOptions::default(), &BusinessHeader::default(),
            &ReceiptLayout::default(), &receipt, ReceiptKind::Reprint);
        assert!(data.windows(b"Ticket 23006".len()).any(|s| s == b"Ticket 23006"));
        assert!(data.windows(b"0,850 kg".len()).any(|s| s == b"0,850 kg"));
        assert!(data.windows(b"$ 1.510,25".len()).any(|s| s == b"$ 1.510,25"));
        assert!(!data.windows(2).any(|s| s == b"\x1bp"));
        assert_eq!(drawer_pulse(false, 25, 250), [27, b'p', 0, 25, 250]);

        // La configuración afecta tanto a la muestra como a la tirilla con datos reales.
        receipt.payments[0].reference = Some("REF-01".into());
        receipt.tax = Money::from_pesos(50);
        receipt.net -= receipt.tax;
        let options = PrinterOptions { paper: Paper::Mm58, ..PrinterOptions::default() };
        let business = BusinessHeader { name: "Café central".into(), footer: "Gracias\nVuelva pronto".into(),
            ..BusinessHeader::default() };
        let layout = ReceiptLayout {
            extra_header: "Bogotá\nHorario 8 a 6".into(), cashier: false, unit_price: true, product_code: true,
            notes: false, payment_references: false, cash_detail: false, show_zero_tax: false,
        };
        let preview = preview_sale_receipt(options, &business, &layout, &receipt, ReceiptKind::Sample);
        let data = sale_receipt(options, &business, &layout, &receipt, ReceiptKind::Sample);
        for text in ["Café central", "Horario 8 a 6", "MUESTRA - NO ES UNA VENTA", "Ticket 23006",
            "Código: 123456", "Precio unit. sin IVA", "Impuesto", "TOTAL", "Gracias", "Vuelva pronto"]
        {
            assert!(preview.contains(text), "{text}");
            let encoded = encode(text, options.page);
            assert!(data.windows(encoded.len()).any(|window| window == encoded), "{text}");
        }
        for text in ["Empleado", "Nota:", "REF-01", "Devuelta"] {
            assert!(!preview.contains(text), "{text}");
            assert!(!data.windows(text.len()).any(|s| s == text.as_bytes()), "{text}");
        }
        assert!(preview.lines().all(|line| line.chars().count() <= options.paper.columns()));
        assert!(!data.windows(2).any(|s| s == b"\x1bp"));
    }

    #[test]
    fn cierre_muestra_faltante_y_pago_por_medio() {
        let totals = pos_core::SessionTotals {
            opening_float: Money::from_pesos(500),
            payments: vec![pos_core::PaymentTotal {
                method: PaymentMethod::Nequi, amount: Money::from_pesos(10_000),
            }],
            sales_net: Money::from_pesos(10_000),
            sales_total: Money::from_pesos(10_000),
            ticket_count: 1,
            ..Default::default()
        };
        let close = totals.close(Money::from_pesos(400)).unwrap();
        let header = CloseHeader {
            host: "Terminal", sequence: 598, opened_at: "2026-11-24 08:00:00",
            closed_at: "2026-11-24 18:00:00", cashier: "Administrador",
        };
        let bytes = close_receipt(PrinterOptions::default(), &BusinessHeader::default(), &header, &close);
        for text in [b"Cierre Terminal - 598".as_slice(), b"Nequi", b"$ -100"] {
            assert!(bytes.windows(text.len()).any(|part| part == text));
        }
    }
}
