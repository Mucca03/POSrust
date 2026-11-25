//! Consulta de tickets confirmados; los cobros históricos nunca se reconstruyen
//! como si hubieran coincidido con las líneas.

use std::str::FromStr;

use pos_core::receipt::{Receipt, ReceiptLine, ReceiptPayment};
use pos_core::{Money, PaymentMethod, Qty, SaleUnit};
use rusqlite::{Connection, OptionalExtension};
use rust_decimal::Decimal;

use crate::properties::read_properties;
use crate::StoreError;

pub fn last_sale(conn: &Connection) -> Result<Option<Receipt>, StoreError> {
    let number = conn.query_row(
        "SELECT TICKETID FROM tickets WHERE TICKETTYPE = 0 ORDER BY TICKETID DESC LIMIT 1",
        [], |row| row.get(0),
    ).optional()?;
    match number {
        Some(number) => ticket_by_number(conn, number),
        None => Ok(None),
    }
}

pub fn ticket_by_number(conn: &Connection, number: i64) -> Result<Option<Receipt>, StoreError> {
    let header: Option<(String, String, String, Option<Vec<u8>>)> = conn.query_row(
        "SELECT t.ID, r.DATENEW, COALESCE(p.NAME, t.PERSON, ''), r.ATTRIBUTES
         FROM tickets t JOIN receipts r ON r.ID = t.ID
         LEFT JOIN people p ON p.ID = t.PERSON
         WHERE t.TICKETID = ?1 AND t.TICKETTYPE = 0",
        [number], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    ).optional()?;
    let Some((id, date, cashier, attrs)) = header else { return Ok(None) };
    let attrs = read_properties(attrs.as_deref().unwrap_or_default());
    let is_pos = attrs.contains_key("pos.ticketid");
    let mut lines = Vec::new();
    let mut net = Decimal::ZERO;
    let mut stmt = conn.prepare(
        "SELECT l.UNITS, l.PRICE, l.ATTRIBUTES, COALESCE(p.NAME, l.PRODUCT, 'Ajuste'),
                COALESCE(p.ISSCALE, 0)
         FROM ticketlines l LEFT JOIN products p ON p.ID = l.PRODUCT
         WHERE l.TICKET = ?1 ORDER BY l.LINE",
    )?;
    let mut rows = stmt.query([&id])?;
    while let Some(row) = rows.next()? {
        let qty = decimal(&row.get::<_, String>(0)?)?;
        let price = decimal(&row.get::<_, String>(1)?)?;
        let raw_attributes: Option<Vec<u8>> = row.get(2)?;
        let mut attributes = read_properties(raw_attributes.as_deref().unwrap_or_default());
        let quantity = Qty::from_decimal(qty);
        let (amount, raw) = if is_pos {
            let amount = Money::from_decimal(price) * quantity;
            (amount, amount.as_decimal())
        } else {
            let raw = price * qty;
            (Money::from_decimal(raw), raw)
        };
        net += raw;
        let unit = match attributes.get("product.unit").map(String::as_str) {
            Some("kg") | Some("Kilogram") => SaleUnit::Kilogram,
            Some("unit" | "ud" | "Unit") => SaleUnit::Unit,
            _ if row.get::<_, i64>(4)? == 1 || !qty.fract().is_zero() => SaleUnit::Kilogram,
            _ => SaleUnit::Unit,
        };
        let name = attributes.remove("product.name").unwrap_or(row.get(3)?);
        lines.push(ReceiptLine {
            name, quantity, unit, amount, unit_price_net: Money::from_decimal(price),
            code: attributes.remove("product.code").filter(|code| !code.trim().is_empty()),
            note: attributes.remove("pos.note"),
        });
    }
    let mut taxes = conn.prepare("SELECT AMOUNT FROM taxlines WHERE RECEIPT = ?1")?;
    let tax_rows = taxes.query_map([&id], |row| row.get::<_, String>(0))?;
    let mut tax = Decimal::ZERO;
    for row in tax_rows {
        tax += decimal(&row?)?;
    }
    let mut stmt = conn.prepare(
        "SELECT PAYMENT, TOTAL, TENDERED, COALESCE(NOTES, TRANSID)
         FROM payments WHERE RECEIPT = ?1 ORDER BY rowid",
    )?;
    let mut rows = stmt.query([&id])?;
    let mut collected = Decimal::ZERO;
    let mut payments = Vec::new();
    while let Some(row) = rows.next()? {
        let method = PaymentMethod::from_code(&row.get::<_, String>(0)?);
        let amount = decimal(&row.get::<_, String>(1)?)?;
        collected += amount;
        let tendered = if method.is_cash() {
            row.get::<_, Option<String>>(2)?.as_deref().map(decimal).transpose()?
                .map(Money::from_decimal)
        } else {
            None
        };
        payments.push(ReceiptPayment {
            method, amount: Money::from_decimal(amount), tendered, reference: row.get(3)?,
        });
    }

    let change = attrs.get("pos.change").map(|s| decimal(s)).transpose()?
        .map(Money::from_decimal).unwrap_or(Money::ZERO);
    Ok(Some(Receipt {
        id, number, date, cashier, lines, payments,
        net: Money::from_decimal(net),
        tax: Money::from_decimal(tax),
        total: Money::from_decimal(net + tax),
        collected: Money::from_decimal(collected),
        change,
    }))
}

fn decimal(value: &str) -> Result<Decimal, StoreError> {
    Decimal::from_str(value.trim()).map_err(|e| {
        StoreError::Verification(format!("decimal inválido en el ticket ({value:?}): {e}"))
    })
}
