//! Parser específico para el formato de `mysqldump`.
//!
//! No pretende ser un parser SQL general: lee las líneas `INSERT INTO ... VALUES`
//! que genera `mysqldump` y devuelve los valores crudos, sin interpretar aún el
//! tipo de columna. Así se conservan bytes, acentos y valores raros del respaldo.

use crate::error::StoreError;

/// Valor crudo leído de una tupla del dump.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawValue {
    Null,
    /// Cadena entre comillas, ya desescapada. Puede contener bytes no UTF-8.
    Text(Vec<u8>),
    /// Número o token sin comillas (incluye exponentes y negativos).
    Number(String),
    /// Literal binario `0x...`.
    Bytes(Vec<u8>),
}

/// Una sentencia INSERT ya separada en filas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedInsert {
    pub table: String,
    pub rows: Vec<Vec<RawValue>>,
}

/// Parsea una línea completa del dump. Devuelve `None` si no es un INSERT.
pub fn parse_insert_line(line: &[u8], line_no: usize) -> Result<Option<ParsedInsert>, StoreError> {
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    if !line.starts_with(b"INSERT INTO ") {
        return Ok(None);
    }

    let mut pos = b"INSERT INTO ".len();
    if line.get(pos) != Some(&b'`') {
        return Err(parse_err(line_no, "se esperaba el nombre de tabla entre backticks"));
    }
    pos += 1;
    let table_start = pos;
    while pos < line.len() && line[pos] != b'`' {
        pos += 1;
    }
    if pos >= line.len() {
        return Err(parse_err(line_no, "nombre de tabla sin backtick de cierre"));
    }
    let table = std::str::from_utf8(&line[table_start..pos])
        .map_err(|_| parse_err(line_no, "nombre de tabla no UTF-8"))?
        .to_string();
    pos += 1;

    skip_ws(line, &mut pos);
    if !starts_with_ascii_case(&line[pos..], b"VALUES") {
        return Err(parse_err(line_no, "se esperaba VALUES después del nombre de tabla"));
    }
    pos += b"VALUES".len();
    skip_ws(line, &mut pos);

    let mut rows = Vec::new();
    loop {
        skip_ws(line, &mut pos);
        match line.get(pos) {
            Some(b';') => {
                pos += 1;
                break;
            }
            Some(b'(') => {
                pos += 1;
                rows.push(parse_row(line, &mut pos, line_no)?);
                skip_ws(line, &mut pos);
                match line.get(pos) {
                    Some(b',') => {
                        pos += 1;
                    }
                    Some(b';') => {
                        pos += 1;
                        break;
                    }
                    _ => return Err(parse_err(line_no, "se esperaba ',' o ';' entre tuplas")),
                }
            }
            _ => return Err(parse_err(line_no, "se esperaba '(' o ';' en VALUES")),
        }
    }

    skip_ws(line, &mut pos);
    if pos != line.len() {
        return Err(parse_err(line_no, "contenido inesperado después del ';'"));
    }

    Ok(Some(ParsedInsert { table, rows }))
}

fn parse_row(line: &[u8], pos: &mut usize, line_no: usize) -> Result<Vec<RawValue>, StoreError> {
    let mut row = Vec::new();
    skip_ws(line, pos);
    if line.get(*pos) == Some(&b')') {
        *pos += 1;
        return Ok(row);
    }

    loop {
        row.push(parse_value(line, pos, line_no)?);
        skip_ws(line, pos);
        match line.get(*pos) {
            Some(b',') => {
                *pos += 1;
                continue;
            }
            Some(b')') => {
                *pos += 1;
                break;
            }
            _ => return Err(parse_err(line_no, "se esperaba ',' o ')' dentro de la tupla")),
        }
    }
    Ok(row)
}

fn parse_value(line: &[u8], pos: &mut usize, line_no: usize) -> Result<RawValue, StoreError> {
    skip_ws(line, pos);
    match line.get(*pos).copied() {
        None => Err(parse_err(line_no, "valor incompleto")),
        Some(b'\'') => parse_string(line, pos, line_no).map(RawValue::Text),
        Some(b'0') if matches!(line.get(*pos + 1), Some(b'x' | b'X')) => {
            parse_hex(line, pos, line_no).map(RawValue::Bytes)
        }
        Some(_) => {
            let start = *pos;
            while let Some(&b) = line.get(*pos) {
                if b == b',' || b == b')' {
                    break;
                }
                *pos += 1;
            }
            let token = &line[start..*pos];
            let token = trim_ascii(token);
            if token.is_empty() {
                return Err(parse_err(line_no, "token vacío"));
            }
            if token.eq_ignore_ascii_case(b"NULL") {
                return Ok(RawValue::Null);
            }
            let text = std::str::from_utf8(token)
                .map_err(|_| parse_err(line_no, "token no UTF-8"))?;
            Ok(RawValue::Number(text.to_string()))
        }
    }
}

fn parse_string(line: &[u8], pos: &mut usize, line_no: usize) -> Result<Vec<u8>, StoreError> {
    debug_assert_eq!(line.get(*pos), Some(&b'\''));
    *pos += 1;
    let mut out = Vec::new();

    while let Some(&b) = line.get(*pos) {
        match b {
            b'\'' => {
                *pos += 1;
                return Ok(out);
            }
            b'\\' => {
                *pos += 1;
                let esc = *line
                    .get(*pos)
                    .ok_or_else(|| parse_err(line_no, "escape incompleto"))?;
                match esc {
                    b'0' => out.push(0),
                    b'b' => out.push(0x08),
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    b'Z' => out.push(0x1a),
                    b'\\' => out.push(b'\\'),
                    b'\'' => out.push(b'\''),
                    b'"' => out.push(b'"'),
                    // MySQL conserva la barra en estos dos casos.
                    b'%' => {
                        out.push(b'\\');
                        out.push(b'%');
                    }
                    b'_' => {
                        out.push(b'\\');
                        out.push(b'_');
                    }
                    other => out.push(other),
                }
                *pos += 1;
            }
            _ => {
                out.push(b);
                *pos += 1;
            }
        }
    }

    Err(parse_err(line_no, "cadena sin comilla de cierre"))
}

fn parse_hex(line: &[u8], pos: &mut usize, line_no: usize) -> Result<Vec<u8>, StoreError> {
    *pos += 2; // 0x
    let start = *pos;
    while let Some(&b) = line.get(*pos) {
        if b == b',' || b == b')' {
            break;
        }
        if !b.is_ascii_hexdigit() {
            return Err(parse_err(line_no, "dígito hexadecimal inválido"));
        }
        *pos += 1;
    }
    let digits = &line[start..*pos];
    if digits.is_empty() || digits.len() % 2 != 0 {
        return Err(parse_err(line_no, "literal hexadecimal inválido"));
    }
    let mut out = Vec::with_capacity(digits.len() / 2);
    let mut i = 0;
    while i < digits.len() {
        let hi = hex_val(digits[i]);
        let lo = hex_val(digits[i + 1]);
        out.push((hi << 4) | lo);
        i += 2;
    }
    Ok(out)
}

fn hex_val(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        b'A'..=b'F' => b - b'A' + 10,
        _ => 0,
    }
}

fn skip_ws(line: &[u8], pos: &mut usize) {
    while let Some(&b) = line.get(*pos) {
        if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' {
            *pos += 1;
        } else {
            break;
        }
    }
}

fn trim_ascii(mut s: &[u8]) -> &[u8] {
    while let Some((&b, rest)) = s.split_first() {
        if b == b' ' || b == b'\t' || b == b'\r' || b == b'\n' {
            s = rest;
        } else {
            break;
        }
    }
    while let Some((&b, rest)) = s.split_last() {
        if b == b' ' || b == b'\t' || b == b'\r' || b == b'\n' {
            s = rest;
        } else {
            break;
        }
    }
    s
}

fn starts_with_ascii_case(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.len() >= needle.len() && haystack[..needle.len()].eq_ignore_ascii_case(needle)
}

fn parse_err(line: usize, message: impl Into<String>) -> StoreError {
    StoreError::Parse {
        line,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_insert() {
        let line = b"INSERT INTO `people` VALUES ('0','Administrador',NULL,'\\0','\x01',NULL);";
        let parsed = parse_insert_line(line, 1).unwrap().unwrap();
        assert_eq!(parsed.table, "people");
        assert_eq!(
            parsed.rows,
            vec![vec![
                RawValue::Text(b"0".to_vec()),
                RawValue::Text(b"Administrador".to_vec()),
                RawValue::Null,
                RawValue::Text(vec![0]),
                RawValue::Text(vec![1]),
                RawValue::Null,
            ]]
        );
    }

    #[test]
    fn parses_escapes_and_commas() {
        let line = b"INSERT INTO `t` VALUES ('a\\'b','c,d','e\\\\f','g\\nh','\\t','\\Z','\\%','\\_');";
        let parsed = parse_insert_line(line, 2).unwrap().unwrap();
        assert_eq!(
            parsed.rows[0],
            vec![
                RawValue::Text(b"a'b".to_vec()),
                RawValue::Text(b"c,d".to_vec()),
                RawValue::Text(b"e\\f".to_vec()),
                RawValue::Text(b"g\nh".to_vec()),
                RawValue::Text(b"\t".to_vec()),
                RawValue::Text(vec![0x1a]),
                RawValue::Text(b"\\%".to_vec()),
                RawValue::Text(b"\\_".to_vec()),
            ]
        );
    }

    #[test]
    fn parses_numbers_hex_and_empty_string() {
        let line = b"INSERT INTO `t` VALUES (-2.34,16000,0x0AFF,'');";
        let parsed = parse_insert_line(line, 3).unwrap().unwrap();
        assert_eq!(
            parsed.rows[0],
            vec![
                RawValue::Number("-2.34".to_string()),
                RawValue::Number("16000".to_string()),
                RawValue::Bytes(vec![0x0a, 0xff]),
                RawValue::Text(vec![]),
            ]
        );
    }

    #[test]
    fn parses_multiple_rows() {
        let line = b"INSERT INTO `t` VALUES (1,'a'),(2,'b'),(3,NULL);";
        let parsed = parse_insert_line(line, 4).unwrap().unwrap();
        assert_eq!(parsed.rows.len(), 3);
        assert_eq!(parsed.rows[2], vec![RawValue::Number("3".into()), RawValue::Null]);
    }

    #[test]
    fn ignores_non_insert_lines() {
        assert!(parse_insert_line(b"CREATE TABLE `x` ();", 1).unwrap().is_none());
        assert!(parse_insert_line(b"-- comment", 2).unwrap().is_none());
    }

    #[test]
    fn rejects_unterminated_string() {
        let err = parse_insert_line(b"INSERT INTO `t` VALUES ('abc);", 5).unwrap_err();
        assert!(matches!(err, StoreError::Parse { .. }));
    }
}
