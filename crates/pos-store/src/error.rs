use std::io;

/// Errores del almacenamiento, del parser del dump y de la verificación.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("permiso insuficiente: {0:?}")]
    PermissionDenied(pos_core::Capability),
    #[error("error de E/S: {0}")]
    Io(#[from] io::Error),

    #[error("error de SQLite: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("error de formato en la línea {line}: {message}")]
    Parse { line: usize, message: String },

    #[error("texto UTF-8 inválido en la línea {line}, columna {column}")]
    Utf8 { line: usize, column: String },

    #[error(
        "la tabla {table} tiene {expected} columnas, pero la fila de la línea {line} trae {got}"
    )]
    ColumnCount {
        table: String,
        expected: usize,
        got: usize,
        line: usize,
    },

    #[error("tabla desconocida en el dump: {table} (línea {line})")]
    UnknownTable { table: String, line: usize },

    #[error("la base de datos ya existe: {0}")]
    DatabaseExists(String),

    #[error("verificación fallida: {0}")]
    Verification(String),

    #[error("argumento inválido: {0}")]
    InvalidArgument(String),

    #[error("error de fecha/hora: {0}")]
    Time(String),

    #[error("error de dominio: {0}")]
    Core(#[from] pos_core::CoreError),
}
