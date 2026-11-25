use pos_core::{Capability, CategoryId, Money, ProductId, SaleUnit, TaxCategoryId, UserId};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use uuid::Uuid;

use crate::{auth::require_capability, catalog::{normalize_code, parse_decimal}, StoreError};

pub struct NewProduct {
    pub reference: String,
    pub code: String,
    pub name: String,
    pub category: CategoryId,
    pub tax: TaxCategoryId,
    pub price: Money,
    pub cost: Money,
    pub unit: SaleUnit,
    pub in_catalog: bool,
}

/// Guarda el alta y su visibilidad en una sola transacción, sin tocar
/// productos importados ni simular un ingreso de mercancía.
pub fn create_product(
    conn: &mut Connection, product: &NewProduct, actor: &UserId, now: &str,
) -> Result<ProductId, StoreError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    require_capability(&tx, actor.as_str(), Capability::Backoffice)?;
    let id = insert_product(&tx, product, actor, now, || Uuid::new_v4().to_string())?;
    tx.commit()?;
    Ok(id)
}

pub(crate) fn insert_product(
    conn: &Connection, product: &NewProduct, actor: &UserId, now: &str,
    mut next_id: impl FnMut() -> String,
) -> Result<ProductId, StoreError> {
    let reference = required(&product.reference, 128, "referencia")?;
    let code = required(&product.code, 128, "código de barras")?;
    let name = required(&product.name, 128, "nombre")?;
    validate_values(conn, product)?;
    check_duplicates(conn, None, [Some(code), Some(reference), Some(name)])?;
    let id = allocate_product_id(conn, &mut next_id)?;
    conn.execute(
        "INSERT INTO products
         (ID, REFERENCE, CODE, CODETYPE, NAME, PRICEBUY, PRICESELL, CATEGORY, TAXCAT,
          STOCKCOST, STOCKVOLUME, ISCOM, ISSCALE, ISCONSTANT, PRINTKB, SENDSTATUS, ISSERVICE,
          ISVPRICE, ISVERPATRIB, WARRANTY, STOCKUNITS, PRINTTO, UOM,
          PRICESELL1, PRICESELL2, PRICESELL3, QTY1, QTY2, QTY3)
         VALUES (?1, ?2, ?3, 'EAN-13', ?4, ?5, ?6, ?7, ?8,
                 '0','0',0,?9,0,0,0,0,0,0,0,'0','1','0','0','0','0','0','0','0')",
        params![id.as_str(), reference, code, name, product.cost.to_plain_string(),
            product.price.to_plain_string(), product.category.as_str(), product.tax.as_str(),
            i64::from(product.unit.is_weighted())],
    )?;
    if product.in_catalog {
        conn.execute("INSERT INTO products_cat (PRODUCT, CATORDER) VALUES (?1,NULL)", [id.as_str()])?;
    }
    conn.execute(
        "INSERT INTO pos_product_flags (PRODUCT_ID, ACTIVE, UPDATED_AT) VALUES (?1, 1, ?2)",
        params![id.as_str(), now],
    )?;
    crate::audit(conn, now, Some(actor.as_str()), "PRODUCT_CREATED", id.as_str())?;
    Ok(id)
}

fn allocate_product_id(conn: &Connection, next: &mut impl FnMut() -> String) -> Result<ProductId, StoreError> {
    let mut exists = conn.prepare_cached(
        "SELECT EXISTS(SELECT 1 FROM products WHERE ID=?1)
             OR EXISTS(SELECT 1 FROM pos_product_flags WHERE PRODUCT_ID=?1)
             OR EXISTS(SELECT 1 FROM products_cat WHERE PRODUCT=?1)")?;
    for _ in 0..8 {
        let id = next();
        if !exists.query_row([&id], |row| row.get::<_, bool>(0))? {
            return Ok(ProductId::new(id));
        }
    }
    Err(invalid("no se pudo generar un ID de producto único; no se guardó el alta"))
}

/// Actualiza solo la ficha operativa; las ventas y el stock conservan sus IDs
/// y valores. Los campos históricos no editados conservan incluso su texto.
pub fn update_product(
    conn: &mut Connection, id: &ProductId, product: &NewProduct, actor: &UserId, now: &str,
) -> Result<(), StoreError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    require_capability(&tx, actor.as_str(), Capability::Backoffice)?;
    let (code, reference, name, old_price, old_cost, scale, in_catalog) = tx.query_row(
        "SELECT p.CODE, p.REFERENCE, p.NAME, p.PRICESELL, p.PRICEBUY, p.ISSCALE,
                EXISTS(SELECT 1 FROM products_cat g WHERE g.PRODUCT=p.ID)
         FROM products p LEFT JOIN pos_product_flags f ON f.PRODUCT_ID=p.ID
         WHERE p.ID=?1 AND COALESCE(f.ACTIVE,1)=1",
        [id.as_str()], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?,
            r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?,
            r.get::<_, bool>(5)?, r.get::<_, bool>(6)?)),
    ).optional()?.ok_or_else(|| invalid("el producto ya no está disponible"))?;
    validate_values(&tx, product)?;
    let changed = |next: &str, previous: &str, label| {
        if next == previous { Ok(None) }
        else { required(next, 128, label).map(|v| Some(v.to_owned())) }
    };
    let next_code = changed(&product.code, &code, "código de barras")?;
    let next_ref = changed(&product.reference, reference.as_deref().unwrap_or_default(), "referencia")?;
    let next_name = changed(&product.name, &name, "nombre")?;
    check_duplicates(&tx, Some(id.as_str()), [next_code.as_deref(), next_ref.as_deref(), next_name.as_deref()])?;
    let next_scale = product.unit.is_weighted();
    if scale != next_scale {
        let used: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM ticketlines WHERE PRODUCT=?1)
                 OR EXISTS(SELECT 1 FROM stockcurrent WHERE PRODUCT=?1)
                 OR EXISTS(SELECT 1 FROM stockdiary WHERE PRODUCT=?1)",
            [id.as_str()], |r| r.get(0),
        )?;
        if used { return Err(invalid("no se puede cambiar la unidad de un producto con ventas o inventario")); }
    }
    let amount = |input: Money, old: String, column: &str| -> Result<String, StoreError> {
        Ok(if Money::from_decimal(parse_decimal(&old, column)?) == input { old } else { input.to_plain_string() })
    };
    tx.execute(
        "UPDATE products SET CODE=COALESCE(?2,CODE), REFERENCE=COALESCE(?3,REFERENCE),
             NAME=COALESCE(?4,NAME), PRICESELL=?5, PRICEBUY=?6, CATEGORY=?7, TAXCAT=?8,
             ISSCALE=?9 WHERE ID=?1",
        params![id.as_str(), next_code, next_ref, next_name,
            amount(product.price, old_price, "products.PRICESELL")?,
            amount(product.cost, old_cost, "products.PRICEBUY")?,
            product.category.as_str(), product.tax.as_str(), next_scale],
    )?;
    match (in_catalog, product.in_catalog) {
        (false, true) => {
            tx.execute("INSERT INTO products_cat (PRODUCT,CATORDER) VALUES (?1,NULL)", [id.as_str()])?;
        }
        (true, false) => { tx.execute("DELETE FROM products_cat WHERE PRODUCT=?1", [id.as_str()])?; }
        _ => {}
    }
    crate::audit(&tx, now, Some(actor.as_str()), "PRODUCT_UPDATED", id.as_str())?;
    tx.commit()?;
    Ok(())
}

fn validate_values(conn: &Connection, product: &NewProduct) -> Result<(), StoreError> {
    if product.price.is_negative() || product.cost.is_negative() {
        return Err(invalid("el precio y el costo no pueden ser negativos"));
    }
    let category_exists: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM categories WHERE ID=?1)",
        [product.category.as_str()], |row| row.get(0))?;
    if !category_exists { return Err(invalid("selecciona una categoría válida")); }
    let tax_exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM taxcategories c JOIN taxes t ON t.CATEGORY=c.ID WHERE c.ID=?1)",
        [product.tax.as_str()], |row| row.get(0))?;
    if !tax_exists { return Err(invalid("selecciona un impuesto válido")); }
    Ok(())
}

fn check_duplicates(conn: &Connection, ignore: Option<&str>, fields: [Option<&str>; 3]) -> Result<(), StoreError> {
    if fields.iter().all(Option::is_none) { return Ok(()); }
    let normalized = fields.map(|field| field.map(normalize_code));
    // El lector compara sin espacios exteriores ni mayúsculas.
    let mut stmt = conn.prepare("SELECT CODE, REFERENCE, NAME FROM products WHERE ?1 IS NULL OR ID<>?1")?;
    let mut rows = stmt.query([ignore])?;
    while let Some(row) = rows.next()? {
        for (idx, label) in ["código de barras", "referencia", "nombre"].into_iter().enumerate() {
            if let Some(norm) = &normalized[idx] {
                if row.get::<_, Option<String>>(idx)?.is_some_and(|old| normalize_code(&old) == *norm) {
                    return Err(invalid(&format!("ya existe un producto con ese {label}")));
                }
            }
        }
    }
    Ok(())
}

fn required<'a>(text: &'a str, max: usize, label: &str) -> Result<&'a str, StoreError> {
    let text = text.trim();
    if text.is_empty() || text.chars().count() > max || text.chars().any(char::is_control) {
        return Err(invalid(&format!("{label}: escribe entre 1 y {max} caracteres válidos")));
    }
    Ok(text)
}

fn invalid(message: &str) -> StoreError {
    StoreError::InvalidArgument(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alta_rechaza_duplicados_y_actores_sin_permiso_sin_escrituras_parciales() {
        let mut db = Connection::open_in_memory().unwrap();
        crate::apply_schema(&db).unwrap();
        db.execute_batch("
            INSERT INTO people (ID, NAME, ROLE, VISIBLE) VALUES ('0','Admin','0',1),('2','Empleado','2',1);
            INSERT INTO categories (ID, NAME) VALUES ('cat','Alimentos');
            INSERT INTO taxcategories (ID, NAME) VALUES ('100','Sin IVA');
            INSERT INTO taxes (ID, NAME, CATEGORY, RATE) VALUES ('100','Sin IVA','100','0');
            INSERT INTO products (ID, CODE, REFERENCE, NAME) VALUES ('legacy','aBC ','L1','Importado');
        ").unwrap();
        let mut product = NewProduct {
            reference: "  N1 ".into(), code: " ABC".into(), name: "Café molido".into(),
            category: "cat".into(), tax: "100".into(),
            price: Money::from_cents(2_380_050), cost: Money::from_pesos(12_000),
            unit: SaleUnit::Kilogram, in_catalog: true,
        };
        let now = "2026-11-24 09:00:00";
        assert!(matches!(create_product(&mut db, &product, &"2".into(), now),
            Err(StoreError::PermissionDenied(Capability::Backoffice))));
        assert!(create_product(&mut db, &product, &"0".into(), now).unwrap_err().to_string().contains("ese código"));
        product.code = "123456789".into();
        product.tax = "no-existe".into();
        assert!(create_product(&mut db, &product, &"0".into(), now).is_err());
        let writes: i64 = db.query_row(
            "SELECT (SELECT COUNT(*) FROM pos_audit) + (SELECT COUNT(*) FROM products_cat)
                  + (SELECT COUNT(*) FROM pos_product_flags) + (SELECT COUNT(*) FROM products)",
            [], |row| row.get(0),
        ).unwrap();
        assert_eq!(writes, 1, "solo existe el producto previo");
        product.tax = "100".into();
        let tx = db.transaction().unwrap();
        assert!(insert_product(&tx, &product, &"0".into(), now, || "legacy".into()).is_err());
        let mut candidates = ["legacy", "nuevo-id"].into_iter();
        let id = insert_product(&tx, &product, &"0".into(), now, || candidates.next().unwrap().into()).unwrap();
        tx.commit().unwrap();
        assert_eq!(id.as_str(), "nuevo-id");
        let stored = db.query_row(
            "SELECT p.PRICESELL, p.ISSCALE, p.REFERENCE FROM products p JOIN products_cat g ON g.PRODUCT=p.ID
             JOIN pos_product_flags f ON f.PRODUCT_ID=p.ID AND f.ACTIVE=1 WHERE p.ID=?1",
            [id.as_str()], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?)),
        ).unwrap();
        assert_eq!(stored, ("23800.5".into(), 1, "N1".into()));
        let original: String = db.query_row("SELECT CODE FROM products WHERE ID='legacy'", [], |r| r.get(0)).unwrap();
        assert_eq!(original, "aBC ");
    }

    #[test]
    fn editar_ficha_conserva_identidad_orden_y_datos_historicos() {
        let mut db = Connection::open_in_memory().unwrap();
        crate::apply_schema(&db).unwrap();
        db.execute_batch("
            INSERT INTO people (ID, NAME, ROLE, VISIBLE) VALUES ('0','Admin','0',1),('2','Empleado','2',1);
            INSERT INTO categories (ID, NAME) VALUES ('cat','Alimentos');
            INSERT INTO taxcategories (ID, NAME) VALUES ('100','Sin IVA');
            INSERT INTO taxes (ID, NAME, CATEGORY, RATE) VALUES ('100','Sin IVA','100','0');
            INSERT INTO products (ID, CODE, REFERENCE, NAME, CATEGORY, TAXCAT, PRICEBUY, PRICESELL, ISSCALE)
                VALUES ('old','aBC ','L1','Importado  ','cat','100','100.000','250.000',0),
                       ('other','BCD','L2','Otro','cat','100','100','250',0);
            INSERT INTO products_cat (PRODUCT, CATORDER) VALUES ('old',7);
            INSERT INTO ticketlines (TICKET, LINE, PRODUCT, UNITS, PRICE, TAXID)
                VALUES ('historic',0,'old','2','250','100');
            INSERT INTO stockcurrent (LOCATION, PRODUCT, UNITS) VALUES ('0','old','12.005');
        ").unwrap();
        let mut update = NewProduct {
            reference: "L1".into(), code: "aBC ".into(), name: "Importado  ".into(),
            category: "cat".into(), tax: "100".into(), price: Money::from_pesos(300),
            cost: Money::from_pesos(100), unit: SaleUnit::Unit, in_catalog: true,
        };
        let id = "old".into();
        let now = "2026-11-24 09:00:00";
        assert!(matches!(update_product(&mut db, &id, &update, &"2".into(), now),
            Err(StoreError::PermissionDenied(Capability::Backoffice))));
        update.code = " bcd ".into();
        assert!(update_product(&mut db, &id, &update, &"0".into(), now).is_err());
        update.code = "aBC ".into();
        update.unit = SaleUnit::Kilogram;
        assert!(update_product(&mut db, &id, &update, &"0".into(), now).is_err());
        update.unit = SaleUnit::Unit;
        update_product(&mut db, &id, &update, &"0".into(), now).unwrap();
        let result: (String, String, String, String, i64, String, String) = db.query_row(
            "SELECT p.CODE,p.NAME,p.PRICEBUY,p.PRICESELL,g.CATORDER,l.PRICE,s.UNITS
             FROM products p JOIN products_cat g ON g.PRODUCT=p.ID
             JOIN ticketlines l ON l.PRODUCT=p.ID JOIN stockcurrent s ON s.PRODUCT=p.ID
             WHERE p.ID='old'",
            [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?)),
        ).unwrap();
        assert_eq!(result, ("aBC ".into(), "Importado  ".into(), "100.000".into(), "300".into(),
            7, "250".into(), "12.005".into()));
    }
}
