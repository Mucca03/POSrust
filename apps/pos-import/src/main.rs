use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use pos_store::import::{import_dump, ImportOptions};
use pos_store::verify::verify_connection;
use pos_store::open_readonly;

const USAGE: &str = "\
pos-import — importa un respaldo MySQL compatible a SQLite\n\
\n\
Uso:\n\
  pos-import --dump <respaldo.sql> --db <pos.db> [opciones]\n\
  pos-import --verify-only --db <pos.db> [--report <informe.txt>]\n\
\n\
Opciones:\n\
  --dump <ruta>          Dump MySQL de entrada (solo lectura).\n\
  --db <ruta>            Base SQLite de salida o a verificar.\n\
  --force                Sobrescribe la base destino (guarda .bak-...).\n\
  --verify-only          Solo verifica una base existente; no importa.\n\
  --offset-minutes <n>   Offset local respecto a UTC (por defecto -300).\n\
  --report <ruta>        Escribe el informe de verificación (.txt y .json).\n\
  --quiet                No muestra progreso de importación.\n\
  -h, --help             Muestra esta ayuda.\n\
";

struct Args {
    dump: Option<PathBuf>,
    db: Option<PathBuf>,
    force: bool,
    verify_only: bool,
    tz_offset_minutes: i32,
    report: Option<PathBuf>,
    progress: bool,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args = parse_args(env::args().skip(1))?;
    let db = args.db.ok_or("falta --db")?;
    if let Some(path) = args.report.as_deref() {
        for target in [path.to_path_buf(), json_report_path(path)] {
            if fs::symlink_metadata(&target).is_ok() || target == db {
                return Err(format!("el destino de informe ya está ocupado: {}", target.display()));
            }
        }
    }

    if args.verify_only {
        let conn = open_readonly(&db).map_err(|e| e.to_string())?;
        let report = verify_connection(&conn).map_err(|e| e.to_string())?;
        emit_report(&report, args.report.as_deref())?;
        if report.has_errors() {
            return Err(format!("la verificación encontró errores: {}", report.error_summary()));
        }
        print!("{}", report.to_text());
        println!("Verificación OK: {}", db.display());
        return Ok(());
    }

    let dump = args.dump.ok_or("falta --dump")?;
    let options = ImportOptions {
        force: args.force,
        tz_offset_minutes: args.tz_offset_minutes,
        progress: args.progress,
    };
    let summary = import_dump(&dump, &db, &options).map_err(|e| e.to_string())?;

    println!("Importación completada");
    println!("  origen:  {}", summary.dump_path);
    println!("  destino: {}", summary.db_path);
    println!("  filas:   {}", summary.total_rows);
    println!("  tiempo:  {} ms", summary.duration_ms);
    println!("  sha256:  {}", summary.dump_sha256);
    if !summary.skipped_tables.is_empty() {
        println!("  tablas fuera del alcance: {}", summary.skipped_tables.join(", "));
    }
    println!();
    print!("{}", summary.verification.to_text());

    emit_report(&summary.verification, args.report.as_deref())?;
    Ok(())
}

fn emit_report(
    report: &pos_store::verify::VerificationReport,
    path: Option<&Path>,
) -> Result<(), String> {
    let Some(path) = path else {
        return Ok(());
    };
    let text_path = path.to_path_buf();
    let json_path = json_report_path(path);
    let text = report.to_text();
    let json = serde_json::to_string_pretty(report)
        .map_err(|e| format!("no se pudo serializar el informe: {e}"))?;
    File::create_new(&text_path).and_then(|mut file| file.write_all(text.as_bytes()))
        .map_err(|e| format!("no se pudo escribir {}: {e}", text_path.display()))?;
    File::create_new(&json_path).and_then(|mut file| file.write_all(json.as_bytes()))
        .map_err(|e| format!("no se pudo escribir {}: {e}", json_path.display()))?;
    eprintln!(
        "informe escrito en {} y {}",
        text_path.display(),
        json_path.display()
    );
    Ok(())
}

fn json_report_path(path: &Path) -> PathBuf {
    let json = path.with_extension("json");
    if json != path {
        return json;
    }
    let mut name = path.as_os_str().to_os_string();
    name.push(".json");
    name.into()
}

fn parse_args<I: Iterator<Item = String>>(mut it: I) -> Result<Args, String> {
    let mut args = Args {
        dump: None,
        db: None,
        force: false,
        verify_only: false,
        tz_offset_minutes: -300,
        report: None,
        progress: true,
    };

    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            "--dump" => args.dump = Some(next_value(&mut it, "--dump")?.into()),
            "--db" => args.db = Some(next_value(&mut it, "--db")?.into()),
            "--force" => args.force = true,
            // Compatibilidad con las instrucciones de versiones anteriores.
            "--no-expected" => {}
            "--verify-only" => args.verify_only = true,
            "--offset-minutes" => {
                let value = next_value(&mut it, "--offset-minutes")?;
                args.tz_offset_minutes = value
                    .parse()
                    .map_err(|_| format!("valor inválido para --offset-minutes: {value}"))?;
            }
            "--report" => args.report = Some(next_value(&mut it, "--report")?.into()),
            "--quiet" => args.progress = false,
            other => return Err(format!("opción desconocida: {other}\n\n{USAGE}")),
        }
    }

    if args.verify_only && args.dump.is_some() {
        return Err("--verify-only no acepta --dump".to_string());
    }
    Ok(args)
}

fn next_value<I: Iterator<Item = String>>(it: &mut I, name: &str) -> Result<String, String> {
    it.next()
        .ok_or_else(|| format!("falta el valor de {name}"))
}
