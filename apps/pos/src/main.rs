#![forbid(unsafe_code)]

use std::{env, error::Error, fs::OpenOptions, path::PathBuf, process::ExitCode};

use pos_ui::LaunchOptions;

const USAGE: &str = "Uso: pos --db <copia-operativa.db> [--printer file:<carpeta>|cups:<cola>|tcp:<host:puerto>|windows_spool:<impresora>] [--paper 58|80]";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("POS: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let mut db = None;
    let mut printer = None;
    let mut paper_mm = None;
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("falta el valor de {arg:?}"));
        match arg.to_str() {
            Some("--help" | "-h") => {
                println!("{USAGE}");
                return Ok(());
            }
            Some("--db") => db = Some(PathBuf::from(value()?)),
            Some("--printer") => printer = Some(value()?.into_string().map_err(|_| "destino inválido")?),
            Some("--paper") => paper_mm = Some(match value()?.to_str() {
                Some("58") => 58,
                Some("80") => 80,
                _ => return Err("--paper acepta 58 u 80".into()),
            }),
            _ => return Err(format!("opción desconocida: {arg:?}\n{USAGE}").into()),
        }
    }
    let db = db.ok_or(USAGE)?.canonicalize()?;
    if !db.is_file() {
        return Err("la ruta de la base no es un archivo".into());
    }
    // Mantener abierto el descriptor toda la ejecución. No se borra el lock
    // al salir: otro proceso podría haber adquirido ya el mismo archivo.
    let mut lock_path = db.as_os_str().to_owned();
    lock_path.push(".pos-lock");
    let lock = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(lock_path)?;
    fs2::FileExt::try_lock_exclusive(&lock).map_err(|error| {
        format!("no se pudo reservar la base para una sola instancia: {error}")
    })?;
    pos_ui::run(LaunchOptions { db, printer, paper_mm })?;
    Ok(())
}
