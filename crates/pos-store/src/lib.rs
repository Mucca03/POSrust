//! Persistencia SQLite e importación de respaldos compatibles.
//!
//! Regla de oro: el dump MySQL original nunca se modifica. La importación crea
//! una base nueva, conserva los IDs y el historial, y solo adapta la
//! representación (BIT, TIMESTAMP, tipos SQLite).

#![forbid(unsafe_code)]

pub mod auth;
pub mod backup;
pub mod catalog;
pub mod clock;
pub mod convert;
pub mod csv_tools;
pub mod error;
pub mod import;
pub mod parser;
pub mod products;
pub mod properties;
pub mod receipts;
pub mod reports;
pub mod sales;
pub mod schema;
pub mod sessions;
pub mod tables;
pub mod verify;

pub use auth::{authenticate, sha1_hex, verify_password};
pub use backup::{backup_now, maybe_weekly_backup, retention_weeks};
pub use catalog::{load_catalog, Catalog};
pub use clock::{local_to_unix, now_local, unix_to_local};
pub use error::StoreError;
pub use import::{import_dump, ImportOptions, ImportSummary};
pub use parser::{parse_insert_line, ParsedInsert, RawValue};
pub use products::{create_product, update_product, NewProduct};
pub use properties::properties_xml;
pub use receipts::{last_sale, ticket_by_number};
pub use reports::{sales_report, ReportCsv, SalesReport};
pub use sales::{
    audit, record_cash_movement, record_drawer_opening, record_removed_line, record_stock_entry, save_sale,
    CashDirection, SaleContext, SaleResult,
};
pub use schema::{apply_schema, ColumnKind, TableSpec, TABLES};
pub use sessions::{
    close_session, current_session, ensure_session, session_info, session_totals, SessionInfo,
};
pub use verify::{verify_connection, VerificationReport};

use rusqlite::Connection;

/// Abre la base operativa con la configuración prevista para la caja.
pub fn open(path: impl AsRef<std::path::Path>) -> Result<Connection, StoreError> {
    let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    configure_runtime(&conn)?;
    Ok(conn)
}

/// Abre la base en solo lectura (reportes o verificación).
pub fn open_readonly(path: impl AsRef<std::path::Path>) -> Result<Connection, StoreError> {
    let conn = Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(conn)
}

/// Configuración de operación normal: WAL, confirmaciones durables y FKs.
pub fn configure_runtime(conn: &Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "PRAGMA busy_timeout = 5000;
         PRAGMA journal_mode = WAL;
         PRAGMA synchronous = FULL;
         PRAGMA foreign_keys = ON;
         PRAGMA temp_store = MEMORY;
         PRAGMA cache_size = -8192;",
    )?;
    schema::apply_extensions(conn)?;
    Ok(())
}

/// Verifica la integridad física del archivo.
pub fn integrity_check(conn: &Connection) -> Result<bool, StoreError> {
    let result: String = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    Ok(result == "ok")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_in_memory_and_applies_schema() {
        let conn = Connection::open_in_memory().unwrap();
        apply_schema(&conn).unwrap();
        assert!(integrity_check(&conn).unwrap());
    }
}
