//! Persistencia de una venta, de las líneas borradas y de los movimientos de
//! efectivo. Todo se escribe en una transacción SQLite.

use std::collections::BTreeMap;
use std::str::FromStr;

use pos_core::{
    sale_stock_movements, Capability, Location, Money, Payment, PaymentMethod, Payments, Qty, Rate,
    RemovedLine, SaleLineKind, StockMovement, StockReason, TaxCategoryId, TicketDraft, User,
};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::error::StoreError;
use crate::properties::properties_xml;
use crate::{auth::require_capability, sessions::require_current_session};

#[derive(Clone, Copy, Debug)]
pub struct SaleContext<'a> {
    pub session_money: &'a str,
    pub user: &'a User,
    pub location: &'a Location,
    pub now: &'a str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaleResult {
    /// `tickets.ID` y `receipts.ID` son el mismo valor.
    pub ticket_id: String,
    pub ticket_number: i64,
    pub total: Money,
    pub change: Money,
}

/// Guarda una venta completa: recibo, ticket, líneas, impuestos, pagos, stock,
/// apertura de cajón y auditoría.
pub fn save_sale(
    conn: &mut Connection,
    draft: &TicketDraft,
    payments: &Payments,
    ctx: SaleContext<'_>,
) -> Result<SaleResult, StoreError> {
    save_sale_with_drawer(conn, draft, payments, ctx, payments.has_cash())
}

/// `drawer` indica que se solicitará un pulso tras confirmar la transacción.
pub fn save_sale_with_drawer(
    conn: &mut Connection, draft: &TicketDraft, payments: &Payments,
    ctx: SaleContext<'_>, drawer: bool,
) -> Result<SaleResult, StoreError> {
    for payment in payments.entries() {
        if matches!(payment.method, PaymentMethod::Other(_)) {
            return Err(StoreError::InvalidArgument("medio de pago no admitido".to_owned()));
        }
        // Payment tiene campos públicos: validar también al confirmar.
        Payment::new(payment.method.clone(), payment.amount, payment.tendered, payment.reference.clone())?;
    }
    let checkout = draft.checkout(payments)?;
    let mut cash_due = checkout.totals.total - checkout.payments.non_cash_paid;

    let ticket_id = Uuid::new_v4().to_string();
    let movements = sale_stock_movements(
        &checkout,
        &ctx.location.id,
        &ctx.user.id,
        None,
    )?;

    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    require_current_session(&tx, ctx.session_money, ctx.now)?;
    require_capability(&tx, ctx.user.id.as_str(), Capability::Sell)?;
    if draft.lines().iter().any(|line| line.original_unit_price_net().is_some()) {
        require_capability(&tx, ctx.user.id.as_str(), Capability::ChangePrice)?;
    }
    for line in &checkout.lines {
        let capability = match line.kind {
            SaleLineKind::LineDiscount => Capability::LineDiscount,
            SaleLineKind::TicketDiscount => Capability::TicketDiscount,
            _ => continue,
        };
        require_capability(&tx, ctx.user.id.as_str(), capability)?;
    }
    let mut taxes = BTreeMap::new();
    for line in &checkout.lines {
        if !taxes.contains_key(&line.tax_category_id) {
            taxes.insert(
                line.tax_category_id.clone(),
                resolve_tax_id(&tx, &line.tax_category_id, line.tax_rate)?,
            );
        }
    }

    let current: i64 = tx
        .query_row("SELECT ID FROM ticketsnum LIMIT 1", [], |r| r.get(0))
        .optional()?
        .ok_or_else(|| {
            StoreError::InvalidArgument("ticketsnum está vacío; no se puede numerar".to_owned())
        })?;
    let ticket_number = current.checked_add(1)
        .ok_or_else(|| StoreError::InvalidArgument("consecutivo agotado".to_owned()))?;
    if tx.execute("UPDATE ticketsnum SET ID = ?1", [ticket_number])? != 1 {
        return Err(StoreError::InvalidArgument("ticketsnum debe tener una fila".to_owned()));
    }

    let mut receipt_attributes = std::collections::BTreeMap::new();
    receipt_attributes.insert("pos.user".to_owned(), ctx.user.id.to_string());
    receipt_attributes.insert("pos.ticketid".to_owned(), ticket_number.to_string());
    receipt_attributes.insert(
        "pos.total".to_owned(),
        checkout.totals.total.to_plain_string(),
    );
    receipt_attributes.insert(
        "pos.change".to_owned(),
        checkout.payments.change.to_plain_string(),
    );

    tx.execute(
        "INSERT INTO receipts (ID, MONEY, DATENEW, ATTRIBUTES, PERSON)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            ticket_id,
            ctx.session_money,
            ctx.now,
            properties_xml(&receipt_attributes),
            ctx.user.id.as_str(),
        ],
    )?;

    tx.execute(
        "INSERT INTO tickets (ID, TICKETTYPE, TICKETID, PERSON, CUSTOMER, STATUS)
         VALUES (?1, 0, ?2, ?3, NULL, 0)",
        params![ticket_id, ticket_number, ctx.user.id.as_str()],
    )?;

    for (index, line) in checkout.lines.iter().enumerate() {
        let mut attributes = line.attributes.clone();
        attributes
            .entry("product.name".to_owned())
            .or_insert_with(|| line.name.clone());
        attributes.insert(
            "product.taxcategoryid".to_owned(),
            line.tax_category_id.to_string(),
        );
        if let Some(code) = &line.code {
            attributes
                .entry("product.code".to_owned())
                .or_insert_with(|| code.clone());
        }

        tx.execute(
            "INSERT INTO ticketlines
                (TICKET, LINE, PRODUCT, ATTRIBUTESETINSTANCE_ID, UNITS, PRICE, TAXID, ATTRIBUTES)
             VALUES (?1, ?2, ?3, NULL, ?4, ?5, ?6, ?7)",
            params![
                ticket_id,
                index as i64,
                line.product_id.as_ref().map(|id| id.as_str()),
                line.quantity.to_plain_string(),
                line.unit_price_net.to_plain_string(),
                taxes[&line.tax_category_id],
                properties_xml(&attributes),
            ],
        )?;
    }

    for tax in &checkout.totals.tax_breakdown {
        tx.execute(
            "INSERT INTO taxlines (ID, RECEIPT, TAXID, BASE, AMOUNT)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                Uuid::new_v4().to_string(),
                ticket_id,
                taxes[&tax.tax_category_id],
                tax.base.to_plain_string(),
                tax.tax.to_plain_string(),
            ],
        )?;
    }

    for payment in payments.entries() {
        let applied = if payment.is_cash() {
            let applied = payment.amount.min(cash_due);
            cash_due -= applied;
            applied
        } else {
            payment.amount
        };
        tx.execute(
            "INSERT INTO payments
                (ID, RECEIPT, PAYMENT, TOTAL, TIP, TRANSID, ISPROCESSED,
                 RETURNMSG, NOTES, TENDERED, CARDNAME, VOUCHER)
              VALUES (?1, ?2, ?3, ?4, '0', NULL, 0, NULL, ?5, ?6, NULL, NULL)",
            params![
                Uuid::new_v4().to_string(),
                ticket_id,
                payment.method.code(),
                applied.to_plain_string(),
                payment.reference.as_deref(),
                payment
                    .tendered
                    .unwrap_or(payment.amount)
                    .to_plain_string(),
            ],
        )?;
    }

    for movement in &movements {
        write_stock(&tx, movement, ctx.now)?;
    }

    if drawer && !payments.is_empty() {
        tx.execute(
            "INSERT INTO draweropened (OPENDATE, NAME, TICKETID) VALUES (?1, ?2, ?3)",
            params![
                ctx.now,
                format!("{} - {}", ctx.user.name, ticket_number),
                ticket_number.to_string(),
            ],
        )?;
        // Nombre de evento del esquema v1: apertura vinculada a un ticket,
        // independientemente del medio de pago usado.
        record_event(&tx, ctx.session_money, "cash_sale")?;
    }

    audit(
        &tx,
        ctx.now,
        Some(ctx.user.id.as_str()),
        "SALE",
        &format!(
            "ticket={ticket_number};total={};change={};lines={}",
            checkout.totals.total.to_plain_string(),
            checkout.payments.change.to_plain_string(),
            checkout.lines.len()
        ),
    )?;

    tx.commit()?;

    Ok(SaleResult {
        ticket_id,
        ticket_number,
        total: checkout.totals.total,
        change: checkout.payments.change,
    })
}

fn resolve_tax_id(conn: &Connection, category: &TaxCategoryId, rate: Rate) -> Result<String, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT ID, RATE FROM taxes
         WHERE CATEGORY = ?1 AND CUSTCATEGORY IS NULL AND PARENTID IS NULL",
    )?;
    let mut rows = stmt.query([category.as_str()])?;
    let mut id = None;
    while let Some(row) = rows.next()? {
        let value: String = row.get(1)?;
        if parse_decimal(&value)? == rate.as_decimal() {
            if id.is_some() {
                return Err(StoreError::InvalidArgument(format!("impuesto ambiguo: {category}")));
            }
            id = Some(row.get(0)?);
        }
    }
    id.ok_or_else(|| StoreError::InvalidArgument(format!("impuesto no encontrado: {category} ({rate})")))
}

pub(crate) fn write_stock(
    tx: &Connection,
    movement: &pos_core::StockMovement,
    now: &str,
) -> Result<(), StoreError> {
    let product_exists: bool = tx.query_row(
        "SELECT EXISTS (
             SELECT 1 FROM products p
             LEFT JOIN pos_product_flags f ON f.PRODUCT_ID = p.ID
             WHERE p.ID = ?1 AND COALESCE(f.ACTIVE, 1) = 1
         ) AND EXISTS (SELECT 1 FROM locations WHERE ID = ?2)",
        params![movement.product_id.as_str(), movement.location_id.as_str()],
        |row| row.get(0),
    )?;
    if !product_exists {
        return Err(StoreError::InvalidArgument("producto o bodega no disponible".to_owned()));
    }
    tx.execute(
        "INSERT INTO stockdiary
            (ID, DATENEW, REASON, LOCATION, PRODUCT, ATTRIBUTESETINSTANCE_ID,
             UNITS, PRICE, APPUSER, SUPPLIER, SUPPLIERDOC)
         VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6, ?7, ?8, NULL, NULL)",
        params![
            Uuid::new_v4().to_string(),
            now,
            movement.reason.as_i32(),
            movement.location_id.as_str(),
            movement.product_id.as_str(),
            movement.quantity_delta.to_plain_string(),
            movement.unit_cost.to_plain_string(),
            movement.user_id.as_str(),
        ],
    )?;
    let current_units: Option<String> = tx
            .query_row(
                "SELECT UNITS FROM stockcurrent
                 WHERE LOCATION = ?1 AND PRODUCT = ?2
                   AND ATTRIBUTESETINSTANCE_ID IS NULL",
                params![movement.location_id.as_str(), movement.product_id.as_str()],
                |r| r.get(0),
            )
            .optional()?;
        let current_quantity = current_units.as_deref().map(parse_decimal).transpose()?;
        let new_quantity = (current_quantity.unwrap_or_default() + movement.quantity_delta.as_decimal())
            .normalize().to_string();

        if current_quantity.is_some() {
            let changed = tx.execute(
                "UPDATE stockcurrent SET UNITS = ?1
                 WHERE LOCATION = ?2 AND PRODUCT = ?3
                   AND ATTRIBUTESETINSTANCE_ID IS NULL",
                params![
                    new_quantity,
                    movement.location_id.as_str(),
                    movement.product_id.as_str(),
                ],
            )?;
            if changed != 1 {
                return Err(StoreError::Verification("stockcurrent contiene filas duplicadas".to_owned()));
            }
        } else {
            tx.execute(
                "INSERT INTO stockcurrent
                    (LOCATION, PRODUCT, ATTRIBUTESETINSTANCE_ID, UNITS)
                 VALUES (?1, ?2, NULL, ?3)",
                params![
                    movement.location_id.as_str(),
                    movement.product_id.as_str(),
                    new_quantity,
                ],
            )?;
        }
    Ok(())
}

/// Registra una entrada de inventario y conserva la precisión del saldo previo.
pub fn record_stock_entry(
    conn: &mut Connection,
    movement: &StockMovement,
    now: &str,
) -> Result<(), StoreError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    require_capability(&tx, movement.user_id.as_str(), Capability::Backoffice)?;
    insert_stock_entry(&tx, movement, now)?;
    tx.commit()?;
    Ok(())
}

pub(crate) fn insert_stock_entry(
    conn: &Connection, movement: &StockMovement, now: &str,
) -> Result<(), StoreError> {
    if movement.reason != StockReason::Entry {
        return Err(StoreError::InvalidArgument("se esperaba una entrada de inventario".to_owned()));
    }
    let movement = StockMovement::entry(
        movement.product_id.clone(), movement.location_id.clone(), movement.quantity_delta,
        movement.unit_cost, movement.user_id.clone(), movement.reference.clone(),
    )?;
    let is_scale: Option<i64> = conn.query_row(
        "SELECT ISSCALE FROM products WHERE ID = ?1", [movement.product_id.as_str()], |row| row.get(0),
    ).optional()?;
    if is_scale == Some(0) && movement.quantity_delta.as_decimal().fract() != Qty::ZERO.as_decimal() {
        return Err(pos_core::CoreError::FractionalUnitQuantity.into());
    }
    write_stock(conn, &movement, now)?;
    audit(conn, now, Some(movement.user_id.as_str()), "STOCK_ENTRY", &format!(
        "product={};units={};ref={}", movement.product_id, movement.quantity_delta.to_plain_string(),
        movement.reference.as_deref().unwrap_or_default()
    ))?;
    Ok(())
}

/// Registra una línea que el cajero quitó antes de cobrar. Nunca se escribe un
/// ticket: igual que el histórico, queda con `TICKETID='Void'`.
pub fn record_removed_line(
    conn: &mut Connection,
    session_money: &str,
    line: &RemovedLine,
    user: &User,
    now: &str,
) -> Result<(), StoreError> {
    record_removed_lines(conn, session_money, std::slice::from_ref(line), user, now)
}

/// Cancelar un borrador registra todas las líneas o ninguna.
pub fn record_removed_lines(
    conn: &mut Connection,
    session_money: &str,
    lines: &[RemovedLine],
    user: &User,
    now: &str,
) -> Result<(), StoreError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    require_current_session(&tx, session_money, now)?;
    require_capability(&tx, user.id.as_str(), Capability::RemoveLine)?;
    for line in lines {
        tx.execute(
            "INSERT INTO lineremoved
                (REMOVEDDATE, NAME, TICKETID, PRODUCTID, PRODUCTNAME, UNITS)
             VALUES (?1, ?2, 'Void', ?3, ?4, ?5)",
            params![
                now,
                user.name,
                line.product_id.as_str(),
                line.name,
                line.quantity.to_plain_string(),
            ],
        )?;
        record_event(&tx, session_money, "removed")?;
    }
    tx.commit()?;
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CashDirection {
    In,
    Out,
}

impl CashDirection {
    pub const fn code(self) -> &'static str {
        match self {
            Self::In => "cashin",
            Self::Out => "cashout",
        }
    }

    const fn sign(self) -> i64 {
        match self {
            Self::In => 1,
            Self::Out => -1,
        }
    }
}

/// Entrada o salida de efectivo: un recibo sin ticket y un pago `cashin` o
/// `cashout`, tal como los 324 movimientos históricos.
pub fn record_cash_movement(
    conn: &mut Connection,
    session_money: &str,
    user: &User,
    direction: CashDirection,
    amount: Money,
    reason: &str,
    now: &str,
) -> Result<String, StoreError> {
    if !amount.is_positive() {
        return Err(StoreError::InvalidArgument(
            "el movimiento de caja debe ser mayor que cero".to_owned(),
        ));
    }
    let receipt_id = Uuid::new_v4().to_string();
    let signed = Money::from_decimal(amount.as_decimal() * Decimal::from(direction.sign()));

    let mut attributes = std::collections::BTreeMap::new();
    attributes.insert(
        "pos.cash_movement".to_owned(),
        direction.code().to_owned(),
    );
    attributes.insert("pos.reason".to_owned(), reason.to_owned());
    attributes.insert("pos.user".to_owned(), user.id.to_string());

    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    require_current_session(&tx, session_money, now)?;
    require_capability(&tx, user.id.as_str(), Capability::CashMovement)?;
    tx.execute(
        "INSERT INTO receipts (ID, MONEY, DATENEW, ATTRIBUTES, PERSON)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            receipt_id,
            session_money,
            now,
            properties_xml(&attributes),
            user.id.as_str(),
        ],
    )?;
    tx.execute(
        "INSERT INTO payments
            (ID, RECEIPT, PAYMENT, TOTAL, TIP, TRANSID, ISPROCESSED,
             RETURNMSG, NOTES, TENDERED, CARDNAME, VOUCHER)
         VALUES (?1, ?2, ?3, ?4, '0', NULL, 0, NULL, ?5, ?6, NULL, NULL)",
        params![
            Uuid::new_v4().to_string(),
            receipt_id,
            direction.code(),
            signed.to_plain_string(),
            reason,
            signed.to_plain_string(),
        ],
    )?;
    audit(
        &tx,
        now,
        Some(user.id.as_str()),
        direction.code(),
        &format!("receipt={receipt_id};amount={};reason={reason}", amount.to_plain_string()),
    )?;
    tx.commit()?;
    Ok(receipt_id)
}

/// Apertura manual del cajón sin venta.
pub fn record_drawer_opening(
    conn: &mut Connection,
    session_money: &str,
    user: &User,
    now: &str,
) -> Result<(), StoreError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    require_current_session(&tx, session_money, now)?;
    require_capability(&tx, user.id.as_str(), Capability::OpenDrawer)?;
    tx.execute(
        "INSERT INTO draweropened (OPENDATE, NAME, TICKETID) VALUES (?1, ?2, 'No Sale')",
        params![now, user.name],
    )?;
    record_event(&tx, session_money, "no_sale")?;
    audit(&tx, now, Some(user.id.as_str()), "OPEN_DRAWER", "manual")?;
    tx.commit()?;
    Ok(())
}

pub fn audit(
    conn: &Connection,
    now: &str,
    user_id: Option<&str>,
    action: &str,
    detail: &str,
) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO pos_audit (TS, USER_ID, ACTION, DETAIL) VALUES (?1, ?2, ?3, ?4)",
        params![now, user_id, action, detail],
    )?;
    Ok(())
}

pub(crate) fn record_event(conn: &Connection, session: &str, kind: &str) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO pos_session_events (MONEY, KIND) VALUES (?1, ?2)",
        params![session, kind],
    )?;
    Ok(())
}

fn parse_decimal(value: &str) -> Result<Decimal, StoreError> {
    Decimal::from_str(value.trim())
        .map_err(|e| StoreError::Verification(format!("decimal inválido {value:?}: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::apply_schema;
    use pos_core::{
        CategoryId, PriceMode, Product, ProductId, SaleUnit,
    };

    fn user() -> User {
        User {
            id: pos_core::UserId::new("0"),
            name: "Administrador".to_owned(),
            role: pos_core::RoleKind::Admin,
            active: true,
        }
    }

    fn location() -> Location {
        Location {
            id: pos_core::LocationId::new("0"),
            name: "General".to_owned(),
        }
    }

    fn product() -> Product {
        Product {
            id: ProductId::new("p1"),
            code: "001".to_owned(),
            reference: "REF-1".to_owned(),
            name: "Café".to_owned(),
            category_id: Some(CategoryId::new("c1")),
            tax_category_id: TaxCategoryId::new("100"),
            tax_rate: Rate::ZERO,
            list_price: Money::from_pesos(4_600),
            cost: Money::from_pesos(2_000),
            unit: SaleUnit::Unit,
            active: true,
            stock: None,
        }
    }

    fn database() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        apply_schema(&conn).unwrap();
        conn.execute(
            "INSERT INTO people (ID, NAME, ROLE, VISIBLE) VALUES ('0','Administrador','0',1), ('2','Empleado','2',1)",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO taxcategories (ID, NAME) VALUES ('100', 'Sin IVA')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO taxes (ID, NAME, CATEGORY, RATE) VALUES ('t100', 'Sin IVA', '100', '0')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO locations (ID, NAME) VALUES ('0', 'General')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO products (ID, REFERENCE, CODE, NAME, PRICEBUY, PRICESELL, CATEGORY, TAXCAT, ISSCALE)
             VALUES ('p1', 'REF-1', '001', 'Café', '2000', '4600', 'c1', '100', 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO stockcurrent (LOCATION, PRODUCT, ATTRIBUTESETINSTANCE_ID, UNITS)
             VALUES ('0', 'p1', NULL, '10')",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO ticketsnum (ID) VALUES (23005)", [])
            .unwrap();
        conn.execute(
            "INSERT INTO closedcash (MONEY, HOST, HOSTSEQUENCE, DATESTART, DATEEND, NOSALES)
             VALUES ('s1', 'Terminal prueba', 597, '2026-11-24 08:00:00', NULL, 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO pos_session_cash (MONEY, OPENED_BY, OPENING_FLOAT)
             VALUES ('s1', '0', '0')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO pos_meta (KEY, VALUE) VALUES ('current_session', 's1')",
            [],
        )
        .unwrap();
        conn
    }

    fn sale() -> (TicketDraft, Payments) {
        let mut draft = TicketDraft::new();
        draft
            .add_product(&product(), Qty::from_units(2), PriceMode::TaxIncluded)
            .unwrap();
        let mut payments = Payments::new();
        payments.add(
            Payment::cash(Money::from_pesos(9_200), Money::from_pesos(10_000)).unwrap(),
        );
        (draft, payments)
    }

    #[test]
    fn cambio_de_precio_exige_permiso_y_no_modifica_el_catalogo() {
        let mut conn = database();
        let (mut draft, _) = sale();
        let key = draft.lines()[0].key();
        draft.edit_line(key, Qty::from_units(2), Some(Money::from_pesos(4_100))).unwrap();
        let mut payments = Payments::new();
        payments.add(Payment::cash(Money::from_pesos(8_200), Money::from_pesos(10_000)).unwrap());
        let mut actor = user();
        actor.id = "2".into(); // El rol del parámetro no reemplaza el persistido.
        let location = location();
        assert!(matches!(save_sale(&mut conn, &draft, &payments, SaleContext {
            session_money: "s1", user: &actor, location: &location, now: "2026-11-24 09:00:00",
        }), Err(StoreError::PermissionDenied(Capability::ChangePrice))));
        assert_eq!(conn.query_row("SELECT ID FROM ticketsnum", [], |r| r.get::<_, i64>(0)).unwrap(), 23005);
        let result = save_sale(&mut conn, &draft, &payments, SaleContext {
            session_money: "s1", user: &user(), location: &location, now: "2026-11-24 09:00:00",
        }).unwrap();
        assert_eq!(result.total, Money::from_pesos(8_200));
        let (actual, attrs, list): (String, Vec<u8>, String) = conn.query_row(
            "SELECT l.PRICE,l.ATTRIBUTES,p.PRICESELL FROM ticketlines l JOIN products p ON p.ID=l.PRODUCT",
            [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        ).unwrap();
        assert_eq!(actual, "4100");
        assert!(String::from_utf8(attrs).unwrap().contains(r#"<entry key="pos.price.original_net">4600</entry>"#));
        assert_eq!(list, "4600");
    }

    #[test]
    fn guarda_venta_completa_y_resta_stock() {
        let mut conn = database();
        let (draft, payments) = sale();
        let result = save_sale(
            &mut conn,
            &draft,
            &payments,
            SaleContext {
                session_money: "s1",
                user: &user(),
                location: &location(),
                now: "2026-11-24 09:00:00",
            },
        )
        .unwrap();

        assert_eq!(result.ticket_number, 23006);
        assert_eq!(result.total, Money::from_pesos(9_200));
        assert_eq!(result.change, Money::from_pesos(800));

        let next: i64 = conn
            .query_row("SELECT ID FROM ticketsnum", [], |r| r.get(0))
            .unwrap();
        assert_eq!(next, 23006);
        let tickets: i64 = conn
            .query_row("SELECT COUNT(*) FROM tickets", [], |r| r.get(0))
            .unwrap();
        assert_eq!(tickets, 1);
        let lines: i64 = conn
            .query_row("SELECT COUNT(*) FROM ticketlines", [], |r| r.get(0))
            .unwrap();
        assert_eq!(lines, 1);
        let payments_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM payments", [], |r| r.get(0))
            .unwrap();
        assert_eq!(payments_count, 1);
        let tendered: String = conn
            .query_row("SELECT TENDERED FROM payments", [], |r| r.get(0))
            .unwrap();
        assert_eq!(tendered, "10000");
        let stock: String = conn
            .query_row("SELECT UNITS FROM stockcurrent WHERE PRODUCT = 'p1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(stock, "8");
        let diary: i64 = conn
            .query_row("SELECT COUNT(*) FROM stockdiary", [], |r| r.get(0))
            .unwrap();
        assert_eq!(diary, 1);
        let drawer: i64 = conn
            .query_row("SELECT COUNT(*) FROM draweropened", [], |r| r.get(0))
            .unwrap();
        assert_eq!(drawer, 1);
        let tax_ids: (String, String) = conn
            .query_row("SELECT (SELECT TAXID FROM ticketlines), (SELECT TAXID FROM taxlines)", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(tax_ids, ("t100".to_owned(), "t100".to_owned()));
    }

    #[test]
    fn pago_mixto_registra_el_importe_aplicado_y_no_el_billete_como_venta() {
        let mut conn = database();
        let (draft, _) = sale();
        let mut payments = Payments::new();
        payments.add(Payment::electronic(PaymentMethod::Nequi, Money::from_pesos(4_200), Some("ref".into())).unwrap());
        payments.add(Payment::cash(Money::from_pesos(10_000), Money::from_pesos(10_000)).unwrap());
        let result = save_sale(
            &mut conn, &draft, &payments,
            SaleContext { session_money: "s1", user: &user(), location: &location(), now: "2026-11-24 09:00:00" },
        ).unwrap();
        assert_eq!(result.change, Money::from_pesos(5_000));
        let cash: (String, String) = conn.query_row(
            "SELECT TOTAL, TENDERED FROM payments WHERE PAYMENT = 'cash'", [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        ).unwrap();
        assert_eq!(cash, ("5000".to_owned(), "10000".to_owned()));
        let summary = crate::session_totals(&conn, "s1").unwrap();
        assert_eq!(summary.expected_cash(), Money::from_pesos(5_000));
        assert_eq!(summary.sales_difference(), Money::ZERO);
    }

    #[test]
    fn protege_cajas_historicas_y_verifica_el_rol_guardado() {
        let mut conn = database();
        conn.execute(
            "INSERT INTO closedcash (MONEY, HOST, HOSTSEQUENCE, DATESTART) VALUES ('legacy','Terminal prueba',596,'2025-01-01 00:00:00')",
            [],
        ).unwrap();
        let now = "2026-11-24 09:00:00";
        let fake_admin = User { id: "2".into(), ..user() };
        assert!(crate::close_session(&mut conn, "legacy", Money::ZERO, &user(), now).is_err());
        assert!(matches!(
            crate::close_session(&mut conn, "s1", Money::ZERO, &fake_admin, now),
            Err(StoreError::PermissionDenied(Capability::CloseSession))
        ));
        let (draft, payments) = sale();
        assert!(save_sale(
            &mut conn, &draft, &payments,
            SaleContext { session_money: "legacy", user: &user(), location: &location(), now },
        ).is_err());
        assert_eq!(conn.query_row("SELECT ID FROM ticketsnum", [], |r| r.get::<_, i64>(0)).unwrap(), 23005);
        assert!(crate::session_info(&conn, "legacy").unwrap().dateend.is_none());
    }

    #[test]
    fn apertura_manual_y_cierre_en_el_mismo_segundo_no_contaminan_la_caja_siguiente() {
        let mut conn = database();
        let now = "2026-11-24 10:00:00";
        record_drawer_opening(&mut conn, "s1", &user(), now).unwrap();
        let close = crate::close_session(&mut conn, "s1", Money::ZERO, &user(), now).unwrap();
        let next = crate::ensure_session(&mut conn, "0", now, Money::ZERO).unwrap();
        let totals = crate::session_totals(&conn, &next.money).unwrap();
        assert_eq!(close.totals.no_sale_count, 1);
        assert_eq!(close.totals.drawer_openings, 2);
        assert_eq!(totals.no_sale_count, 0);
        assert_eq!(totals.drawer_openings, 0);
    }

    #[test]
    fn primera_apertura_continua_la_numeracion_del_terminal_importado_mas_reciente() {
        let mut conn = database();
        conn.execute_batch(
            "DELETE FROM pos_meta WHERE KEY IN ('current_session', 'current_host');
             UPDATE closedcash SET HOST = 'Tienda Norte', HOSTSEQUENCE = 76 WHERE MONEY = 's1';
             INSERT INTO closedcash (MONEY, HOST, HOSTSEQUENCE, DATESTART)
                VALUES ('otro','Otro terminal',999,'2025-01-01 00:00:00')",
        ).unwrap();
        let session = crate::ensure_session(&mut conn, "0", "2026-11-24 10:00:00", Money::ZERO).unwrap();
        assert_eq!((&*session.host, session.sequence), ("Tienda Norte", 77));
        assert!(crate::session_info(&conn, "s1").unwrap().dateend.is_none());
    }

    #[test]
    fn vender_no_redondea_el_saldo_importado() {
        let mut conn = database();
        conn.execute("UPDATE stockcurrent SET UNITS = '0.1234567'", []).unwrap();
        let (draft, payments) = sale();
        save_sale(&mut conn, &draft, &payments, SaleContext {
            session_money: "s1", user: &user(), location: &location(), now: "2026-11-24 09:00:00",
        }).unwrap();
        let stock: String = conn.query_row("SELECT UNITS FROM stockcurrent", [], |r| r.get(0)).unwrap();
        assert_eq!(stock, "-1.8765433");
    }

    #[test]
    fn registra_movimiento_de_caja_sin_ticket() {
        let mut conn = database();
        let receipt = record_cash_movement(
            &mut conn,
            "s1",
            &user(),
            CashDirection::Out,
            Money::from_pesos(5_000),
            "Compra de bolsas",
            "2026-11-24 09:10:00",
        )
        .unwrap();
        let tickets: i64 = conn
            .query_row("SELECT COUNT(*) FROM tickets", [], |r| r.get(0))
            .unwrap();
        assert_eq!(tickets, 0);
        let total: String = conn
            .query_row(
                "SELECT TOTAL FROM payments WHERE RECEIPT = ?1",
                [&receipt],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(total, "-5000");
    }

    #[test]
    fn cierre_calcula_efectivo_esperado_y_diferencia() {
        let mut conn = database();
        let (draft, payments) = sale();
        save_sale(
            &mut conn,
            &draft,
            &payments,
            SaleContext {
                session_money: "s1",
                user: &user(),
                location: &location(),
                now: "2026-11-24 09:00:00",
            },
        )
        .unwrap();

        let totals = crate::sessions::session_totals(&conn, "s1").unwrap();
        assert_eq!(totals.sales_total, Money::from_pesos(9_200));
        assert_eq!(totals.cash_sales(), Money::from_pesos(9_200));
        assert_eq!(totals.expected_cash(), Money::from_pesos(9_200));

        let close = crate::sessions::close_session(
            &mut conn,
            "s1",
            Money::from_pesos(9_000),
            &user(),
            "2026-11-24 18:00:00",
        )
        .unwrap();
        assert_eq!(close.difference, Money::from_pesos(-200));
    }
}
