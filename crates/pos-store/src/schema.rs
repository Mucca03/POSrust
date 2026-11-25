use rusqlite::Connection;

use crate::error::StoreError;

pub use crate::tables::{Column, TableSpec, TABLES};

/// Tipo lógico de una columna heredada.
///
/// Los importes y cantidades se guardan como TEXT decimal exacto; nunca como
/// REAL, para no introducir coma flotante en el dinero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnKind {
    Text,
    Int,
    Decimal,
    Bit,
    Timestamp,
    DateTime,
    Blob,
}

/// Script completo del esquema (tablas heredadas + extensiones `pos_*`).
pub const SCHEMA_SQL: &str = include_str!("schema.sql");

/// Aplica el esquema a una conexión nueva.
pub fn apply_schema(conn: &Connection) -> Result<(), StoreError> {
    conn.execute_batch(SCHEMA_SQL)?;
    apply_extensions(conn)?;
    Ok(())
}

// Extensión aditiva: también se instala al abrir una importación anterior.
pub(crate) fn apply_extensions(conn: &Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS pos_session_events (
            ID INTEGER PRIMARY KEY,
            MONEY TEXT NOT NULL REFERENCES closedcash(MONEY),
            KIND TEXT NOT NULL CHECK (KIND IN ('removed', 'cash_sale', 'no_sale', 'close'))
         );
         CREATE INDEX IF NOT EXISTS pos_session_events_money ON pos_session_events(MONEY, KIND);
         CREATE TABLE IF NOT EXISTS pos_csv_log (
             ID INTEGER PRIMARY KEY, BATCH TEXT NOT NULL, TS TEXT NOT NULL, ACTOR TEXT NOT NULL,
             KIND TEXT NOT NULL, LINE INTEGER NOT NULL, CODE TEXT NOT NULL, DETAIL TEXT NOT NULL,
             ARCHIVED INTEGER NOT NULL DEFAULT 0
         );
         CREATE INDEX IF NOT EXISTS pos_csv_log_visible ON pos_csv_log(ARCHIVED, KIND, ID);",
    )?;
    Ok(())
}

/// Busca la especificación de una tabla del dump.
pub fn table_spec(name: &str) -> Option<&'static TableSpec> {
    TABLES.iter().find(|t| t.name == name)
}

/// Nombres de las tablas que el importador reconoce.
pub fn table_names() -> impl Iterator<Item = &'static str> {
    TABLES.iter().map(|t| t.name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_creates_legacy_and_pos_tables() {
        let conn = Connection::open_in_memory().unwrap();
        apply_schema(&conn).unwrap();

        for table in table_names() {
            let n: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "falta la tabla {table}");
        }

        for table in [
            "pos_meta",
            "pos_settings",
            "pos_session_cash",
            "pos_payment_methods",
            "pos_product_flags",
            "pos_audit",
        ] {
            let n: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "falta la tabla {table}");
        }
    }

    #[test]
    fn all_tables_have_columns_and_are_known() {
        assert!(TABLES.len() >= 20);
        for t in TABLES {
            assert!(!t.columns.is_empty(), "{} sin columnas", t.name);
            assert!(table_spec(t.name).is_some());
        }
    }
}
