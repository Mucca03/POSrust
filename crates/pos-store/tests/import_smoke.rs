use std::fs;

use pos_store::{import_dump, open_readonly, ImportOptions};
use tempfile::tempdir;

#[test]
fn imports_minimal_dump_and_publishes_new_db() {
    let dir = tempdir().unwrap();
    let dump = dir.path().join("mini.sql");
    let db = dir.path().join("pos.db");

    fs::write(
        &dump,
        "INSERT INTO `applications` VALUES ('app-1','POS anterior','1.0','2024-01-02 03:04:05');\n",
    )
    .unwrap();

    let options = ImportOptions {
        progress: false,
        ..Default::default()
    };
    let summary = import_dump(&dump, &db, &options).unwrap();

    assert_eq!(summary.total_rows, 1);
    assert!(dump.exists(), "el dump original no se toca");
    assert!(db.exists());

    let conn = open_readonly(&db).unwrap();
    let rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM applications", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 1);

    let name: String = conn
        .query_row("SELECT NAME FROM applications", [], |r| r.get(0))
        .unwrap();
    assert_eq!(name, "POS anterior");

    // El TIMESTAMP del dump es UTC y se guarda en hora local de Colombia.
    let instdate: String = conn
        .query_row("SELECT INSTDATE FROM applications", [], |r| r.get(0))
        .unwrap();
    assert_eq!(instdate, "2024-01-01 22:04:05");

    let source: String = conn
        .query_row(
            "SELECT \"VALUE\" FROM pos_meta WHERE \"KEY\"='source_dump'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(source, dump.display().to_string());
}
#[test]
fn force_no_puede_reemplazar_el_original_ni_otro_archivo() {
    let dir = tempfile::tempdir().unwrap();
    let dump = dir.path().join("respaldo.sql");
    let other = dir.path().join("otro.db");
    let bytes = b"INSERT INTO `applications` VALUES ('id','POS','4.2.2','2024-01-02 03:04:05');\n";
    std::fs::write(&dump, bytes).unwrap();
    std::fs::write(&other, b"archivo del usuario").unwrap();
    let options = pos_store::ImportOptions { force: true, ..Default::default() };
    assert!(pos_store::import_dump(&dump, &dump, &options).is_err());
    assert!(pos_store::import_dump(&dump, &other, &options).is_err());
    assert_eq!(std::fs::read(&dump).unwrap(), bytes);
    assert_eq!(std::fs::read(&other).unwrap(), b"archivo del usuario");
}

#[test]
fn no_publica_un_respaldo_sin_tablas_compatibles() {
    let dir = tempdir().unwrap();
    let dump = dir.path().join("otro.sql");
    let db = dir.path().join("pos.db");
    fs::write(&dump, "INSERT INTO `tabla_ajena` VALUES ('id');\n").unwrap();
    assert!(import_dump(&dump, &db, &ImportOptions::default()).is_err());
    assert!(!db.exists());
}
