//! Sesiones de caja (`closedcash`) y arqueo.
//!
//! La sesión nueva continúa la numeración histórica: con el respaldo actual la
//! primera sesión del POS es la 598. Las 16 sesiones antiguas abiertas se dejan
//! intactas.

use std::collections::BTreeMap;
use std::str::FromStr;

use pos_core::{Capability, Money, PaymentMethod, PaymentTotal, Qty, SessionClose, SessionTotals, User};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::error::StoreError;
use crate::sales::{audit, record_event};
use crate::auth::require_capability;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionInfo {
    pub money: String,
    pub host: String,
    pub sequence: i64,
    pub datestart: String,
    pub dateend: Option<String>,
    pub opening_float: Money,
}

/// Sesión marcada como actual en `pos_meta`, si existe.
pub fn current_session(conn: &Connection) -> Result<Option<SessionInfo>, StoreError> {
    let money: Option<String> = conn
        .query_row(
            "SELECT VALUE FROM pos_meta WHERE KEY = 'current_session'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    match money {
        Some(money) => Ok(Some(session_info(conn, &money)?)),
        None => Ok(None),
    }
}

pub(crate) fn require_current_session(
    conn: &Connection,
    money: &str,
    now: &str,
) -> Result<SessionInfo, StoreError> {
    let current = current_session(conn)?;
    let owned: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pos_session_cash WHERE MONEY = ?1)",
        [money],
        |row| row.get(0),
    )?;
    current
        .filter(|s| owned && s.money == money && s.dateend.is_none() && now >= s.datestart.as_str())
        .ok_or_else(|| StoreError::InvalidArgument("se requiere una sesión POS vigente".to_owned()))
}

pub fn session_info(conn: &Connection, money: &str) -> Result<SessionInfo, StoreError> {
    conn.query_row(
        "SELECT c.MONEY, c.HOST, c.HOSTSEQUENCE, c.DATESTART, c.DATEEND,
                COALESCE(s.OPENING_FLOAT, '0')
         FROM closedcash c
         LEFT JOIN pos_session_cash s ON s.MONEY = c.MONEY
         WHERE c.MONEY = ?1",
        [money],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, String>(5)?,
            ))
        },
    )
    .optional()?
    .map(|(money, host, sequence, datestart, dateend, opening)| {
        Ok::<SessionInfo, StoreError>(SessionInfo {
            money,
            host,
            sequence,
            datestart,
            dateend,
            opening_float: Money::from_decimal(parse_decimal(
                &opening,
                "pos_session_cash.OPENING_FLOAT",
            )?),
        })
    })
    .transpose()?
    .ok_or_else(|| StoreError::InvalidArgument(format!("sesión inexistente: {money}")))
}

/// Devuelve la sesión abierta actual o crea la siguiente.
pub fn ensure_session(
    conn: &mut Connection,
    user_id: &str,
    now: &str,
    opening_float: Money,
) -> Result<SessionInfo, StoreError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    require_capability(&tx, user_id, Capability::Sell)?;

    if opening_float.is_negative() {
        return Err(StoreError::InvalidArgument("el fondo inicial no puede ser negativo".to_owned()));
    }
    if let Some(current) = current_session(&tx)? {
        if current.dateend.is_none() {
            return require_current_session(&tx, &current.money, now);
        }
    }

    let host: Option<String> = tx
        .query_row(
            "SELECT COALESCE(
                (SELECT VALUE FROM pos_settings WHERE KEY = 'host' AND TRIM(VALUE) <> ''),
                (SELECT VALUE FROM pos_meta WHERE KEY = 'current_host' AND TRIM(VALUE) <> ''),
                (SELECT HOST FROM closedcash WHERE TRIM(HOST) <> ''
                 ORDER BY DATESTART DESC, HOSTSEQUENCE DESC, MONEY DESC LIMIT 1)
             )",
            [],
            |r| r.get(0),
        )?;
    let host = host.unwrap_or_else(|| "Caja".to_owned());
    let sequence: i64 = tx.query_row(
        "SELECT COALESCE(MAX(HOSTSEQUENCE), 0) + 1 FROM closedcash WHERE HOST = ?1",
        [&host],
        |r| r.get(0),
    )?;
    let money = Uuid::new_v4().to_string();

    tx.execute(
        "INSERT INTO closedcash (MONEY, HOST, HOSTSEQUENCE, DATESTART, DATEEND, NOSALES)
         VALUES (?1, ?2, ?3, ?4, NULL, 0)",
        params![money, host, sequence, now],
    )?;
    tx.execute(
        "INSERT INTO pos_session_cash (MONEY, OPENED_BY, OPENING_FLOAT)
         VALUES (?1, ?2, ?3)",
        params![money, user_id, opening_float.to_plain_string()],
    )?;
    tx.execute(
        "INSERT OR REPLACE INTO pos_meta (KEY, VALUE) VALUES ('current_session', ?1)",
        [&money],
    )?;
    tx.execute(
        "INSERT OR REPLACE INTO pos_meta (KEY, VALUE) VALUES ('current_host', ?1)",
        [&host],
    )?;
    audit(
        &tx,
        now,
        Some(user_id),
        "OPEN_SESSION",
        &format!(
            "session={money};host={host};sequence={sequence};float={}",
            opening_float.to_plain_string()
        ),
    )?;
    tx.commit()?;

    Ok(SessionInfo {
        money,
        host,
        sequence,
        datestart: now.to_owned(),
        dateend: None,
        opening_float,
    })
}

/// Calcula los totales de una sesión a partir de los recibos y sus tickets.
#[derive(Debug)]
pub struct SessionTax {
    pub category: String,
    pub base: Money,
    pub tax: Money,
}

pub fn session_taxes(conn: &Connection, money: &str) -> Result<Vec<SessionTax>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT COALESCE(c.ID, tl.TAXID, ''), COALESCE(c.NAME, t.NAME, 'Sin categoría'), tl.BASE, tl.AMOUNT
         FROM receipts r JOIN tickets tk ON tk.ID=r.ID JOIN taxlines tl ON tl.RECEIPT=r.ID
         LEFT JOIN taxes t ON t.ID=tl.TAXID LEFT JOIN taxcategories c ON c.ID=t.CATEGORY
         WHERE r.MONEY=?1 ORDER BY 2,1")?;
    let mut rows = stmt.query([money])?;
    let mut summary = BTreeMap::new();
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        let category: String = row.get(1)?;
        let base: String = row.get(2)?;
        let tax: String = row.get(3)?;
        let entry = summary.entry((category.clone(), id)).or_insert(SessionTax {
            category, base: Money::ZERO, tax: Money::ZERO,
        });
        entry.base += Money::from_decimal(parse_decimal(&base, "taxlines.BASE")?);
        entry.tax += Money::from_decimal(parse_decimal(&tax, "taxlines.AMOUNT")?);
    }
    Ok(summary.into_values().collect())
}

pub fn session_totals(conn: &Connection, money: &str) -> Result<SessionTotals, StoreError> {
    let info = session_info(conn, money)?;
    let end = info.dateend.clone();

    let opening_float = info.opening_float;
    let ticket_count: i64 = conn.query_row(
        "SELECT COUNT(*)
         FROM tickets t
         JOIN receipts r ON r.ID = t.ID
         WHERE r.MONEY = ?1",
        [money],
        |r| r.get(0),
    )?;

    let mut sales_net = Money::ZERO;
    {
        let mut stmt = conn.prepare(
            "SELECT l.UNITS, l.PRICE
             FROM ticketlines l
             JOIN tickets t ON t.ID = l.TICKET
             JOIN receipts r ON r.ID = t.ID
             WHERE r.MONEY = ?1",
        )?;
        let rows = stmt.query_map([money], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (units, price) = row?;
            let units = Qty::from_decimal(parse_decimal(&units, "ticketlines.UNITS")?);
            let price = Money::from_decimal(parse_decimal(&price, "ticketlines.PRICE")?);
            sales_net += price * units;
        }
    }

    let mut sales_tax = Money::ZERO;
    {
        let mut stmt = conn.prepare(
            "SELECT tl.AMOUNT
             FROM taxlines tl
             JOIN tickets t ON t.ID = tl.RECEIPT
             JOIN receipts r ON r.ID = t.ID
             WHERE r.MONEY = ?1",
        )?;
        let rows = stmt.query_map([money], |r| r.get::<_, String>(0))?;
        for row in rows {
            sales_tax += Money::from_decimal(parse_decimal(&row?, "taxlines.AMOUNT")?);
        }
    }

    let mut payments = BTreeMap::<PaymentMethod, Money>::new();
    let mut cash_in = Money::ZERO;
    let mut cash_out = Money::ZERO;
    {
        let mut stmt = conn.prepare(
            "SELECT p.PAYMENT, p.TOTAL
             FROM payments p
             JOIN receipts r ON r.ID = p.RECEIPT
             WHERE r.MONEY = ?1",
        )?;
        let rows = stmt.query_map([money], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (code, total) = row?;
            let amount = Money::from_decimal(parse_decimal(&total, "payments.TOTAL")?);
            match code.as_str() {
                "cashin" => cash_in += amount.abs(),
                "cashout" => cash_out += amount.abs(),
                _ => {
                    let method = PaymentMethod::from_code(&code);
                    *payments.entry(method).or_insert(Money::ZERO) += amount;
                }
            }
        }
    }

    let is_pos: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pos_session_cash WHERE MONEY = ?1)",
        [money],
        |row| row.get(0),
    )?;
    let (removed_lines, drawer_openings, no_sale_count): (i64, i64, i64) = if is_pos {
        conn.query_row(
            "SELECT COUNT(*) FILTER (WHERE KIND = 'removed'),
                    COUNT(*) FILTER (WHERE KIND <> 'removed'),
                    COUNT(*) FILTER (WHERE KIND = 'no_sale')
             FROM pos_session_events WHERE MONEY = ?1",
            [money],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?
    } else {
        // El histórico no registra un ID de sesión para estos eventos.
        let removed_lines = count_by_date(
        conn,
        "SELECT COUNT(*) FROM lineremoved
         WHERE REMOVEDDATE >= ?1 AND (?2 IS NULL OR REMOVEDDATE <= ?2)",
        &info.datestart,
        end.as_deref(),
    )?;
        let drawer_openings = count_by_date(
        conn,
        "SELECT COUNT(*) FROM draweropened
         WHERE OPENDATE >= ?1 AND (?2 IS NULL OR OPENDATE <= ?2)",
        &info.datestart,
        end.as_deref(),
    )?;
        let no_sale_count = count_by_date(
        conn,
        "SELECT COUNT(*) FROM draweropened
         WHERE OPENDATE >= ?1 AND (?2 IS NULL OR OPENDATE <= ?2)
           AND TICKETID = 'No Sale'",
        &info.datestart,
        end.as_deref(),
        )?;
        (removed_lines, drawer_openings, no_sale_count)
    };

    let totals = SessionTotals {
        opening_float,
        ticket_count: ticket_count as u32,
        sales_net,
        sales_tax,
        sales_total: sales_net + sales_tax,
        payments: payments
            .into_iter()
            .map(|(method, amount)| PaymentTotal { method, amount })
            .collect(),
        cash_in,
        cash_out,
        removed_lines: removed_lines as u32,
        drawer_openings: drawer_openings as u32,
        no_sale_count: no_sale_count as u32,
    };
    Ok(totals)
}

/// Cierra la sesión con el efectivo contado, guarda el arqueo y registra la
/// apertura de cajón de cierre. No abre la siguiente sesión.
pub fn close_session(
    conn: &mut Connection,
    money: &str,
    counted_cash: Money,
    user: &User,
    now: &str,
) -> Result<SessionClose, StoreError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    require_current_session(&tx, money, now)?;
    require_capability(&tx, user.id.as_str(), Capability::CloseSession)?;
    let latest: Option<String> = tx.query_row(
        "SELECT MAX(DATENEW) FROM receipts WHERE MONEY = ?1",
        [money],
        |r| r.get(0),
    )?;
    if latest.as_deref().is_some_and(|latest| latest > now) {
        return Err(StoreError::InvalidArgument("el cierre no puede preceder al último cobro".to_owned()));
    }
    record_event(&tx, money, "close")?;
    let totals = session_totals(&tx, money)?;
    let close = totals.close(counted_cash)?;

    tx.execute(
        "UPDATE closedcash SET DATEEND = ?1, NOSALES = ?2 WHERE MONEY = ?3",
        params![now, close.totals.no_sale_count as i64, money],
    )?;

    tx.execute(
        "UPDATE pos_session_cash
         SET CLOSED_BY = ?1, COUNTED_CASH = ?2, EXPECTED_CASH = ?3, DIFFERENCE = ?4
         WHERE MONEY = ?5",
        params![
            user.id.as_str(),
            close.counted_cash.to_plain_string(),
            close.expected_cash.to_plain_string(),
            close.difference.to_plain_string(),
            money,
        ],
    )?;
    tx.execute(
        "INSERT INTO draweropened (OPENDATE, NAME, TICKETID)
         VALUES (?1, ?2, 'Close Cash')",
        params![now, user.name],
    )?;
    tx.execute("DELETE FROM pos_meta WHERE KEY = 'current_session'", [])?;
    audit(
        &tx,
        now,
        Some(user.id.as_str()),
        "CLOSE_SESSION",
        &format!(
            "session={money};expected={};counted={};difference={}",
            close.expected_cash.to_plain_string(),
            close.counted_cash.to_plain_string(),
            close.difference.to_plain_string()
        ),
    )?;
    tx.commit()?;
    Ok(close)
}

fn count_by_date(
    conn: &Connection,
    sql: &str,
    start: &str,
    end: Option<&str>,
) -> Result<i64, StoreError> {
    let count = conn.query_row(sql, params![start, end], |r| r.get(0))?;
    Ok(count)
}

fn parse_decimal(value: &str, column: &str) -> Result<Decimal, StoreError> {
    Decimal::from_str(value.trim()).map_err(|e| {
        StoreError::Verification(format!("decimal inválido en {column}: {value:?} ({e})"))
    })
}
