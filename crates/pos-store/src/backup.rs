//! Respaldos consistentes de SQLite con `VACUUM INTO`.
//!
//! El respaldo semanal es automático; el backoffice también puede lanzarlo a
//! mano. Nunca se copia el archivo en caliente: `VACUUM INTO` produce una copia
//! íntegra aunque la caja esté escribiendo.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension};
use uuid::Uuid;

use crate::clock::local_to_unix;
use crate::error::StoreError;

const DEFAULT_RETENTION_WEEKS: u32 = 12;
const WEEK_SECONDS: i64 = 7 * 24 * 60 * 60;

/// Copia consistente en `dir`, con nombre único.
pub fn backup_now(conn: &Connection, dir: &Path) -> Result<PathBuf, StoreError> {
    backup(conn, dir, "pos-manual-")
}

fn backup(conn: &Connection, dir: &Path, prefix: &str) -> Result<PathBuf, StoreError> {
    fs::create_dir_all(dir)?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| StoreError::Time(format!("reloj del sistema inválido: {e}")))?
        .as_secs();
    let path = dir.join(format!("{prefix}{stamp}-{}.db", Uuid::new_v4().simple()));
    let temp = tempfile::Builder::new().prefix(".pos-backup-").tempfile_in(dir)?.into_temp_path();
    let filename = temp.to_str().ok_or_else(|| {
        StoreError::InvalidArgument("ruta de respaldo no representable en SQLite".to_owned())
    })?;
    conn.execute("VACUUM INTO ?1", [filename])?;
    {
        let copy = crate::open_readonly(&temp)?;
        if !crate::integrity_check(&copy)? {
            return Err(StoreError::Verification("el respaldo no pasó integrity_check".to_owned()));
        }
    }
    fs::File::options().write(true).open(&temp)?.sync_all()?;
    temp.persist_noclobber(&path).map_err(|e| e.error)?;
    Ok(path)
}

/// Hace el respaldo semanal si pasaron siete días desde el último.
pub fn maybe_weekly_backup(
    conn: &Connection,
    now: &str,
    dir: Option<&Path>,
) -> Result<Option<PathBuf>, StoreError> {
    let dir = match dir {
        Some(dir) => dir.to_path_buf(),
        None => {
            let configured = setting(conn, "backup_dir")?.unwrap_or_else(|| "./backups".to_owned());
            PathBuf::from(configured)
        }
    };

    let imported_offset: Option<String> = conn
        .query_row("SELECT VALUE FROM pos_meta WHERE KEY = 'timezone_offset_minutes'", [], |r| r.get(0))
        .optional()?;
    let offset = setting(conn, "timezone_offset_minutes")?.or(imported_offset)
        .and_then(|value| value.parse::<i32>().ok())
        .unwrap_or(-300);
    let now_unix = local_to_unix(now, offset)?;

    if let Some(last) = setting(conn, "last_backup_at")? {
        if let Ok(last_unix) = local_to_unix(&last, offset) {
            if now_unix - last_unix < WEEK_SECONDS {
                return Ok(None);
            }
        }
    }

    let path = backup(conn, &dir, "pos-weekly-")?;
    set_setting(conn, "last_backup_at", now)?;
    prune(&dir, retention_weeks(conn)?)?;
    Ok(Some(path))
}

pub fn retention_weeks(conn: &Connection) -> Result<u32, StoreError> {
    Ok(setting(conn, "backup_retention_weeks")?
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|weeks| *weeks > 0)
        .unwrap_or(DEFAULT_RETENTION_WEEKS))
}

fn prune(dir: &Path, weeks: u32) -> Result<(), StoreError> {
    let keep = i64::from(weeks) * 7 * 24 * 60 * 60;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| StoreError::Time(format!("reloj del sistema inválido: {e}")))?
        .as_secs() as i64;
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let Some(timestamp) = name.to_str().and_then(weekly_timestamp) else {
            continue;
        };
        if now.saturating_sub(timestamp) > keep && entry.file_type()?.is_file() {
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

fn weekly_timestamp(name: &str) -> Option<i64> {
    let (stamp, id) = name.strip_prefix("pos-weekly-")?.strip_suffix(".db")?.split_once('-')?;
    if !stamp.bytes().all(|b| b.is_ascii_digit()) || id.len() != 32 {
        return None;
    }
    let id = Uuid::parse_str(id).ok()?;
    (id.get_version_num() == 4).then(|| stamp.parse().ok()).flatten()
}

pub fn setting(conn: &Connection, key: &str) -> Result<Option<String>, StoreError> {
    let value = conn
        .query_row(
            "SELECT VALUE FROM pos_settings WHERE KEY = ?1",
            [key],
            |r| r.get(0),
        )
        .optional()?;
    Ok(value)
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO pos_settings (KEY, VALUE) VALUES (?1, ?2)
         ON CONFLICT(KEY) DO UPDATE SET VALUE = excluded.VALUE",
        [key, value],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::apply_schema;

    fn database() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        apply_schema(&conn).unwrap();
        conn
    }

    #[test]
    fn respalda_y_puede_abrir_la_copia() {
        let conn = database();
        let dir = tempfile::tempdir().unwrap();
        let path = backup_now(&conn, dir.path()).unwrap();
        assert!(path.exists());
        let copy = Connection::open(&path).unwrap();
        let count: i64 = copy
            .query_row("SELECT COUNT(*) FROM products", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn respaldo_semanal_solo_cuando_toca() {
        let conn = database();
        let dir = tempfile::tempdir().unwrap();
        let first = maybe_weekly_backup(&conn, "2026-11-24 10:00:00", Some(dir.path()))
            .unwrap()
            .expect("primer respaldo");
        assert!(first.exists());
        assert!(maybe_weekly_backup(&conn, "2026-11-25 10:00:00", Some(dir.path()))
            .unwrap()
            .is_none());
        assert!(maybe_weekly_backup(&conn, "2026-12-02 10:00:01", Some(dir.path()))
            .unwrap()
            .is_some());
    }
}
