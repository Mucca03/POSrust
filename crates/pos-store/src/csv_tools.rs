//! Importación operativa de lotes pequeños, separada del importador histórico.
use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

use pos_core::{Capability, LocationId, Money, Qty, SaleUnit, StockMovement, UserId};
use rusqlite::{params, Connection, TransactionBehavior};
use uuid::Uuid;

use crate::{auth::require_capability, catalog::{normalize_code, parse_decimal}, products::{insert_product, NewProduct}, sales::insert_stock_entry, StoreError};

const MAX_BYTES: u64 = 1_048_576;
pub const MAX_ROWS: usize = 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportKind { Products, Scanner, Customers }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogKind { Added, Price, Stock, Category, Missing, Duplicate, Customer, Unchanged }

impl LogKind {
    pub fn key(self) -> &'static str {
        match self {
            Self::Added => "product_added", Self::Price => "price_change", Self::Stock => "stock_entry",
            Self::Category => "invalid_category", Self::Missing => "missing_bad_values",
            Self::Duplicate => "invalid_duplicate", Self::Customer => "customer_added", Self::Unchanged => "unchanged",
        }
    }

    pub fn accepted(self) -> bool { matches!(self, Self::Added | Self::Price | Self::Stock | Self::Customer) }
}

#[derive(Debug)]
pub struct LogRow {
    pub line: usize,
    pub code: String,
    pub kind: String,
    pub detail: String,
    pub date: String,
}

#[derive(Debug)]
pub struct BatchReport {
    pub rows: Vec<LogRow>,
    pub accepted: usize,
}

pub struct Batch {
    kind: ImportKind,
    rows: Vec<(usize, BTreeMap<String, String>)>,
}

impl Batch {
    pub fn from_file(path: &Path, kind: ImportKind) -> Result<Self, StoreError> {
        if !std::fs::metadata(path)?.is_file() { return Err(invalid("selecciona un archivo CSV normal")); }
        let mut bytes = Vec::new();
        std::fs::File::open(path)?.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
        Self::from_bytes(&bytes, kind)
    }

    fn from_bytes(bytes: &[u8], kind: ImportKind) -> Result<Self, StoreError> {
        if kind == ImportKind::Scanner { return Err(invalid("usa el lector en modo teclado")); }
        if bytes.len() as u64 > MAX_BYTES { return Err(invalid("el CSV supera 1 MB")); }
        let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
        let header = bytes.split(|&b| b == b'\n').next().unwrap_or_default();
        let delimiter = if header.contains(&b';') { b';' } else { b',' };
        let mut reader = csv::ReaderBuilder::new().delimiter(delimiter).trim(csv::Trim::All).from_reader(bytes);
        let heads = reader.headers().map_err(csv_error)?.iter().map(normalize_code).collect::<Vec<_>>();
        let (required, allowed): (&[&str], &[&str]) = match kind {
            ImportKind::Products => (&["accion", "codigo"], &[
                "accion", "codigo", "referencia", "nombre", "categoria", "impuesto",
                "precio", "costo", "unidad", "catalogo", "cantidad",
            ]),
            ImportKind::Customers => (&["codigo", "nombre"], &["codigo", "nombre", "nit", "telefono", "correo"]),
            ImportKind::Scanner => unreachable!(),
        };
        if required.iter().any(|h| !heads.iter().any(|v| v == h))
            || heads.iter().enumerate().any(|(i, h)| !allowed.contains(&h.as_str()) || heads[..i].contains(h))
        {
            return Err(invalid(&format!("encabezado inválido; requerido: {}; permitido: {}",
                required.join(", "), allowed.join(", "))));
        }
        let mut rows = Vec::new();
        for record in reader.records() {
            let record = record.map_err(csv_error)?;
            if rows.len() >= MAX_ROWS { return Err(invalid("el lote supera 1.000 filas")); }
            // Número de registro lógico (la cabecera es 1), incluso si un
            // campo entre comillas incluye saltos de línea.
            let line = rows.len() + 2;
            rows.push((line, heads.iter().cloned().zip(record.iter().map(str::to_owned)).collect()));
        }
        if rows.is_empty() { return Err(invalid("el CSV no contiene registros")); }
        Ok(Self { kind, rows })
    }

    pub fn from_scans(scans: &[String]) -> Result<Self, StoreError> {
        if scans.is_empty() || scans.len() > MAX_ROWS { return Err(invalid("escanea de 1 a 1.000 unidades")); }
        let mut counts = BTreeMap::new();
        for (index, code) in scans.iter().enumerate() {
            let entry = counts.entry(normalize_code(code)).or_insert((index + 1, code.trim(), 0usize));
            entry.2 += 1;
        }
        let rows = counts.into_values().map(|(line, code, count)| (line, BTreeMap::from([
            ("accion".into(), "entrada".into()), ("codigo".into(), code.into()), ("cantidad".into(), count.to_string()),
        ]))).collect();
        Ok(Self { kind: ImportKind::Scanner, rows })
    }
}

/// Valida con las mismas reglas de escritura, usando una transacción revertida.
/// Al aplicar, cada fila válida se escribe con su auditoría; las inválidas se registran sin efectos.
pub fn process(
    conn: &mut Connection, batch: &Batch, actor: &UserId, location: &LocationId,
    now: &str, apply: bool,
) -> Result<BatchReport, StoreError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    require_capability(&tx, actor.as_str(), Capability::Backoffice)?;
    let batch_id = Uuid::new_v4().to_string();
    let mut report = BatchReport { rows: Vec::with_capacity(batch.rows.len()), accepted: 0 };
    for (line, fields) in &batch.rows {
        let code = fields.get("codigo").map_or("", String::as_str);
        tx.execute_batch("SAVEPOINT csv_row")?;
        let (kind, detail) = match row(&tx, batch.kind, fields, actor, location, now) {
            Ok(result) => result,
            Err(RowError::Invalid(kind, detail)) => {
                tx.execute_batch("ROLLBACK TO csv_row")?;
                (kind, detail)
            }
            Err(RowError::Store(error)) => return Err(error),
        };
        tx.execute_batch("RELEASE csv_row")?;
        report.accepted += usize::from(kind.accepted());
        // Nunca persiste cadenas arbitrariamente largas de una fila inválida.
        let code: String = code.chars().filter(|c| !c.is_control()).take(128).collect();
        if apply {
            tx.execute("INSERT INTO pos_csv_log (BATCH,TS,ACTOR,KIND,LINE,CODE,DETAIL) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![batch_id, now, actor.as_str(), kind.key(), *line as i64, code, detail])?;
        }
        report.rows.push(LogRow { line: *line, code, kind: kind.key().into(), detail, date: now.into() });
    }
    if apply {
        crate::audit(&tx, now, Some(actor.as_str()), "CSV_IMPORT",
            &format!("batch={batch_id};type={:?};rows={};applied={}", batch.kind, report.rows.len(), report.accepted))?;
        tx.commit()?;
    } else {
        tx.rollback()?;
    }
    Ok(report)
}

pub fn log(conn: &Connection, actor: &UserId, kind: LogKind) -> Result<Vec<LogRow>, StoreError> {
    require_capability(conn, actor.as_str(), Capability::Reports)?;
    let mut stmt = conn.prepare(
        "SELECT LINE,CODE,KIND,DETAIL,TS FROM pos_csv_log WHERE ARCHIVED=0 AND KIND=?1 ORDER BY ID DESC LIMIT 500")?;
    let rows = stmt.query_map([kind.key()], |r| Ok(LogRow {
        line: r.get(0)?, code: r.get(1)?, kind: r.get(2)?, detail: r.get(3)?, date: r.get(4)?,
    }))?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn archive_log(conn: &mut Connection, actor: &UserId, now: &str) -> Result<usize, StoreError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    require_capability(&tx, actor.as_str(), Capability::Backoffice)?;
    let count = tx.execute("UPDATE pos_csv_log SET ARCHIVED=1 WHERE ARCHIVED=0", [])?;
    crate::audit(&tx, now, Some(actor.as_str()), "CSV_LOG_ARCHIVED", &count.to_string())?;
    tx.commit()?;
    Ok(count)
}

enum RowError { Invalid(LogKind, String), Store(StoreError) }
impl From<StoreError> for RowError { fn from(error: StoreError) -> Self { Self::Store(error) } }
impl From<rusqlite::Error> for RowError { fn from(error: rusqlite::Error) -> Self { Self::Store(error.into()) } }

fn row(
    conn: &Connection, kind: ImportKind, fields: &BTreeMap<String, String>,
    actor: &UserId, location: &LocationId, now: &str,
) -> Result<(LogKind, String), RowError> {
    let get = |key| fields.get(key).map_or("", String::as_str);
    let code = text(get("codigo"), "código", 128, true)?;
    if kind == ImportKind::Customers {
        let name = text(get("nombre"), "nombre", 128, true)?;
        let tax = text(get("nit"), "NIT", 64, false)?;
        let phone = text(get("telefono"), "teléfono", 40, false)?;
        let email = text(get("correo"), "correo", 128, false)?;
        let normalized = normalize_code(code);
        let normalized_tax = normalize_code(tax);
        let mut stmt = conn.prepare("SELECT SEARCHKEY,TAXID FROM customers")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let old_code: Option<String> = row.get(0)?;
            let old_tax: Option<String> = row.get(1)?;
            if old_code.is_some_and(|v| normalize_code(&v) == normalized)
                || (!tax.is_empty() && old_tax.is_some_and(|v| normalize_code(&v) == normalized_tax)) {
                return Err(issue(LogKind::Duplicate, "ya existe un cliente con ese código o NIT"));
            }
        }
        let id = Uuid::new_v4().to_string();
        conn.execute("INSERT INTO customers (ID,SEARCHKEY,NAME,TAXID,PHONE,EMAIL,MAXDEBT,CURDEBT,VISIBLE,ISVIP,DISCOUNT)
                      VALUES (?1,?2,?3,NULLIF(?4,''),NULLIF(?5,''),NULLIF(?6,''),'0','0',1,0,'0')",
            params![id, code, name, tax, phone, email])?;
        crate::audit(conn, now, Some(actor.as_str()), "CUSTOMER_CREATED", &id)?;
        return Ok((LogKind::Customer, "Cliente creado".into()));
    }
    match normalize_code(get("accion")).as_str() {
        "alta" => {
            let category = category(conn, get("categoria"))?;
            let tax = if get("impuesto").is_empty() { "100" } else { get("impuesto") };
            let new = NewProduct {
                code: code.into(), reference: text(get("referencia"), "referencia", 128, true)?.into(),
                name: text(get("nombre"), "nombre", 128, true)?.into(), category: category.into(), tax: tax.into(),
                price: amount(get("precio"), "precio")?, cost: if get("costo").is_empty() { Money::ZERO } else { amount(get("costo"), "costo")? },
                unit: match normalize_code(get("unidad")).as_str() {
                    "" | "unidad" | "ud" => SaleUnit::Unit, "kg" => SaleUnit::Kilogram,
                    _ => return Err(issue(LogKind::Missing, "unidad: usa unidad o kg")),
                },
                in_catalog: match get("catalogo") {
                    "" | "1" => true, "0" => false, _ => return Err(issue(LogKind::Missing, "catálogo: usa 1 o 0")),
                },
            };
            if !get("cantidad").is_empty() {
                return Err(issue(LogKind::Missing, "para stock usa una fila separada con acción entrada"));
            }
            insert_product(conn, &new, actor, now, || Uuid::new_v4().to_string()).map_err(product_error)?;
            Ok((LogKind::Added, new.name))
        }
        action @ ("precio" | "entrada") => {
            let product = existing_product(conn, code)?;
            if action == "precio" {
                let new = amount(get("precio"), "precio")?;
                let old = Money::from_decimal(parse_decimal(&product.1, "products.PRICESELL")?);
                if new == old { return Ok((LogKind::Unchanged, "El precio ya coincide".into())); }
                conn.execute("UPDATE products SET PRICESELL=?2 WHERE ID=?1", params![product.0, new.to_plain_string()])?;
                let detail = format!("{} → {}", old.format_cop_exact(), new.format_cop_exact());
                crate::audit(conn, now, Some(actor.as_str()), "CSV_PRICE_CHANGED", &format!("{};{detail}", product.0))?;
                Ok((LogKind::Price, detail))
            } else {
                if kind == ImportKind::Scanner && product.3 {
                    return Err(issue(LogKind::Missing, "producto por kg: declara su peso en un CSV"));
                }
                let raw = decimal_input(get("cantidad"), "cantidad", 3)?;
                let quantity = Qty::parse(&raw).map_err(|_| issue(LogKind::Missing, "cantidad incorrecta"))?;
                if !quantity.is_positive() || (!product.3 && quantity.as_decimal().fract() != Qty::ZERO.as_decimal()) {
                    return Err(issue(LogKind::Missing, "cantidad positiva; para unidades no uses decimales"));
                }
                let cost = if get("costo").is_empty() {
                    Money::from_decimal(parse_decimal(&product.2, "products.PRICEBUY")?)
                } else { amount(get("costo"), "costo")? };
                let movement = StockMovement::entry(product.0.into(), location.clone(), quantity, cost, actor.clone(),
                    Some(if kind == ImportKind::Scanner { "Lector HID" } else { "Importación CSV" }.into()))
                    .map_err(|e| issue(LogKind::Missing, &e.to_string()))?;
                insert_stock_entry(conn, &movement, now)?;
                Ok((LogKind::Stock, format!("Entrada: {} {}", quantity.to_plain_string(),
                    if product.3 { "kg" } else { "ud" })))
            }
        }
        _ => Err(issue(LogKind::Missing, "acción inválida: usa alta, precio o entrada")),
    }
}

fn existing_product(conn: &Connection, code: &str) -> Result<(String, String, String, bool), RowError> {
    let mut stmt = conn.prepare("SELECT p.CODE,p.ID,p.PRICESELL,p.PRICEBUY,COALESCE(p.ISSCALE,0)
        FROM products p LEFT JOIN pos_product_flags f ON f.PRODUCT_ID=p.ID WHERE COALESCE(f.ACTIVE,1)=1")?;
    let mut rows = stmt.query([])?;
    let code = normalize_code(code);
    let mut found = None;
    while let Some(row) = rows.next()? {
        let existing: Option<String> = row.get(0)?;
        if existing.is_some_and(|s| normalize_code(&s) == code) {
            if found.is_some() { return Err(issue(LogKind::Duplicate, "código de barras ambiguo")); }
            found = Some((row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?));
        }
    }
    found.ok_or_else(|| issue(LogKind::Duplicate, "código de barras no encontrado"))
}

fn category(conn: &Connection, name: &str) -> Result<String, RowError> {
    if name.is_empty() { return Err(issue(LogKind::Category, "falta la categoría")); }
    let mut stmt = conn.prepare("SELECT ID, NAME FROM categories")?;
    let mut rows = stmt.query([])?;
    let key = normalize_code(name);
    let mut matches = Vec::new();
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        if id == name { return Ok(id); }
        let label: Option<String> = row.get(1)?;
        if label.is_some_and(|v| normalize_code(&v) == key) { matches.push(id); }
    }
    if matches.len() == 1 { Ok(matches.pop().unwrap()) }
    else { Err(issue(LogKind::Category, "categoría no encontrada o ambigua; usa su ID")) }
}

fn decimal_input(value: &str, label: &str, decimals: usize) -> Result<String, RowError> {
    let value = value.replace(',', ".");
    let (integer, fraction) = value.split_once('.').unwrap_or((&value, ""));
    if integer.is_empty() || integer.len() > 12 || fraction.len() > decimals
        || value.ends_with('.') || !integer.bytes().chain(fraction.bytes()).all(|b| b.is_ascii_digit())
    {
        return Err(issue(LogKind::Missing, &format!("{label}: número sin separador de miles, máx. {decimals} decimales")));
    }
    Ok(value)
}

fn amount(value: &str, label: &str) -> Result<Money, RowError> {
    Money::parse(&decimal_input(value, label, 2)?).map_err(|_| issue(LogKind::Missing, "importe inválido"))
}

fn text<'a>(s: &'a str, label: &str, max: usize, required: bool) -> Result<&'a str, RowError> {
    if (required && s.is_empty()) || s.chars().count() > max || s.chars().any(char::is_control) {
        return Err(issue(LogKind::Missing, &format!("{label}: valor requerido o inválido")));
    }
    Ok(s)
}

fn product_error(error: StoreError) -> RowError {
    match error {
        StoreError::InvalidArgument(detail) if detail.starts_with("ya existe") => RowError::Invalid(LogKind::Duplicate, detail),
        StoreError::InvalidArgument(detail) => RowError::Invalid(LogKind::Missing, detail),
        error => RowError::Store(error),
    }
}

fn issue(kind: LogKind, message: &str) -> RowError { RowError::Invalid(kind, message.into()) }
fn invalid(message: &str) -> StoreError { StoreError::InvalidArgument(message.into()) }
fn csv_error(error: csv::Error) -> StoreError { invalid(&format!("CSV inválido (usa UTF-8): {error}")) }

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: &str = "2026-11-24 09:00:00";

    fn db() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        crate::apply_schema(&db).unwrap();
        db.execute_batch("
            INSERT INTO people (ID,ROLE,VISIBLE) VALUES ('0','0',1),('2','2',1);
            INSERT INTO locations (ID,NAME) VALUES ('0','General');
            INSERT INTO categories (ID,NAME) VALUES ('c','Alimentos');
            INSERT INTO taxcategories (ID,NAME) VALUES ('100','Sin IVA');
            INSERT INTO taxes (ID,NAME,CATEGORY,RATE) VALUES ('100','Sin IVA','100','0');
        ").unwrap();
        db
    }

    fn count(db: &Connection, table: &str) -> i64 {
        db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row.get(0)).unwrap()
    }

    #[test]
    fn preview_no_guarda_y_aplicar_registra_altas_precios_stock_y_errores() {
        let mut db = db();
        let batch = Batch::from_bytes(concat!(
            "\u{feff}accion;codigo;referencia;nombre;categoria;precio;costo;unidad;cantidad\r\n",
            "alta;001;R1;\"Café; oscuro\";Alimentos;1200,50;700;kg;\r\n",
            "alta;001;R2;Duplicado;c;100;50;unidad;\r\n",
            "precio;001;;;;1300,25;;;\r\n",
            "entrada;001;;;;;;;0,850\r\n",
            "alta;002;R3;Fuera;desconocida;100;50;unidad;\r\n",
            "precio;001;;;;1.001;;;\r\n",
        ).as_bytes(), ImportKind::Products).unwrap();
        let actor = "0".into();
        let location = "0".into();
        assert!(matches!(process(&mut db, &batch, &"2".into(), &location, NOW, true),
            Err(StoreError::PermissionDenied(Capability::Backoffice))));
        let preview = process(&mut db, &batch, &actor, &location, NOW, false).unwrap();
        assert_eq!(preview.accepted, 3);
        assert_eq!(preview.rows[0].line, 2);
        for table in ["products", "stockcurrent", "stockdiary", "pos_csv_log", "pos_audit"] {
            assert_eq!(count(&db, table), 0);
        }
        let applied = process(&mut db, &batch, &actor, &location, NOW, true).unwrap();
        assert_eq!(applied.accepted, 3);
        let (name, price, qty, reason): (String, String, String, i64) = db.query_row(
            "SELECT p.NAME,p.PRICESELL,s.UNITS,d.REASON FROM products p JOIN stockcurrent s ON s.PRODUCT=p.ID
            JOIN stockdiary d ON d.PRODUCT=p.ID WHERE p.CODE='001'", [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        ).unwrap();
        assert_eq!((name, price, qty, reason), ("Café; oscuro".into(), "1300.25".into(), "0.85".into(), 1));
        for kind in [LogKind::Added, LogKind::Price, LogKind::Stock, LogKind::Category, LogKind::Duplicate, LogKind::Missing] {
            assert_eq!(log(&db, &actor, kind).unwrap().len(), 1, "{kind:?}");
        }
        assert_eq!(archive_log(&mut db, &actor, NOW).unwrap(), 6);
        assert!(log(&db, &actor, LogKind::Stock).unwrap().is_empty());
        assert_eq!(count(&db, "pos_csv_log"), 6);
        assert_eq!(count(&db, "stockdiary"), 1);
    }

    #[test]
    fn lector_agrupa_unidades_y_clientes_no_reemplaza_duplicados() {
        let mut db = db();
        db.execute_batch("
            INSERT INTO products (ID,CODE,PRICESELL,PRICEBUY,ISSCALE)
            VALUES ('one','AB','100','50',0),('kg','KG','100','50',1);
        ").unwrap();
        let actor = "0".into();
        let location = "0".into();
        let scans = Batch::from_scans(&["ab".into(), " AB ".into(), "KG".into(), "X".into()]).unwrap();
        let result = process(&mut db, &scans, &actor, &location, NOW, true).unwrap();
        assert_eq!(result.accepted, 1);
        assert_eq!(result.rows.len(), 3);
        let qty: String = db.query_row("SELECT UNITS FROM stockcurrent", [], |r| r.get(0)).unwrap();
        assert_eq!(qty, "2");
        let customers = Batch::from_bytes(b"codigo,nombre,nit\nC1,Ana,100\nc1,Otra,101\nC2,Otro,100\n",
            ImportKind::Customers).unwrap();
        let applied = process(&mut db, &customers, &actor, &location, NOW, true).unwrap();
        assert_eq!(applied.accepted, 1);
        assert_eq!(count(&db, "customers"), 1);
        assert_eq!(log(&db, &actor, LogKind::Customer).unwrap()[0].code, "C1");
        assert!(Batch::from_bytes(b"codigo,codigo,nombre\n1,2,Ana", ImportKind::Customers).is_err());
    }

    #[test]
    fn fallo_sql_revierte_el_lote_entero() {
        let mut db = db();
        let batch = Batch::from_bytes(b"codigo;nombre\nC1;Ana\nC2;Luis\n", ImportKind::Customers).unwrap();
        db.execute_batch("CREATE TRIGGER fail_before_insert BEFORE INSERT ON customers WHEN NEW.SEARCHKEY='C2'
            BEGIN SELECT RAISE(ABORT, 'simulated I/O'); END;").unwrap();
        assert!(process(&mut db, &batch, &"0".into(), &"0".into(), NOW, true).is_err());
        for table in ["customers", "pos_csv_log", "pos_audit"] { assert_eq!(count(&db, table), 0); }
    }
}
