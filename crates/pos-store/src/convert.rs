use rusqlite::types::Value;
use time::macros::format_description;
use time::{PrimitiveDateTime, UtcOffset};

use crate::error::StoreError;
use crate::parser::RawValue;
use crate::schema::{Column, ColumnKind};

const DATE_FORMAT: &[time::format_description::FormatItem<'static>] =
    format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");

/// Convierte una fila cruda a valores SQLite según el tipo de cada columna.
pub fn convert_row(
    table: &str,
    columns: &[Column],
    row: &[RawValue],
    line_no: usize,
    tz_offset_minutes: i32,
) -> Result<Vec<Value>, StoreError> {
    if row.len() != columns.len() {
        return Err(StoreError::ColumnCount {
            table: table.to_string(),
            expected: columns.len(),
            got: row.len(),
            line: line_no,
        });
    }

    let mut out = Vec::with_capacity(row.len());
    for (raw, col) in row.iter().zip(columns) {
        out.push(convert_value(table, col.name, col.kind, raw, line_no, tz_offset_minutes)?);
    }
    Ok(out)
}

fn convert_value(
    table: &str,
    column: &str,
    kind: ColumnKind,
    raw: &RawValue,
    line_no: usize,
    tz_offset_minutes: i32,
) -> Result<Value, StoreError> {
    if matches!(raw, RawValue::Null) {
        return Ok(Value::Null);
    }

    match kind {
        ColumnKind::Text | ColumnKind::DateTime => {
            let bytes = as_bytes(raw);
            let text = std::str::from_utf8(bytes).map_err(|_| StoreError::Utf8 {
                line: line_no,
                column: format!("{table}.{column}"),
            })?;
            Ok(Value::Text(text.to_string()))
        }
        ColumnKind::Int => {
            let n = as_i64(raw, table, column, line_no)?;
            Ok(Value::Integer(n))
        }
        ColumnKind::Decimal => {
            let bytes = as_bytes(raw);
            let text = std::str::from_utf8(bytes).map_err(|_| StoreError::Utf8 {
                line: line_no,
                column: format!("{table}.{column}"),
            })?;
            Ok(Value::Text(text.to_string()))
        }
        ColumnKind::Bit => {
            let n = as_bit(raw, table, column, line_no)?;
            Ok(Value::Integer(n))
        }
        ColumnKind::Timestamp => {
            let bytes = as_bytes(raw);
            let text = std::str::from_utf8(bytes).map_err(|_| StoreError::Utf8 {
                line: line_no,
                column: format!("{table}.{column}"),
            })?;
            Ok(Value::Text(convert_timestamp(
                text,
                tz_offset_minutes,
                table,
                column,
                line_no,
            )?))
        }
        ColumnKind::Blob => match raw {
            RawValue::Text(b) | RawValue::Bytes(b) => Ok(Value::Blob(b.clone())),
            RawValue::Number(s) => Ok(Value::Blob(s.as_bytes().to_vec())),
            RawValue::Null => Ok(Value::Null),
        },
    }
}

/// Convierte un TIMESTAMP del dump (UTC) a hora local de Colombia.
///
/// Las columnas DATETIME ya vienen en hora local y no pasan por aquí.
fn convert_timestamp(
    text: &str,
    tz_offset_minutes: i32,
    table: &str,
    column: &str,
    line_no: usize,
) -> Result<String, StoreError> {
    // Algunos backups antiguos traen fechas cero; se conservan tal cual para no
    // inventar información.
    if text.starts_with("0000-00-00") {
        return Ok(text.to_string());
    }

    let parsed = PrimitiveDateTime::parse(text, DATE_FORMAT).map_err(|e| StoreError::Parse {
        line: line_no,
        message: format!("fecha inválida en {table}.{column}: {text:?} ({e})"),
    })?;

    let offset = UtcOffset::from_whole_seconds(tz_offset_minutes.saturating_mul(60)).map_err(|e| {
        StoreError::Parse {
            line: line_no,
            message: format!("offset horario inválido: {e}"),
        }
    })?;

    let local = parsed.assume_utc().to_offset(offset);
    local
        .format(DATE_FORMAT)
        .map_err(|e| StoreError::Parse {
            line: line_no,
            message: format!("no se pudo formatear {table}.{column}: {e}"),
        })
}

fn as_bytes(raw: &RawValue) -> &[u8] {
    match raw {
        RawValue::Text(b) | RawValue::Bytes(b) => b,
        RawValue::Number(s) => s.as_bytes(),
        RawValue::Null => &[],
    }
}

fn as_i64(
    raw: &RawValue,
    table: &str,
    column: &str,
    line_no: usize,
) -> Result<i64, StoreError> {
    let s = match raw {
        RawValue::Number(s) => s.as_str(),
        RawValue::Text(b) | RawValue::Bytes(b) => std::str::from_utf8(b).map_err(|_| {
            StoreError::Utf8 {
                line: line_no,
                column: format!("{table}.{column}"),
            }
        })?,
        RawValue::Null => return Ok(0),
    };
    s.trim().parse::<i64>().map_err(|_| StoreError::Parse {
        line: line_no,
        message: format!("entero inválido en {table}.{column}: {s:?}"),
    })
}

fn as_bit(
    raw: &RawValue,
    table: &str,
    column: &str,
    line_no: usize,
) -> Result<i64, StoreError> {
    let value = match raw {
        RawValue::Number(s) => s.trim().parse::<i64>().ok(),
        RawValue::Text(b) | RawValue::Bytes(b) => match b.as_slice() {
            [] => Some(0),
            [0] => Some(0),
            [1] => Some(1),
            [b'0'] => Some(0),
            [b'1'] => Some(1),
            _ => None,
        },
        RawValue::Null => Some(0),
    };

    value.ok_or_else(|| StoreError::Parse {
        line: line_no,
        message: format!("BIT inválido en {table}.{column}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_utc_timestamp_to_bogota() {
        let out = convert_timestamp("2026-10-18 17:17:24", -300, "t", "c", 1).unwrap();
        assert_eq!(out, "2026-10-18 12:17:24");
    }

    #[test]
    fn converts_crossing_midnight() {
        let out = convert_timestamp("2026-01-01 02:00:00", -300, "t", "c", 1).unwrap();
        assert_eq!(out, "2025-12-31 21:00:00");
    }

    #[test]
    fn converts_positive_offset() {
        let out = convert_timestamp("2026-01-01 02:00:00", 120, "t", "c", 1).unwrap();
        assert_eq!(out, "2026-01-01 04:00:00");
    }

    #[test]
    fn bit_values() {
        assert_eq!(as_bit(&RawValue::Text(vec![0]), "t", "c", 1).unwrap(), 0);
        assert_eq!(as_bit(&RawValue::Text(vec![1]), "t", "c", 1).unwrap(), 1);
        assert_eq!(as_bit(&RawValue::Number("0".into()), "t", "c", 1).unwrap(), 0);
        assert!(as_bit(&RawValue::Text(vec![2]), "t", "c", 1).is_err());
    }
}
