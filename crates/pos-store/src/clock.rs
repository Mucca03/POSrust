//! Fechas locales de la caja.
//!
//! El esquema heredado guarda los `DATETIME` en hora local. El POS mantiene esa
//! convención y no depende de la zona horaria configurada en el sistema.

use time::format_description::FormatItem;
use time::macros::format_description;
use time::{OffsetDateTime, UtcOffset};

use crate::error::StoreError;

const LOCAL_FORMAT: &[FormatItem<'static>] =
    format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");

/// Devuelve la hora local en el formato que usan las columnas `DATETIME`.
pub fn now_local(offset_minutes: i32) -> Result<String, StoreError> {
    let seconds = offset_minutes
        .checked_mul(60)
        .ok_or_else(|| StoreError::InvalidArgument("desplazamiento horario inválido".to_owned()))?;
    let offset = UtcOffset::from_whole_seconds(seconds)
        .map_err(|e| StoreError::Time(format!("zona horaria inválida: {e}")))?;
    OffsetDateTime::now_utc()
        .to_offset(offset)
        .format(LOCAL_FORMAT)
        .map_err(|e| StoreError::Time(format!("no se pudo formatear la fecha: {e}")))
}

/// Convierte una fecha local `YYYY-MM-DD HH:MM:SS` a epoch segundos, para
/// nombres de archivo y cálculos simples.
pub fn local_to_unix(value: &str, offset_minutes: i32) -> Result<i64, StoreError> {
    let parsed = time::PrimitiveDateTime::parse(value, LOCAL_FORMAT)
        .map_err(|e| StoreError::Time(format!("fecha inválida {value:?}: {e}")))?;
    let seconds = offset_minutes
        .checked_mul(60)
        .ok_or_else(|| StoreError::InvalidArgument("desplazamiento horario inválido".to_owned()))?;
    let offset = UtcOffset::from_whole_seconds(seconds)
        .map_err(|e| StoreError::Time(format!("zona horaria inválida: {e}")))?;
    Ok(parsed.assume_offset(offset).unix_timestamp())
}

/// Fecha local de un instante Unix, útil para reportes y respaldos.
pub fn unix_to_local(timestamp: i64, offset_minutes: i32) -> Result<String, StoreError> {
    let seconds = offset_minutes
        .checked_mul(60)
        .ok_or_else(|| StoreError::InvalidArgument("desplazamiento horario inválido".to_owned()))?;
    let offset = UtcOffset::from_whole_seconds(seconds)
        .map_err(|e| StoreError::Time(format!("zona horaria inválida: {e}")))?;
    OffsetDateTime::from_unix_timestamp(timestamp)
        .map_err(|e| StoreError::Time(format!("instante inválido: {e}")))?
        .to_offset(offset)
        .format(LOCAL_FORMAT)
        .map_err(|e| StoreError::Time(format!("no se pudo formatear la fecha: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ida_y_vuelta_de_fecha_local() {
        let value = "2026-11-24 09:15:30";
        let timestamp = local_to_unix(value, -300).unwrap();
        assert_eq!(unix_to_local(timestamp, -300).unwrap(), value);
    }
}
