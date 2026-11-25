use std::collections::{BTreeMap, HashMap};
use std::str::FromStr;

use rusqlite::Connection;
use rust_decimal::Decimal;
use serde::Serialize;

use crate::error::StoreError;
use crate::schema::table_names;

#[derive(Debug, Clone, Serialize)]
pub struct PaymentMismatch {
    pub receipt_id: String,
    pub ticket_id: i64,
    pub date: String,
    pub lines_total: String,
    pub payments_total: String,
    pub difference: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StockDrift {
    pub location: String,
    pub product: String,
    pub attributes: Option<String>,
    pub current: String,
    pub diary_sum: String,
    pub difference: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OddCode {
    pub product_id: String,
    pub code: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct VerificationReport {
    pub counts: BTreeMap<String, u64>,
    pub max_ticket_id: Option<i64>,
    pub ticket_sequence: Option<i64>,
    pub max_session_sequence: Option<i64>,
    pub open_sessions: u64,
    pub cash_movement_receipts: u64,
    pub tickets_without_taxlines: u64,
    pub payment_mismatch_count: u64,
    pub payment_mismatches: Vec<PaymentMismatch>,
    pub stock_drift_count: u64,
    pub stock_drift: Vec<StockDrift>,
    pub orphan_counts: BTreeMap<String, u64>,
    pub odd_codes: Vec<OddCode>,
    pub lines_without_product: u64,
    pub scale_products: u64,
    pub negative_stock_products: u64,
    pub integrity_ok: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl VerificationReport {
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    pub fn error_summary(&self) -> String {
        if self.errors.is_empty() {
            "sin errores".to_string()
        } else {
            self.errors.join("; ")
        }
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Verificación del respaldo\n");
        out.push_str("========================\n\n");
        out.push_str("Conteos por tabla:\n");
        for (table, count) in &self.counts {
            out.push_str(&format!("  {table:24} {count}\n"));
        }
        out.push_str(&format!(
            "\nTicket máximo: {:?} / secuencia: {:?}\n",
            self.max_ticket_id, self.ticket_sequence
        ));
        out.push_str(&format!(
            "Sesión máxima: {:?} / abiertas: {}\n",
            self.max_session_sequence, self.open_sessions
        ));
        out.push_str(&format!(
            "Receipts sin ticket (movimientos de caja): {}\n",
            self.cash_movement_receipts
        ));
        out.push_str(&format!(
            "Tickets sin taxlines: {}\n",
            self.tickets_without_taxlines
        ));
        out.push_str(&format!(
            "Diferencias líneas/pagos: {}\n",
            self.payment_mismatch_count
        ));
        out.push_str(&format!(
            "Derivas de stock: {}\n",
            self.stock_drift_count
        ));
        out.push_str(&format!(
            "Líneas sin producto (ajustes/descuentos): {}\n",
            self.lines_without_product
        ));
        out.push_str(&format!("Productos pesables: {}\n", self.scale_products));
        out.push_str(&format!(
            "Productos con stock negativo: {}\n",
            self.negative_stock_products
        ));
        out.push_str(&format!("Integridad SQLite: {}\n", self.integrity_ok));
        out.push_str(&format!("Códigos no numéricos: {}\n", self.odd_codes.len()));

        if !self.orphan_counts.is_empty() {
            out.push_str("\nClaves huérfanas:\n");
            for (name, count) in &self.orphan_counts {
                out.push_str(&format!("  {name}: {count}\n"));
            }
        }

        if !self.payment_mismatches.is_empty() {
            out.push_str("\nPrimeras diferencias líneas/pagos:\n");
            for m in self.payment_mismatches.iter().take(10) {
                out.push_str(&format!(
                    "  ticket {} ({}) líneas {} pagos {} diferencia {}\n",
                    m.ticket_id, m.date, m.lines_total, m.payments_total, m.difference
                ));
            }
            if self.payment_mismatches.len() > 10 {
                out.push_str(&format!(
                    "  ... y {} más\n",
                    self.payment_mismatches.len() - 10
                ));
            }
        }

        if !self.stock_drift.is_empty() {
            out.push_str("\nPrimeras derivas de stock:\n");
            for d in self.stock_drift.iter().take(10) {
                out.push_str(&format!(
                    "  {} / {}: actual {} vs diario {} (dif {})\n",
                    d.location, d.product, d.current, d.diary_sum, d.difference
                ));
            }
            if self.stock_drift.len() > 10 {
                out.push_str(&format!("  ... y {} más\n", self.stock_drift.len() - 10));
            }
        }

        if !self.warnings.is_empty() {
            out.push_str("\nAdvertencias:\n");
            for w in &self.warnings {
                out.push_str(&format!("  - {w}\n"));
            }
        }

        if !self.errors.is_empty() {
            out.push_str("\nErrores:\n");
            for e in &self.errors {
                out.push_str(&format!("  - {e}\n"));
            }
        } else {
            out.push_str("\nResultado: OK (las anomalías históricas quedan como advertencias)\n");
        }
        out
    }
}

pub fn verify_connection(
    conn: &Connection,
) -> Result<VerificationReport, StoreError> {
    let mut counts = BTreeMap::new();
    for table in table_names() {
        let count: u64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM \"{table}\""),
            [],
            |r| r.get(0),
        )?;
        counts.insert(table.to_string(), count);
    }

    let max_ticket_id: Option<i64> =
        conn.query_row("SELECT MAX(TICKETID) FROM tickets", [], |r| r.get(0))?;
    let ticket_sequence: Option<i64> =
        conn.query_row("SELECT (SELECT ID FROM ticketsnum LIMIT 1)", [], |r| r.get(0))?;
    let max_session_sequence: Option<i64> =
        conn.query_row("SELECT MAX(HOSTSEQUENCE) FROM closedcash", [], |r| r.get(0))?;
    let open_sessions: u64 = conn.query_row(
        "SELECT COUNT(*) FROM closedcash WHERE DATEEND IS NULL",
        [],
        |r| r.get(0),
    )?;
    let cash_movement_receipts: u64 = conn.query_row(
        "SELECT COUNT(*) FROM receipts r LEFT JOIN tickets t ON t.ID = r.ID WHERE t.ID IS NULL",
        [],
        |r| r.get(0),
    )?;
    let tickets_without_taxlines: u64 = conn.query_row(
        "SELECT COUNT(*) FROM tickets t WHERE NOT EXISTS (SELECT 1 FROM taxlines x WHERE x.RECEIPT = t.ID)",
        [],
        |r| r.get(0),
    )?;

    let lines_total_by_ticket = load_ticket_line_totals(conn)?;
    let payments_total_by_receipt = load_payment_totals(conn)?;
    let ticket_rows: Vec<(String, i64, String)> = {
        let mut stmt = conn.prepare(
            "SELECT t.ID, t.TICKETID, r.DATENEW FROM tickets t JOIN receipts r ON r.ID = t.ID ORDER BY t.TICKETID",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };

    let mut payment_mismatches = Vec::new();
    for (receipt_id, ticket_id, date) in ticket_rows {
        let lines_total = lines_total_by_ticket
            .get(&receipt_id)
            .copied()
            .unwrap_or(Decimal::ZERO);
        let payments_total = payments_total_by_receipt
            .get(&receipt_id)
            .copied()
            .unwrap_or(Decimal::ZERO);
        if lines_total != payments_total {
            payment_mismatches.push(PaymentMismatch {
                receipt_id,
                ticket_id,
                date,
                lines_total: lines_total.to_string(),
                payments_total: payments_total.to_string(),
                difference: (lines_total - payments_total).to_string(),
            });
        }
    }

    // Las diferencias grandes son las que importan; las de 1e-12 son ruido
    // de coma flotante del respaldo y no deben ocupar las primeras líneas.
    payment_mismatches.sort_by(|a, b| {
        let da = Decimal::from_str(&a.difference).unwrap_or_default().abs();
        let db = Decimal::from_str(&b.difference).unwrap_or_default().abs();
        db.cmp(&da)
    });

    let (stock_drift, negative_stock_products) = check_stock(conn)?;
    let orphan_counts = check_orphans(conn)?;
    let odd_codes = load_odd_codes(conn)?;

    let lines_without_product: u64 = conn.query_row(
        "SELECT COUNT(*) FROM ticketlines WHERE PRODUCT IS NULL",
        [],
        |r| r.get(0),
    )?;
    let scale_products: u64 =
        conn.query_row("SELECT COUNT(*) FROM products WHERE ISSCALE = 1", [], |r| {
            r.get(0)
        })?;

    let integrity_ok: bool = conn
        .query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
        .map(|s| s == "ok")
        .unwrap_or(false);

    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    if !integrity_ok {
        errors.push("PRAGMA integrity_check no devolvió 'ok'".to_string());
    }
    for (name, count) in &orphan_counts {
        if *count > 0 {
            errors.push(format!("claves huérfanas en {name}: {count}"));
        }
    }

    if open_sessions > 0 {
        warnings.push(format!(
            "{open_sessions} sesiones abiertas (se conservan)"
        ));
    }
    if payment_mismatches.len() as u64 > 0 {
        warnings.push(format!(
            "{} receipts históricos con pagos distintos a la suma de líneas (se conservan)",
            payment_mismatches.len()
        ));
    }
    if !stock_drift.is_empty() {
        warnings.push(format!(
            "{} productos con deriva de stock por coma flotante histórica (se conservan)",
            stock_drift.len()
        ));
    }
    if tickets_without_taxlines > 0 {
        warnings.push(format!(
            "{tickets_without_taxlines} tickets históricos sin taxlines"
        ));
    }
    if cash_movement_receipts > 0 {
        warnings.push(format!(
            "{cash_movement_receipts} receipts sin ticket (cashin/cashout históricos)"
        ));
    }
    if !odd_codes.is_empty() {
        warnings.push(format!(
            "{} productos con código no numérico o con espacios (se conservan)",
            odd_codes.len()
        ));
    }
    if lines_without_product > 0 {
        warnings.push(format!(
            "{lines_without_product} líneas históricas sin producto (ajustes de redondeo)"
        ));
    }
    if negative_stock_products > 0 {
        warnings.push(format!(
            "{negative_stock_products} productos con stock negativo (permitido)"
        ));
    }

    Ok(VerificationReport {
        counts,
        max_ticket_id,
        ticket_sequence,
        max_session_sequence,
        open_sessions,
        cash_movement_receipts,
        tickets_without_taxlines,
        payment_mismatch_count: payment_mismatches.len() as u64,
        payment_mismatches,
        stock_drift_count: stock_drift.len() as u64,
        stock_drift,
        orphan_counts,
        odd_codes,
        lines_without_product,
        scale_products,
        negative_stock_products,
        integrity_ok,
        errors,
        warnings,
    })
}

fn parse_decimal(s: &str) -> Result<Decimal, StoreError> {
    Decimal::from_str(s).map_err(|e| {
        StoreError::Verification(format!("decimal inválido en la base: {s:?} ({e})"))
    })
}

fn load_ticket_line_totals(
    conn: &Connection,
) -> Result<HashMap<String, Decimal>, StoreError> {
    let mut stmt = conn.prepare("SELECT TICKET, PRICE, UNITS FROM ticketlines")?;
    let mut rows = stmt.query([])?;
    let mut map: HashMap<String, Decimal> = HashMap::new();
    while let Some(row) = rows.next()? {
        let ticket: String = row.get(0)?;
        let price = parse_decimal(&row.get::<_, String>(1)?)?;
        let units = parse_decimal(&row.get::<_, String>(2)?)?;
        *map.entry(ticket).or_insert(Decimal::ZERO) += price * units;
    }
    Ok(map)
}

fn load_payment_totals(conn: &Connection) -> Result<HashMap<String, Decimal>, StoreError> {
    let mut stmt = conn.prepare("SELECT RECEIPT, TOTAL FROM payments")?;
    let mut rows = stmt.query([])?;
    let mut map: HashMap<String, Decimal> = HashMap::new();
    while let Some(row) = rows.next()? {
        let receipt: String = row.get(0)?;
        let total = parse_decimal(&row.get::<_, String>(1)?)?;
        *map.entry(receipt).or_insert(Decimal::ZERO) += total;
    }
    Ok(map)
}

fn check_stock(conn: &Connection) -> Result<(Vec<StockDrift>, u64), StoreError> {
    #[derive(Hash, PartialEq, Eq, Clone)]
    struct Key {
        location: String,
        product: String,
        attributes: Option<String>,
    }

    let mut current: HashMap<Key, Decimal> = HashMap::new();
    let mut negative = 0u64;
    {
        let mut stmt =
            conn.prepare("SELECT LOCATION, PRODUCT, ATTRIBUTESETINSTANCE_ID, UNITS FROM stockcurrent")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let units = parse_decimal(&row.get::<_, String>(3)?)?;
            if units < Decimal::ZERO {
                negative += 1;
            }
            current.insert(
                Key {
                    location: row.get(0)?,
                    product: row.get(1)?,
                    attributes: row.get(2)?,
                },
                units,
            );
        }
    }

    let mut diary: HashMap<Key, Decimal> = HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT LOCATION, PRODUCT, ATTRIBUTESETINSTANCE_ID, UNITS FROM stockdiary",
        )?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let units = parse_decimal(&row.get::<_, String>(3)?)?;
            *diary
                .entry(Key {
                    location: row.get(0)?,
                    product: row.get(1)?,
                    attributes: row.get(2)?,
                })
                .or_insert(Decimal::ZERO) += units;
        }
    }

    let epsilon = Decimal::new(1, 9); // 0.000000001
    let mut drift = Vec::new();
    let mut keys: Vec<&Key> = current.keys().collect();
    keys.sort_by(|a, b| {
        (&a.location, &a.product, &a.attributes).cmp(&(&b.location, &b.product, &b.attributes))
    });
    for key in keys {
        let c = current.get(key).copied().unwrap_or(Decimal::ZERO);
        let d = diary.get(key).copied().unwrap_or(Decimal::ZERO);
        let diff = c - d;
        if diff.abs() > epsilon {
            drift.push(StockDrift {
                location: key.location.clone(),
                product: key.product.clone(),
                attributes: key.attributes.clone(),
                current: c.to_string(),
                diary_sum: d.to_string(),
                difference: diff.to_string(),
            });
        }
    }
    for key in diary.keys() {
        if !current.contains_key(key) {
            let d = diary.get(key).copied().unwrap_or(Decimal::ZERO);
            drift.push(StockDrift {
                location: key.location.clone(),
                product: key.product.clone(),
                attributes: key.attributes.clone(),
                current: "0".to_string(),
                diary_sum: d.to_string(),
                difference: (-d).to_string(),
            });
        }
    }
    Ok((drift, negative))
}

fn check_orphans(conn: &Connection) -> Result<BTreeMap<String, u64>, StoreError> {
    let checks = [
        (
            "ticketlines.product",
            "SELECT COUNT(*) FROM ticketlines x WHERE x.PRODUCT IS NOT NULL AND NOT EXISTS (SELECT 1 FROM products p WHERE p.ID = x.PRODUCT)",
        ),
        (
            "ticketlines.ticket",
            "SELECT COUNT(*) FROM ticketlines x WHERE NOT EXISTS (SELECT 1 FROM tickets t WHERE t.ID = x.TICKET)",
        ),
        (
            "ticketlines.taxid",
            "SELECT COUNT(*) FROM ticketlines x WHERE NOT EXISTS (SELECT 1 FROM taxes t WHERE t.ID = x.TAXID)",
        ),
        (
            "payments.receipt",
            "SELECT COUNT(*) FROM payments x WHERE NOT EXISTS (SELECT 1 FROM receipts r WHERE r.ID = x.RECEIPT)",
        ),
        (
            "taxlines.receipt",
            "SELECT COUNT(*) FROM taxlines x WHERE NOT EXISTS (SELECT 1 FROM receipts r WHERE r.ID = x.RECEIPT)",
        ),
        (
            "taxlines.taxid",
            "SELECT COUNT(*) FROM taxlines x WHERE NOT EXISTS (SELECT 1 FROM taxes t WHERE t.ID = x.TAXID)",
        ),
        (
            "receipts.money",
            "SELECT COUNT(*) FROM receipts x WHERE NOT EXISTS (SELECT 1 FROM closedcash c WHERE c.MONEY = x.MONEY)",
        ),
        (
            "tickets.receipt",
            "SELECT COUNT(*) FROM tickets x WHERE NOT EXISTS (SELECT 1 FROM receipts r WHERE r.ID = x.ID)",
        ),
        (
            "tickets.person",
            "SELECT COUNT(*) FROM tickets x WHERE NOT EXISTS (SELECT 1 FROM people p WHERE p.ID = x.PERSON)",
        ),
        (
            "tickets.customer",
            "SELECT COUNT(*) FROM tickets x WHERE x.CUSTOMER IS NOT NULL AND NOT EXISTS (SELECT 1 FROM customers c WHERE c.ID = x.CUSTOMER)",
        ),
        (
            "products.category",
            "SELECT COUNT(*) FROM products x WHERE NOT EXISTS (SELECT 1 FROM categories c WHERE c.ID = x.CATEGORY)",
        ),
        (
            "products.taxcat",
            "SELECT COUNT(*) FROM products x WHERE NOT EXISTS (SELECT 1 FROM taxcategories c WHERE c.ID = x.TAXCAT)",
        ),
        (
            "stockcurrent.product",
            "SELECT COUNT(*) FROM stockcurrent x WHERE NOT EXISTS (SELECT 1 FROM products p WHERE p.ID = x.PRODUCT)",
        ),
        (
            "stockdiary.product",
            "SELECT COUNT(*) FROM stockdiary x WHERE NOT EXISTS (SELECT 1 FROM products p WHERE p.ID = x.PRODUCT)",
        ),
        (
            "people.role",
            "SELECT COUNT(*) FROM people x WHERE NOT EXISTS (SELECT 1 FROM roles r WHERE r.ID = x.ROLE)",
        ),
        (
            "taxes.category",
            "SELECT COUNT(*) FROM taxes x WHERE NOT EXISTS (SELECT 1 FROM taxcategories c WHERE c.ID = x.CATEGORY)",
        ),
    ];

    let mut out = BTreeMap::new();
    for (name, sql) in checks {
        let count: u64 = conn.query_row(sql, [], |r| r.get(0))?;
        out.insert(name.to_string(), count);
    }
    Ok(out)
}

fn load_odd_codes(conn: &Connection) -> Result<Vec<OddCode>, StoreError> {
    let mut stmt = conn.prepare("SELECT ID, CODE, NAME FROM products ORDER BY NAME")?;
    let rows = stmt.query_map([], |r| {
        Ok(OddCode {
            product_id: r.get(0)?,
            code: r.get(1)?,
            name: r.get(2)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        let item = row?;
        if item.code.is_empty() || item.code.chars().any(|c| !c.is_ascii_digit()) {
            out.push(item);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::apply_schema;

    #[test]
    fn empty_db_has_no_orphans_and_integrity_ok() {
        let conn = Connection::open_in_memory().unwrap();
        apply_schema(&conn).unwrap();
        let report = verify_connection(&conn).unwrap();
        assert!(report.integrity_ok);
        assert!(report.orphan_counts.values().all(|&v| v == 0));
        assert_eq!(report.counts["products"], 0);
    }
}
