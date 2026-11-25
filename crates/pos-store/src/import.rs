use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::Instant;

use rusqlite::{params_from_iter, Connection};
use serde::Serialize;
use sha2::{Digest, Sha256};
use time::macros::format_description;
use time::{OffsetDateTime, UtcOffset};

use crate::convert::convert_row;
use crate::error::StoreError;
use crate::parser::parse_insert_line;
use crate::schema::{apply_schema, table_spec, TableSpec};
use crate::verify::{verify_connection, VerificationReport};

#[derive(Debug, Clone)]
pub struct ImportOptions {
    /// Sobrescribe la base destino (guardando una copia `.bak-...`).
    pub force: bool,
    /// Offset local respecto a UTC, en minutos. Colombia: -300.
    pub tz_offset_minutes: i32,
    /// Muestra progreso por stderr.
    pub progress: bool,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            force: false,
            tz_offset_minutes: -300,
            progress: true,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportSummary {
    pub db_path: String,
    pub dump_path: String,
    pub dump_sha256: String,
    pub dump_bytes: u64,
    pub duration_ms: u128,
    pub total_rows: u64,
    pub rows_by_table: BTreeMap<String, u64>,
    pub skipped_tables: Vec<String>,
    pub verification: VerificationReport,
}

/// Importa el dump a una base nueva y verifica el resultado.
///
/// El dump original solo se lee. La base se construye en un archivo temporal y
/// solo se publica si la verificación no encuentra errores.
pub fn import_dump(
    dump: &Path,
    db: &Path,
    options: &ImportOptions,
) -> Result<ImportSummary, StoreError> {
    let started = Instant::now();
    if !dump.is_file() {
        return Err(StoreError::InvalidArgument(format!(
            "no existe el dump: {}",
            dump.display()
        )));
    }
    check_destination(dump, db, options.force)?;
    let parent = db.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let temp = tempfile::Builder::new().prefix(".pos-import-").tempfile_in(parent)?.into_temp_path();

    let dump_bytes = fs::metadata(dump)?.len();
    (|| -> Result<ImportSummary, StoreError> {
        let mut conn = Connection::open(&temp)?;
        configure_for_import(&conn)?;
        apply_schema(&conn)?;

        let mut hasher = Sha256::new();
        let mut reader = BufReader::with_capacity(1024 * 1024, File::open(dump)?);
        let mut line = Vec::new();
        let mut line_no = 0usize;
        let mut total_rows = 0u64;
        let mut rows_by_table: BTreeMap<String, u64> = BTreeMap::new();
        let mut skipped_tables: BTreeMap<String, u64> = BTreeMap::new();
        {
            let tx = conn.transaction()?;
            loop {
                line.clear();
                let n = reader.read_until(b'\n', &mut line)?;
                if n == 0 {
                    break;
                }
                line_no += 1;
                hasher.update(&line);
                let content = line.strip_suffix(b"\n").unwrap_or(&line);

                let Some(parsed) = parse_insert_line(content, line_no)? else {
                    continue;
                };

                let Some(spec) = table_spec(&parsed.table) else {
                    *skipped_tables.entry(parsed.table).or_insert(0) += parsed.rows.len() as u64;
                    continue;
                };

                let sql = build_insert_sql(spec);
                let mut stmt = tx.prepare(&sql)?;
                for row in &parsed.rows {
                    let values = convert_row(
                        spec.name,
                        spec.columns,
                        row,
                        line_no,
                        options.tz_offset_minutes,
                    )?;
                    stmt.execute(params_from_iter(values.iter()))?;
                    *rows_by_table.entry(spec.name.to_string()).or_insert(0) += 1;
                    total_rows += 1;
                    if options.progress && total_rows % 25_000 == 0 {
                        eprintln!("  importadas {total_rows} filas...");
                    }
                }
            }
            tx.commit()?;
        }

        let sha256 = hex::encode(hasher.finalize());
        seed_extensions(&conn, dump, &sha256, dump_bytes, options.tz_offset_minutes)?;

        // Se deja el archivo en modo rollback para poder moverlo sin sidecars.
        let _: String = conn.query_row("PRAGMA journal_mode = DELETE", [], |r| r.get(0))?;
        conn.execute_batch("PRAGMA optimize;")?;

        if total_rows == 0 {
            return Err(StoreError::Verification("la exportación no contiene filas de tablas compatibles".into()));
        }
        let mut verification = verify_connection(&conn)?;
        for (table, saved) in &verification.counts {
            let read = rows_by_table.get(table).copied().unwrap_or_default();
            if *saved != read {
                verification.errors.push(format!("filas de {table}: {read} en el respaldo, {saved} en SQLite"));
            }
        }
        if verification.has_errors() {
            return Err(StoreError::Verification(verification.to_text()));
        }

        drop(conn);

        File::options().write(true).open(&temp)?.sync_all()?;
        publish_db(temp, dump, db, options.force)?;

        Ok(ImportSummary {
            db_path: db.display().to_string(),
            dump_path: dump.display().to_string(),
            dump_sha256: sha256,
            dump_bytes,
            duration_ms: started.elapsed().as_millis(),
            total_rows,
            rows_by_table,
            skipped_tables: skipped_tables.keys().cloned().collect(),
            verification,
        })
    })()
}

fn configure_for_import(conn: &Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "PRAGMA journal_mode = MEMORY;
         PRAGMA synchronous = OFF;
         PRAGMA temp_store = MEMORY;
         PRAGMA cache_size = -65536;
         PRAGMA foreign_keys = OFF;
         PRAGMA locking_mode = EXCLUSIVE;",
    )?;
    Ok(())
}

fn build_insert_sql(spec: &TableSpec) -> String {
    let placeholders = std::iter::repeat("?")
        .take(spec.columns.len())
        .collect::<Vec<_>>()
        .join(",");
    format!("INSERT INTO \"{}\" VALUES ({placeholders})", spec.name)
}

fn seed_extensions(
    conn: &Connection,
    dump: &Path,
    sha256: &str,
    dump_bytes: u64,
    tz_offset_minutes: i32,
) -> Result<(), StoreError> {
    let now = now_local(tz_offset_minutes)?;
    let meta = [
        ("schema_version", "1".to_string()),
        ("source_dump", dump.display().to_string()),
        ("source_dump_sha256", sha256.to_string()),
        ("source_dump_bytes", dump_bytes.to_string()),
        ("imported_at", now.clone()),
        ("timezone_offset_minutes", tz_offset_minutes.to_string()),
    ];
    for (key, value) in meta {
        conn.execute(
            "INSERT OR REPLACE INTO pos_meta(\"KEY\", \"VALUE\") VALUES (?1, ?2)",
            rusqlite::params![key, value],
        )?;
    }

    let settings = [
        ("prices_include_tax", "1"),
        ("default_tax_category", "100"),
        ("cash_rounding", "0"),
        ("payment_methods", "cash,nequi,daviplata,bancolombia,magcard"),
        ("printer_width", "48"),
        ("printer_transport", "file:receipt.bin"),
        ("business_name", ""),
        ("business_taxid", ""),
        ("business_address", ""),
        ("business_phone", ""),
        ("receipt_footer", "Gracias por su compra"),
        ("backup_dir", "./backups"),
        ("backup_retention_weeks", "12"),
        ("scale_manual", "1"),
        ("scale_barcode_prefix", ""),
    ];
    for (key, value) in settings {
        conn.execute(
            "INSERT OR IGNORE INTO pos_settings(\"KEY\", \"VALUE\") VALUES (?1, ?2)",
            rusqlite::params![key, value],
        )?;
    }

    let methods = [
        ("cash", "Efectivo", "cash", 0),
        ("nequi", "Nequi", "electronic", 1),
        ("daviplata", "Daviplata", "electronic", 2),
        ("bancolombia", "Bancolombia", "electronic", 3),
        ("magcard", "Tarjeta", "card", 4),
    ];
    for (code, label, kind, order) in methods {
        conn.execute(
            "INSERT OR IGNORE INTO pos_payment_methods
             (\"CODE\", \"LABEL\", \"KIND\", \"ENABLED\", \"REQUIRES_REFERENCE\", \"SORT_ORDER\")
             VALUES (?1, ?2, ?3, 1, 0, ?4)",
            rusqlite::params![code, label, kind, order],
        )?;
    }

    conn.execute(
        "INSERT OR REPLACE INTO pos_product_flags(\"PRODUCT_ID\", \"ACTIVE\", \"UPDATED_AT\")
         SELECT ID, 1, ?1 FROM products",
        [&now],
    )?;

    conn.pragma_update(None, "user_version", 1)?;
    Ok(())
}

fn check_destination(dump: &Path, db: &Path, force: bool) -> Result<(), StoreError> {
    match fs::symlink_metadata(db) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err.into()),
        Ok(meta) if !force || !meta.is_file() => {
            return Err(StoreError::DatabaseExists(db.display().to_string()));
        }
        Ok(_) => {}
    }
    if fs::canonicalize(dump)? == fs::canonicalize(db)? {
        return Err(StoreError::InvalidArgument("el respaldo no puede ser el destino".to_owned()));
    }
    let mut header = [0; 16];
    if File::open(db)?.read_exact(&mut header).is_err() || &header != b"SQLite format 3\0" {
        return Err(StoreError::InvalidArgument(
            "--force solo permite reemplazar una base SQLite existente".to_owned(),
        ));
    }
    for suffix in ["-wal", "-shm", "-journal"] {
        if fs::symlink_metadata(with_suffix(db, suffix)).is_ok() {
            return Err(StoreError::InvalidArgument(
                "cierre la base destino y asegúrese de que no queden archivos WAL/journal".to_owned(),
            ));
        }
    }
    Ok(())
}

fn publish_db(temp: tempfile::TempPath, dump: &Path, db: &Path, force: bool) -> Result<(), StoreError> {
    check_destination(dump, db, force)?;
    if !force || !db.exists() {
        temp.persist_noclobber(db).map_err(|e| e.error)?;
    } else {
        let backup = with_suffix(db, &format!(".bak-{}", uuid::Uuid::new_v4()));
        fs::hard_link(db, &backup)?; // No sobrescribe un archivo ajeno.
        temp.persist(db).map_err(|e| e.error)?;
    }
    Ok(())
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    name.into()
}

fn now_local(tz_offset_minutes: i32) -> Result<String, StoreError> {
    let offset = UtcOffset::from_whole_seconds(tz_offset_minutes.saturating_mul(60)).map_err(|e| {
        StoreError::InvalidArgument(format!("offset horario inválido: {e}"))
    })?;
    let fmt = format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");
    OffsetDateTime::now_utc()
        .to_offset(offset)
        .format(&fmt)
        .map_err(|e| StoreError::InvalidArgument(format!("fecha actual inválida: {e}")))
}
