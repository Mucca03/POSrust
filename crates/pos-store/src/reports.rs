//! Reporte operativo por fechas, productos y medios de pago. SQLite filtra
//! las filas, pero los importes se calculan en Rust con Decimal.

use std::collections::BTreeMap;
use std::io::Write;
use std::str::FromStr;

use pos_core::{Money, PaymentMethod, Qty};
use rusqlite::{params, Connection};
use rust_decimal::Decimal;
use time::{macros::format_description, Date};

use crate::{properties::read_properties, StoreError};

#[derive(Clone, Debug, Default)]
pub struct SalesReport {
    pub days: BTreeMap<String, DaySales>,
    pub products: BTreeMap<Option<String>, ProductSales>,
    pub payments: BTreeMap<String, Decimal>,
}

#[derive(Clone, Debug, Default)]
pub struct DaySales {
    pub tickets: u32,
    pub net: Decimal,
    pub tax: Decimal,
    pub collected: Decimal,
    pub cash_in: Decimal,
    pub cash_out: Decimal,
}

impl DaySales {
    pub fn total(&self) -> Decimal {
        self.net + self.tax
    }

    pub fn difference(&self) -> Decimal {
        self.total() - self.collected
    }
}

#[derive(Clone, Debug, Default)]
pub struct ProductSales {
    pub name: String,
    pub units: Decimal,
    pub net: Decimal,
}

#[derive(Clone, Copy, Debug)]
pub enum ReportCsv {
    ByDay,
    ByProduct,
    ByPayment,
}

/// Las dos fechas `YYYY-MM-DD` son inclusivas y se interpretan en hora local.
pub fn sales_report(
    conn: &Connection,
    first_day: &str,
    last_day: &str,
) -> Result<SalesReport, StoreError> {
    let format = format_description!("[year]-[month]-[day]");
    let first = Date::parse(first_day, format);
    let last = Date::parse(last_day, format);
    if !matches!((first, last), (Ok(a), Ok(b)) if a <= b) {
        return Err(StoreError::InvalidArgument("rango de fechas inválido".to_owned()));
    }
    let start = format!("{first_day} 00:00:00");
    let end = format!("{last_day} 23:59:59");
    let tx = conn.unchecked_transaction()?;
    let mut report = SalesReport::default();
    {
        let mut statement = tx.prepare(
            "SELECT substr(r.DATENEW, 1, 10), COUNT(*)
             FROM receipts r JOIN tickets t ON t.ID = r.ID
             WHERE r.DATENEW BETWEEN ?1 AND ?2 AND t.TICKETTYPE = 0
             GROUP BY substr(r.DATENEW, 1, 10)",
        )?;
        let mut rows = statement.query(params![start, end])?;
        while let Some(row) = rows.next()? {
            report.days.entry(row.get(0)?).or_default().tickets = row.get(1)?;
        }
    }
    {
        let mut statement = tx.prepare(
            "SELECT substr(r.DATENEW, 1, 10), r.ID, r.ATTRIBUTES,
                    l.PRODUCT, p.NAME, l.ATTRIBUTES, l.UNITS, l.PRICE
             FROM receipts r JOIN tickets t ON t.ID = r.ID
             JOIN ticketlines l ON l.TICKET = r.ID LEFT JOIN products p ON p.ID = l.PRODUCT
             WHERE r.DATENEW BETWEEN ?1 AND ?2 AND t.TICKETTYPE = 0
             ORDER BY r.DATENEW, r.ID, l.LINE",
        )?;
        let mut rows = statement.query(params![start, end])?;
        let mut current = String::new();
        let mut is_pos = false;
        while let Some(row) = rows.next()? {
            let receipt_id: String = row.get(1)?;
            if receipt_id != current {
                current = receipt_id;
                let attrs: Option<Vec<u8>> = row.get(2)?;
                is_pos = read_properties(attrs.as_deref().unwrap_or_default()).contains_key("pos.ticketid");
            }
            let product: Option<String> = row.get(3)?;
            let units = decimal(&row.get::<_, String>(6)?)?;
            let price = decimal(&row.get::<_, String>(7)?)?;
            let net = if is_pos {
                (Money::from_decimal(price) * Qty::from_decimal(units)).as_decimal()
            } else {
                units * price
            };
            report.days.entry(row.get(0)?).or_default().net += net;
            let product_id = product.as_deref();
            let counts_as_product = product.is_some();
            let name = match report.products.get_mut(&product) {
                Some(entry) => {
                    if counts_as_product {
                        entry.units += units;
                    }
                    entry.net += net;
                    continue;
                }
                None => match row.get::<_, Option<String>>(4)? {
                    Some(name) => name,
                    None if product.is_none() => "Ajustes y descuentos".to_owned(),
                    None => {
                        let attrs: Option<Vec<u8>> = row.get(5)?;
                        read_properties(attrs.as_deref().unwrap_or_default())
                            .remove("product.name")
                            .unwrap_or_else(|| product_id.unwrap_or_default().to_owned())
                    }
                },
            };
            report.products.insert(
                product,
                ProductSales { name, units: if counts_as_product { units } else { Decimal::ZERO }, net },
            );
        }
    }
    {
        let mut statement = tx.prepare(
            "SELECT substr(r.DATENEW, 1, 10), x.AMOUNT
             FROM receipts r JOIN tickets t ON t.ID = r.ID JOIN taxlines x ON x.RECEIPT = r.ID
             WHERE r.DATENEW BETWEEN ?1 AND ?2 AND t.TICKETTYPE = 0",
        )?;
        let mut rows = statement.query(params![start, end])?;
        while let Some(row) = rows.next()? {
            report.days.entry(row.get(0)?).or_default().tax += decimal(&row.get::<_, String>(1)?)?;
        }
    }
    {
        let mut statement = tx.prepare(
            "SELECT substr(r.DATENEW, 1, 10), p.PAYMENT, p.TOTAL, t.ID IS NOT NULL
             FROM receipts r JOIN payments p ON p.RECEIPT = r.ID
             LEFT JOIN tickets t ON t.ID = r.ID AND t.TICKETTYPE = 0
             WHERE r.DATENEW BETWEEN ?1 AND ?2
               AND (t.ID IS NOT NULL OR p.PAYMENT IN ('cashin', 'cashout'))",
        )?;
        let mut rows = statement.query(params![start, end])?;
        while let Some(row) = rows.next()? {
            let code: String = row.get(1)?;
            let amount = decimal(&row.get::<_, String>(2)?)?;
            let is_sale: bool = row.get(3)?;
            let day = report.days.entry(row.get(0)?).or_default();
            match code.as_str() {
                "cashin" => day.cash_in += amount,
                "cashout" => day.cash_out += amount.abs(),
                _ if is_sale => day.collected += amount,
                _ => {}
            }
            *report.payments.entry(code).or_default() += amount;
        }
    }
    tx.commit()?;
    Ok(report)
}

impl SalesReport {
    /// Separador `;`, UTF-8 con BOM y decimales exactos sin separador de miles.
    pub fn write_csv(&self, mut writer: impl Write, kind: ReportCsv) -> std::io::Result<()> {
        writer.write_all(b"\xef\xbb\xbf")?;
        match kind {
            ReportCsv::ByDay => {
                writer.write_all("Fecha;Tickets;Neto;Impuesto;Ventas;Cobrado;Diferencia;Entradas;Salidas\r\n".as_bytes())?;
                for (date, day) in &self.days {
                    writeln!(writer, "{date};{};{};{};{};{};{};{};{}\r",
                        day.tickets, day.net, day.tax, day.total(), day.collected,
                        day.difference(), day.cash_in, day.cash_out)?;
                }
            }
            ReportCsv::ByProduct => {
                writer.write_all("ID;Producto;Cantidad;Neto\r\n".as_bytes())?;
                for (id, product) in &self.products {
                    write_text_cell(&mut writer, id.as_deref().unwrap_or_default())?;
                    writer.write_all(b";")?;
                    write_text_cell(&mut writer, &product.name)?;
                    writeln!(writer, ";{};{}\r", product.units, product.net)?;
                }
            }
            ReportCsv::ByPayment => {
                writer.write_all("Código;Medio;Importe\r\n".as_bytes())?;
                for (code, total) in &self.payments {
                    let method = PaymentMethod::from_code(code);
                    let label = match code.as_str() {
                        "cashin" => "Entrada de caja",
                        "cashout" => "Salida de caja",
                        _ => method.label(),
                    };
                    write_text_cell(&mut writer, code)?;
                    writer.write_all(b";")?;
                    write_text_cell(&mut writer, label)?;
                    writeln!(writer, ";{total}\r")?;
                }
            }
        }
        Ok(())
    }
}

fn write_text_cell(writer: &mut impl Write, value: &str) -> std::io::Result<()> {
    writer.write_all(b"\"")?;
    if value.trim_start().starts_with(['=', '+', '-', '@']) {
        writer.write_all(b"'")?;
    }
    writer.write_all(value.replace('"', "\"\"").as_bytes())?;
    writer.write_all(b"\"")
}

fn decimal(value: &str) -> Result<Decimal, StoreError> {
    Decimal::from_str(value.trim())
        .map_err(|e| StoreError::Verification(format!("decimal inválido en reporte ({value:?}): {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separa_cobros_movimientos_y_ventas_y_exporta_csv() {
        let conn = Connection::open_in_memory().unwrap();
        crate::apply_schema(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO receipts (ID, DATENEW) VALUES
                 ('a', '2026-11-20 23:59:59'), ('b', '2026-11-20 13:00:00'), ('c', '2026-11-21 00:00:00');
             INSERT INTO tickets (ID, TICKETID, TICKETTYPE) VALUES ('a', 1, 0), ('c', 2, 0);
             INSERT INTO products (ID, NAME) VALUES ('prod', '  =ejemplo;\"test\"');
             INSERT INTO ticketlines (TICKET, LINE, PRODUCT, UNITS, PRICE) VALUES
                 ('a', 0, 'prod', '0.850', '4600'), ('a', 1, NULL, '1', '-1'), ('c', 0, 'prod', '1', '5000');
             INSERT INTO payments (ID, RECEIPT, PAYMENT, TOTAL) VALUES
                 ('p1', 'a', 'nequi', '1000'), ('p2', 'a', 'cash', '2000'), ('p3', 'b', 'cashout', '-100');",
        ).unwrap();
        let report = sales_report(&conn, "2026-11-20", "2026-11-20").unwrap();
        let day = &report.days["2026-11-20"];
        assert_eq!(report.days.len(), 1);
        assert_eq!(day.tickets, 1);
        assert_eq!(day.total(), Decimal::from(3909));
        assert_eq!(day.collected, Decimal::from(3000));
        assert_eq!(day.cash_out, Decimal::from(100));
        assert_eq!(day.difference(), Decimal::from(909));
        assert_eq!(report.payments["cashout"], Decimal::from(-100));
        let mut csv = Vec::new();
        report.write_csv(&mut csv, ReportCsv::ByProduct).unwrap();
        let csv = String::from_utf8(csv).unwrap();
        assert!(csv.starts_with('\u{feff}'));
        assert!(csv.contains("\"prod\";\"'  =ejemplo;\"\"test\"\"\";0.850;3910.000"));
        assert!(sales_report(&conn, "2026-11-21", "2026-11-20").is_err());
    }
}
